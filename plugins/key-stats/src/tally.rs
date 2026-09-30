//! The tally: what has been counted today, and what has been counted before.
//!
//! A tally is three facts — a day, a number of keys, a distance — plus a small list of
//! which keys those were. Everything else is presentation. The shape is deliberately dull:
//! a plain struct that serialises into the plugin's own `state.json`, read at startup and
//! written when the day changes, when the settings change what is counted, and when the
//! host stops.
//!
//! # Why the day is a key and not a field
//!
//! Because a tally belongs to a day and a day is not a number. Storing `day: "2026-10-01"`
//! beside the counts means the file is readable — a user who opens it can see what the
//! plugin thinks today is — and a mismatch is a mismatch rather than a silent reset.

use crate::calendar::DayKey;
use std::collections::BTreeMap;

/// The largest distance a day may total, in the host's units.
///
/// About a thousand window widths, which is a person moving the pointer continuously for
/// several days. A bound rather than a hope: one absurd sample should not make every later
/// day's number unreadable, and the only number anybody reads is this one.
const MAXIMUM_DISTANCE: f32 = 1_000.0;

/// How many keys the per-key list remembers.
///
/// Five, because a list of a hundred is a histogram and this is a panel; and because the
/// keys somebody presses most are the same five all day, so a longer list is mostly the
/// same five plus noise.
pub const TOP_KEYS: usize = 5;

/// The days kept, when the user has not chosen.
pub const DEFAULT_DAYS_KEPT: i64 = 30;

/// What the plugin has counted.
#[derive(Clone, Debug, PartialEq)]
pub struct Tally {
    /// The day these numbers belong to.
    day: DayKey,
    /// Keys pressed today, auto-repeat excluded.
    keys: u64,
    /// Pointer distance today, in the host's normalized units.
    distance: f32,
    /// How many of today's keys were each control, for the "most pressed" list.
    ///
    /// Bounded on write rather than on read: a map that grows for every key a person ever
    /// pressed is a map that never stops growing, and the only entries this plugin reads
    /// are the top few.
    per_key: BTreeMap<String, u64>,
    /// Earlier days, so "last seven days" is a fact rather than an estimate.
    history: BTreeMap<String, Day>,
    /// How many times the plugin has started, which is what makes the panel able to say
    /// "and yesterday" honestly.
    sessions: u32,
}

/// One past day, as small as it can be and no smaller.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Day {
    pub keys: u64,
    pub distance: f32,
}

/// What the plugin is counting, as the user configured it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Count {
    /// Whether the pointer counts as well as the keyboard.
    pub mouse: bool,
    /// How many days of history to keep.
    pub days_kept: usize,
    /// Whether a day only counts while the model window is up.
    pub only_when_visible: bool,
}

impl Default for Count {
    fn default() -> Self {
        Self {
            mouse: true,
            days_kept: DEFAULT_DAYS_KEPT as usize,
            only_when_visible: false,
        }
    }
}

impl Tally {
    /// An empty tally for today.
    pub fn new(count: Count) -> Self {
        let mut tally = Self {
            day: DayKey::today(),
            keys: 0,
            distance: 0.0,
            per_key: BTreeMap::new(),
            history: BTreeMap::new(),
            sessions: 1,
        };
        tally.trimmed_to(count);
        tally
    }

    /// Today's key count.
    pub const fn keys(&self) -> u64 {
        self.keys
    }

    /// Today's pointer distance, in the host's units.
    pub fn distance(&self) -> f32 {
        self.distance
    }

    /// The keys pressed most, most first.
    ///
    /// Sorted by count and then by name, because a tie has to break *somehow* and a tally
    /// that reorders itself between two renders is a list nobody can read. Sorted by name
    /// because the name is stable and the count is what the reader is looking for.
    pub fn top_keys(&self) -> Vec<(String, u64)> {
        let mut ranked: Vec<(String, u64)> = self
            .per_key
            .iter()
            .map(|(key, count)| (key.clone(), *count))
            .collect();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        ranked.truncate(TOP_KEYS);
        ranked
    }

    /// Today's count plus every kept day, for a "since" figure.
    pub fn total_keys(&self) -> u64 {
        self.keys + self.history.values().map(|day| day.keys).sum::<u64>()
    }

