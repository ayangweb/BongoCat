//! What a plugin said, kept where the product can read it.
//!
//! A plugin's diagnostics are forwarded rather than interpreted. The SDK already
//! bounds a log line, and the session's reader already bounds what arrives on stderr,
//! so by the time a line reaches this module it is text with a level attached and
//! nothing more to decide.
//!
//! It goes into a **bounded, in-memory ring** rather than into the product's log file,
//! and that is a deliberate choice worth stating. The product's log is a closed
//! vocabulary of event codes with a fixed message per code — it is a thing a
//! diagnostics report and a bug report read, not a stream anything may write to. A
//! plugin's arbitrary text would have to be either a new code per plugin or an
//! escape hatch, and both weaken a log the product reads by machine. So the lines are
//! held here, where the plugin center and a diagnostics export can show them, and the
//! run's own log records only the fact that a plugin logged at all.

use bongocat_plugin_protocol::{LogLevel, PluginId};
use std::collections::VecDeque;
use std::sync::Mutex;

/// How many lines are kept per plugin.
///
/// Bounded because a plugin in a loop will produce lines until something stops it,
/// and this is held in memory for the whole run. A hundred is enough to diagnose a
/// plugin that is failing and small enough that four plugins cannot matter.
pub const CAPACITY: usize = 100;

/// The longest a kept line may be, in characters.
///
/// Bounded here as well as in the SDK, because the ring is the last place a line can
/// grow and a line that reached it from a plugin's own stderr never passed through
/// the SDK's bound.
pub const MAXIMUM_CHARS: usize = 500;

/// One line a plugin wrote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Line {
    /// Which plugin wrote it.
    pub id: PluginId,
    pub level: LogLevel,
    /// The text, bounded and prefixed with the plugin's id so a reader knows who said
    /// it without consulting the id beside it.
    pub message: String,
}

impl Line {
    /// Whether this line is one a user would want to see.
    ///
    /// A debug line is a plugin talking to itself. A warning or an error is a plugin
    /// telling the user something went wrong, and those are the ones the plugin center
    /// shows — a page that listed every internal line would be unreadable and would
    /// train the user to ignore it.
    pub fn is_user_visible(&self) -> bool {
        matches!(self.level, LogLevel::Warn | LogLevel::Error)
    }
}

// The ring is **thread-local**, which is what lets a line be written on a session's
// reader thread and read back on the worker's without a lock or an owner — and it is
// only true because the reader hands its stderr to the worker rather than recording it
// itself. The worker thread is the one the plugin center reads back.
thread_local! {
    static LOGS: Ring = Ring::new();
}

/// Record one line on this thread's ring.
pub fn record(id: &PluginId, level: LogLevel, message: &str) {
    LOGS.with(|logs| logs.record(id, level, message));
}

/// One plugin's lines, oldest first.
pub fn lines_for(id: &PluginId) -> Vec<Line> {
    LOGS.with(|logs| logs.lines_for(id))
}

/// Forget one plugin's lines, for a plugin that has been removed.
pub fn forget(id: &PluginId) {
    LOGS.with(|logs| logs.forget(id));
}

