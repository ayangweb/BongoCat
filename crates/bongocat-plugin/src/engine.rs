//! Running a plugin's behaviors and turning their values into a scene.
//!
//! This is the whole of a plugin's runtime, and it is small on purpose. A plugin
//! declares four kinds of behavior; this file owns one running instance of each,
//! advances them on a monotonic clock, applies the six actions a button can ask
//! for, and writes the resulting values into a binding table the scene reads.
//!
//! Two properties are worth stating because the rest of the system relies on them:
//!
//! * **Nothing here touches the filesystem, the network, or a clock it was not
//!   given.** [`PluginInstance::evaluate`] takes the elapsed time and the wall
//!   clock as arguments, so a test drives a whole plugin's life deterministically
//!   and the product has no hidden time source that could disagree with the
//!   runtime's.
//! * **Every bound value is written every evaluation.** A binding table is rebuilt
//!   rather than patched, so a behavior that was removed cannot leave a stale
//!   value behind for a scene that still binds to it.
//!
//! # Why a behavior is one type and not two
//!
//! A spec says what a behavior *is* and a state says where it *has got to*, and
//! the two are always the same kind: a countdown's state is a countdown's state.
//! Keeping them as one enum means every `match` over them is a match over four
//! kinds rather than over sixteen pairs, nine of which cannot exist. That is the
//! difference between a `match` the compiler checks for completeness and one that
//! needs a wildcard arm that hides a real case.

use bongocat_plugin_protocol::{
    BehaviorAction, BehaviorId, BehaviorSpec, BindingTable, BindingValue, CountdownSpec,
    CounterSpec, LocalTimeSpec, MAXIMUM_TIMER_SECONDS, NamedBehavior, PluginError, PluginErrorCode,
    StopwatchSpec,
};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

/// One running behavior: what it is, and where it has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum Behavior {
    Countdown {
        spec: CountdownSpec,
        /// Seconds left in the current run.
        remaining_seconds: u32,
        running: bool,
        /// How many runs have finished. A panel binds to this for "sets
        /// completed" without the plugin having to count anything itself.
        completed_runs: u64,
    },
    Stopwatch {
        spec: StopwatchSpec,
        elapsed_seconds: u32,
        running: bool,
    },
    LocalTime {
        spec: LocalTimeSpec,
    },
    Counter {
        spec: CounterSpec,
        value: i64,
    },
}

/// How far a countdown has got.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CountdownState {
    pub remaining_seconds: u32,
    pub running: bool,
    pub completed_runs: u64,
}

/// How far a stopwatch has got.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StopwatchState {
    pub elapsed_seconds: u32,
    pub running: bool,
}

/// Where a counter is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CounterState {
    pub value: i64,
}

impl Behavior {
    /// A behavior at its starting position.
    ///
    /// Built from an already-clamped spec, so the initial value needs no clamping
    /// of its own: a countdown declared with a zero duration is already a
    /// one-second timer by the time it reaches here.
    fn start(spec: BehaviorSpec) -> Self {
        match spec {
            BehaviorSpec::Countdown(spec) => {
                let remaining_seconds = spec.duration_seconds;
                let running = spec.auto_start;
                Self::Countdown {
                    spec,
                    remaining_seconds,
                    running,
                    completed_runs: 0,
                }
            }
            BehaviorSpec::Stopwatch(spec) => {
                let running = spec.auto_start;
                Self::Stopwatch {
                    spec,
                    elapsed_seconds: 0,
                    running,
                }
            }
            BehaviorSpec::LocalTime(spec) => Self::LocalTime { spec },
            BehaviorSpec::Counter(spec) => {
                let value = spec.initial.clamp(spec.minimum, spec.maximum);
                Self::Counter { spec, value }
            }
        }
    }

    /// The starting position, as the plain state a caller inspecting this behavior
    /// would want.
    pub fn state(&self) -> BehaviorState {
        match self {
            Self::Countdown {
                remaining_seconds,
                running,
                completed_runs,
                ..
            } => BehaviorState::Countdown(CountdownState {
                remaining_seconds: *remaining_seconds,
                running: *running,
                completed_runs: *completed_runs,
            }),
            Self::Stopwatch {
                elapsed_seconds,
                running,
                ..
            } => BehaviorState::Stopwatch(StopwatchState {
                elapsed_seconds: *elapsed_seconds,
                running: *running,
            }),
            Self::LocalTime { .. } => BehaviorState::LocalTime,
            Self::Counter { value, .. } => BehaviorState::Counter(CounterState { value: *value }),
        }
    }

