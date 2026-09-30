//! Telling anybody who asked to see the input stream.
//!
//! The runtime owns input: the platform layer publishes every edge into it, and it is the
//! only place that knows whether an edge was applied, refused as a duplicate, or dropped
//! because a device vanished. A second consumer — a plugin counting keystrokes, a plugin
//! showing the keys you are holding — must not learn any of that by reading the runtime's
//! snapshot, because a snapshot is a summary and a tally needs every event.
//!
//! So the runtime offers a **subscription**: a bounded channel of the same events, fed
//! from the same producer, in the same order. The policy when a consumer falls behind is
//! the part worth stating, because it is the same policy a key edge gets everywhere else
//! in this product:
//!
//! * A subscription is *opt-in*. A consumer that has not asked is sent nothing, so a
//!   product with no plugins pays nothing for this file.
//! * **A full channel drops the event and counts it.** A consumer that stops reading must
//!   not be able to stall the platform layer's thread, and a dropped event has to be
//!   visible rather than quietly missing.
//! * **Pointer movement is not here.** Movement reaches the runtime on its own
//!   latest-value channel, where two samples in a frame become one, so a subscriber sees
//!   no mouse motion at all. A plugin that wants distance asks the plugin feed for it,
//!   because that is where the folding is decided.
//!
//! The fan-out sits on the producer's submitter rather than on the runtime's own
//! consumption, which is what makes the ordering real: a subscriber sees events in the
//! order they were published, and it sees each one whether or not the runtime applied it.

use crate::{InputEvent, SequencedInputEvent};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

/// How many events one subscription may hold before an event is dropped for it.
///
/// Deeper than the plugin feed's own queue, and for one reason: this channel sits between
/// the platform layer and a *thread that translates*, not between the host and a plugin
/// process. A burst of a few hundred keystrokes is a paste rather than a flood, and
/// losing the tail of a paste because the translating thread was descheduled would be a
/// tally that reads low with nothing to show for it.
pub const SUBSCRIPTION_CAPACITY: usize = 1024;

/// One consumer's view of the input stream.
///
/// Not `Clone`, deliberately: a `Receiver` owns its own position in the queue, so a second
/// handle to the same channel would be a second cursor reading the same events twice. A
/// consumer that wants two views subscribes twice, and each subscription has its own
/// drop count, which is the more useful answer anyway.
#[derive(Debug)]
pub struct InputSubscription {
    registry: Arc<Subscribers>,
    slot: usize,
    receiver: Receiver<InputEvent>,
}

impl InputSubscription {
    /// The next event, without waiting.
    ///
    /// Non-blocking because the consumer is a translator feeding a bounded queue of its
    /// own: a translator that waited here would be a translator whose latency is the
    /// runtime's, and a plugin's panel would then be as stale as the slowest thing in the
    /// product.
    pub fn take(&self) -> Option<InputEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Up to `limit` events, oldest first.
    ///
    /// Bounded by the caller because the events arrive one at a time and a consumer that
    /// drains without a limit holds the lock in the producer for as long as the queue is
    /// long — which is the one way a consumer could make the platform layer wait.
    pub fn drain(&self, limit: usize) -> Vec<InputEvent> {
        let mut events = Vec::with_capacity(limit.min(64));
        while events.len() < limit {
            match self.take() {
                Some(event) => events.push(event),
                None => break,
            }
        }
        events
    }

