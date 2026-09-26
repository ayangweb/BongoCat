//! A line is read back exactly, or not at all.

use super::*;

#[test]
fn strict_parser_returns_catalog_fields_without_context_values() {
    let record = LogRecord::new(
        SystemTime::UNIX_EPOCH,
        LogLevel::Warn,
        "application",
        "application/previous_run_unclean",
        "The previous run ended unexpectedly",
    )
    .with_context("reason", "forced")
    .with_context("private", "must-not-be-exposed");
    let line = format_log_line(&record);
    let parsed = parse_log_line(line.trim_end_matches('\n')).expect("parse log line");
    assert_eq!(parsed.timestamp, "1970-01-01T00:00:00.000Z");
    assert_eq!(parsed.level, LogLevel::Warn);
    assert_eq!(parsed.module, "application");
    assert_eq!(parsed.code, "application/previous_run_unclean");
    assert_eq!(parsed.message, "The previous run ended unexpectedly");
    assert_eq!(parsed.context_field_count, 2);

    assert!(parse_log_line("not a log line").is_none());
    assert!(parse_log_line(&line.replace("application/previous", "application/unknown")).is_none());
    assert!(parse_log_line(&line.replace("WARN ", "WARNX")).is_none());
    assert!(!is_log_file_name(
        LogStream::Application,
        "application-2026-02-30.log"
    ));
    assert!(!is_log_file_name(
        LogStream::Application,
        "application-2026-02-28.0.log"
    ));
    assert!(is_log_file_name(
        LogStream::Application,
        "application-2026-02-28.7.log"
    ));
}
