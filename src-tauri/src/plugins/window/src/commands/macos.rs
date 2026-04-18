#![allow(deprecated)]
use crate::MAIN_WINDOW_LABEL;
use tauri::{AppHandle, Runtime, WebviewWindow, command};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel};

enum MacOSPanelStatus {
    Show,
    Hide,
    SetAlwaysOnTop(bool),
}

fn is_main_window<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    window.label() == MAIN_WINDOW_LABEL
}

fn set_macos_panel<R: Runtime>(
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

                        panel.set_collection_behavior(
                            CollectionBehavior::new()
                                .stationary()
                                .can_join_all_spaces()
                                .full_screen_auxiliary()
                                .into(),
                        );
                    }
                    MacOSPanelStatus::Hide => {
                        panel.hide();

                        panel.set_collection_behavior(
                            CollectionBehavior::new()
                                .stationary()
                                .move_to_active_space()
                                .full_screen_auxiliary()
                                .into(),
                        );
                    }
                    MacOSPanelStatus::SetAlwaysOnTop(always_on_top) => {
                        if always_on_top {
                            panel.set_level(PanelLevel::Dock.value());
                        } else {
                            panel.set_level(-1);
                        };
                    }
                }
            }
        });
    }
}

#[command]
pub async fn show_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    if is_main_window(&window) {
        set_macos_panel(&app_handle, &window, MacOSPanelStatus::Show);
    } else {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[command]
pub async fn hide_window<R: Runtime>(app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    if is_main_window(&window) {
        set_macos_panel(&app_handle, &window, MacOSPanelStatus::Hide);
    } else {
        let _ = window.hide();
    }
}

#[command]
pub async fn set_always_on_top<R: Runtime>(
    app_handle: AppHandle<R>,
    window: WebviewWindow<R>,
    always_on_top: bool,
) {
    if is_main_window(&window) {
        return set_macos_panel(
            &app_handle,
            &window,
            MacOSPanelStatus::SetAlwaysOnTop(always_on_top),
        );
    }

    if always_on_top {
        let _ = window.set_always_on_bottom(false);
        let _ = window.set_always_on_top(true);
    } else {
        let _ = window.set_always_on_top(false);
        let _ = window.set_always_on_bottom(true);
    }
}

#[command]
pub async fn set_taskbar_visibility<R: Runtime>(app_handle: AppHandle<R>, visible: bool) {
    let _ = app_handle.set_dock_visibility(visible);
}
