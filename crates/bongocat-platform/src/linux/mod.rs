//! Wayland capabilities. Global placement is intentionally unavailable.
mod input;
pub use input::LinuxInputService;
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
