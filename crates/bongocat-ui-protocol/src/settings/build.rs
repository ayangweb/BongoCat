//! Which build is running, and what it can export.
//!
//! A Development build and a Production build are the same binary with
//! different data roots, and a diagnostics bundle that mixed the two would be
//! useless. The environment travels in the snapshot so the window can say which
//! it is talking to.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsBuildEnvironment {
    Development,
    Production,
}

impl SettingsBuildEnvironment {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsBuildInfo {
    pub product_version: String,
    pub environment: SettingsBuildEnvironment,
}

/// Version of the anonymous diagnostics export JSON contract.
pub const DIAGNOSTICS_EXPORT_FORMAT_VERSION: u32 = 1;
