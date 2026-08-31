#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use std::fmt;

mod directory_picker;
#[cfg(target_os = "macos")]
mod directory_picker_macos;
#[cfg(target_os = "windows")]
mod directory_picker_windows;
pub use directory_picker::{DirectoryPickerError, DirectoryPickerOutcome};

mod single_instance;
pub use single_instance::{SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceError};
#[cfg(target_os = "windows")]
mod single_instance_windows;
#[cfg(target_os = "windows")]
pub use single_instance_windows::{SingleInstance, SingleInstanceStart};

mod system_menu;
pub use system_menu::{SystemMenuAction, SystemMenuError};
#[cfg(target_os = "macos")]
mod system_menu_macos;
#[cfg(target_os = "macos")]
pub use system_menu_macos::SystemMenu;
#[cfg(target_os = "windows")]
mod system_menu_windows;
#[cfg(target_os = "windows")]
pub use system_menu_windows::SystemMenu;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    MacInputService, input_monitoring_permission, request_input_monitoring_permission,
};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsInputService;
#[cfg(target_os = "windows")]
pub use windows::{
    NativeWindowError, hide_native_window, request_native_window_close, show_native_window,
    terminate_after_product_shutdown,
};

pub fn pick_model_directory(
    on_complete: impl FnOnce(Result<DirectoryPickerOutcome, DirectoryPickerError>) + Send + 'static,
) -> Result<(), DirectoryPickerError> {
    #[cfg(target_os = "macos")]
    return directory_picker_macos::pick_model_directory(on_complete);

    #[cfg(target_os = "windows")]
    return directory_picker_windows::pick_model_directory(on_complete);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = on_complete;
        Err(DirectoryPickerError::UnsupportedPlatform)
    }
}

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
    pub reconciled_releases: u64,
    pub decode_errors: u64,
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
    WindowClassRegistrationFailed,
    WindowCreateFailed,
    SessionNotificationFailed,
    RawInputRegistrationFailed,
    TimerCreateFailed,
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
            Self::WindowClassRegistrationFailed => "Raw Input window class registration failed",
            Self::WindowCreateFailed => "Raw Input owner window creation failed",
            Self::SessionNotificationFailed => "Windows session notification registration failed",
            Self::RawInputRegistrationFailed => "Raw Input device registration failed",
            Self::TimerCreateFailed => "Windows input service timer creation failed",
            Self::RuntimeStopped => "runtime stopped while the input service was active",
            Self::StartupTimedOut => "input service startup timed out",
            Self::ShutdownTimedOut => "input service shutdown timed out",
            Self::WorkerPanicked => "input service worker panicked",
        })
    }
}

impl std::error::Error for PlatformInputError {}
