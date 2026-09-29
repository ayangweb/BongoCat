//! The small set of things a plugin can actually do over time.
//!
//! A plugin that is data has no loops, so everything that changes on its own has
//! to be a behavior the host already knows how to run. The set is deliberately
//! closed and small: a countdown, a stopwatch, a clock, a counter. Between them
//! they cover the display half of a focus timer, an agent status readout and a
//! countdown to anything, which is the whole of what the model window is for.
//!
//! A future `api_version` may add a behavior. It may not change one, because a
//! plugin's own state is reachable from its scene and a changed meaning would
//! quietly redraw every panel that used it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The most behaviors one plugin may declare.
///
/// Behaviors are evaluated on a worker thread, not on the frame path, but a
/// plugin that declares hundreds is either doing in a loop what a panel should
/// show once or is not a panel at all.
pub const MAXIMUM_BEHAVIORS_PER_PLUGIN: usize = 32;

/// The longest a timer may run, in seconds.
///
/// Twenty-four hours is past every honest use of a countdown and well inside
/// what a monotonic millisecond counter and a `u64` hold, so no behavior can be
/// made to overflow its own state by declaring a large duration.
pub const MAXIMUM_TIMER_SECONDS: u32 = 86_400;

/// Identifies a behavior within its own plugin.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BehaviorId(String);

impl BehaviorId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BehaviorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A behavior a plugin declares.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BehaviorSpec {
    /// Counts down from a duration and stops at zero.
    Countdown(CountdownSpec),
    /// Counts up from zero.
    Stopwatch(StopwatchSpec),
    /// Reads the host's local wall clock and formats it.
    LocalTime(LocalTimeSpec),
    /// A number a button moves.
    Counter(CounterSpec),
}

