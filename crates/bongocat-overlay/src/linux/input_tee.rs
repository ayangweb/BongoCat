//! A non-blocking copy of the local input stream, for the multiplayer publisher.
//!
//! The tee is an [`InputSubmitter`] that decorates the runtime's own submitter:
//! every event still goes to the runtime exactly as before, and a copy of the
//! ones a peer can draw is *also* pushed into a bounded channel that the network
//! thread drains.
//!
//! The copy must never be able to slow the original down. This call sits on the
//! path that drives the cat's animation, so a network stall that back-pressured
//! here would surface as a frozen model. The channel is therefore drained with
//! [`SyncSender::try_send`] and a full queue drops the frame: an input sample
//! that arrives late is worthless, so losing it is the correct answer and
//! waiting for the network is not.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{SyncSender, TrySendError},
};

use bongocat_input::{
    InputControl, InputEdge, InputEvent, InputSource, InputSubmitError, InputSubmitter,
    SequencedInputEvent,
};
use bongocat_net::protocol::InputFrame;

/// What the tee did with the events it was handed, for the publisher's
/// diagnostics: one count of what reached the wire, and one for each reason an
/// event did not. That is enough to tell a peer's "nothing happens" apart from
/// a stalled network, a silent filter, or a device reporting garbage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TeeSnapshot {
    forwarded: u64,
    dropped_queue_full: u64,
    dropped_sink_closed: u64,
    repeated_within_millisecond: u64,
    not_user_input: u64,
}

#[derive(Debug, Default)]
struct TeeCounters {
    forwarded: AtomicU64,
    dropped_queue_full: AtomicU64,
    dropped_sink_closed: AtomicU64,
    repeated_within_millisecond: AtomicU64,
    not_user_input: AtomicU64,
}

impl TeeCounters {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> TeeSnapshot {
        TeeSnapshot {
            forwarded: self.forwarded.load(Ordering::Relaxed),
            dropped_queue_full: self.dropped_queue_full.load(Ordering::Relaxed),
            dropped_sink_closed: self.dropped_sink_closed.load(Ordering::Relaxed),
            repeated_within_millisecond: self.repeated_within_millisecond.load(Ordering::Relaxed),
            not_user_input: self.not_user_input.load(Ordering::Relaxed),
        }
    }
}

/// Drops a repeat of the same edge inside one millisecond.
///
/// A held button reports nothing, but nothing stops a backend from reporting the
/// same edge twice inside one millisecond; and the day pointer motion reaches
/// this path, a 1000 Hz device produces eight times the frames a ~120 Hz
/// publisher can carry. Two events with the same control, the same edge and the
/// same millisecond describe the same state, so the second one adds nothing and
/// is the cheapest thing to lose.
///
/// The edge is part of the key on purpose: a press and a release that share a
/// millisecond are two different states, and swallowing the release would leave
/// the peer's cat holding a key down forever.
#[derive(Debug, Default)]
struct Coalescer {
    previous: Option<(InputControl, InputEdge, u64)>,
}

impl Coalescer {
    fn admit(&mut self, control: InputControl, edge: InputEdge, at: u64) -> bool {
        let key = (control, edge, at);
        if self.previous == Some(key) {
            return false;
        }
        self.previous = Some(key);
        true
    }
}

/// An [`InputSubmitter`] that copies user input to a bounded channel.
///
/// Submission to the runtime is unconditional and comes last, so the runtime
/// sees a full queue here exactly as it would have without the tee: the copy is
/// an observer of that path, never a gate on it.
pub struct TeeSubmitter {
    inner: Arc<dyn InputSubmitter>,
    sink: SyncSender<InputFrame>,
    /// Whether the local pet is drawing this machine's own input.
    ///
    /// A machine that switched "this machine" off in the device list still
    /// publishes (the tee's whole purpose), but its pet must not dance to the
    /// keys this user presses. When this flag is set the copy still reaches the
    /// sink and the runtime never sees the event; the room is the only audience.
    suppress_local: Arc<AtomicBool>,
    /// The tee's own sequence, not the runtime's. The runtime's sequence also
    /// counts resets, reconciliations and the events this tee filters out, so
    /// reusing it would report a gap for every frame that was never sent.
    sequence: AtomicU64,
    coalescer: Mutex<Coalescer>,
    counters: TeeCounters,
}

