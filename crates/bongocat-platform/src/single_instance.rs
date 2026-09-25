#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleInstanceEnvironment {
    Development,
    Production,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleInstanceAction {
    OpenSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SingleInstanceError {
    #[error("the single-instance mutex could not be created")]
    MutexCreateFailed,
    #[error("the single-instance wake message could not be registered")]
    WakeMessageRegistrationFailed,
    #[error("the single-instance owner window class could not be registered")]
    WindowClassRegistrationFailed,
    #[error("the single-instance owner window could not be created")]
    WindowCreateFailed,
    #[error("the primary application instance did not become available")]
    PrimaryUnavailable,
    #[error("the primary application instance could not be notified")]
    WakeFailed,
    #[error("the single-instance owner did not shut down cleanly")]
    ShutdownFailed,
}

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
