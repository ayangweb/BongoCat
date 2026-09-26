//! Reading a line back, which is what the diagnostics view does.
//!
//! The parser is strict. A line it cannot read exactly is not guessed at, because
//! a half-parsed line shown as if it were whole is worse than a line the view
//! says it could not read — the log is what a user attaches to a bug report, and
//! it has to be trustworthy enough to act on.

use super::*;

/// Strictly parsed prefix of one project text-log record.
///
/// The parser validates the shared line grammar but deliberately does not
/// expose context values. Diagnostics may use the fixed level/module/code and
/// message after matching the code against its own closed catalog, but cannot
/// accidentally copy timestamps or user-adjacent context into an export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsedLogLine<'a> {
    pub timestamp: &'a str,
    pub level: LogLevel,
    pub module: &'a str,
    pub code: &'a str,
    pub message: &'a str,
    pub context_field_count: usize,
}

/// Parse one line produced by the shared text writer without exposing its
/// context values to the caller.
pub fn parse_log_line(line: &str) -> Option<ParsedLogLine<'_>> {
    let bytes = line.as_bytes();
    if bytes.len() < 40 || bytes[23] != b'Z' || bytes[24] != b' ' {
        return None;
    }
    let timestamp = line.get(..24)?;
    if !is_valid_timestamp(timestamp) {
        return None;
    }
    let level = parse_padded_level(line.get(25..30)?)?;
    if bytes[30] != b' ' || bytes[31] != b'[' {
        return None;
    }

    let module_remainder = line.get(32..)?;
    let module_end = module_remainder.find("] ")?;
    let module = &module_remainder[..module_end];
    if !is_stable_fragment(module, MAXIMUM_MODULE_BYTES) {
        return None;
    }
    let payload_start = 32 + module_end + 2;
    let (code, remainder) = line.get(payload_start..)?.split_once(" | ")?;
    if !is_stable_fragment(code, MAXIMUM_CODE_BYTES) {
        return None;
    }
    let (message, context) = remainder
        .split_once(" | ")
        .map_or((remainder, ""), |(message, context)| (message, context));
    if !is_bounded_sanitized_text(message, MAXIMUM_MESSAGE_BYTES) {
        return None;
    }

    let mut context_field_count = 0;
    if !context.is_empty() {
        for field in context.split(" | ") {
            context_field_count += 1;
            if context_field_count > MAXIMUM_CONTEXT_FIELDS {
                return None;
            }
            let (key, value) = field.split_once('=')?;
            if !is_stable_fragment(key, MAXIMUM_CONTEXT_KEY_BYTES)
                || !is_bounded_sanitized_text(value, MAXIMUM_CONTEXT_VALUE_BYTES)
            {
                return None;
            }
        }
    }

    Some(ParsedLogLine {
        timestamp,
        level,
        module,
        code,
        message,
        context_field_count,
    })
}

pub(crate) fn is_valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        })
        && UtcDate::parse(&value[..10]).is_some()
        && parse_decimal(&bytes[11..13]).is_some_and(|hour| hour < 24)
        && parse_decimal(&bytes[14..16]).is_some_and(|minute| minute < 60)
        && parse_decimal(&bytes[17..19]).is_some_and(|second| second < 60)
        && parse_decimal(&bytes[20..23]).is_some_and(|millisecond| millisecond < 1_000)
}

pub(crate) fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse::<u32>().ok()
}

pub(crate) fn parse_padded_level(value: &str) -> Option<LogLevel> {
    match value.as_bytes() {
        b"ERROR" | b"ERROR " => Some(LogLevel::Error),
        b"WARN " => Some(LogLevel::Warn),
        b"INFO " => Some(LogLevel::Info),
        b"DEBUG" | b"DEBUG " => Some(LogLevel::Debug),
        b"TRACE" | b"TRACE " => Some(LogLevel::Trace),
        _ => None,
    }
}

pub(crate) fn is_stable_fragment(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

pub(crate) fn is_bounded_sanitized_text(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && !value.chars().any(char::is_control)
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| byte != b'|' || index > 0 && value.as_bytes()[index - 1] == b'\\')
}
