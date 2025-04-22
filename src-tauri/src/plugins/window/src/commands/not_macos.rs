use super::{shared_hide_window, shared_show_window};
use tauri::{command, AppHandle, Runtime, WebviewWindow};

#[command]
pub async fn show_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    shared_show_window(&app_handle, &window);
}

#[command]
pub async fn hide_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    shared_hide_window(&app_handle, &window);
}
