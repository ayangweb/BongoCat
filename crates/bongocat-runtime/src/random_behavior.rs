use bongocat_model::ModelBehaviorSnapshot;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

/// The default delay between automatic behavior selections.
pub const DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 30;
/// Automatic behavior selection must never become a per-frame operation.
pub const MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 1;
/// A practical bound for the persisted settings value.
pub const MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 3_600;

/// Runtime-owned settings for periodic model behavior selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RandomBehaviorSettings {
    pub enabled: bool,
    pub interval_seconds: u32,
}

impl Default for RandomBehaviorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        }
    }
}

impl RandomBehaviorSettings {
    pub const fn is_valid(self) -> bool {
        self.interval_seconds >= MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS
            && self.interval_seconds <= MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS
    }

    const fn interval(self) -> Duration {
        Duration::from_secs(self.interval_seconds as u64)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RandomBehaviorScheduler {
    settings: RandomBehaviorSettings,
    next_due: Option<Duration>,
    last_seen: Option<Duration>,
    random: XorShift64,
}

impl RandomBehaviorScheduler {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            settings: RandomBehaviorSettings::default(),
            next_due: None,
            last_seen: None,
            random: XorShift64::new(seed),
        }
    }

    /// Apply a settings change and re-anchor the schedule. A clock rollback is
    /// ignored for scheduling purposes: it must not make a deadline appear
    /// closer or replay a missed automatic selection.
    pub(crate) fn set_settings(&mut self, settings: RandomBehaviorSettings, now: Duration) {
        let anchor = self.effective_now(now);
        let changed = self.settings != settings;
        self.settings = settings;
        if !settings.enabled {
            self.next_due = None;
        } else if changed || self.next_due.is_none() {
            self.next_due = Some(deadline(anchor, settings.interval()));
        }
    }

    /// A successful model commit starts a fresh full interval. The scheduler is
    /// deliberately not tied to model identity so a failed prepare cannot make
    /// the old model run a new random action.
    pub(crate) fn reset(&mut self, now: Duration) {
        if self.settings.enabled {
            let anchor = self.effective_now(now);
            self.next_due = Some(deadline(anchor, self.settings.interval()));
        } else {
            self.next_due = None;
        }
    }

    /// Return one due behavior, if the active model has one. A long pause causes
    /// one selection and re-anchors from the current monotonic time; it never
    /// produces a catch-up burst.
    pub(crate) fn poll<F>(&mut self, now: Duration, behaviors: F) -> Option<ModelBehaviorSnapshot>
    where
        F: FnOnce() -> Vec<ModelBehaviorSnapshot>,
    {
        if !self.settings.enabled {
            return None;
        }
        let anchor = self.advance_clock(now)?;
        let due = self.next_due?;
        if anchor < due {
            return None;
        }
        self.next_due = Some(deadline(anchor, self.settings.interval()));
        let behaviors = behaviors();
        let index = self.random.index(behaviors.len());
        behaviors.get(index).cloned()
    }

    fn effective_now(&mut self, now: Duration) -> Duration {
        self.advance_clock(now).or(self.last_seen).unwrap_or(now)
    }

    /// Observe a monotonic timestamp and return the effective timestamp. The
    /// `None` result is a clock rollback, which is ignored until the clock
    /// catches up to the last observed value.
    fn advance_clock(&mut self, now: Duration) -> Option<Duration> {
        if let Some(last_seen) = self.last_seen
            && now < last_seen
        {
            return None;
        }
        self.last_seen = Some(now);
        Some(now)
    }
}

fn deadline(now: Duration, interval: Duration) -> Duration {
    now.checked_add(interval)
        .unwrap_or_else(|| Duration::from_secs(u64::MAX))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    fn index(&mut self, length: usize) -> usize {
        if length <= 1 {
            return 0;
        }
        let length = u64::try_from(length).unwrap_or(u64::MAX);
        // Reject the short tail instead of taking a modulo of the whole u64
        // range. This keeps the selection uniform even when the model exposes
        // a list whose length does not divide 2^64.
        let limit = u64::MAX - (u64::MAX % length);
        loop {
            let value = self.next();
            if value < limit {
                return usize::try_from(value % length).unwrap_or(0);
            }
        }
    }
}

/// A per-process seed. The scheduler itself is deterministic when a test seed
/// is supplied; this seed only prevents two application processes started in
/// the same clock tick from following the same sequence.
pub(crate) fn system_seed() -> u64 {
    let mut hasher = DefaultHasher::new();
    std::process::id().hash(&mut hasher);
    Instant::now().hash(&mut hasher);
    hasher.finish() | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn behaviors() -> Vec<ModelBehaviorSnapshot> {
        vec![
            ModelBehaviorSnapshot::Motion {
                group: "idle".to_owned(),
                index: 0,
            },
            ModelBehaviorSnapshot::Expression {
                name: "happy.exp3.json".to_owned(),
            },
            ModelBehaviorSnapshot::Motion {
                group: "tap".to_owned(),
                index: 1,
            },
        ]
    }

    #[test]
    fn settings_reject_zero_and_overlarge_intervals() {
        assert!(
            !RandomBehaviorSettings {
                enabled: false,
                interval_seconds: 0,
            }
            .is_valid()
        );
        assert!(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
            }
            .is_valid()
        );
        assert!(
            !RandomBehaviorSettings {
                enabled: true,
                interval_seconds: MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS + 1,
            }
            .is_valid()
        );
    }

    #[test]
    fn a_seed_produces_a_repeatable_sequence_and_waits_one_interval() {
        let settings = RandomBehaviorSettings {
            enabled: true,
            interval_seconds: 2,
        };
        let mut first = RandomBehaviorScheduler::new(7);
        let mut second = RandomBehaviorScheduler::new(7);
        first.set_settings(settings, Duration::ZERO);
        second.set_settings(settings, Duration::ZERO);
        let behaviors = behaviors();
        assert!(
            first
                .poll(Duration::from_secs(1), || behaviors.clone())
                .is_none()
        );
        let first_selection = first.poll(Duration::from_secs(2), || behaviors.clone());
        let second_selection = second.poll(Duration::from_secs(2), || behaviors.clone());
        assert_eq!(first_selection, second_selection);
        assert!(first_selection.is_some());
        assert!(
            first
                .poll(Duration::from_secs(3), || behaviors.clone())
                .is_none()
        );
    }

    #[test]
    fn disable_and_model_reset_clear_the_pending_deadline() {
        let mut scheduler = RandomBehaviorScheduler::new(11);
        scheduler.set_settings(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 10,
            },
            Duration::ZERO,
        );
        scheduler.reset(Duration::from_secs(20));
        assert!(scheduler.poll(Duration::from_secs(29), behaviors).is_none());
        scheduler.set_settings(
            RandomBehaviorSettings {
                enabled: false,
                interval_seconds: 10,
            },
            Duration::from_secs(29),
        );
        scheduler.set_settings(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 10,
            },
            Duration::from_secs(29),
        );
        assert!(scheduler.poll(Duration::from_secs(38), behaviors).is_none());
    }

    #[test]
    fn a_clock_rollback_does_not_replay_a_due_selection() {
        let mut scheduler = RandomBehaviorScheduler::new(13);
        scheduler.set_settings(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 1,
            },
            Duration::from_secs(10),
        );
        assert!(scheduler.poll(Duration::from_secs(11), behaviors).is_some());
        assert!(scheduler.poll(Duration::from_secs(5), behaviors).is_none());
        assert!(scheduler.poll(Duration::from_secs(12), behaviors).is_some());
        assert!(scheduler.poll(Duration::from_secs(13), behaviors).is_some());
    }
}