    /// The declaration this behavior was built from.
    pub fn spec(&self) -> BehaviorSpec {
        match self {
            Self::Countdown { spec, .. } => BehaviorSpec::Countdown(spec.clone()),
            Self::Stopwatch { spec, .. } => BehaviorSpec::Stopwatch(spec.clone()),
            Self::LocalTime { spec } => BehaviorSpec::LocalTime(spec.clone()),
            Self::Counter { spec, .. } => BehaviorSpec::Counter(spec.clone()),
        }
    }

    /// Whether this behavior can change without a press.
    ///
    /// The declaration's half of the answer. A countdown that is *running* also
    /// changes without a press, and that is state rather than declaration, so a
    /// caller deciding how often to evaluate must ask both — which is what
    /// [`PluginInstance::is_clock_driven`] does.
    pub fn is_clock_driven(&self) -> bool {
        match self {
            Self::Countdown { spec, .. } => spec.auto_repeat,
            Self::Stopwatch { .. } | Self::LocalTime { .. } => true,
            Self::Counter { spec, .. } => spec.loop_back,
        }
    }

    /// Whether this behavior's value is moving right now.
    pub fn is_moving(&self) -> bool {
        match self {
            Self::Countdown { running, .. } | Self::Stopwatch { running, .. } => *running,
            Self::LocalTime { .. } | Self::Counter { .. } => false,
        }
    }

    /// Apply one action.
    fn apply(&mut self, action: BehaviorAction) {
        match self {
            Self::Countdown {
                spec,
                remaining_seconds,
                running,
                ..
            } => match action {
                BehaviorAction::Start => *running = true,
                BehaviorAction::Pause => *running = false,
                BehaviorAction::Toggle => *running = !*running,
                BehaviorAction::Reset => {
                    *remaining_seconds = spec.duration_seconds;
                    *running = false;
                }
                // A countdown has no number to move; a counter is the behavior that
                // does. Ignoring it is the same answer as a clock's.
                BehaviorAction::Increment | BehaviorAction::Decrement => {}
            },
            Self::Stopwatch {
                elapsed_seconds,
                running,
                ..
            } => match action {
                BehaviorAction::Start => *running = true,
                BehaviorAction::Pause => *running = false,
                BehaviorAction::Toggle => *running = !*running,
                BehaviorAction::Reset => {
                    *elapsed_seconds = 0;
                    *running = false;
                }
                BehaviorAction::Increment | BehaviorAction::Decrement => {}
            },
            Self::LocalTime { .. } => {
                // A clock has nothing to act on. `toggle` on one is a plugin
                // author's mistake rather than a thing to fail over.
            }
            Self::Counter { spec, value } => match action {
                BehaviorAction::Increment | BehaviorAction::Decrement => {
                    let step = if action == BehaviorAction::Decrement {
                        -spec.step
                    } else {
                        spec.step
                    };
                    let next = value.saturating_add(step);
                    *value = if spec.loop_back {
                        wrap(next, spec.minimum, spec.maximum)
                    } else {
                        next.clamp(spec.minimum, spec.maximum)
                    };
                }
                // A counter has no running state, so every action that names one
                // returns it to where it started. That is the one reading a plugin
                // author could reasonably have meant, and it is idempotent, which
                // `start` and `pause` are not.
                BehaviorAction::Start
                | BehaviorAction::Pause
                | BehaviorAction::Toggle
                | BehaviorAction::Reset => {
                    *value = spec.initial.clamp(spec.minimum, spec.maximum);
                }
            },
        }
    }

    /// Advance by `whole` whole seconds.
    fn advance(&mut self, whole: u32) {
        if whole == 0 {
            return;
        }
        match self {
            Self::Countdown {
                spec,
                remaining_seconds,
                running,
                completed_runs,
            } => {
                if !*running {
                    return;
                }
                if *remaining_seconds > whole {
                    *remaining_seconds -= whole;
                    return;
                }
                // The run reached zero inside this interval. A repeating countdown
                // starts again and carries the remainder forward, so a 25-minute
                // timer on a 30-second tick stays 25 minutes instead of drifting
                // by one tick per run.
                *completed_runs = completed_runs.saturating_add(1);
                if spec.auto_repeat {
                    let duration = spec.duration_seconds.max(1);
                    *remaining_seconds = duration - ((whole - *remaining_seconds) % duration);
                } else {
                    *remaining_seconds = 0;
                    *running = false;
                }
            }
            Self::Stopwatch {
                spec,
                elapsed_seconds,
                running,
            } => {
                if !*running {
                    return;
                }
                let advanced = elapsed_seconds.saturating_add(whole);
                // A stopwatch with a period wraps rather than growing without
                // bound, which is what makes its `progress` binding a proportion
                // at all.
                *elapsed_seconds = match spec.period_seconds {
                    Some(period) if period > 0 => advanced % period,
                    Some(_) => advanced,
                    None => advanced.min(MAXIMUM_TIMER_SECONDS),
                };
            }
            Self::LocalTime { .. } | Self::Counter { .. } => {}
        }
    }

