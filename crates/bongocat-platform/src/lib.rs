use std::path::PathBuf;

pub use bongocat_input::{PlatformInputDiagnostics, PlatformInputServiceStatus};

mod display;
pub use display::DisplayBounds;

mod input_error;
pub use input_error::PlatformInputError;

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
    InputPermission, STARTUP_PERMISSION_CAPABILITY, StartupPermissionPrompt,
    StartupPermissionStatus, check_startup_permission, startup_permission_available,
};

mod native_window;
pub use native_window::NativeWindowError;

#[cfg(target_os = "macos")]
mod gilrs_gamepad;
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
mod gilrs_gamepad;
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

/// Revalidate and canonicalize a model folder supplied by a non-dialog source.
///
/// File drops bypass the native picker, but they must not bypass its filesystem
/// boundary. Callers run this away from a UI executor; it performs metadata and
/// canonicalization I/O before returning the same stable selection vocabulary.
pub fn validate_model_folder(
    selected: PathBuf,
) -> Result<ModelSourcePickerOutcome, ModelSourcePickerError> {
    model_source_picker::validate_selected_folder(selected)
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