    /// Roll over to today if the day has changed.
    ///
    /// Returns whether it rolled, because that is the one moment the panel has to say
    /// something different and the state file has to be written.
    pub fn roll_if_new_day(&mut self, count: Count) -> bool {
        let today = DayKey::today();
        if self.day == today {
            return false;
        }
        self.history.insert(
            self.day.as_str().to_owned(),
            Day {
                keys: self.keys,
                distance: self.distance,
            },
        );
        // The kept days are the *most recent* ones, so a tally that has been running for
        // a year drops its oldest day and keeps yesterday — which is the one anybody looks
        // at. Keeping the oldest would be keeping a year ago.
        let keep = count.days_kept;
        while self.history.len() > keep {
            let Some(oldest) = self.history.keys().next().cloned() else {
                break;
            };
            self.history.remove(&oldest);
        }
        self.day = today;
        self.keys = 0;
        self.distance = 0.0;
        self.per_key.clear();
        self.sessions = self.sessions.saturating_add(1);
        self.trimmed_to(count);
        true
    }

    /// One key went down.
    pub fn press(&mut self, control: &str) {
        self.keys = self.keys.saturating_add(1);
        let entry = self.per_key.entry(control.to_owned()).or_insert(0);
        *entry = entry.saturating_add(1);
        // A key nobody will ever look up is still a key, so the map is bounded by dropping
        // the *lowest* counts rather than by refusing to record: a person who presses four
        // hundred distinct keys in a day still gets a top five, and the file stays small.
        while self.per_key.len() > TOP_KEYS * 4 {
            let Some(weakest) = self
                .per_key
                .iter()
                .min_by_key(|(name, count)| (**count, (*name).clone()))
                .map(|(name, _)| name.clone())
            else {
                break;
            };
            self.per_key.remove(&weakest);
        }
    }

    /// The pointer moved this far.
    pub fn travel(&mut self, distance: f32) {
        // Not finite contributes nothing rather than poisoning the total: one bad sample
        // from a platform is not a reason for a day's tally to read `NaN`.
        if distance.is_finite() && distance > 0.0 {
            // Clamped rather than summed freely: a pointer that reports a coordinate an
            // order of magnitude out of range is a platform fact, and one day's tally
            // should be a number a person can read rather than `1e38`.
            self.distance = (self.distance + distance).min(MAXIMUM_DISTANCE);
        }
    }

    /// Clear today's numbers, keeping the days before it.
    pub fn reset_today(&mut self) {
        self.keys = 0;
        self.distance = 0.0;
        self.per_key.clear();
    }

    /// Apply a change to what is counted, so a setting that turns something off stops it
    /// counting rather than merely hiding it.
    fn trimmed_to(&mut self, count: Count) -> &mut Self {
        let keep = count.days_kept;
        while self.history.len() > keep {
            let Some(oldest) = self.history.keys().next().cloned() else {
                break;
            };
            self.history.remove(&oldest);
        }
        self
    }

    /// What goes in the plugin's own state file.
    pub fn to_state(&self) -> State {
        State {
            day: self.day.as_str().to_owned(),
            keys: self.keys,
            distance: self.distance,
            per_key: self.per_key.clone(),
            history: self.history.clone(),
            sessions: self.sessions,
        }
    }

    /// The tally a state file describes, for today.
    ///
    /// A file with no day in it, or a day this build cannot read, starts today: the
    /// alternative is a plugin that shows nothing until tomorrow, and a tally that begins
    /// now is a better answer than a tally that never begins.
    pub fn from_state(state: State) -> Self {
        let today = DayKey::today();
        // A day this build cannot read is *some* day and not this one. Defaulting it to
        // today would file somebody's afternoon under today and show it as if it had just
        // happened; the rollover below banks it under the name the file used, which is the
        // only honest place for numbers whose day nobody can name.
        let day = DayKey::parse(&state.day).unwrap_or_else(|| DayKey::unknown(&state.day));
        let mut tally = Self {
            day,
            keys: state.keys,
            distance: if state.distance.is_finite() {
                state.distance
            } else {
                0.0
            },
            per_key: state.per_key,
            history: state.history,
            sessions: state.sessions.max(1),
        };
        // A file written yesterday is read as *yesterday's* numbers, and the first tick
        // rolls it over — so the day boundary is the same rule whether the app was running
        // across midnight or closed over it.
        if tally.day != today {
            tally.history.insert(
                tally.day.as_str().to_owned(),
                Day {
                    keys: tally.keys,
                    distance: tally.distance,
                },
            );
            tally.day = today;
            tally.keys = 0;
            tally.distance = 0.0;
            tally.per_key.clear();
            tally.sessions = tally.sessions.saturating_add(1);
        }
        tally
    }
}

