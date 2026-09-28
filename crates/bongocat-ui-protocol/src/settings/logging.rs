//! Language and logging policy.
//!
//! The log level is a policy the window sets and the application obeys, so the
//! catalogue of levels is part of the protocol rather than a UI list: an
//! application that did not know a level could not be configured to it.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsLanguage {
    #[default]
    System,
    ChineseSimplified,
    EnglishUnitedStates,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsLogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl SettingsLogLevel {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsLogging {
    pub level: SettingsLogLevel,
    pub retention_days: u8,
}

impl Default for SettingsLogging {
    fn default() -> Self {
        Self {
            level: SettingsLogLevel::Info,
            retention_days: 7,
        }
    }
}

impl SettingsLanguage {
    pub const ALL: [Self; 3] = [
        Self::System,
        Self::ChineseSimplified,
        Self::EnglishUnitedStates,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ChineseSimplified => "zh-CN",
            Self::EnglishUnitedStates => "en-US",
        }
    }

    /// Returns the stable locale code a presentation adapter should use.
    ///
    /// The platform resolves `system` before it reaches a UI snapshot. Keeping
    /// the English fallback here also makes pure UI presentation deterministic
    /// when a snapshot is not available yet.
    pub const fn catalog_locale(self) -> &'static str {
        match self {
            Self::ChineseSimplified => "zh-CN",
            Self::System | Self::EnglishUnitedStates => "en-US",
        }
    }
}
