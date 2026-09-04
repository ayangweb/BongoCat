#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use std::fmt;

pub use bongocat_runtime::{PlatformInputDiagnostics, PlatformInputServiceStatus};

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
    window_content_top_inset,
};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsInputService;
#[cfg(target_os = "windows")]
pub use windows::{
    NativeWindowError, current_display_bounds, display_bounds_for_window, global_window_origin,
    hide_native_window, local_window_origin, request_native_window_close, show_native_window,
    terminate_after_product_shutdown,
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
