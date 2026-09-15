use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleInstanceEnvironment {
    Development,
    Production,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleInstanceAction {
    OpenSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleInstanceError {
    MutexCreateFailed,
    WakeMessageRegistrationFailed,
    WindowClassRegistrationFailed,
    WindowCreateFailed,
    PrimaryUnavailable,
    WakeFailed,
    ShutdownFailed,
}

impl fmt::Display for SingleInstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MutexCreateFailed => "the single-instance mutex could not be created",
            Self::WakeMessageRegistrationFailed => {
                "the single-instance wake message could not be registered"
            }
            Self::WindowClassRegistrationFailed => {
                "the single-instance owner window class could not be registered"
            }
            Self::WindowCreateFailed => "the single-instance owner window could not be created",
            Self::PrimaryUnavailable => "the primary application instance did not become available",
            Self::WakeFailed => "the primary application instance could not be notified",
            Self::ShutdownFailed => "the single-instance owner did not shut down cleanly",
        })
    }
}

impl std::error::Error for SingleInstanceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_are_explicit_and_distinct() {
        assert_ne!(
            SingleInstanceEnvironment::Development,
            SingleInstanceEnvironment::Production
        );
    }
}
