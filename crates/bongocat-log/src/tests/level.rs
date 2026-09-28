//! The levels are ordered, and a padded one lines up.

use super::*;

#[test]
fn levels_are_ordered_from_error_to_trace() {
    assert!(LogLevel::Error.is_enabled(LogLevel::Trace));
    assert!(LogLevel::Warn.is_enabled(LogLevel::Info));
    assert!(LogLevel::Info.is_enabled(LogLevel::Info));
    assert!(!LogLevel::Debug.is_enabled(LogLevel::Info));
    assert_eq!(LogLevel::default(), LogLevel::Info);
    assert_eq!(LogLevel::Trace.to_string(), "TRACE");
}
