//! Wayland capabilities. Global placement is intentionally unavailable.
mod alpha_admin;
pub use alpha_admin::{
    AlphaAdminAction, AlphaAdminError, AlphaInstallationStatus, AlphaManagementUnavailable,
    AlphaSystemStatus, inspect_alpha_system, manage_alpha_camera, run_alpha_helper,
};
mod alpha_output;
pub use alpha_output::{
    ALPHA_CAMERA_DEVELOPMENT_NAME, ALPHA_CAMERA_NAME, ALPHA_FRAME_BYTES, ALPHA_FRAME_HEIGHT,
    ALPHA_FRAME_WIDTH, AlphaCameraError, AlphaFrameWrite, LinuxAlphaCamera, alpha_camera_name,
};
mod input;
pub use input::{LinuxInputService, unprivileged_input_is_possible};
pub fn system_language() -> bongocat_config::Language {
    sys_locale::get_locale().map_or_else(Default::default, |locale| {
        bongocat_config::Language::from_system_locale(&locale)
    })
}
pub(crate) static INPUT_AUTHORIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

static POINTER_SENSITIVITY: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(100);
/// The application supplies the validated, committed configuration value.
pub fn set_linux_pointer_sensitivity(percent: u16) {
    POINTER_SENSITIVITY.store(percent, std::sync::atomic::Ordering::Relaxed);
}

mod tray;
pub use tray::LinuxSystemTray;
