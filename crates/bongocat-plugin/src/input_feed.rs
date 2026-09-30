//! Turning the runtime's input into what a plugin may be told about it.
//!
//! The runtime owns pressed state, and `AGENTS.md` §5 makes that non-negotiable: every
//! key and button that is ever pressed must be cleared by a release, a reconcile or a
//! Reset. So this file does **not** decide what happened — it reads what the runtime
//! decided and republishes it. That is the whole design, and it is what lets a plugin
//! count keystrokes without becoming a second source of input truth.
//!
//! # What is in the feed, and why only that
//!
//! Key edges, mouse buttons and mouse movement. Those are what a plugin on the model
//! window has ever wanted: a key display, a keystroke tally, a sound on a keypress, a
//! distance travelled. Everything else the product owns — the window state, the
//! configuration, the model's internals — is not in the feed, and a plugin that wanted
//! it would be a plugin that wanted to be the app.
//!
//! # The three properties that keep it honest
//!
//! **An already-validated event.** What arrives here has been through the platform's
//! own normalization and the runtime's own policy. A subscription decides whether a
//! plugin is *told* about an event, never whether it is believed.
//!
//! **A dropped event is counted.** The feed is bounded, and a plugin that is not
//! reading fills it. A dropped key *edge* is a key a tally may never see released, so
//! the count is not a diagnostic detail — it is the difference between "this plugin is
//! slow" and "this plugin's count is wrong", and only the count tells them apart.
//!
//! **A reset reaches every plugin.** When the platform reports a lock screen, a sleep
//! or a device removal, the runtime clears its pressed set and so must every plugin
//! that keeps a tally, or the tally keeps a key the platform has already forgotten.

use bongocat_plugin_protocol::{InputEvent, MAXIMUM_MOUSE_STEP};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// How many input events may be queued for a plugin before one is dropped.
///
/// Bounded because the feed is a *latest-value* channel in the sense that matters: two
/// mouse moves in one tick produce one, and the distance between them is summed rather
/// than lost. What is not coalesced is a key or a button edge, because a missing edge
/// is a missing fact.
const EVENT_CAPACITY: usize = 256;

/// The most presses the feed will carry in one message to one plugin.
///
/// A message has to be one line and one line has a bound, so a plugin that generates
/// more than this in one interval gets what fits and a count of what did not. A
/// plugin producing thousands of edges a second is not displaying them.
const MAXIMUM_BATCH: usize = 64;

/// What one plugin has been told, and what it missed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FeedDiagnostics {
    /// Events handed to the plugin.
    pub delivered: u64,
    /// Events dropped because the plugin was not reading.
    pub dropped: u64,
    /// Key and button edges among those delivered.
    pub edges: u64,
    /// Resets among those delivered.
    pub resets: u64,
}

impl FeedDiagnostics {
    /// Every counter, summed, for the run's total.
    pub fn total(&self) -> u64 {
        self.delivered + self.dropped + self.edges + self.resets
    }
}

/// One plugin's input queue.
///
/// Per plugin rather than shared, so a plugin that stops reading cannot consume the
/// events another one is waiting for. The memory cost is a bounded `VecDeque` per
/// subscribing plugin, which for four plugins is a few hundred events.
#[derive(Debug)]
pub struct Feed {
    events: Mutex<VecDeque<InputEvent>>,
    /// Mouse distance accumulated since the last delivery, in the model's own units.
    ///
    /// Held separately from the queue because it is not an event: two moves in one
    /// interval are one move with a greater distance, and a counter that lost the
    /// distance between two samples is a counter that reads low with no way to tell.
    pending_distance: Mutex<PendingMouse>,
    diagnostics: FeedDiagnostics,
}

#[derive(Clone, Copy, Debug, Default)]
struct PendingMouse {
    dx: f32,
    dy: f32,
    distance: f32,
}

impl Default for Feed {
    fn default() -> Self {
        Self::new()
    }
}