    /// How many events were dropped because this subscription was not keeping up.
    ///
    /// The number a consumer reports when its own tally looks wrong: a missing keystroke is
    /// invisible by nature, and this is what makes it visible.
    pub fn dropped(&self) -> u64 {
        self.registry
            .dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(self.slot)
            .map(|counter| counter.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    /// Wait for something to arrive, or for `timeout` to pass with nothing.
    ///
    /// The shape a consumer wants, and the reason it lives here rather than being built
    /// from [`Self::take`]: waiting on the channel itself is the only way to be woken by an
    /// event rather than by a timer, and a consumer that polled would be a consumer whose
    /// latency is its poll interval however quiet the product is.
    pub fn wait_for(&self, limit: usize, timeout: std::time::Duration) -> Wait {
        match self.receiver.recv_timeout(timeout) {
            Ok(first) => {
                let mut events = Vec::with_capacity(limit.min(64));
                events.push(first);
                while events.len() < limit {
                    match self.receiver.try_recv() {
                        Ok(event) => events.push(event),
                        Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                    }
                }
                Wait::Events(events)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Wait::Nothing,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Wait::Closed,
        }
    }

    /// Whether this subscription has been closed, by this handle or by the runtime.
    pub fn is_closed(&self) -> bool {
        !self
            .registry
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(self.slot)
            .is_some_and(Option::is_some)
    }

    /// Stop being sent events, and release the slot.
    ///
    /// Idempotent, and not something a consumer has to remember: a subscription dropped
    /// without this leaves one empty slot behind, and the registry grows by one per
    /// subscription over a long run. Closing is how a consumer that stops early says so.
    pub fn close(&self) {
        self.registry.close(self.slot);
    }
}

impl Drop for InputSubscription {
    fn drop(&mut self) {
        // The sender goes with the last reference to the slot, so a consumer that simply
        // goes away stops costing the producer a `try_send` per event.
        self.registry.close(self.slot);
    }
}

/// What one wait produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Wait {
    /// These events, oldest first.
    Events(Vec<InputEvent>),
    /// The timeout passed with nothing arriving.
    Nothing,
    /// The producer is gone, and no event will ever arrive again.
    Closed,
}

impl Wait {
    /// Whether the subscription will never produce anything again.
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// Every open subscription, and the counters beside them.
///
/// Public because the runtime's owner is what constructs it and the client is what hands
/// out subscriptions; not public because nothing outside this crate may publish to it.
#[derive(Debug, Default)]
pub struct Subscribers {
    /// One sender and receiver per slot. A slot is reused only by accident of arithmetic —
    /// slots are never recycled — so a closed slot stays `None` and is skipped.
    channels: Mutex<Vec<Option<SyncSender<InputEvent>>>>,
    /// Behind the same lock as the channels, because the two grow together and a counter
    /// for a slot that does not exist yet is a counter nothing can read.
    dropped: Mutex<Vec<AtomicU64>>,
    next_slot: AtomicU64,
}

impl Subscribers {
    /// Open a subscription.
    pub fn subscribe(self: &Arc<Self>) -> InputSubscription {
        let (sender, receiver) = std::sync::mpsc::sync_channel(SUBSCRIPTION_CAPACITY);
        let slot = self.next_slot.fetch_add(1, Ordering::AcqRel) as usize;
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while channels.len() <= slot {
            channels.push(None);
            self.dropped
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(AtomicU64::new(0));
        }
        channels[slot] = Some(sender);
        InputSubscription {
            registry: Arc::clone(self),
            slot,
            receiver,
        }
    }

    /// How many subscriptions are open.
    ///
    /// Read by this module's own tests, and by a diagnostics export that wants to say how
    /// many consumers there are rather than how many there were.
    #[allow(
        dead_code,
        reason = "read by this module's tests, and by a diagnostics export that does not exist yet"
    )]
    pub fn open(&self) -> usize {
        self.channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|channel| channel.is_some())
            .count()
    }

    /// Every event dropped across every subscription.
    ///
    /// The per-subscription count is what a consumer acts on; this is the one a whole
    /// product reports, and it exists so a run that dropped a keystroke can say so.
    #[allow(
        dead_code,
        reason = "read by this module's tests, and by a diagnostics export that does not exist yet"
    )]
    pub fn dropped_total(&self) -> u64 {
        self.dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|counter| counter.load(Ordering::Acquire))
            .sum()
    }

    fn count_drop(&self, slot: usize) {
        let dropped = self
            .dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(counter) = dropped.get(slot) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Hand one event to every open subscription.
    ///
    /// Never blocks and never reports failure: the caller is the platform layer's thread,
    /// and a subscriber that is gone or full is counted and skipped rather than being
    /// allowed to stall the input stack.
    pub fn publish(&self, event: &InputEvent) {
        let channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (slot, channel) in channels.iter().enumerate() {
            let Some(sender) = channel else {
                continue;
            };
            if let Err(TrySendError::Full(_)) = sender.try_send(event.clone()) {
                self.count_drop(slot);
            }
        }
    }

    fn close(&self, slot: usize) {
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = channels.get_mut(slot) {
            *entry = None;
        }
    }
}

/// Wraps the runtime's own submitter so a published event also reaches the subscribers.
///
/// The wrapper rather than a second call site in the runtime's consumption, because the
/// guarantee that matters is *order*: a subscriber that learned about an event from the
/// runtime's own dispatch would see it after the runtime had decided what to do with it,
/// and two consumers reading the same stream at different points in it is two orderings.
pub struct TappingSubmitter {
    inner: Arc<dyn crate::InputSubmitter>,
    subscribers: Arc<Subscribers>,
}

