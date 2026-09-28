//! The login item, and the platforms that cannot have one.
//!
//! A platform without a login item, a build that must not install one, and a
//! user who turned it off are three different states rather than one boolean:
//! only the last is something the window can change.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemStatus {
    State(SettingsStartupItemState),
    ReadError(SettingsStartupItemError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemState {
    Unsupported(SettingsStartupItemUnsupportedReason),
    Disabled,
    Enabled,
    Stale,
    RequiresApproval,
    NotFound,
}

impl SettingsStartupItemState {
    pub const fn can_set_enabled(self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemUnsupportedReason {
    Platform,
    OperatingSystem,
    BuildEnvironment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemError {
    CurrentExecutableUnavailable,
    InvalidExecutablePath,
    BackendUnavailable,
    StateReadFailed,
    EnableFailed,
    DisableFailed,
}
