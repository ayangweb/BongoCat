//! A file name's date is validated, not assumed.

use super::*;

#[test]
fn utc_dates_use_calendar_validation_and_round_trip() {
    let leap_day = SystemTime::UNIX_EPOCH + Duration::from_secs(951_782_400);
    assert_eq!(
        UtcDate::from_system_time(leap_day).as_string(),
        "2000-02-29"
    );
    assert_eq!(format_timestamp(leap_day), "2000-02-29T00:00:00.000Z");
    assert_eq!(
        UtcDate::parse("2000-02-29")
            .map(UtcDate::as_string)
            .as_deref(),
        Some("2000-02-29")
    );
    assert!(UtcDate::parse("1900-02-29").is_none());
    assert!(UtcDate::parse("+000-01-01").is_none());
    assert!(UtcDate::parse("2000-02-29 ").is_none());
}