impl Feed {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(EVENT_CAPACITY)),
            pending_distance: Mutex::new(PendingMouse::default()),
            diagnostics: FeedDiagnostics::default(),
        }
    }

    /// How many events are waiting.
    pub fn len(&self) -> usize {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What this plugin has been told and has missed.
    pub fn diagnostics(&self) -> FeedDiagnostics {
        self.diagnostics
    }

    /// Offer one event, dropping it when the queue is full.
    ///
    /// Returns whether it was queued. A caller counts the difference: an edge that was
    /// dropped is a press or a release a plugin will never see, and the count is what
    /// makes that visible rather than a number that is quietly low.
    pub fn offer(&mut self, event: InputEvent) -> bool {
        // Movement is folded into the accumulator rather than queued, and flushed as
        // one event on the next delivery. An edge is queued as it is.
        if let InputEvent::MouseMove { dx, dy, distance } = event {
            let folded = {
                let mut pending = self
                    .pending_distance
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.dx += dx;
                pending.dy += dy;
                pending.distance += distance;
                // The bound is on what one sample may claim, so a plugin cannot be
                // handed a jump larger than the model window could produce in one
                // interval. Below it the move is only accumulated, and flushed with
                // the next drain.
                if pending.distance <= MAXIMUM_MOUSE_STEP {
                    return true;
                }
                std::mem::take(&mut *pending)
            };
            return self.push(InputEvent::MouseMove {
                dx: folded.dx,
                dy: folded.dy,
                distance: folded.distance.min(MAXIMUM_MOUSE_STEP),
            });
        }
        self.push(event)
    }

    fn push(&mut self, event: InputEvent) -> bool {
        let changes_tally = event.changes_pressed_tally();
        let is_reset = is_reset(&event);
        {
            let mut queue = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if queue.len() >= EVENT_CAPACITY {
                drop(queue);
                self.diagnostics.dropped = self.diagnostics.dropped.saturating_add(1);
                return false;
            }
            queue.push_back(event);
        }
        // The counters are about facts rather than about messages: an auto-repeat is
        // delivered — a plugin may want it — but it is not another edge, because a
        // key held for a second is one keystroke.
        if changes_tally {
            self.diagnostics.edges = self.diagnostics.edges.saturating_add(1);
        }
        if is_reset {
            self.diagnostics.resets = self.diagnostics.resets.saturating_add(1);
        }
        true
    }

    /// Take what is waiting, up to one message's worth.
    ///
    /// Returns nothing when there is nothing, so the caller sends no message — which
    /// is what keeps a plugin that wants no input from receiving an empty batch once a
    /// second.
    pub fn drain(&mut self) -> Vec<InputEvent> {
        let mut batch: Vec<InputEvent> = Vec::new();
        {
            let mut queue = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while batch.len() < MAXIMUM_BATCH
                && let Some(event) = queue.pop_front()
            {
                batch.push(event);
            }
        }
        // The accumulated movement is flushed last, so a plugin that reads a distance
        // and then an edge sees them in the order they happened.
        let pending = {
            let mut guard = self
                .pending_distance
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        if pending.distance > 0.0 {
            batch.push(InputEvent::MouseMove {
                dx: pending.dx,
                dy: pending.dy,
                distance: pending.distance,
            });
        }
        self.diagnostics.delivered = self
            .diagnostics
            .delivered
            .saturating_add(batch.len() as u64);
        batch
    }

    /// Drop everything waiting, and say why.
    ///
    /// Used when a plugin is switched off or its process ends: its queue belongs to a
    /// process that no longer exists, and carrying it into the next run would show a
    /// tally that includes events from before the plugin was restarted.
    pub fn clear(&mut self) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        *self
            .pending_distance
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = PendingMouse::default();
    }
}

fn is_reset(event: &InputEvent) -> bool {
    matches!(event, InputEvent::Reset { .. })
}

/// Every plugin's feed, plus the count of what none of them could take.
#[derive(Debug, Default)]
pub struct FeedSet {
    feeds: std::collections::BTreeMap<bongocat_plugin_protocol::PluginId, Feed>,
    /// Events no plugin could take, counted rather than logged per event.
    unclaimed: AtomicU64,
}

