#![cfg_attr(not(target_os = "macos"), forbid(unsafe_code))]

use std::fmt;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    MacInputService, input_monitoring_permission, request_input_monitoring_permission,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPermission {
    Denied,
    Granted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformInputDiagnostics {
    pub captured_edges: u64,
    pub queued_edges: u64,
    pub consumed_edges: u64,
    pub unmapped_keys: u64,
    pub unsupported_buttons: u64,
    pub callback_panics: u64,
    pub capture_queue_overflows: u64,
    pub capture_queue_discarded: u64,
    pub runtime_queue_overflows: u64,
    pub recovery_resets: u64,
    pub reconciliation_runs: u64,
    pub tap_restarts: u64,
    pub rejected_after_stop: u64,
    pub cursor_captured: u64,
    pub cursor_coalesced: u64,
    pub cursor_consumed: u64,
    pub cursor_display_lookup_failures: u64,
    pub cursor_publish_rejections: u64,
    pub cursor_rejected_after_stop: u64,
    pub clean_shutdown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformInputError {
    BackendUnavailable,
    PermissionDenied,
    TapCreateFailed,
    RunLoopSourceFailed,
    RuntimeStopped,
    StartupTimedOut,
    ShutdownTimedOut,
    WorkerPanicked,
}

impl fmt::Display for PlatformInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BackendUnavailable => "the platform input backend is not available",
            Self::PermissionDenied => "input monitoring permission is denied",
            Self::TapCreateFailed => "CGEventTap could not be created",
            Self::RunLoopSourceFailed => "CGEventTap run-loop source could not be created",
            Self::RuntimeStopped => "runtime stopped while the input service was active",
            Self::StartupTimedOut => "input service startup timed out",
            Self::ShutdownTimedOut => "input service shutdown timed out",
            Self::WorkerPanicked => "input service worker panicked",
        })
    }
}

impl std::error::Error for PlatformInputError {}
