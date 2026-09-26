//! Keeping real user data out of the log.
//!
//! A log line can carry a path, a title or an error message, and those are the
//! things a log must not carry: the product's own rule is that no real key
//! sequence, clipboard content, user file content or secret is written. What goes
//! in their place is bounded and marked, so a reader can tell that something was
//! there and see how much of it survived.

pub(crate) const MAXIMUM_MODULE_BYTES: usize = 64;

pub(crate) const MAXIMUM_CODE_BYTES: usize = 128;

pub(crate) const MAXIMUM_MESSAGE_BYTES: usize = 512;

pub(crate) const MAXIMUM_CONTEXT_KEY_BYTES: usize = 48;

pub(crate) const MAXIMUM_CONTEXT_VALUE_BYTES: usize = 160;

pub(crate) const MAXIMUM_CONTEXT_FIELDS: usize = 8;

pub(crate) const TRUNCATION_MARKER: &str = "...[truncated]";

pub(crate) fn sanitize_fragment(value: &str, maximum_bytes: usize, escape_pipe: bool) -> String {
    let mut output = String::with_capacity(value.len().min(maximum_bytes));
    let mut truncated = false;
    for character in value.chars() {
        let rendered = match character {
            '\r' => "\\r".to_owned(),
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\u{2028}' => "\\u{2028}".to_owned(),
            '\u{2029}' => "\\u{2029}".to_owned(),
            '|' if escape_pipe => "\\|".to_owned(),
            character if character.is_control() => "\u{fffd}".to_owned(),
            character => character.to_string(),
        };
        if output.len().saturating_add(rendered.len()) > maximum_bytes {
            truncated = true;
            break;
        }
        output.push_str(&rendered);
    }
    if truncated {
        let marker = TRUNCATION_MARKER.len().min(maximum_bytes);
        while output.len().saturating_add(marker) > maximum_bytes {
            output.pop();
        }
        output.push_str(&TRUNCATION_MARKER[..marker]);
    }
    output
}
