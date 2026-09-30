//! What a day is, in this plugin's own terms.
//!
//! The host hands a plugin the time of day and nothing else: no date, no timezone, no
//! calendar. That is enough to count within a session and not enough to say "today", which
//! is the whole of what a daily tally needs. So this plugin answers the question itself,
//! with its own date library, in its own process, on its own thread.
//!
//! That is not a workaround; it is the shape the system is for. The application has no
//! date logic to lend because no application feature needs one, and a plugin that needs
//! one brings its own rather than making the product carry a calendar for it. The
//! `plugins/` lockfile is where that dependency lands, and the product's is not touched.

use time::{Date, OffsetDateTime, UtcOffset};

/// The local offset, read once.
///
/// `current_local_offset` is documented as sound only when one thread at a time asks, and
/// a plugin has exactly one thread — it is the session loop. Read once at startup and
/// never again, because a timezone change mid-session is not worth a second system-wide
/// query and a tally that is an hour out for one session is a tally nobody will notice.
fn local_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

/// Today's date in the user's own timezone.
///
/// Falls back to UTC rather than refusing: a machine whose timezone cannot be read is a
/// machine where a day boundary is a few hours out, and a tally that is a few hours out
/// is still a tally. Refusing to start would be worse than being slightly wrong.
pub fn today() -> Date {
    OffsetDateTime::now_utc().to_offset(local_offset()).date()
}

/// Today, as the plugin writes it in its own state file.
///
/// ISO 8601 rather than anything prettier, because this string is a key rather than
/// something a person reads: the panel shows numbers, and the file is read by the same
/// code that wrote it.
pub fn today_key() -> String {
    today().to_string()
}

/// A remembered day, ready to be compared with today's.
///
/// The only reason this type exists rather than a `String`: comparing two days is a
/// calendar question, and a `String` comparison happens to answer it correctly only
/// because the format sorts. If the format ever changes, this is the one place that has
/// to change with it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DayKey(String);

impl DayKey {
    pub fn today() -> Self {
        Self(today_key())
    }

    pub fn parse(stored: &str) -> Option<Self> {
        Date::parse(stored, &time::format_description::well_known::Iso8601::DATE)
            .ok()
            .map(|date| Self(date.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A day this build cannot read, kept under the name the file used.
    ///
    /// Not a valid day on purpose: the tally must *not* recognise it as today, and a
    /// sentinel that parsed would be a day the file never claimed to be. Bounded and
    /// filtered, because the state file is written back on every day change — so whatever
    /// a hand-edited file carried goes straight back out, and a megabyte of nonsense in a
    /// key is a file this plugin would keep rewriting.
    pub fn unknown(stored: &str) -> Self {
        let name: String = stored
            .trim()
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
            .take(32)
            .collect();
        Self(if name.is_empty() {
            "unknown".to_owned()
        } else {
            name
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_is_a_day_the_plugin_can_write_down_and_read_back() {
        let key = today_key();
        assert_eq!(
            key.len(),
            10,
            "an ISO date, so the string sorts as the day it names: {key}"
        );
        assert!(key.starts_with('2'), "{key}");
        let parsed = DayKey::parse(&key).expect("today parses as a day");
        assert_eq!(parsed.as_str(), key, "and reads back as itself");
    }

    #[test]
    fn a_stored_day_that_is_not_a_day_is_refused_rather_than_compared_as_text() {
        assert!(DayKey::parse("yesterday").is_none());
        assert!(DayKey::parse("").is_none());
        assert!(DayKey::parse("2026-13-01").is_none());
        assert!(
            DayKey::parse("2026-10-01").is_some(),
            "so a hand-edited state file with a nonsense day is a day this plugin cannot \\
             recognise rather than a day it silently believes"
        );
    }
}
