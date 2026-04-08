use tauri::{AppHandle, WebviewWindow};

pub fn platform(
    _app_handle: &AppHandle,
    _main_window: WebviewWindow,
    preference_window: WebviewWindow,
) {
    let _ = preference_window.set_skip_taskbar(true);
}
