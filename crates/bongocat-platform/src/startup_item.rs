use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupItemEnvironment {
    Development,
    Production,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupItemUnsupportedReason {
    Platform,
    OperatingSystem,
    BuildEnvironment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupItemState {
    Unsupported(StartupItemUnsupportedReason),
    Disabled,
    Enabled,
    Stale,
    RequiresApproval,
    NotFound,
}

impl StartupItemState {
    pub const fn can_set_enabled(self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupItemError {
    CurrentExecutableUnavailable,
    InvalidExecutablePath,
    BackendUnavailable,
    StateReadFailed,
    EnableFailed,
    DisableFailed,
}

impl StartupItemError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CurrentExecutableUnavailable => "startup_item_current_executable_unavailable",
            Self::InvalidExecutablePath => "startup_item_invalid_executable_path",
            Self::BackendUnavailable => "startup_item_backend_unavailable",
            Self::StateReadFailed => "startup_item_state_read_failed",
            Self::EnableFailed => "startup_item_enable_failed",
            Self::DisableFailed => "startup_item_disable_failed",
        }
    }
}

impl fmt::Display for StartupItemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for StartupItemError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_keep_actionable_capabilities_distinct() {
        assert!(StartupItemState::Disabled.can_set_enabled());
        assert!(StartupItemState::Enabled.can_set_enabled());
        assert!(StartupItemState::Stale.can_set_enabled());
        assert!(StartupItemState::RequiresApproval.can_set_enabled());
        assert!(StartupItemState::NotFound.can_set_enabled());
        assert!(
            !StartupItemState::Unsupported(StartupItemUnsupportedReason::Platform)
                .can_set_enabled()
        );
    }

    #[test]
    fn errors_have_stable_anonymous_codes() {
        assert_eq!(
            StartupItemError::CurrentExecutableUnavailable.as_str(),
            "startup_item_current_executable_unavailable"
        );
        assert_eq!(
            StartupItemError::InvalidExecutablePath.as_str(),
            "startup_item_invalid_executable_path"
        );
        assert_eq!(
            StartupItemError::BackendUnavailable.as_str(),
            "startup_item_backend_unavailable"
        );
        assert_eq!(
            StartupItemError::StateReadFailed.as_str(),
            "startup_item_state_read_failed"
        );
        assert_eq!(
            StartupItemError::EnableFailed.as_str(),
            "startup_item_enable_failed"
        );
        assert_eq!(
            StartupItemError::DisableFailed.as_str(),
            "startup_item_disable_failed"
        );
    }

    #[test]
    fn environments_are_explicit_and_distinct() {
        assert_ne!(
            StartupItemEnvironment::Development,
            StartupItemEnvironment::Production
        );
    }
}
