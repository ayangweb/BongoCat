//! One line, and the two streams support tells apart.
//!
//! The application and Cubism Core write to separate files so a support thread
//! can tell a vendor message from the product's own. The line format is fixed
//! rather than configurable: a stable line is what makes the log parseable by
//! the diagnostics view and by hand, and a stable line is worth more than a
//! configurable one.

use super::*;

/// The two product-owned text streams. Adding a third stream requires making
/// its filename and diagnostics ownership explicit in the same change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogStream {
    Application,
    CubismCore,
}

impl LogStream {
    pub(crate) const fn prefix(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::CubismCore => "cubism-core",
        }
    }

    pub(crate) fn active_file_name(self, date: UtcDate) -> String {
        format!("{}-{}.log", self.prefix(), date.as_string())
    }

    pub(crate) fn rotated_file_name(self, date: UtcDate, generation: u64) -> String {
        format!("{}-{}.{generation}.log", self.prefix(), date.as_string())
    }
}

/// One fully bounded text record. `module`, `code`, and `message` are intended
/// to be fixed project values. Context is an allow-listed set of small values;
/// callers must not pass paths, raw I/O errors, input events, or resource text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogRecord {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub module: String,
    pub code: String,
    pub message: String,
    pub context: Vec<(String, String)>,
}

impl LogRecord {
    pub fn new(
        timestamp: SystemTime,
        level: LogLevel,
        module: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            level,
            module: module.into(),
            code: code.into(),
            message: message.into(),
            context: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }
}

pub(crate) fn format_log_line(record: &LogRecord) -> String {
    let timestamp = format_timestamp(record.timestamp);
    let level = record.level.to_string();
    let module = sanitize_fragment(&record.module, MAXIMUM_MODULE_BYTES, false);
    let code = sanitize_fragment(&record.code, MAXIMUM_CODE_BYTES, false);
    let message = sanitize_fragment(&record.message, MAXIMUM_MESSAGE_BYTES, true);
    let mut line = format!("{timestamp} {level:<5} [{module}] {code} | {message}");

    for (key, value) in record.context.iter().take(MAXIMUM_CONTEXT_FIELDS) {
        let key = sanitize_fragment(key, MAXIMUM_CONTEXT_KEY_BYTES, false);
        let value = sanitize_fragment(value, MAXIMUM_CONTEXT_VALUE_BYTES, true);
        if !key.is_empty() && !value.is_empty() {
            line.push_str(" | ");
            line.push_str(&key);
            line.push('=');
            line.push_str(&value);
        }
    }
    line.push('\n');
    line
}