impl TeeSubmitter {
    pub fn new(
        inner: Arc<dyn InputSubmitter>,
        sink: SyncSender<InputFrame>,
        suppress_local: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner,
            sink,
            suppress_local,
            sequence: AtomicU64::new(0),
            coalescer: Mutex::new(Coalescer::default()),
            counters: TeeCounters::default(),
        }
    }

    // Read by the tests below to pin the filter behaviour. Production does
    // not read it yet — the diagnostics export would be its consumer — which
    // is why a non-test build sees this as dead code.
    #[cfg_attr(not(test), allow(dead_code))]
    fn diagnostics(&self) -> TeeSnapshot {
        self.counters.snapshot()
    }

    /// Offers one event to the sink. Never blocks, never fails the caller.
    fn offer(&self, event: &SequencedInputEvent) {
        let InputEvent::Edge {
            control,
            edge,
            source,
            at,
        } = &event.event
        else {
            // A reset or a reconciliation is transport bookkeeping between the
            // adapter and the runtime, not something a peer's cat can draw.
            TeeCounters::bump(&self.counters.not_user_input);
            return;
        };
        if *source != InputSource::Capture {
            // A reconciliation edge is the runtime re-deriving its own state
            // from a snapshot rather than the user pressing something.
            // Publishing it would let one device's replay bounce back and forth
            // between machines as if it were real input.
            TeeCounters::bump(&self.counters.not_user_input);
            return;
        }
        let at = at.value();
        let admitted = self
            .coalescer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admit(*control, *edge, at);
        if !admitted {
            TeeCounters::bump(&self.counters.repeated_within_millisecond);
            return;
        }
        let frame = InputFrame {
            // The relay stamps the sender's identity; the tee does not know
            // which device it is running on.
            src: String::new(),
            seq: self.sequence.fetch_add(1, Ordering::Relaxed),
            t: at,
            control: *control,
            // The edge is what makes the frame interpretable at the far end: a
            // receiver can only rebuild the held-key set if it knows whether each
            // control went down or came back up.
            edge: *edge,
            // Bounds describe the sending display, and no input event carries a
            // pointer position: motion lives in `CursorProducer`, sampled
            // separately. A frame built here therefore has nothing to fill in.
            bounds: None,
        };
        match self.sink.try_send(frame) {
            Ok(()) => TeeCounters::bump(&self.counters.forwarded),
            Err(TrySendError::Full(_)) => TeeCounters::bump(&self.counters.dropped_queue_full),
            Err(TrySendError::Disconnected(_)) => {
                TeeCounters::bump(&self.counters.dropped_sink_closed);
            }
        }
    }
}

