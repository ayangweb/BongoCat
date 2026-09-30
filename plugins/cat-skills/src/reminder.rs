//! Reminders the user armed, and the one that is due.
//!
//! A reminder is three facts — when, what to say, and whether it has been said — and a file.
//! There is nothing clever here and that is the point: the whole of what a reminder needs to
//! survive the app closing is those three facts written somewhere the plugin owns.
//!
//! Why the plugin owns it rather than the product: a reminder is not a product feature that
//! happens to be implemented by a plugin, it is *this plugin's* feature, and a reminder that
//! lived in the product's configuration would be a thing the product had to understand,
//! migrate and keep when nobody wanted a reminder. Here it is a JSON file in a directory the
//! host created and never writes inside, so replacing this plugin's program cannot replace the
//! user's reminders.
//!
//! # The time is an instant, not a wall clock
//!
//! A reminder is due at an absolute instant — seconds since the epoch — and never at "14:30".
//! A reminder stored as a time of day is a reminder that fires at the wrong moment twice a
//! year, and one stored as a date needs a calendar and a timezone, which is a dependency and a
//! set of questions for something that only ever needs "has this moment passed". So the file
//! holds a number, and the only way it can go wrong is if the machine's clock jumps — which is
//! a fact about the machine rather than about the reminder.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Now, in seconds since the epoch.
///
/// The one clock a reminder can be compared against, and the only one available without a
/// calendar: a local time needs a timezone and a date needs a calendar, and both are more
/// machinery than a countdown can justify.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// What the user asked to be reminded about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reminder {
    /// A number this plugin chose, so an armed reminder can be replaced by a later one
    /// without a second button.
    pub id: u64,
    /// When it is due, in seconds since the epoch.
    pub due_unix: i64,
    /// What the cat says when it is due.
    pub text: String,
    /// Whether it has been said already.
    ///
    /// A field rather than a deletion because the panel shows what is armed, and a reminder
    /// that vanishes the instant it fires is a reminder the user cannot check.
    pub said: bool,
}

impl Reminder {
    /// A reminder due `minutes` from now, saying `text`.
    pub fn in_minutes(id: u64, minutes: i64, text: &str) -> Self {
        Self {
            id,
            due_unix: now_unix().saturating_add(minutes.saturating_mul(60)),
            text: text.trim().to_owned(),
            said: false,
        }
    }

    /// The shortest a reminder's text may be.
    ///
    /// One character, because a reminder with no words is a reminder the user cannot tell from
    /// one that failed to save.
    pub const MINIMUM_CHARS: usize = 1;

    /// How many seconds are left before it is due, or zero once it is.
    pub fn seconds_left(&self, now: i64) -> i64 {
        (self.due_unix - now).max(0)
    }

    /// Whether it is time.
    pub fn is_due(&self, now: i64) -> bool {
        !self.said && self.due_unix <= now
    }

    /// Whether it is worth arming at all.
    pub fn is_worth_arming(&self) -> bool {
        self.text.chars().count() >= Self::MINIMUM_CHARS
    }
}

/// Every reminder this plugin knows about.
///
/// A map keyed by the reminder's own number rather than a list, because replacing one
/// reminder must not depend on where it happened to sit in the file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Book {
    reminders: BTreeMap<u64, Reminder>,
    /// The number the next reminder will get.
    ///
    /// Bounded rather than wrapping: a number that wraps would replace an armed reminder by
    /// accident, and a user with that many reminders has a different problem.
    next_id: u64,
    /// How many reminders this will hold, whatever the file says.
    pub capacity: usize,
}

impl Book {
    /// An empty book.
    pub fn new() -> Self {
        Self {
            capacity: 32,
            ..Self::default()
        }
    }