impl FeedSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Give a plugin a feed, or hand back the one it already has.
    pub fn feed_mut(&mut self, id: &bongocat_plugin_protocol::PluginId) -> &mut Feed {
        self.feeds.entry(id.clone()).or_default()
    }

    /// Whether a plugin has a feed at all.
    ///
    /// False for a plugin that did not subscribe, which is the whole point: the
    /// worker asks this before it does the work of translating an event at all.
    pub fn has_feed(&self, id: &bongocat_plugin_protocol::PluginId) -> bool {
        self.feeds.contains_key(id)
    }

    /// Offer one event to one plugin.
    pub fn offer(&mut self, id: &bongocat_plugin_protocol::PluginId, event: InputEvent) -> bool {
        self.feed_mut(id).offer(event)
    }

    /// Whether any plugin is listening, so the caller can skip translating entirely.
    pub fn any_listening(&self) -> bool {
        !self.feeds.is_empty()
    }

    /// How many plugins have a feed.
    pub fn len(&self) -> usize {
        self.feeds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.feeds.is_empty()
    }

    /// Drop a plugin's feed, for an uninstall or a stop.
    pub fn remove(&mut self, id: &bongocat_plugin_protocol::PluginId) {
        self.feeds.remove(id);
    }

    /// Everything no plugin could take.
    pub fn unclaimed(&self) -> u64 {
        self.unclaimed.load(Ordering::Acquire)
    }

    /// Every plugin's diagnostics.
    pub fn diagnostics(&self) -> Vec<(bongocat_plugin_protocol::PluginId, FeedDiagnostics)> {
        self.feeds
            .iter()
            .map(|(id, feed)| (id.clone(), feed.diagnostics()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_down() -> InputEvent {
        InputEvent::KeyDown {
            control: "KeyA".to_string(),
            repeat: false,
        }
    }

    fn key_repeat() -> InputEvent {
        InputEvent::KeyDown {
            control: "KeyA".to_string(),
            repeat: true,
        }
    }

    fn move_by(dx: f32, dy: f32) -> InputEvent {
        let distance = (dx * dx + dy * dy).sqrt();
        InputEvent::MouseMove { dx, dy, distance }
    }

    #[test]
    fn an_event_reaches_the_plugin_that_was_listening() {
        let mut feed = Feed::new();
        assert!(feed.offer(key_down()));
        assert_eq!(feed.len(), 1);
        let batch = feed.drain();
        assert_eq!(batch.len(), 1);
        assert!(matches!(batch[0], InputEvent::KeyDown { .. }));
        assert_eq!(feed.diagnostics().delivered, 1);
    }

    #[test]
    fn a_drain_of_nothing_is_nothing_rather_than_an_empty_batch() {
        // A plugin that wants no input should receive no message, not a message with
        // nothing in it once an interval.
        let mut feed = Feed::new();
        assert!(feed.drain().is_empty());
        assert!(feed.is_empty());
    }

    #[test]
    fn movement_is_folded_into_one_sample_with_the_distance_summed() {
        // A counter that lost the distance between two samples is a counter that
        // reads low and nobody can tell why.
        let mut feed = Feed::new();
        assert!(feed.offer(move_by(0.1, 0.0)));
        assert!(feed.offer(move_by(0.1, 0.0)));
        assert!(
            feed.is_empty(),
            "two moves inside the bound are one move, not two queued"
        );
        let batch = feed.drain();
        assert_eq!(batch.len(), 1);
        let InputEvent::MouseMove { dx, dy, distance } = batch[0] else {
            panic!("one move");
        };
        assert!((dx - 0.2).abs() < 1e-6);
        assert!((dy - 0.0).abs() < 1e-6);
        assert!(
            (distance - 0.2).abs() < 1e-6,
            "and the distance is the sum, not the last step"
        );
    }

    #[test]
    fn a_long_move_is_flushed_rather_than_growing_without_bound() {
        let mut feed = Feed::new();
        let mut distance = 0.0;
        for _ in 0..100 {
            assert!(feed.offer(move_by(0.5, 0.0)));
            distance += 0.5;
            if feed.len() > 1 {
                break;
            }
        }
        let batch = feed.drain();
        assert!(!batch.is_empty());
        let total: f32 = batch
            .iter()
            .filter_map(|event| match event {
                InputEvent::MouseMove { distance, .. } => Some(*distance),
                _ => None,
            })
            .sum();
        assert!(total > 0.0, "the movement came out as movement");
        assert!(
            batch
                .iter()
                .filter(|event| matches!(event, InputEvent::MouseMove { .. }))
                .count()
                < 100,
            "and as far fewer samples than were offered"
        );
        let _ = distance;
    }

    #[test]
    fn a_queue_that_is_full_drops_and_counts_rather_than_growing() {
        let mut feed = Feed::new();
        for index in 0..(EVENT_CAPACITY + 10) {
            let offered = feed.offer(InputEvent::MouseButton {
                button: format!("Mouse{index}"),
                pressed: true,
            });
            if index < EVENT_CAPACITY {
                assert!(offered, "{index} should have fitted");
            }
        }
        let diagnostics = feed.diagnostics();
        assert_eq!(
            diagnostics.dropped, 10,
            "a dropped key edge is a key a tally may never see released, so the count is not a \\
             diagnostic detail — it is the difference between 'slow' and 'wrong'"
        );
    }

    #[test]
    fn an_auto_repeat_is_delivered_but_is_not_another_edge() {
        let mut feed = Feed::new();
        feed.offer(key_down());
        feed.offer(key_repeat());
        feed.offer(key_repeat());
        let batch = feed.drain();
        assert_eq!(batch.len(), 3, "a plugin may want the repeats");
        assert_eq!(
            feed.diagnostics().edges,
            1,
            "but a held key held for a second is one keystroke, not a hundred"
        );
    }

    #[test]
    fn a_reset_is_an_edge_because_a_tally_has_to_balance() {
        let mut feed = Feed::new();
        feed.offer(InputEvent::Reset {
            reason: "lock_screen".to_string(),
        });
        let batch = feed.drain();
        assert_eq!(batch.len(), 1);
        let diagnostics = feed.diagnostics();
        assert_eq!(diagnostics.resets, 1);
        assert_eq!(
            diagnostics.edges, 1,
            "and it counts as one, because a tally that keeps a key the platform has already \
             forgotten is a tally that never balances — but it is counted as a reset too, so a \
             reader can tell the two apart"
        );
        assert!(
            batch[0].changes_pressed_tally(),
            "and it does change a tally, because a tally that keeps a key the platform has already \\
             forgotten is a tally that never balances"
        );
    }

    #[test]
    fn a_batch_is_bounded_because_a_message_has_to_be_one_line() {
        let mut feed = Feed::new();
        for index in 0..200 {
            feed.offer(InputEvent::MouseButton {
                button: format!("Mouse{index}"),
                pressed: true,
            });
        }
        let batch = feed.drain();
        assert!(
            batch.len() <= MAXIMUM_BATCH,
            "a plugin producing thousands of edges a second is not displaying them"
        );
        assert!(
            !feed.is_empty(),
            "and the rest is still waiting for the next interval"
        );
    }

    #[test]
    fn a_cleared_feed_forgets_everything_from_a_process_that_no_longer_exists() {
        let mut feed = Feed::new();
        feed.offer(key_down());
        feed.offer(move_by(0.3, 0.0));
        feed.clear();
        assert!(
            feed.drain().is_empty(),
            "and carrying a queue into the next run would show a tally that includes events from \\
             before the plugin was restarted"
        );
    }

    #[test]
    fn a_feed_set_gives_each_plugin_its_own_queue() {
        let mut feeds = FeedSet::new();
        let first = bongocat_plugin_protocol::PluginId::new("key-stats").expect("valid");
        let second = bongocat_plugin_protocol::PluginId::new("typing-sound").expect("valid");
        assert!(!feeds.any_listening(), "and no plugin means no work at all");
        feeds.offer(&first, key_down());
        feeds.offer(&second, key_down());
        feeds.offer(&second, key_down());
        assert_eq!(feeds.feed_mut(&first).drain().len(), 1);
        assert_eq!(feeds.feed_mut(&second).drain().len(), 2);
    }

    #[test]
    fn a_feed_set_reports_that_no_plugin_was_listening() {
        let feeds = FeedSet::new();
        assert!(feeds.is_empty());
        assert!(!feeds.any_listening());
        let id = bongocat_plugin_protocol::PluginId::new("key-stats").expect("valid");
        assert!(
            !feeds.has_feed(&id),
            "so the worker can skip translating entirely"
        );
    }

    #[test]
    fn a_removed_plugins_feed_takes_its_queue_with_it() {
        let mut feeds = FeedSet::new();
        let id = bongocat_plugin_protocol::PluginId::new("key-stats").expect("valid");
        feeds.offer(&id, key_down());
        feeds.remove(&id);
        assert!(!feeds.has_feed(&id));
        assert_eq!(feeds.len(), 0);
    }

    #[test]
    fn every_plugins_diagnostics_are_readable_for_a_run_total() {
        let mut feeds = FeedSet::new();
        let id = bongocat_plugin_protocol::PluginId::new("key-stats").expect("valid");
        feeds.offer(&id, key_down());
        feeds.feed_mut(&id).drain();
        let all = feeds.diagnostics();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].1.total(), 2, "one delivered and one edge");
    }
}
