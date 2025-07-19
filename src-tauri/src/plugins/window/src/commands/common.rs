use super::{shared_hide_window, shared_set_always_on_top, shared_show_window};
use tauri::{AppHandle, Runtime, WebviewWindow, command};

#[command]
pub async fn show_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    shared_show_window(&app_handle, &window);
}

#[command]
pub async fn hide_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    shared_hide_window(&app_handle, &window);
}

#[command]
pub async fn set_always_on_top<R: Runtime>(
    app_handle: AppHandle<R>,
    window: WebviewWindow<R>,
    always_on_top: bool,
) {
    shared_set_always_on_top(&app_handle, &window, always_on_top);
}

#[command]
pub async fn set_taskbar_visibility<R: Runtime>(window: WebviewWindow<R>, visible: bool) {
    let _ = window.set_skip_taskbar(!visible);
}