    /// Write this behavior's values, prefixed with its id.
    fn write_bindings(&self, id: &BehaviorId, table: &mut BindingTable, clock: WallClock) {
        let prefix = id.as_str();
        match self {
            Self::Countdown {
                spec,
                remaining_seconds,
                running,
                completed_runs,
            } => {
                table.set(
                    format!("{prefix}.remaining_seconds"),
                    BindingValue::Text(remaining_seconds.to_string()),
                );
                // Two spellings, because a panel showing `25:00` and a panel showing
                // a large number are both reasonable and the plugin should not have
                // to format either itself.
                table.set(
                    format!("{prefix}.remaining_text"),
                    BindingValue::Text(format_duration(*remaining_seconds)),
                );
                let duration = spec.duration_seconds.max(1) as f32;
                table.set(
                    format!("{prefix}.progress"),
                    BindingValue::Fraction(
                        (1.0 - *remaining_seconds as f32 / duration).clamp(0.0, 1.0),
                    ),
                );
                table.set(format!("{prefix}.running"), BindingValue::Flag(*running));
                table.set(
                    format!("{prefix}.completed_runs"),
                    BindingValue::Text(completed_runs.to_string()),
                );
            }
            Self::Stopwatch {
                spec,
                elapsed_seconds,
                running,
            } => {
                table.set(
                    format!("{prefix}.elapsed_seconds"),
                    BindingValue::Text(elapsed_seconds.to_string()),
                );
                table.set(
                    format!("{prefix}.elapsed_text"),
                    BindingValue::Text(format_duration(*elapsed_seconds)),
                );
                let period = spec.period_seconds.unwrap_or(MAXIMUM_TIMER_SECONDS).max(1);
                table.set(
                    format!("{prefix}.progress"),
                    BindingValue::Fraction(
                        (*elapsed_seconds as f32 / period as f32).clamp(0.0, 1.0),
                    ),
                );
                table.set(format!("{prefix}.running"), BindingValue::Flag(*running));
            }
            Self::LocalTime { spec } => {
                table.set(
                    format!("{prefix}.text"),
                    BindingValue::Text(format_time(&spec.format, clock)),
                );
            }
            Self::Counter { value, .. } => {
                table.set(
                    format!("{prefix}.value"),
                    BindingValue::Text(value.to_string()),
                );
            }
        }
    }
}

/// A behavior's position, without its declaration.
///
/// What a caller inspecting a running plugin wants: where a countdown has got to,
/// not what a countdown is.
#[derive(Clone, Debug, PartialEq)]
pub enum BehaviorState {
    Countdown(CountdownState),
    Stopwatch(StopwatchState),
    LocalTime,
    Counter(CounterState),
}

/// The wall clock a `local_time` behavior formats.
///
/// A struct rather than a bare value so the source of the time is named at every
/// call site: the product supplies it, and a test supplies a fixed one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WallClock {
    /// Hours since midnight, 0..=23.
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl WallClock {
    /// A reading, with each field clamped into range.
    pub fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
            second: second.min(59),
        }
    }
}

/// One plugin's running behaviors.
#[derive(Clone, Debug)]
pub struct PluginInstance {
    id: bongocat_plugin_protocol::PluginId,
    behaviors: Vec<(BehaviorId, Behavior)>,
}

impl PluginInstance {
    /// Build an instance from a validated manifest.
    ///
    /// Behaviors are clamped here rather than at each use, so every consumer can
    /// assume a behavior's values are in range. A countdown that starts at zero
    /// because its author wrote `"duration_seconds": 0` is a one-second timer, not
    /// a panel that refuses to load.
    pub fn new(
        id: bongocat_plugin_protocol::PluginId,
        behaviors: &[NamedBehavior],
    ) -> Result<Self, PluginError> {
        let clamped = behaviors
            .iter()
            .map(|named| {
                let mut spec = named.spec.clone();
                bongocat_plugin_protocol::clamp_behavior_spec(&mut spec);
                (named.id.clone(), Behavior::start(spec))
            })
            .collect();
        Ok(Self {
            id,
            behaviors: clamped,
        })
    }

    pub fn id(&self) -> &bongocat_plugin_protocol::PluginId {
        &self.id
    }

    /// Every behavior this instance runs, in declaration order.
    pub fn behaviors(&self) -> impl Iterator<Item = (&BehaviorId, &Behavior)> {
        self.behaviors.iter().map(|(id, behavior)| (id, behavior))
    }