impl TappingSubmitter {
    pub fn new(inner: Arc<dyn crate::InputSubmitter>, subscribers: Arc<Subscribers>) -> Self {
        Self { inner, subscribers }
    }
}

impl crate::InputSubmitter for TappingSubmitter {
    fn submit(&self, event: SequencedInputEvent) -> Result<(), crate::InputSubmitError> {
        self.subscribers.publish(&event.event);
        self.inner.submit(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputControl, InputEdge, InputSource, MonotonicMillis, PhysicalKey};

    fn edge(usage: u16, edge: InputEdge) -> InputEvent {
        InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::from_hid_usage(usage)),
            edge,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        }
    }

    #[test]
    fn a_subscriber_sees_every_event_in_the_order_they_were_published() {
        let registry = Arc::new(Subscribers::default());
        let subscription = registry.subscribe();
        registry.publish(&edge(0x04, InputEdge::Down));
        registry.publish(&edge(0x04, InputEdge::Up));
        let events = subscription.drain(8);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            InputEvent::Edge {
                edge: InputEdge::Down,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            InputEvent::Edge {
                edge: InputEdge::Up,
                ..
            }
        ));
        assert_eq!(subscription.dropped(), 0);
    }

    #[test]
    fn nobody_who_did_not_ask_is_sent_anything() {
        let registry = Arc::new(Subscribers::default());
        assert_eq!(registry.open(), 0);
        registry.publish(&edge(0x04, InputEdge::Down));
        assert_eq!(
            registry.open(),
            0,
            "publishing to nobody is free and counts for nobody"
        );
        assert_eq!(registry.dropped_total(), 0);
    }

    #[test]
    fn a_consumer_that_stops_reading_loses_events_and_is_told_how_many() {
        // The whole reason the drop is counted: a tally that silently loses a keystroke is
        // a tally that reads low and cannot be believed.
        let registry = Arc::new(Subscribers::default());
        let subscription = registry.subscribe();
        for _ in 0..(SUBSCRIPTION_CAPACITY + 5) {
            registry.publish(&edge(0x04, InputEdge::Down));
        }
        assert_eq!(
            subscription.dropped(),
            5,
            "so a consumer whose tally is short can say by how much"
        );
        assert_eq!(
            subscription.drain(SUBSCRIPTION_CAPACITY).len(),
            SUBSCRIPTION_CAPACITY,
            "and the events that did fit are still there, oldest first"
        );
    }

    #[test]
    fn one_consumer_falling_behind_does_not_cost_another_one_an_event() {
        // The bound is per subscription, because a shared one would let a stalled consumer
        // silently blind a working one.
        let registry = Arc::new(Subscribers::default());
        let stalled = registry.subscribe();
        let keeping_up = registry.subscribe();
        let mut seen = 0;
        for _ in 0..(SUBSCRIPTION_CAPACITY + 5) {
            registry.publish(&edge(0x04, InputEdge::Down));
            seen += keeping_up.drain(8).len();
        }
        assert_eq!(stalled.dropped(), 5, "the one that stopped reading");
        assert_eq!(
            keeping_up.dropped(),
            0,
            "and a full queue is per subscription, because a shared bound would let a stalled \
             consumer silently blind a working one"
        );
        assert_eq!(
            seen,
            SUBSCRIPTION_CAPACITY + 5,
            "so the working one saw all of them"
        );
    }

    #[test]
    fn a_subscription_that_goes_away_stops_being_published_to() {
        let registry = Arc::new(Subscribers::default());
        let subscription = registry.subscribe();
        assert_eq!(registry.open(), 1);
        drop(subscription);
        assert_eq!(registry.open(), 0);
        // Publishing to a closed registry is still fine, and still costs nothing.
        registry.publish(&edge(0x04, InputEdge::Down));
    }

    #[test]
    fn closing_is_idempotent_because_a_consumer_may_close_twice() {
        let registry = Arc::new(Subscribers::default());
        let subscription = registry.subscribe();
        subscription.close();
        subscription.close();
        assert!(subscription.is_closed());
        assert!(
            subscription.take().is_none(),
            "and nothing arrives afterwards"
        );
    }
}
