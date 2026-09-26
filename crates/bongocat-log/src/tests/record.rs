//! The line format is stable, and a multiline value is escaped.

use super::*;

#[test]
fn text_format_is_stable_and_escapes_multiline_context() {
    let record = LogRecord::new(
        SystemTime::UNIX_EPOCH,
        LogLevel::Info,
        "application",
        "application/started",
        "Application started",
    )
    .with_context("model_id", "standard\nnext");
    assert_eq!(
        format_log_line(&record),
        "1970-01-01T00:00:00.000Z INFO  [application] application/started | Application started | model_id=standard\\nnext\n"
    );
}