impl BehaviorSpec {
    /// The kind as it appears in a plugin file, for messages that name it.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Countdown(_) => "countdown",
            Self::Stopwatch(_) => "stopwatch",
            Self::LocalTime(_) => "local_time",
            Self::Counter(_) => "counter",
        }
    }

    /// Whether a behavior of this kind can hold a value that changes on its own.
    ///
    /// This is the weaker half of the answer, and the only half a manifest can
    /// give: it says a counter never changes by itself and a clock always does.
    /// Whether a *given instance* is currently moving — a countdown that is
    /// stopped, a stopwatch that was never started — is state, not declaration,
    /// and the host answers that when it evaluates. Keeping the two apart is why
    /// this takes a spec and not a running behavior.
    pub const fn is_clock_driven(&self) -> bool {
        match self {
            // A countdown is still time-driven while it runs; only `auto_repeat`
            // makes it reach a *new* value with no press at all.
            Self::Countdown(spec) => spec.auto_repeat,
            Self::Stopwatch(_) => true,
            Self::LocalTime(_) => true,
            Self::Counter(counter) => counter.loop_back,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CountdownSpec {
    /// How long one run lasts.
    pub duration_seconds: u32,
    /// Start as soon as the plugin is enabled.
    #[serde(default)]
    pub auto_start: bool,
    /// Return to the full duration when a run reaches zero, instead of stopping
    /// there. A focus timer wants this; a countdown to a deadline does not.
    #[serde(default)]
    pub auto_repeat: bool,
}

impl Default for CountdownSpec {
    fn default() -> Self {
        Self {
            duration_seconds: 1_500,
            auto_start: false,
            auto_repeat: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StopwatchSpec {
    #[serde(default)]
    pub auto_start: bool,
    /// After this long, the elapsed value wraps back to zero.
    ///
    /// Without it a stopwatch grows without bound and its `progress` binding has
    /// nothing to be a fraction of. The wrap is what makes the binding useful,
    /// and it is capped like every other timer.
    #[serde(default)]
    pub period_seconds: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalTimeSpec {
    /// A subset of the host's time format: `HH`, `MM`, `SS` and the literal
    /// separators between them. Anything else is refused, because a plugin
    /// asking the host to format a date in a locale it does not name would
    /// produce a string nobody chose.
    #[serde(default = "default_time_format")]
    pub format: String,
}

fn default_time_format() -> String {
    "HH:MM".to_string()
}

impl Default for LocalTimeSpec {
    fn default() -> Self {
        Self {
            format: default_time_format(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CounterSpec {
    #[serde(default)]
    pub initial: i64,
    #[serde(default)]
    pub minimum: i64,
    #[serde(default = "one")]
    pub step: i64,
    #[serde(default = "max_i64")]
    pub maximum: i64,
    /// Wrap from the maximum back to the minimum rather than stopping there.
    #[serde(default)]
    pub loop_back: bool,
}

const fn one() -> i64 {
    1
}

const fn max_i64() -> i64 {
    i64::MAX
}

impl Default for CounterSpec {
    fn default() -> Self {
        Self {
            initial: 0,
            minimum: 0,
            step: 1,
            maximum: 1_000,
            loop_back: false,
        }
    }
}

/// What a press on a button runs.
///
/// A closed set on purpose. An open "call this named function" would need a
/// plugin to ship something callable, which is the code execution the whole
/// design avoids; a closed set of verbs over behaviors the host already runs
/// covers the same panels with none of that.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorAction {
    #[default]
    Toggle,
    Start,
    Pause,
    Reset,
    /// Move a counter by its step, in `target`'s direction.
    Increment,
    Decrement,
}

impl BehaviorAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Reset => "reset",
            Self::Increment => "increment",
            Self::Decrement => "decrement",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Toggle,
            Self::Start,
            Self::Pause,
            Self::Reset,
            Self::Increment,
            Self::Decrement,
        ]
        .into_iter()
        .find(|action| action.as_str() == value)
    }

    /// Whether the action can be refused because the behavior is not in a state
    /// that admits it.
    pub const fn changes_a_number(self) -> bool {
        matches!(self, Self::Increment | Self::Decrement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_kinds_round_trip_through_their_written_names() {
        let spec: BehaviorSpec =
            serde_json::from_str(r#"{"kind":"countdown","duration_seconds":60}"#).unwrap();
        assert_eq!(spec.kind(), "countdown");
        let spec: BehaviorSpec = serde_json::from_str(r#"{"kind":"counter"}"#).unwrap();
        assert_eq!(spec.kind(), "counter");
    }

    #[test]
    fn a_countdown_needs_a_duration_and_the_rest_have_defaults() {
        assert!(serde_json::from_str::<BehaviorSpec>(r#"{"kind":"countdown"}"#).is_err());
        let spec: BehaviorSpec =
            serde_json::from_str(r#"{"kind":"countdown","duration_seconds":1500}"#).unwrap();
        assert!(!spec.is_clock_driven());
    }

    #[test]
    fn a_repeating_countdown_is_clock_driven_even_when_it_starts_stopped() {
        let spec = CountdownSpec {
            auto_repeat: true,
            ..CountdownSpec::default()
        };
        assert!(BehaviorSpec::Countdown(spec).is_clock_driven());
    }

    #[test]
    fn a_counter_only_needs_a_clock_when_it_wraps() {
        assert!(!BehaviorSpec::Counter(CounterSpec::default()).is_clock_driven());
        assert!(
            BehaviorSpec::Counter(CounterSpec {
                loop_back: true,
                ..CounterSpec::default()
            })
            .is_clock_driven()
        );
    }

    #[test]
    fn a_clock_always_needs_one() {
        assert!(BehaviorSpec::LocalTime(LocalTimeSpec::default()).is_clock_driven());
        assert!(BehaviorSpec::Stopwatch(StopwatchSpec::default()).is_clock_driven());
    }

    #[test]
    fn a_clock_defaults_to_hours_and_minutes() {
        let spec: LocalTimeSpec = serde_json::from_str("{}").unwrap();
        assert_eq!(spec.format, "HH:MM");
    }

    #[test]
    fn actions_round_trip_and_reject_anything_else() {
        for action in [
            BehaviorAction::Toggle,
            BehaviorAction::Start,
            BehaviorAction::Pause,
            BehaviorAction::Reset,
            BehaviorAction::Increment,
            BehaviorAction::Decrement,
        ] {
            assert_eq!(BehaviorAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(BehaviorAction::parse("explode"), None);
    }

    #[test]
    fn only_the_counter_moves_a_number() {
        assert!(BehaviorAction::Increment.changes_a_number());
        assert!(!BehaviorAction::Toggle.changes_a_number());
    }
}
