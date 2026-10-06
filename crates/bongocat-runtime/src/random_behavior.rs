use bongocat_model::{ModelBehaviorSnapshot, ModelId, ModelOrigin};
use std::{
    collections::{BTreeSet, hash_map::DefaultHasher},
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
    pub mode: RandomBehaviorMode,
    pub interval_seconds: u32,
}

impl Default for RandomBehaviorSettings {
    fn default() -> Self {
        Self {
            mode: RandomBehaviorMode::default(),
            interval_seconds: DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        }
    }
}

/// What the idle scheduler may pick on its own, or that it does nothing.
///
/// The runtime owns this vocabulary rather than borrowing the persisted one: a
/// mode is only meaningful next to the model behaviors it filters, and the filter
/// below is the whole point of the value. A model that declares none of the
/// selected kind leaves the scheduler idle — it does not fall back to the other
/// kind, because the mode is the user's decision about what plays, not a ranking
/// between two sets that are both available.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RandomBehaviorMode {
    #[default]
    Off,
    Expressions,
    Motions,
    MotionsAndExpressions,
}

impl RandomBehaviorMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Expressions,
        Self::Motions,
        Self::MotionsAndExpressions,
    ];

    /// Whether this mode schedules anything at all.
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }

    const fn admits(self, behavior: &ModelBehaviorSnapshot) -> bool {
        matches!(
            (self, behavior),
            (Self::Expressions, ModelBehaviorSnapshot::Expression { .. })
                | (Self::Motions, ModelBehaviorSnapshot::Motion { .. })
                | (Self::MotionsAndExpressions, _)
        )
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

/// The behaviors one model plays on its own, or every behavior it declares.
///
/// The model identity travels with the set rather than being assumed by the
/// receiver, because the selection follows a model rather than the application:
/// switching models has to change what may play, and a set that named no model
/// could not say whether it still applies. The runtime compares it against the
/// model actually in effect and ignores a set that belongs to another one, so a
/// selection that arrives late cannot filter the wrong model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RandomBehaviorInclusion {
    pub model: ModelId,
    pub model_origin: ModelOrigin,
    pub behavior_ids: BTreeSet<String>,
}

impl RandomBehaviorInclusion {
    /// Whether this selection is the one belonging to the model now in effect.
    pub fn belongs_to(&self, model: &ModelId, origin: ModelOrigin) -> bool {
        self.model == *model && self.model_origin == origin
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
        if !settings.mode.is_active() {
            self.next_due = None;
        } else if changed || self.next_due.is_none() {
            self.next_due = Some(deadline(anchor, settings.interval()));
        }
    }

    /// A successful model commit starts a fresh full interval. The scheduler is
    /// deliberately not tied to model identity so a failed prepare cannot make
    /// the old model run a new random action.
    pub(crate) fn reset(&mut self, now: Duration) {
        if self.settings.mode.is_active() {
            let anchor = self.effective_now(now);
            self.next_due = Some(deadline(anchor, self.settings.interval()));
        } else {
            self.next_due = None;
        }
    }

