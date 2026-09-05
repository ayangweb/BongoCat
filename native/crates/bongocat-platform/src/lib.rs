#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use std::fmt;

pub use bongocat_runtime::{PlatformInputDiagnostics, PlatformInputServiceStatus};

mod installation;
pub use installation::InstallationLayout;

mod accessibility;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use accessibility::SettingsAccessibilityBridge;
pub use accessibility::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityBounds, AccessibilityDiagnostics,
    AccessibilityError, AccessibilityNode, AccessibilityNodeId, AccessibilityRole,
    AccessibilityToggle, AccessibilityTree,
};

mod directory_picker;
#[cfg(target_os = "macos")]
mod directory_picker_macos;
#[cfg(target_os = "windows")]
mod directory_picker_windows;
pub use directory_picker::{DirectoryPickerError, DirectoryPickerOutcome};

mod directory_opener;
pub use directory_opener::{DirectoryOpenError, open_directory};

mod clipboard;
#[cfg(target_os = "macos")]
mod clipboard_macos;
#[cfg(target_os = "windows")]
mod clipboard_windows;
pub use clipboard::{ClipboardError, read_clipboard_text, write_clipboard_text};

mod url_opener;
pub use url_opener::{ExternalUrlOpenError, open_external_url};

mod single_instance;
pub use single_instance::{SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceError};

mod shortcut;
pub use shortcut::{ShortcutDispatch, ShortcutDispatchError, ShortcutDispatcher, ShortcutMatcher};
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

mod startup_item;
pub use startup_item::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
};
#[cfg(target_os = "macos")]
mod startup_item_macos;
#[cfg(target_os = "windows")]
mod startup_item_windows;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    MacInputService, current_display_bounds, display_bounds_for_window, global_window_origin,
    input_monitoring_permission, local_window_origin, request_input_monitoring_permission,
    system_language, window_content_top_inset,
};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsInputService;
#[cfg(target_os = "windows")]
pub use windows::{
    NativeWindowError, current_display_bounds, display_bounds_for_window, global_window_origin,
    hide_native_window, local_window_origin, request_native_window_close, set_taskbar_icon_visible,
    show_native_window, system_language, taskbar_icon_is_visible, terminate_after_product_shutdown,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayBounds {
    pub display_id: Option<u32>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DisplayBounds {
    #[cfg(any(test, target_os = "macos", target_os = "windows"))]
    fn intersects_window(self, x: f32, y: f32, width: f32, height: f32) -> bool {
        x < self.x + self.width
            && x + width > self.x
            && y < self.y + self.height
            && y + height > self.y
    }
}

#[cfg(test)]
mod display_bounds_tests {
    use super::DisplayBounds;

    #[test]
    fn window_visibility_handles_negative_and_edge_touching_displays() {
        let secondary = DisplayBounds {
            display_id: Some(1),
            x: -1920.0,
            y: -240.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert!(secondary.intersects_window(-1200.0, 100.0, 800.0, 600.0));
        assert!(secondary.intersects_window(-10.0, 100.0, 800.0, 600.0));
        assert!(!secondary.intersects_window(0.0, 100.0, 800.0, 600.0));
        assert!(!secondary.intersects_window(-1200.0, 840.0, 800.0, 600.0));
    }
}

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

pub fn startup_item_state(
    environment: StartupItemEnvironment,
) -> Result<StartupItemState, StartupItemError> {
    #[cfg(target_os = "macos")]
    return startup_item_macos::state(environment);

    #[cfg(target_os = "windows")]
    return startup_item_windows::state(environment);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = environment;
        Ok(StartupItemState::Unsupported(
            StartupItemUnsupportedReason::Platform,
        ))
    }
}

pub fn set_startup_item_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<StartupItemState, StartupItemError> {
    #[cfg(target_os = "macos")]
    return startup_item_macos::set_enabled(environment, enabled);

    #[cfg(target_os = "windows")]
    return startup_item_windows::set_enabled(environment, enabled);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (environment, enabled);
        Ok(StartupItemState::Unsupported(
            StartupItemUnsupportedReason::Platform,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPermission {
    Denied,
    Granted,
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

impl fmt::Display for PlatformInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for PlatformInputError {}

#[cfg(test)]
mod platform_input_error_tests {
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
    }
}
