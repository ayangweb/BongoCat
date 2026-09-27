//! What the logger may say, and how loudly.
//!
//! The level is stored by name rather than by number so a hand-edited document
//! that says `"warn"` stays readable and so an unknown name is a validation
//! failure rather than a silent fallback to a level nobody asked for.

use super::*;

/// User-controlled filtering and retention for the human-readable application
/// and Cubism Core logs. Daily rollover and the per-file size guard are fixed
/// safety policy and therefore intentionally do not appear in configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: LoggingLevel,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 30))
    )]
    pub retention_days: u8,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LoggingLevel::default(),
            retention_days: DEFAULT_LOG_RETENTION_DAYS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoggingLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LoggingLevel {
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}
