//! The stable error vocabulary the platform input services report.
//!
//! Both backends fail for their own reasons — a raw-input registration, an
//! event tap, a run-loop source — but the product reads one closed set of
//! outcomes: the service maps every native failure onto a variant here, and the
//! service status, the application log and the anonymous diagnostics report all
//! quote the same code.

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PlatformInputError {
    #[error("{}", Self::BackendUnavailable.as_str())]
    BackendUnavailable,
    #[error("{}", Self::PermissionDenied.as_str())]
    PermissionDenied,
    #[error("{}", Self::TapCreateFailed.as_str())]
    TapCreateFailed,
    #[error("{}", Self::RunLoopSourceFailed.as_str())]
    RunLoopSourceFailed,
    #[error("{}", Self::WindowClassRegistrationFailed.as_str())]
    WindowClassRegistrationFailed,
    #[error("{}", Self::WindowCreateFailed.as_str())]
    WindowCreateFailed,
    #[error("{}", Self::SessionNotificationFailed.as_str())]
    SessionNotificationFailed,
    #[error("{}", Self::RawInputRegistrationFailed.as_str())]
    RawInputRegistrationFailed,
    #[error("{}", Self::TimerCreateFailed.as_str())]
    TimerCreateFailed,
    #[error("{}", Self::RuntimeStopped.as_str())]
    RuntimeStopped,
    #[error("{}", Self::StartupTimedOut.as_str())]
    StartupTimedOut,
    #[error("{}", Self::ShutdownTimedOut.as_str())]
    ShutdownTimedOut,
    #[error("{}", Self::WorkerPanicked.as_str())]
    WorkerPanicked,
}

impl PlatformInputError {
    pub const ALL: [Self; 13] = [
        Self::BackendUnavailable,
        Self::PermissionDenied,
        Self::TapCreateFailed,
        Self::RunLoopSourceFailed,
        Self::WindowClassRegistrationFailed,
        Self::WindowCreateFailed,
        Self::SessionNotificationFailed,
        Self::RawInputRegistrationFailed,
        Self::TimerCreateFailed,
        Self::RuntimeStopped,
        Self::StartupTimedOut,
        Self::ShutdownTimedOut,
        Self::WorkerPanicked,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BackendUnavailable => "platform_input_backend_unavailable",
            Self::PermissionDenied => "platform_input_permission_denied",
            Self::TapCreateFailed => "platform_input_tap_create_failed",
            Self::RunLoopSourceFailed => "platform_input_run_loop_source_failed",
            Self::WindowClassRegistrationFailed => {
                "platform_input_window_class_registration_failed"
            }
            Self::WindowCreateFailed => "platform_input_window_create_failed",
            Self::SessionNotificationFailed => "platform_input_session_notification_failed",
            Self::RawInputRegistrationFailed => "platform_input_raw_input_registration_failed",
            Self::TimerCreateFailed => "platform_input_timer_create_failed",
            Self::RuntimeStopped => "platform_input_runtime_stopped",
            Self::StartupTimedOut => "platform_input_startup_timed_out",
            Self::ShutdownTimedOut => "platform_input_shutdown_timed_out",
            Self::WorkerPanicked => "platform_input_worker_panicked",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformInputError;

    #[test]
    fn platform_input_error_codes_are_stable_and_unique() {
        let mut codes = PlatformInputError::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| code.starts_with("platform_input_")));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), PlatformInputError::ALL.len());
        assert_eq!(
            PlatformInputError::PermissionDenied.to_string(),
            "platform_input_permission_denied"
        );
        assert!(
            PlatformInputError::ALL
                .iter()
                .all(|code| bongocat_input::is_stable_platform_input_error_code(code.as_str()))
        );
    }
}