    /// Arm one, and say what it is now.
    ///
    /// Returns `None` when the text is empty, which is the one thing that makes a reminder
    /// pointless: a reminder the cat cannot say is a reminder that does not exist, and it is
    /// better to arm nothing than to arm something that fires silently.
    pub fn arm(&mut self, minutes: i64, text: &str) -> Option<Reminder> {
        let reminder = Reminder::in_minutes(self.next_id, minutes, text);
        if !reminder.is_worth_arming() {
            return None;
        }
        let id = reminder.id;
        self.next_id = self.next_id.saturating_add(1).max(id.saturating_add(1));
        // One at a time. A user who arms a second reminder meant to replace the first, and a
        // list of pending reminders on a cat-sized panel is a list nobody can read.
        self.reminders.clear();
        self.trim_to_capacity();
        self.reminders.insert(id, reminder.clone());
        Some(reminder)
    }

    /// Forget everything.
    pub fn clear(&mut self) {
        self.reminders.clear();
    }

    /// The reminder that is due, if one is, marked as said.
    ///
    /// Takes it out of the set of pending reminders and marks it said in one step, so a tick
    /// that runs twice cannot say the same thing twice: the second call finds nothing pending.
    pub fn take_due(&mut self, now: i64) -> Option<Reminder> {
        let id = self
            .reminders
            .iter()
            .find(|(_, reminder)| reminder.is_due(now))
            .map(|(id, _)| *id)?;
        let reminder = self.reminders.get_mut(&id)?;
        reminder.said = true;
        Some(reminder.clone())
    }

    /// Whether a reminder is armed.
    ///
    /// The question the panel asks — is there a "Forget it" button to draw — so it is a name
    /// rather than a call to [`Book::shown`], which answers a different question.
    pub fn is_armed(&self) -> bool {
        self.shown().is_some()
    }

    /// The reminder to show, if any.
    ///
    /// The one with the nearest due time, said or not: a said reminder is still the reminder
    /// the user armed, and hiding it the moment it fires is a reminder that appears to have
    /// been lost.
    pub fn shown(&self) -> Option<&Reminder> {
        self.reminders
            .values()
            .min_by_key(|reminder| reminder.due_unix)
    }

    /// Drop the reminders furthest in the past, until the book is within its capacity.
    ///
    /// Bounded because this is a file that grows: the only reminders anybody reads are the one
    /// that is armed and the last few that fired, and a year of them is a year of somebody
    /// forgetting to clear them.
    fn trim_to_capacity(&mut self) {
        while self.reminders.len() > self.capacity.max(1) {
            let Some(furthest) = self
                .reminders
                .values()
                .max_by_key(|reminder| reminder.due_unix)
                .map(|reminder| reminder.id)
            else {
                break;
            };
            self.reminders.remove(&furthest);
        }
    }