    /// Return one due behavior, if the active model has one the mode allows and
    /// the user selected. A long pause causes one selection and re-anchors from
    /// the current monotonic time; it never produces a catch-up burst.
    ///
    /// `included` is the active model's own selection, or `None` when it has none.
    /// The caller resolves that against the model actually in effect, so this stays
    /// a pure filter over what it is handed: an empty set means the model selected
    /// nothing and therefore has nothing to play, which is a state the settings page
    /// spells out rather than one that falls back to "everything".
    pub(crate) fn poll<F>(
        &mut self,
        now: Duration,
        included: Option<&BTreeSet<String>>,
        behaviors: F,
    ) -> Option<ModelBehaviorSnapshot>
    where
        F: FnOnce() -> Vec<ModelBehaviorSnapshot>,
    {
        if !self.settings.mode.is_active() {
            return None;
        }
        let anchor = self.advance_clock(now)?;
        let due = self.next_due?;
        if anchor < due {
            return None;
        }
        self.next_due = Some(deadline(anchor, self.settings.interval()));
        let behaviors = behaviors();
        // The mode and the selection narrow the candidate set *before* the draw, so
        // a selection is uniform over the behaviors the user allowed rather than
        // over everything the model happens to declare. The passes scan one
        // identifier list and build no second copy, which is what keeps this on a
        // per-tick path; `mode` is hoisted so the draw can still borrow the
        // generator.
        let mode = self.settings.mode;
        let mut candidates = behaviors
            .iter()
            .filter(|behavior| mode.admits(behavior))
            .filter(|behavior| match included {
                Some(included) => included.contains(&behavior_id(behavior)),
                None => true,
            });
        let rank = self.random.index(candidates.clone().count());
        candidates.nth(rank).cloned()
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

/// The canonical spelling one behavior is selected and bound under.
///
/// It has to be the same string the configuration persists, because the selection
/// is compared against what the model declares rather than against an index: the
/// order of a package's declarations is not part of its contract, so an index would
/// make a selection mean a different behavior after a model update.
fn behavior_id(behavior: &ModelBehaviorSnapshot) -> String {
    match behavior {
        ModelBehaviorSnapshot::Motion { group, index } => format!("motion:{group}:{index}"),
        ModelBehaviorSnapshot::Expression { name } => format!("expression:{name}"),
    }
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

    /// Settings for one interval, so a test states the mode it is about and
    /// leaves the interval at the one value every scheduling test uses.
    fn settings(mode: RandomBehaviorMode, interval_seconds: u32) -> RandomBehaviorSettings {
        RandomBehaviorSettings {
            mode,
            interval_seconds,
        }
    }

    /// Every selection a scheduler makes across a whole run, one poll per
    /// interval. A mode test cannot assert on a single draw — the point is that
    /// *no* draw ever leaves the allowed set, over any number of them.
    fn drawn(
        seed: u64,
        mode: RandomBehaviorMode,
        draws: usize,
        behaviors: &[ModelBehaviorSnapshot],
    ) -> Vec<ModelBehaviorSnapshot> {
        drawn_with(seed, mode, draws, None, behaviors)
    }

    /// The same drive with a per-behavior selection in force, so a mode test and a
    /// selection test read the same way and neither has to restate the scheduler.
    fn drawn_with(
        seed: u64,
        mode: RandomBehaviorMode,
        draws: usize,
        included: Option<&BTreeSet<String>>,
        behaviors: &[ModelBehaviorSnapshot],
    ) -> Vec<ModelBehaviorSnapshot> {
        let mut scheduler = RandomBehaviorScheduler::new(seed);
        scheduler.set_settings(settings(mode, 1), Duration::ZERO);
        (0..draws)
            .filter_map(|index| {
                scheduler.poll(Duration::from_secs(index as u64 + 1), included, || {
                    behaviors.to_vec()
                })
            })
            .collect()
    }

    /// A selection over the fixture behaviors, named the way the configuration
    /// names them.
    fn selected(behavior_ids: &[&str]) -> BTreeSet<String> {
        behavior_ids
            .iter()
            .map(|behavior_id| (*behavior_id).to_owned())
            .collect()
    }

    /// The kind a selection belongs to, as a one-word name. A mode constrains the
    /// *category* an automatic selection comes from, not which member of that
    /// category is drawn, so the assertion is written in these terms.
    fn kind(behavior: &ModelBehaviorSnapshot) -> &'static str {
        match behavior {
            ModelBehaviorSnapshot::Motion { .. } => "motion",
            ModelBehaviorSnapshot::Expression { .. } => "expression",
        }
    }

    #[test]
    fn settings_reject_zero_and_overlarge_intervals() {
        assert!(!settings(RandomBehaviorMode::default(), 0).is_valid());
        assert!(
            settings(
                RandomBehaviorMode::default(),
                MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS
            )
            .is_valid()
        );
        assert!(
            !settings(
                RandomBehaviorMode::default(),
                MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS + 1
            )
            .is_valid()
        );
        // The interval is still validated while the mode is off, so a
        // configuration that parks an out-of-range value is rejected rather than
        // stored and discovered when the user finally turns the mode on.
        assert!(!settings(RandomBehaviorMode::Off, 0).is_valid());
        assert!(settings(RandomBehaviorMode::Off, 30).is_valid());
    }

    #[test]
    fn the_off_mode_never_schedules_anything() {
        let behaviors = behaviors();
        for seed in 1..=24u64 {
            let mut scheduler = RandomBehaviorScheduler::new(seed);
            scheduler.set_settings(settings(RandomBehaviorMode::Off, 1), Duration::ZERO);
            for second in 1..=24u64 {
                assert!(
                    scheduler
                        .poll(Duration::from_secs(second), None, || behaviors.clone())
                        .is_none(),
                    "the off mode selected a behavior at {second}s"
                );
            }
        }
    }

    #[test]
    fn each_mode_never_selects_outside_its_own_kind() {
        let behaviors = behaviors();
        // A single seed per mode is not enough: one draw from a list of three can
        // land on the allowed entry by luck. Each mode is driven with a spread of
        // seeds, and every single selection has to hold.
        for seed in 1..=24u64 {
            for (mode, allowed) in [
                (RandomBehaviorMode::Expressions, "expression"),
                (RandomBehaviorMode::Motions, "motion"),
            ] {
                let selection = drawn(seed, mode, 12, &behaviors);
                assert!(
                    selection.iter().all(|behavior| kind(behavior) == allowed),
                    "{mode:?} seed {seed} selected outside its own kind: {selection:?}"
                );
            }
        }
    }

    #[test]
    fn the_combined_mode_reaches_both_kinds() {
        let behaviors = behaviors();
        let mut seen = Vec::new();
        for seed in 1..=24u64 {
            seen.extend(drawn(
                seed,
                RandomBehaviorMode::MotionsAndExpressions,
                12,
                &behaviors,
            ));
        }
        let kinds: BTreeSet<&str> = seen.iter().map(kind).collect();
        assert_eq!(
            kinds,
            BTreeSet::from(["motion", "expression"]),
            "the combined mode must still reach both kinds"
        );
    }

    #[test]
    fn a_model_without_the_selected_kind_stays_idle() {
        // One expression, no motion at all: the expression-only and combined modes
        // still answer, and the motion-only mode has nothing to draw rather than
        // silently playing an expression the user excluded.
        let expressions = vec![ModelBehaviorSnapshot::Expression {
            name: "happy.exp3.json".to_owned(),
        }];
        for seed in 1..=24u64 {
            assert!(drawn(seed, RandomBehaviorMode::Motions, 12, &expressions).is_empty());
            assert!(
                drawn(seed, RandomBehaviorMode::Expressions, 12, &expressions).len() == 12,
                "the declared expression must remain selectable"
            );
        }
    }

    #[test]
    fn a_seed_produces_a_repeatable_sequence_and_waits_one_interval() {
        let settings = settings(RandomBehaviorMode::MotionsAndExpressions, 2);
        let mut first = RandomBehaviorScheduler::new(7);
        let mut second = RandomBehaviorScheduler::new(7);
        first.set_settings(settings, Duration::ZERO);
        second.set_settings(settings, Duration::ZERO);
        let behaviors = behaviors();
        assert!(
            first
                .poll(Duration::from_secs(1), None, || behaviors.clone())
                .is_none()
        );
        let first_selection = first.poll(Duration::from_secs(2), None, || behaviors.clone());
        let second_selection = second.poll(Duration::from_secs(2), None, || behaviors.clone());
        assert_eq!(first_selection, second_selection);
        assert!(first_selection.is_some());
        assert!(
            first
                .poll(Duration::from_secs(3), None, || behaviors.clone())
                .is_none()
        );
    }

    #[test]
    fn turning_the_mode_off_and_model_reset_clear_the_pending_deadline() {
        let mut scheduler = RandomBehaviorScheduler::new(11);
        scheduler.set_settings(settings(RandomBehaviorMode::Motions, 10), Duration::ZERO);
        scheduler.reset(Duration::from_secs(20));
        assert!(
            scheduler
                .poll(Duration::from_secs(29), None, behaviors)
                .is_none()
        );
        scheduler.set_settings(
            settings(RandomBehaviorMode::Off, 10),
            Duration::from_secs(29),
        );
        scheduler.set_settings(
            settings(RandomBehaviorMode::Motions, 10),
            Duration::from_secs(29),
        );
        assert!(
            scheduler
                .poll(Duration::from_secs(38), None, behaviors)
                .is_none()
        );
    }

    #[test]
    fn a_clock_rollback_does_not_replay_a_due_selection() {
        let mut scheduler = RandomBehaviorScheduler::new(13);
        scheduler.set_settings(
            settings(RandomBehaviorMode::MotionsAndExpressions, 1),
            Duration::from_secs(10),
        );
        assert!(
            scheduler
                .poll(Duration::from_secs(11), None, behaviors)
                .is_some()
        );
        assert!(
            scheduler
                .poll(Duration::from_secs(5), None, behaviors)
                .is_none()
        );
        assert!(
            scheduler
                .poll(Duration::from_secs(12), None, behaviors)
                .is_some()
        );
        assert!(
            scheduler
                .poll(Duration::from_secs(13), None, behaviors)
                .is_some()
        );
    }

    /// A selection narrows the draw to what the user checked, over any number of
    /// draws from any number of seeds.
    ///
    /// Asserting on a single draw would prove nothing: one pick from three can land
    /// on the allowed entry by luck. The mode tests above already use this shape,
    /// and a selection has to hold at least as strictly — an excluded behavior that
    /// appears is exactly the failure the user is reporting.
    #[test]
    fn a_selection_never_draws_outside_what_the_user_checked() {
        let behaviors = behaviors();
        let included = selected(&["motion:tap:1", "expression:happy.exp3.json"]);
        for seed in 1..=24u64 {
            for mode in [
                RandomBehaviorMode::MotionsAndExpressions,
                RandomBehaviorMode::Motions,
                RandomBehaviorMode::Expressions,
            ] {
                let selection = drawn_with(seed, mode, 12, Some(&included), &behaviors);
                assert!(
                    selection.iter().all(|behavior| matches!(
                        behavior,
                        ModelBehaviorSnapshot::Motion { group, .. } if group == "tap"
                    ) || kind(behavior) == "expression"),
                    "{mode:?} seed {seed} selected outside the selection: {selection:?}"
                );
            }
        }
    }

    /// An empty selection means the model plays nothing on its own.
    ///
    /// It does **not** fall back to "everything": the settings page says this state
    /// out loud, and a fallback would make unchecking every box read as though it
    /// had done the opposite.
    #[test]
    fn an_empty_selection_leaves_the_scheduler_with_nothing_to_play() {
        let behaviors = behaviors();
        let none = selected(&[]);
        for seed in 1..=24u64 {
            for mode in RandomBehaviorMode::ALL {
                assert!(
                    drawn_with(seed, mode, 12, Some(&none), &behaviors).is_empty(),
                    "{mode:?} seed {seed} played something with nothing selected"
                );
            }
        }
    }

    /// The mode and the selection are two filters, and either one can empty the set.
    ///
    /// A model that selected only motions while the mode is expressions-only has
    /// nothing to play, and the product must not answer by playing a motion the mode
    /// excluded — the mode is the user's decision about what kind plays, not a
    /// ranking between two sets that are both available.
    #[test]
    fn a_selection_of_the_other_kind_leaves_the_scheduler_idle() {
        let behaviors = behaviors();
        let motions_only = selected(&["motion:idle:0", "motion:tap:1"]);
        for seed in 1..=24u64 {
            assert!(
                drawn_with(
                    seed,
                    RandomBehaviorMode::Expressions,
                    12,
                    Some(&motions_only),
                    &behaviors
                )
                .is_empty()
            );
            assert!(
                drawn_with(
                    seed,
                    RandomBehaviorMode::Motions,
                    12,
                    Some(&motions_only),
                    &behaviors
                )
                .len()
                    == 12,
                "the selected motions must remain selectable"
            );
        }
    }

    /// A selection identifies the model it belongs to, and a mismatch reads as "no
    /// selection" rather than as somebody else's choice.
    ///
    /// The application re-publishes on every activation, so a selection that names a
    /// model which has moved on is one that has not been replaced yet. Applying it
    /// anyway would filter whichever model happens to be live — the failure mode a
    /// shared configuration setting across models would have.
    #[test]
    fn a_selection_only_applies_to_the_model_it_names() {
        let inclusion = RandomBehaviorInclusion {
            model: ModelId::parse("standard").expect("model id"),
            model_origin: ModelOrigin::Preset,
            behavior_ids: selected(&["motion:tap:1"]),
        };
        assert!(inclusion.belongs_to(&ModelId::parse("standard").unwrap(), ModelOrigin::Preset));
        assert!(!inclusion.belongs_to(&ModelId::parse("keyboard").unwrap(), ModelOrigin::Preset));
        assert!(
            !inclusion.belongs_to(&ModelId::parse("standard").unwrap(), ModelOrigin::Installed),
            "the same id from another source is a different model"
        );
    }
}
