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
    /// The version of the vendored Cubism Core the running binary is linked
    /// against, in the notation Cubism's own release notes use.
    ///
    /// This is part of the build identity rather than a separate piece of
    /// information because a Live2D model is only guaranteed to load against one
    /// Core version: which models open, and how a parameter is evaluated, both
    /// depend on it. A report that names the app version but not this one cannot
    /// distinguish "this model is malformed" from "this model predates the Core
    /// we ship".
    pub cubism_core_version: String,
}

/// Version of the anonymous diagnostics export JSON contract.
pub const DIAGNOSTICS_EXPORT_FORMAT_VERSION: u32 = 1;
