//! The date in a log's file name, without a date crate.
//!
//! A file name is a date the user will read and type, so it is validated rather
//! than formatted from whatever the clock happened to hold: an impossible date is
//! not a date, and a log named for one cannot be aged out correctly.

use super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct UtcDate(pub(crate) Date);

impl UtcDate {
    pub(crate) fn from_system_time(timestamp: SystemTime) -> Self {
        let duration = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
        let days = total_seconds.div_euclid(SECONDS_PER_DAY as i64);
        let unix_epoch_julian_day = Date::from_ordinal_date(1970, 1)
            .expect("the Unix epoch is a valid date")
            .to_julian_day();
        let maximum_julian_day = Date::MAX.to_julian_day();
        let julian_day = i32::try_from(days)
            .ok()
            .and_then(|days| days.checked_add(unix_epoch_julian_day))
            .filter(|julian_day| *julian_day <= maximum_julian_day)
            .unwrap_or(maximum_julian_day);
        Self(Date::from_julian_day(julian_day).unwrap_or(Date::MAX))
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        // `time` accepts an optional sign for years outside the fixed-width
        // format. Log filenames are a strict ASCII `YYYY-MM-DD` contract.
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        {
            return None;
        }
        Date::parse(value, &UTC_DATE_FORMAT).ok().map(Self)
    }

    pub(crate) fn as_string(self) -> String {
        self.0
            .format(&UTC_DATE_FORMAT)
            .expect("the fixed UTC date format is valid")
    }
}

pub(crate) fn format_timestamp(timestamp: SystemTime) -> String {
    let duration = timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let (day, hour, minute, second) = i64::try_from(duration.as_secs())
        .ok()
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
        .map(|timestamp| {
            (
                UtcDate(timestamp.date()),
                timestamp.hour(),
                timestamp.minute(),
                timestamp.second(),
            )
        })
        // `time::Date` intentionally has a bounded year range. A clock outside
        // that range is malformed input, so keep the log record representable
        // instead of emitting an unbounded or wrapped civil date.
        .unwrap_or((UtcDate(Date::MAX), 23, 59, 59));
    format!(
        "{}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        day.as_string(),
        duration.subsec_millis()
    )
}