/// Every kept line, oldest first.
#[derive(Debug)]
pub struct Ring {
    lines: Mutex<VecDeque<Line>>,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(VecDeque::with_capacity(CAPACITY)),
        }
    }

    /// Record one line.
    ///
    /// Returns whether it was kept. A ring past the capacity drops the *oldest* rather
    /// than the newest: the recent lines are the ones that explain the failure that has
    /// not happened yet.
    pub fn record(&self, id: &PluginId, level: LogLevel, message: &str) -> bool {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if lines.len() >= CAPACITY {
            lines.pop_front();
        }
        lines.push_back(Line {
            id: id.clone(),
            level,
            message: format!(
                "plugin {id}: {}",
                message.chars().take(MAXIMUM_CHARS).collect::<String>()
            ),
        });
        true
    }

    /// Every line kept, oldest first.
    pub fn lines(&self) -> Vec<Line> {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// How many lines are kept.
    #[allow(dead_code, reason = "read by the tests that pin the bound")]
    pub fn len(&self) -> usize {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    #[allow(dead_code, reason = "read by the tests that pin the bound")]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The lines one plugin wrote, oldest first.
    pub fn lines_for(&self, id: &PluginId) -> Vec<Line> {
        self.lines()
            .into_iter()
            .filter(|line| &line.id == id)
            .collect()
    }

    /// The lines worth showing a user, oldest first.
    #[allow(dead_code, reason = "read by the plugin center once a card shows them")]
    pub fn user_visible(&self) -> Vec<Line> {
        self.lines()
            .into_iter()
            .filter(Line::is_user_visible)
            .collect()
    }

    /// Forget everything one plugin wrote, for a plugin that has been removed.
    ///
    /// Its lines are about a plugin the user can no longer see, and a card explaining
    /// an absent plugin with its last words is a bug report, not a feature.
    pub fn forget(&self, id: &PluginId) {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|line| &line.id != id);
    }

    /// Forget everything.
    #[allow(dead_code, reason = "read by the tests that pin the bound")]
    pub fn clear(&self) {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> PluginId {
        PluginId::new("key-stats").expect("valid")
    }

    #[test]
    fn a_line_is_kept_with_its_plugin_attached() {
        let ring = Ring::new();
        assert!(ring.record(&id(), LogLevel::Info, "started"));
        let lines = ring.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].id, id());
        assert_eq!(lines[0].level, LogLevel::Info);
        assert_eq!(
            lines[0].message, "plugin key-stats: started",
            "and the message names its plugin, so a reader knows who said it without consulting \
             the id beside it"
        );
    }

    #[test]
    fn a_warning_or_an_error_is_shown_and_a_debug_line_is_not() {
        // A page that listed every internal line would be unreadable and would train
        // the user to ignore it.
        let ring = Ring::new();
        for level in [
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            ring.record(&id(), level, "line");
        }
        let visible = ring.user_visible();
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().all(Line::is_user_visible));
    }

    #[test]
    fn a_line_is_bounded_because_the_last_place_it_can_grow_is_here() {
        let ring = Ring::new();
        ring.record(&id(), LogLevel::Info, &"x".repeat(MAXIMUM_CHARS * 2));
        let message = &ring.lines()[0].message;
        assert_eq!(
            message.chars().count(),
            MAXIMUM_CHARS + "plugin key-stats: ".chars().count(),
            "and the bound is here as well as in the SDK, because a line from a plugin's own stderr \
             never passed through the SDK's"
        );
    }

    #[test]
    fn the_ring_drops_the_oldest_line_when_it_is_full() {
        // The recent lines are the ones that explain the failure that has not happened
        // yet, so the oldest goes rather than the newest.
        let ring = Ring::new();
        for index in 0..(CAPACITY + 5) {
            ring.record(&id(), LogLevel::Info, &format!("line {index}"));
        }
        let lines = ring.lines();
        assert_eq!(
            lines.len(),
            CAPACITY,
            "and it is bounded, because this is held for the run"
        );
        assert_eq!(lines[0].message, "plugin key-stats: line 5");
        assert!(
            lines
                .last()
                .expect("a last line")
                .message
                .ends_with(&format!("line {}", CAPACITY + 4))
        );
    }

    #[test]
    fn one_plugins_lines_are_readable_on_their_own() {
        let ring = Ring::new();
        let other = PluginId::new("typing-sound").expect("valid");
        ring.record(&id(), LogLevel::Warn, "a");
        ring.record(&other, LogLevel::Warn, "b");
        ring.record(&id(), LogLevel::Warn, "c");
        let mine = ring.lines_for(&id());
        assert_eq!(mine.len(), 2);
        assert!(mine[0].message.ends_with('a'));
        assert!(mine[1].message.ends_with('c'));
    }

    #[test]
    fn a_removed_plugins_lines_are_forgotten_rather_than_explaining_an_absent_card() {
        // A card explaining a plugin the user can no longer see is a bug report.
        let ring = Ring::new();
        let other = PluginId::new("typing-sound").expect("valid");
        ring.record(&id(), LogLevel::Error, "mine");
        ring.record(&other, LogLevel::Error, "theirs");
        ring.forget(&id());
        assert!(ring.lines_for(&id()).is_empty());
        assert_eq!(ring.lines_for(&other).len(), 1);
    }

    #[test]
    fn an_empty_ring_reports_itself_empty() {
        let ring = Ring::new();
        assert!(ring.is_empty());
        assert!(ring.lines().is_empty());
        ring.record(&id(), LogLevel::Info, "x");
        assert!(!ring.is_empty());
        ring.clear();
        assert!(ring.is_empty());
    }
}
