#![allow(deprecated)]
use super::{is_main_window, shared_hide_window, shared_set_always_on_top, shared_show_window};
use crate::MAIN_WINDOW_LABEL;
use tauri::{AppHandle, Runtime, WebviewWindow, command};
use tauri_nspanel::{ManagerExt, cocoa::appkit::NSMainMenuWindowLevel};

pub enum MacOSPanelStatus {
    Show,
    Hide,
    SetAlwaysOnTop(bool),
}

#[command]
pub async fn show_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    if is_main_window(&window) {
        set_macos_panel(&app_handle, &window, MacOSPanelStatus::Show);
    } else {
        shared_show_window(&app_handle, &window);
    }
}

#[command]
pub async fn hide_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    if is_main_window(&window) {
        set_macos_panel(&app_handle, &window, MacOSPanelStatus::Hide);
    } else {
        shared_hide_window(&app_handle, &window);
    }
}

#[command]
pub async fn set_always_on_top<R: Runtime>(
    app_handle: AppHandle<R>,
    window: WebviewWindow<R>,
    always_on_top: bool,
) {
    if is_main_window(&window) {
        set_macos_panel(
            &app_handle,
            &window,
            MacOSPanelStatus::SetAlwaysOnTop(always_on_top),
        );
    } else {
        shared_set_always_on_top(&app_handle, &window, always_on_top);
    }
}

pub fn set_macos_panel<R: Runtime>(
    app_handle: &AppHandle<R>,
    window: &WebviewWindow<R>,
    status: MacOSPanelStatus,
) {
    if is_main_window(window) {
        let app_handle_clone = app_handle.clone();

        let _ = app_handle.run_on_main_thread(move || {
            if let Ok(panel) = app_handle_clone.get_webview_panel(MAIN_WINDOW_LABEL) {
                match status {
                    MacOSPanelStatus::Show => {
                        panel.show();
                    }
                    MacOSPanelStatus::Hide => {
                        panel.order_out(None);
                    }
                    MacOSPanelStatus::SetAlwaysOnTop(always_on_top) => {
                        if always_on_top {
                            panel.set_level(NSMainMenuWindowLevel);
                        } else {
                            panel.set_level(-1);
                        };
                    }
                }
            }
        });
    }
}