impl InputSubmitter for TeeSubmitter {
    fn submit(&self, event: SequencedInputEvent) -> Result<(), InputSubmitError> {
        self.offer(&event);
        if self.suppress_local.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.submit(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_input::{InputResetReason, MonotonicMillis, PhysicalKey};
    use std::sync::mpsc;

    /// A stand-in for the runtime's submitter: records what reached it and
    /// answers `Ok`, so a test can tell "the tee dropped the copy" apart from
    /// "the tee swallowed the event".
    #[derive(Default)]
    struct Recorder {
        submitted: Mutex<Vec<SequencedInputEvent>>,
    }

    impl Recorder {
        fn count(&self) -> usize {
            self.submitted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len()
        }
    }

    impl InputSubmitter for Recorder {
        fn submit(&self, event: SequencedInputEvent) -> Result<(), InputSubmitError> {
            self.submitted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(event);
            Ok(())
        }
    }

    fn key_edge(edge: InputEdge, at: u64, sequence: u64) -> SequencedInputEvent {
        SequencedInputEvent {
            sequence,
            event: InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge,
                source: InputSource::Capture,
                at: MonotonicMillis::new(at),
            },
        }
    }

    fn tee(bound: usize) -> (TeeSubmitter, Arc<Recorder>, mpsc::Receiver<InputFrame>) {
        let recorder = Arc::new(Recorder::default());
        let (sender, receiver) = mpsc::sync_channel(bound);
        let inner: Arc<dyn InputSubmitter> = recorder.clone();
        (
            TeeSubmitter::new(inner, sender, Arc::new(AtomicBool::new(false))),
            recorder,
            receiver,
        )
    }

    #[test]
    fn an_edge_reaches_the_runtime_and_the_sink() {
        let (tee, recorder, receiver) = tee(4);
        tee.submit(key_edge(InputEdge::Down, 7, 0)).expect("submit");

        assert_eq!(recorder.count(), 1, "the runtime must still see the event");
        let frame = receiver.try_recv().expect("the copy must be queued");
        assert_eq!(frame.control, InputControl::Key(PhysicalKey::KEY_A));
        assert_eq!(frame.t, 7, "the frame carries the event's own timestamp");
        assert_eq!(frame.seq, 0);
        assert!(frame.src.is_empty(), "the publisher fills the sender id");
        assert!(frame.bounds.is_none(), "no input event carries a position");

        assert_eq!(tee.diagnostics().forwarded, 1);
    }

    #[test]
    fn a_full_sink_drops_the_copy_and_still_delivers_to_the_runtime() {
        // A rendezvous channel with no waiting receiver is always full, which is
        // the worst case a stalled network thread can present.
        let (tee, recorder, _receiver) = tee(0);
        for sequence in 0..3u64 {
            tee.submit(key_edge(InputEdge::Down, sequence, sequence))
                .expect("the runtime result must be unaffected by the copy");
        }

        assert_eq!(recorder.count(), 3, "a stalled sink must not stall input");
        let diagnostics = tee.diagnostics();
        assert_eq!(diagnostics.forwarded, 0);
        assert_eq!(diagnostics.dropped_queue_full, 3);
    }

    #[test]
    fn a_repeated_edge_inside_one_millisecond_is_sent_once() {
        let (tee, recorder, receiver) = tee(4);
        tee.submit(key_edge(InputEdge::Down, 9, 0)).expect("submit");
        tee.submit(key_edge(InputEdge::Down, 9, 1)).expect("submit");

        assert_eq!(recorder.count(), 2, "the runtime keeps both reports");
        assert_eq!(receiver.try_recv().expect("one copy").seq, 0);
        assert!(
            receiver.try_recv().is_err(),
            "the duplicate must not reach the wire"
        );
        assert_eq!(tee.diagnostics().repeated_within_millisecond, 1);

        // The same edge in a later millisecond is a new report, not a repeat.
        tee.submit(key_edge(InputEdge::Down, 10, 2))
            .expect("submit");
        assert_eq!(receiver.try_recv().expect("a later report").seq, 1);
    }

    #[test]
    fn a_quick_click_keeps_both_edges_even_inside_one_millisecond() {
        let (tee, _recorder, receiver) = tee(4);
        tee.submit(key_edge(InputEdge::Down, 5, 0)).expect("submit");
        tee.submit(key_edge(InputEdge::Up, 5, 1)).expect("submit");

        assert_eq!(receiver.try_recv().expect("press").seq, 0);
        assert_eq!(receiver.try_recv().expect("release").seq, 1);
    }

    #[test]
    fn bookkeeping_events_never_reach_the_wire() {
        let (tee, recorder, receiver) = tee(4);
        tee.submit(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::Reset {
                reason: InputResetReason::DeviceRemoved,
                at: MonotonicMillis::new(1),
            },
        })
        .expect("submit");

        assert_eq!(recorder.count(), 1, "the runtime must still see the reset");
        assert!(receiver.try_recv().is_err());
        assert_eq!(tee.diagnostics().not_user_input, 1);
    }
}
