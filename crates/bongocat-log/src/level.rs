//! How loud the logger is, and what a level reads as.
//!
//! The order is error-first, so a filter is a comparison rather than a set: a
//! logger set to `Info` writes `Info`, `Warn` and `Error` and drops the rest. The
//! padded rendering is what makes a line's level column line up, which is why
//! `Display` is not just the variant's name.

use super::*;

/// Severity used both as the record level and as the configured write
/// threshold. Lower numeric values are more severe.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub const fn is_enabled(self, configured: Self) -> bool {
        (self as u8) <= (configured as u8)
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        })
    }
}