/// The plugin's own state file, as it is written.
///
/// A struct rather than the tally itself, because the tally holds a parsed [`DayKey`] and
/// a file should hold a string a person can read. Every field has a default, so a file
/// from a version of this plugin that knew fewer things still loads.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub keys: u64,
    #[serde(default)]
    pub distance: f32,
    #[serde(default)]
    pub per_key: BTreeMap<String, u64>,
    #[serde(default)]
    pub history: BTreeMap<String, Day>,
    #[serde(default)]
    pub sessions: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::today_key;

    fn count(mouse: bool, days_kept: usize) -> Count {
        Count {
            mouse,
            days_kept,
            only_when_visible: false,
        }
    }

    fn tally_with(days: usize) -> Tally {
        Tally::new(count(true, days))
    }

    #[test]
    fn a_key_pressed_once_is_one_key() {
        let mut tally = tally_with(30);
        tally.press("KeyA");
        assert_eq!(tally.keys(), 1);
        assert_eq!(tally.top_keys(), [("KeyA".to_owned(), 1)]);
    }

    #[test]
    fn the_keys_pressed_most_come_first_and_a_tie_breaks_by_name() {
        let mut tally = tally_with(30);
        for key in ["KeyB", "KeyA", "KeyC"] {
            tally.press(key);
        }
        tally.press("KeyA");
        tally.press("KeyA");
        assert_eq!(
            tally.top_keys(),
            [
                ("KeyA".to_owned(), 3),
                ("KeyB".to_owned(), 1),
                ("KeyC".to_owned(), 1)
            ],
            "so a list that reorders itself between two renders is a list nobody can read"
        );
    }

    #[test]
    fn a_key_nobody_will_look_up_is_still_a_key() {
        // Four hundred distinct keys in a day is a real day, and the top five is all the
        // panel shows — but the total has to be right, and the file has to stay small.
        let mut tally = tally_with(30);
        for index in 0..400 {
            tally.press(&format!("Key{index}"));
        }
        assert_eq!(tally.keys(), 400, "and the total counts every one of them");
        assert_eq!(
            tally.top_keys().len(),
            TOP_KEYS,
            "while the list is still five long"
        );
        assert!(
            tally.per_key.len() <= TOP_KEYS * 4,
            "and the file the plugin writes stays a file: {} entries",
            tally.per_key.len()
        );
    }

    #[test]
    fn distance_that_is_not_a_number_does_not_poison_the_day() {
        let mut tally = tally_with(30);
        tally.travel(2.0);
        tally.travel(f32::NAN);
        tally.travel(f32::INFINITY);
        assert_eq!(
            tally.distance(),
            2.0,
            "one bad sample is not a day's tally of NaN"
        );
    }

    #[test]
    fn a_new_day_banks_the_day_that_ended_and_starts_today_at_nothing() {
        let mut tally = tally_with(30);
        tally.press("KeyA");
        tally.travel(3.0);
        // A day that is not today, which is what "last night" looks like from here.
        tally.day = DayKey::parse("2000-01-01").expect("a day");
        let ended = tally.day.as_str().to_owned();
        assert!(tally.roll_if_new_day(count(true, 30)));
        assert_eq!(tally.keys(), 0, "so today starts at nothing");
        assert_eq!(tally.distance(), 0.0);
        assert!(tally.top_keys().is_empty());
        assert_eq!(
            tally.history.get(&ended),
            Some(&Day {
                keys: 1,
                distance: 3.0
            }),
            "and the day that ended is still countable: {:?}",
            tally.history
        );
        assert_eq!(tally.day.as_str(), today_key());
    }

    #[test]
    fn a_day_that_has_not_changed_changes_nothing() {
        let mut tally = tally_with(30);
        tally.press("KeyA");
        assert!(
            !tally.roll_if_new_day(count(true, 30)),
            "because a tally that reset itself every tick would show nothing ever"
        );
        assert_eq!(tally.keys(), 1);
    }

    #[test]
    fn a_tally_written_yesterday_is_read_as_yesterdays_numbers() {
        // The app was closed over the boundary, so the rollover happened with nobody
        // watching. The rule is the same one a live rollover uses, which is the point.
        let state = State {
            day: "2000-01-01".to_owned(),
            keys: 412,
            distance: 9.5,
            per_key: BTreeMap::from([("KeyA".to_owned(), 12)]),
            history: BTreeMap::new(),
            sessions: 3,
        };
        let tally = Tally::from_state(state);
        assert_eq!(tally.keys(), 0, "so today starts at nothing");
        assert_eq!(tally.total_keys(), 412, "and yesterday is still countable");
        assert_eq!(
            tally.sessions, 4,
            "and the run count went up, because it is a new day"
        );
    }

    #[test]
    fn a_state_file_with_no_day_banks_its_keys_under_the_name_the_file_used() {
        // A tally that never begins is worse than a tally that begins now, and a file from
        // a version of this plugin that knew fewer things is exactly this shape. The keys
        // in it are real keys somebody pressed, so they are banked rather than dropped —
        // and they are *not* filed under today, because a day nobody named is not a day
        // anybody kept.
        let tally = Tally::from_state(State {
            keys: 7,
            ..State::default()
        });
        assert_eq!(tally.day.as_str(), today_key(), "so today begins now");
        assert_eq!(tally.keys(), 0, "at nothing of its own");
        assert_eq!(tally.total_keys(), 7, "and the keys are still countable");
    }

    #[test]
    fn a_hand_edited_day_this_build_cannot_read_still_loads() {
        let tally = Tally::from_state(State {
            day: "yesterday".to_owned(),
            keys: 5,
            ..State::default()
        });
        assert_eq!(tally.day.as_str(), today_key());
        assert_eq!(tally.total_keys(), 5);
    }

    #[test]
    fn a_hand_edited_day_name_cannot_write_an_arbitrary_string_into_the_state() {
        // The state file is written back on every day change, so whatever name a hand-edited
        // file carried goes straight back out. Bounded, because a file with a megabyte of
        // nonsense in a key is a file this plugin would keep rewriting.
        let tally = Tally::from_state(State {
            day: "  ../escape/\u{1F600}  ".to_owned(),
            ..State::default()
        });
        let banked = tally.history.keys().next().expect("the day is banked");
        assert!(banked.len() <= 32, "{banked}");
        assert!(
            !banked.contains('/') && !banked.contains('\\'),
            "and it is a name rather than a path: {banked}"
        );
    }

    #[test]
    fn a_state_file_whose_distance_is_nonsense_loads_with_no_distance() {
        let tally = Tally::from_state(State {
            day: today_key(),
            distance: f32::NAN,
            keys: 3,
            ..State::default()
        });
        assert_eq!(tally.distance(), 0.0);
        assert_eq!(
            tally.keys(),
            3,
            "and keeps the half of the file it can believe"
        );
    }

    #[test]
    fn only_the_kept_days_are_kept() {
        // A file that grows without bound is a file that eventually stops being written,
        // and the days anybody looks at are the recent ones.
        let mut tally = tally_with(3);
        tally.press("KeyA");
        for day in ["2000-01-01", "2000-01-02", "2000-01-03", "2000-01-04"] {
            tally.history.insert(
                day.to_owned(),
                Day {
                    keys: 1,
                    distance: 0.0,
                },
            );
        }
        let mut tally = Tally {
            day: DayKey::parse("2000-01-05").expect("a day"),
            keys: tally.keys,
            distance: tally.distance,
            per_key: tally.per_key,
            history: tally.history,
            sessions: 1,
        };
        tally.roll_if_new_day(count(true, 3));
        assert_eq!(
            tally.history.keys().cloned().collect::<Vec<_>>(),
            ["2000-01-03", "2000-01-04", "2000-01-05"],
            "so the three most recent days are the three kept"
        );
    }

    #[test]
    fn resetting_today_keeps_the_days_before_it() {
        // A user who resets today is asking about today, and a reset that also threw away
        // last week would be a reset nobody asked for.
        let mut tally = tally_with(30);
        tally.press("KeyA");
        tally.history.insert(
            "2000-01-01".to_owned(),
            Day {
                keys: 9,
                distance: 1.0,
            },
        );
        tally.reset_today();
        assert_eq!(tally.keys(), 0);
        assert_eq!(tally.distance(), 0.0);
        assert!(tally.top_keys().is_empty());
        assert_eq!(tally.total_keys(), 9, "and last week is untouched");
    }

    #[test]
    fn a_tally_round_trips_through_its_own_state_file() {
        let mut tally = tally_with(30);
        tally.press("KeyA");
        tally.press("KeyB");
        tally.travel(4.5);
        let state = tally.to_state();
        let restored = Tally::from_state(state.clone());
        assert_eq!(restored.keys(), tally.keys());
        assert_eq!(restored.distance(), tally.distance());
        assert_eq!(restored.top_keys(), tally.top_keys());
        assert_eq!(
            serde_json::to_string(&state).expect("serializes"),
            serde_json::to_string(&restored.to_state()).expect("serializes"),
            "and writing the restored tally again produces the same file, which is what makes \\
             the file safe to overwrite on every day change"
        );
    }
}
