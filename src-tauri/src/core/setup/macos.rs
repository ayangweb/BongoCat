#![allow(deprecated)]
use tauri::{AppHandle, Emitter, EventTarget, Manager, WebviewWindow};
use tauri_nspanel::{CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt, tauri_panel};
use tauri_plugin_custom_window::MAIN_WINDOW_LABEL;

const WINDOW_FOCUS_EVENT: &str = "tauri://focus";
const WINDOW_BLUR_EVENT: &str = "tauri://blur";
const WINDOW_MOVED_EVENT: &str = "tauri://move";
const WINDOW_RESIZED_EVENT: &str = "tauri://resize";

tauri_panel! {
    panel!(NsPanel {
        config: {
            is_floating_panel: true,
            can_become_key_window: true,
            can_become_main_window: false
        }
    })

    panel_event!(NsPanelEventHandler {
        window_did_become_key(notification: &NSNotification) -> (),
        window_did_resign_key(notification: &NSNotification) -> (),
        window_did_resize(notification: &NSNotification) -> (),
        window_did_move(notification: &NSNotification) -> (),
    })
}

pub fn platform(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    _preference_window: WebviewWindow,
) {
    let _ = app_handle.plugin(tauri_nspanel::init());

    let _ = app_handle.set_dock_visibility(false);

    let panel = main_window.to_panel::<NsPanel>().unwrap();

    panel.set_level(PanelLevel::Dock.value());

    panel.set_style_mask(StyleMask::empty().resizable().nonactivating_panel().into());

    panel.set_collection_behavior(
        CollectionBehavior::new()
            .stationary()
            .move_to_active_space()
            .full_screen_auxiliary()
            .into(),
    );

    let handler = NsPanelEventHandler::new();

    let focus_window = main_window.clone();
    handler.window_did_become_key(move |_| {
        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        let _ = focus_window.emit_to(target, WINDOW_FOCUS_EVENT, true);
    });

    let blur_window = main_window.clone();
    handler.window_did_resign_key(move |_| {
        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        let _ = blur_window.emit_to(target, WINDOW_BLUR_EVENT, true);
    });

    fn emit_position(window: &WebviewWindow) {
        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        if let Ok(position) = window.outer_position() {
            let _ = window.emit_to(target, WINDOW_MOVED_EVENT, position);
        }
    }

    let resize_window = main_window.clone();
    handler.window_did_resize(move |_| {
        emit_position(&resize_window);

        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        if let Ok(size) = resize_window.inner_size() {
            let _ = resize_window.emit_to(target, WINDOW_RESIZED_EVENT, size);
        }
    });

    let move_window = main_window.clone();
    handler.window_did_move(move |_| {
        emit_position(&move_window);
    });

    panel.set_event_handler(Some(handler.as_ref()));
}
