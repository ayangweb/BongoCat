use std::fmt;

pub use bongocat_runtime::{PlatformInputDiagnostics, PlatformInputServiceStatus};

mod installation;
pub use installation::InstallationLayout;

mod model_source_picker;
pub use model_source_picker::{ModelSourcePickerError, ModelSourcePickerOutcome};

mod directory_opener;
pub use directory_opener::{DirectoryOpenError, open_directory};

mod clipboard;
pub use clipboard::{ClipboardError, read_clipboard_text, write_clipboard_text};

mod url_opener;
pub use url_opener::{ExternalUrlOpenError, open_external_url};

mod single_instance;
pub use single_instance::{SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceError};

mod shortcut;
pub use shortcut::{
    GlobalShortcutCounters, GlobalShortcutService, GlobalShortcutServiceError, ShortcutHotkeyError,
};
pub use shortcut::{ShortcutDispatch, ShortcutDispatchError, ShortcutDispatcher};
#[cfg(target_os = "windows")]
mod single_instance_windows;
#[cfg(target_os = "windows")]
pub use single_instance_windows::{SingleInstance, SingleInstanceStart};

mod theme;
#[cfg(target_os = "macos")]
pub use theme::system_appearance;
pub use theme::{
    AppTheme, NativeThemeError, SystemAppearance, apply_process_theme, apply_theme,
    init_native_theme,
};

mod system_menu;
pub use system_menu::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
mod system_menu_native;
pub use system_menu_native::SystemMenu;

mod startup_item;
pub use startup_item::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
};
mod startup_item_native;

mod startup_permission;
pub use startup_permission::{
    STARTUP_PERMISSION_CAPABILITY, StartupPermissionPrompt, StartupPermissionStatus,
    check_startup_permission, startup_permission_available,
};

mod native_window;
pub use native_window::NativeWindowError;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    MacInputService, current_display_bounds, display_bounds_for_window, global_window_origin,
    hide_native_window, input_monitoring_permission, local_window_origin,
    request_input_monitoring_permission, show_native_window, system_language,
    window_content_top_inset,
};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsInputService;
#[cfg(target_os = "windows")]
pub use windows::{
    current_display_bounds, display_bounds_for_window, global_window_origin, hide_native_window,
    local_window_origin, request_native_window_close, set_taskbar_icon_visible, show_native_window,
    system_language, taskbar_icon_is_visible, terminate_after_product_shutdown,
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

/// Let the user choose the model folder to import.
///
/// The folder a user exported is the only source this entry point offers, and
/// both supported platforms answer it with their own folder panel. A model
/// archive is not selectable here and nothing downstream reads one: the store's
/// archive source was removed together with its reader and its diagnostics
/// (ADR-0036 已撤回), so a dialog that returned any file would promise a source
/// the rest of this path cannot accept.
pub fn pick_model_folder(
    on_complete: impl FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
) -> Result<(), ModelSourcePickerError> {
    model_source_picker::pick_model_folder(on_complete)
}

/// Let the user choose the image that replaces a model's cover.
///
/// The selection vocabulary is shared with the model source picker: the failures
/// a dialog can produce are properties of the dialog, and the settings service
/// reports its own stable code when the chosen bytes are not a cover.
pub fn pick_model_cover(
    on_complete: impl FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
) -> Result<(), ModelSourcePickerError> {
    model_source_picker::pick_model_cover(on_complete)
}

pub fn startup_item_state(
    environment: StartupItemEnvironment,
) -> Result<StartupItemState, StartupItemError> {
    startup_item_native::state(environment)
}

pub fn set_startup_item_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<StartupItemState, StartupItemError> {
    startup_item_native::set_enabled(environment, enabled)
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
        assert!(
            PlatformInputError::ALL
                .iter()
                .all(|code| bongocat_runtime::is_stable_platform_input_error_code(code.as_str()))
        );
    }
}