    /// What goes in the plugin's own file.
    pub fn to_state(&self) -> State {
        State {
            next_id: self.next_id,
            reminders: self
                .reminders
                .values()
                .map(|reminder| {
                    (
                        reminder.id,
                        Entry {
                            due_unix: reminder.due_unix,
                            text: reminder.text.clone(),
                            said: reminder.said,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The book a saved file describes.
    ///
    /// Never fails. A file that will not parse, one from a version that wrote a different
    /// shape, and one that is empty all mean the same thing to a user: their reminder is gone
    /// and they would rather the plugin started working than that it complained. So a bad file
    /// is an empty book, and the plugin's own log says which happened — because a reminder
    /// that silently vanished would leave a user waiting for something that was never going to
    /// arrive.
    pub fn from_state(state: State) -> Self {
        let mut book = Self::new();
        book.next_id = state.next_id;
        for (id, entry) in state.reminders {
            book.reminders.insert(
                id,
                Reminder {
                    id,
                    due_unix: entry.due_unix,
                    text: entry.text,
                    said: entry.said,
                },
            );
        }
        // A file written by a build with a bigger capacity is not read as having more
        // reminders than this build can hold: the bound is this build's.
        book.trim_to_capacity();
        // The next number must be above every existing one, or arming would replace a
        // reminder from the file rather than add one.
        let highest = book.reminders.keys().copied().max().unwrap_or_default();
        book.next_id = book.next_id.max(highest.saturating_add(1));
        book
    }
}

/// The plugin's own reminder file, as it is written.
///
/// Every field has a default and every entry is optional, so a file from a version of this
/// plugin that knew fewer things still loads — the same rule as every other state file in the
/// product, and for the same reason: a reminder the user armed is not theirs to lose because
/// this plugin was updated.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub reminders: BTreeMap<u64, Entry>,
}

/// One saved reminder.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub due_unix: i64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub said: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_armed_reminder_is_due_after_the_minutes_the_user_asked_for() {
        let now = now_unix();
        let reminder = Reminder::in_minutes(1, 10, "stand up");
        assert!(
            reminder.due_unix >= now + 600 && reminder.due_unix <= now_unix() + 600,
            "so 'in ten minutes' means ten minutes from now and not from some other moment: \
             {:?}",
            reminder
        );
        assert_eq!(reminder.seconds_left(now), 600);
        assert!(!reminder.is_due(now));
        assert!(reminder.is_due(now + 600));
    }

    #[test]
    fn a_countdown_never_reads_as_a_negative_number_of_seconds() {
        // A laptop resuming from sleep can hand back a clock that is behind, and a panel
        // showing "-3599s" is showing a bug rather than a time. So the countdown reads zero,
        // which is true, and a reminder that was due and was said stays said either way.
        let reminder = Reminder::in_minutes(1, 1, "drink water");
        let due = reminder.due_unix;
        assert!(reminder.is_due(due));
        assert_eq!(reminder.seconds_left(due), 0);
        assert_eq!(
            reminder.seconds_left(due - 3_600),
            3_600,
            "so a clock behind the due time reads as more time left, which is true"
        );
        assert!(
            (0..=3_600).all(|back| reminder.seconds_left(due + back) == 0),
            "and once the moment has passed it reads zero at every point after it, never a \
             negative number: {:?}",
            reminder.seconds_left(due + 3_600)
        );
        assert!(
            !reminder.is_due(due - 3_600),
            "and a reminder the clock has not reached is still not due, because a clock that \
             went backwards has not yet arrived at the moment it names"
        );
    }

    #[test]
    fn a_reminder_with_no_words_is_not_armed() {
        // A reminder the cat cannot say is a reminder that fires silently, which from the
        // user's side is a reminder that failed.
        assert!(!Reminder::in_minutes(1, 5, "").is_worth_arming());
        assert!(!Reminder::in_minutes(1, 5, "   ").is_worth_arming());
        assert!(Reminder::in_minutes(1, 5, "x").is_worth_arming());

        let mut book = Book::new();
        assert_eq!(book.arm(5, "   "), None, "so nothing is armed");
        assert!(!book.is_armed());
    }

    #[test]
    fn arming_a_second_reminder_replaces_the_first() {
        // A user who arms a second one meant to change their mind, and a list of pending
        // reminders on a cat-sized panel is a list nobody can read.
        let mut book = Book::new();
        book.arm(10, "drink water").expect("armed");
        let second = book.arm(20, "stretch").expect("armed");
        assert_eq!(
            book.shown().map(|reminder| reminder.text.as_str()),
            Some("stretch")
        );
        assert_ne!(second.id, 0, "so it has a number of its own");
    }

    #[test]
    fn a_due_reminder_is_said_once_and_not_twice() {
        // The bug this shape exists to prevent: a tick that runs twice, or a session that
        // restarts, saying the same thing to a person who already heard it.
        let now = now_unix();
        let mut book = Book::new();
        book.arm(0, "stand up").expect("armed");
        assert_eq!(
            book.take_due(now).map(|reminder| reminder.text),
            Some("stand up".to_string())
        );
        assert_eq!(
            book.take_due(now),
            None,
            "because the second call finds nothing pending"
        );
        assert_eq!(
            book.take_due(now + 10_000),
            None,
            "and it is still not pending an hour later"
        );
        assert!(
            book.is_armed(),
            "while the panel can still show what was armed, because a reminder that vanishes \
             the instant it fires is one the user cannot check"
        );
    }

    #[test]
    fn the_reminder_on_show_is_the_nearest_one_by_due_time() {
        let mut book = Book::new();
        book.arm(30, "later").expect("armed");
        let soon = book.arm(1, "sooner").expect("armed");
        assert_eq!(
            book.shown().map(|reminder| reminder.id),
            Some(soon.id),
            "so the panel does not show a reminder an hour away while one a minute away is \
             waiting"
        );
    }

    #[test]
    fn a_book_of_reminders_survives_its_own_file() {
        let mut book = Book::new();
        book.arm(10, "drink water").expect("armed");
        let armed = book.shown().expect("a reminder").clone();
        let restored = Book::from_state(book.to_state());
        assert_eq!(restored.shown(), Some(&armed));
        assert_eq!(
            Book::from_state(restored.to_state()).to_state(),
            restored.to_state(),
            "and writing the restored book again produces the same file, which is what makes \
             the file safe to overwrite"
        );
    }

    #[test]
    fn a_file_from_a_version_that_knew_fewer_things_still_loads() {
        let state: State = serde_json::from_str(r#"{}"#).expect("an empty file");
        assert!(!Book::from_state(state).is_armed());
        let older: State =
            serde_json::from_str(r#"{"next_id":7,"reminders":{"3":{"due_unix":100,"text":"hi"}}}"#)
                .expect("a file without a `said` field");
        let book = Book::from_state(older);
        assert_eq!(
            book.shown().map(|reminder| reminder.text.as_str()),
            Some("hi")
        );
        assert!(
            !book.shown().expect("a reminder").said,
            "and a reminder written before this field existed has not been said, because \
             defaulting it to true would throw away a reminder that had not fired yet"
        );
    }

    #[test]
    fn a_number_below_every_existing_reminder_does_not_replace_one() {
        // The next number comes out of the file, and a hand-edited file or an old one could
        // carry a low value. Arming must still add rather than overwrite.
        let state: State = serde_json::from_str(
            r#"{"next_id":1,"reminders":{"1":{"due_unix":100,"text":"old"},"9":{"due_unix":200,"text":"newer"}}}"#,
        )
        .expect("a file whose counter is behind its contents");
        let mut book = Book::from_state(state);
        let armed = book.arm(5, "third").expect("armed");
        assert!(
            armed.id > 9,
            "so the new reminder gets a number above every one in the file: {}",
            armed.id
        );
        assert_eq!(
            book.to_state().reminders.len(),
            1,
            "and arming still replaces"
        );
    }

    #[test]
    fn a_file_with_more_reminders_than_this_build_holds_is_bounded_on_read() {
        // The bound is this build's, not the file's: a plugin that read a hundred would show
        // one and hide ninety-nine.
        let entries: Vec<String> = (1..=100_u64)
            .map(|id| {
                format!(
                    r#""{id}":{{"due_unix":{},"text":"r{id}","said":false}}"#,
                    1_000 + id as i64
                )
            })
            .collect();
        let entries = entries.join(",");
        let file = format!("{{\"next_id\":101,\"reminders\":{{{}}}}}", entries);
        let state: State = serde_json::from_str(&file).expect("a file with a hundred reminders");
        let book = Book::from_state(state);
        assert_eq!(
            book.to_state().reminders.len(),
            book.capacity,
            "so only the capacity is kept, and the nearest ones at that"
        );
        assert_eq!(
            book.shown().map(|reminder| reminder.text.as_str()),
            Some("r1"),
            "and the one on show is the nearest due, which the capacity kept"
        );
    }

    #[test]
    fn clearing_removes_everything_and_the_file_says_so() {
        let mut book = Book::new();
        book.arm(5, "x").expect("armed");
        book.clear();
        assert!(!book.is_armed());
        assert_eq!(book.to_state().reminders.len(), 0);
    }
}
