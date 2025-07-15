#![allow(deprecated)]
use tauri::{AppHandle, Emitter, EventTarget, WebviewWindow};
use tauri_nspanel::{WebviewWindowExt, cocoa::appkit::NSWindowCollectionBehavior, panel_delegate};
use tauri_plugin_custom_window::MAIN_WINDOW_LABEL;

#[allow(non_upper_case_globals)]
const NSWindowStyleMaskNonActivatingPanel: i32 = 1 << 7;
#[allow(non_upper_case_globals)]
const NSResizableWindowMask: i32 = 1 << 3;
const WINDOW_FOCUS_EVENT: &str = "tauri://focus";
const WINDOW_BLUR_EVENT: &str = "tauri://blur";
const WINDOW_MOVED_EVENT: &str = "tauri://move";
const WINDOW_RESIZED_EVENT: &str = "tauri://resize";

pub fn platform(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    _preference_window: WebviewWindow,
) {
    let _ = app_handle.plugin(tauri_nspanel::init());

    let _ = app_handle.set_dock_visibility(false);

    let panel = main_window.to_panel().unwrap();

    panel.set_style_mask(NSWindowStyleMaskNonActivatingPanel | NSResizableWindowMask);

    panel.set_collection_behaviour(
        NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
    );

    let delegate = panel_delegate!(EcoPanelDelegate {
        window_did_become_key,
        window_did_resign_key,
        window_did_resize,
        window_did_move
    });

    delegate.set_listener(Box::new(move |delegate_name: String| {
        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        let window_move_event = || {
            if let Ok(position) = main_window.outer_position() {
                let _ = main_window.emit_to(target.clone(), WINDOW_MOVED_EVENT, position);
            }
        };

        match delegate_name.as_str() {
            "window_did_become_key" => {
                let _ = main_window.emit_to(target, WINDOW_FOCUS_EVENT, true);
            }
            "window_did_resign_key" => {
                let _ = main_window.emit_to(target, WINDOW_BLUR_EVENT, true);
            }
            "window_did_resize" => {
                window_move_event();

                if let Ok(size) = main_window.inner_size() {
                    let _ = main_window.emit_to(target, WINDOW_RESIZED_EVENT, size);
                }
            }
            "window_did_move" => window_move_event(),
            _ => (),
        }
    }));

    panel.set_delegate(delegate);
}