    /// One behavior by name.
    pub fn behavior(&self, id: &BehaviorId) -> Option<&Behavior> {
        self.behaviors
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, behavior)| behavior)
    }

    /// Whether anything in this instance can change without a press.
    ///
    /// The answer decides how often the host has to evaluate: a panel of nothing
    /// but stopped counters produces the same picture every time, so re-evaluating
    /// it sixty times a second would be sixty identical rasters.
    ///
    /// Both halves are needed. The declaration covers a repeating countdown that
    /// has not started; the state covers one that has.
    pub fn is_clock_driven(&self) -> bool {
        self.behaviors
            .iter()
            .any(|(_, behavior)| behavior.is_clock_driven() || behavior.is_moving())
    }

    /// Apply one action to one behavior.
    ///
    /// An action naming a behavior that is not declared is ignored rather than
    /// refused: the manifest was validated against the same list, so reaching this
    /// means a press arrived for a panel that has since changed, and a stale press
    /// is not a reason to take a plugin down.
    pub fn apply(&mut self, target: &BehaviorId, action: BehaviorAction) {
        if let Some((_, behavior)) = self
            .behaviors
            .iter_mut()
            .find(|(candidate, _)| candidate == target)
        {
            behavior.apply(action);
        }
    }

    /// Advance every behavior by `elapsed`, and write the values a scene binds to.
    pub fn evaluate(&mut self, elapsed: Duration, clock: WallClock) -> BindingTable {
        let whole = u32::try_from(elapsed.as_secs()).unwrap_or(u32::MAX);
        for (_, behavior) in &mut self.behaviors {
            behavior.advance(whole);
        }
        self.bindings(clock)
    }

    /// Write the values a scene binds to, without advancing anything.
    pub fn bindings(&self, clock: WallClock) -> BindingTable {
        let mut table = BindingTable::new();
        for (id, behavior) in &self.behaviors {
            behavior.write_bindings(id, &mut table, clock);
        }
        table
    }
}

impl fmt::Display for BehaviorState {
    /// The kind's name, for a log line.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Countdown(state) => write!(formatter, "countdown {}", state.remaining_seconds),
            Self::Stopwatch(state) => write!(formatter, "stopwatch {}", state.elapsed_seconds),
            Self::LocalTime => formatter.write_str("local_time"),
            Self::Counter(state) => write!(formatter, "counter {}", state.value),
        }
    }
}

/// Bring a value back inside a range, wrapping by the range's own width.
///
/// `minimum` and `maximum` have already been ordered, and a counter's range cannot
/// span more than `i64::MAX`, so the width is representable.
fn wrap(value: i64, minimum: i64, maximum: i64) -> i64 {
    let span = maximum - minimum;
    if span == i64::MAX {
        return value.clamp(minimum, maximum);
    }
    minimum + value.rem_euclid(span + 1)
}

/// `mm:ss` below an hour, `h:mm:ss` at or above it.
///
/// A panel almost always wants a clock face, and a countdown over 99 minutes
/// written as `99:99` is wrong, so the hour is added exactly when it is needed.
pub fn format_duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, remainder)
    } else {
        format!("{minutes:02}:{remainder:02}")
    }
}

/// Format a wall-clock reading with `HH`, `MM`, `SS` and literal separators.
pub fn format_time(format: &str, clock: WallClock) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    // `str::starts_with` plus slicing, rather than a byte walk: a format is ASCII by
    // validation, but this stays correct for a non-ASCII separator, which `len_utf8`
    // handling would otherwise break on.
    while !rest.is_empty() {
        let matched = ["HH", "MM", "SS"]
            .into_iter()
            .find(|field| rest.starts_with(field));
        match matched {
            Some("HH") => out.push_str(&format!("{:02}", clock.hour)),
            Some("MM") => out.push_str(&format!("{:02}", clock.minute)),
            Some("SS") => out.push_str(&format!("{:02}", clock.second)),
            Some(_) => {}
            None => {
                let character = rest.chars().next().expect("rest is not empty");
                out.push(character);
                rest = &rest[character.len_utf8()..];
                continue;
            }
        }
        rest = &rest[2..];
    }
    out
}

/// Check that every binding a scene names resolves to a declared behavior.
///
/// Kept here rather than in the protocol crate because it needs the running
/// instance's ids, and because refusing a plugin is a host decision.
pub fn validate_bindings(
    behaviors: &[NamedBehavior],
    bindings: &[String],
) -> Result<(), PluginError> {
    let sources: BTreeMap<&str, ()> = behaviors
        .iter()
        .map(|named| (named.id.as_str(), ()))
        .collect();
    for binding in bindings {
        let Some((source, _)) = binding.split_once('.') else {
            return Err(PluginError::new(PluginErrorCode::InvalidBinding));
        };
        // The host's own paths are a fixed list, checked by the caller, which
        // knows whether the host is running.
        if source == crate::HOST_PREFIX {
            continue;
        }
        if !sources.contains_key(source) {
            return Err(PluginError::new(PluginErrorCode::UnknownBinding));
        }
    }
    Ok(())
}
