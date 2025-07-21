use tauri::{AppHandle, WebviewWindow};
use tauri_nspanel::{CollectionBehavior, StyleMask, WebviewWindowExt, tauri_panel};

// const WINDOW_FOCUS_EVENT: &str = "tauri://focus";
// const WINDOW_BLUR_EVENT: &str = "tauri://blur";
// const WINDOW_MOVED_EVENT: &str = "tauri://move";
// const WINDOW_RESIZED_EVENT: &str = "tauri://resize";

tauri_panel! {
    panel!(MainPanel {
        config: {
            canBecomeKeyWindow: true,
            canBecomeMainWindow: false
        }
    })

    panel_event!(PanelEventHandler {
        windowDidBecomeKey(notification: &NSNotification) -> (),
        windowDidResignKey(notification: &NSNotification) -> (),
        windowDidMove(notification: &NSNotification) -> (),
        windowDidResize(notification: &NSNotification) -> ()
    })
}

pub fn platform(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    _preference_window: WebviewWindow,
) {
    let _ = app_handle.plugin(tauri_nspanel::init());

    let _ = app_handle.set_dock_visibility(false);

    let panel = main_window.to_panel::<MainPanel>().unwrap();

    panel.set_style_mask(StyleMask::empty().nonactivating_panel().resizable().into());

    panel.set_collection_behavior(
        CollectionBehavior::new()
            .full_screen_auxiliary()
            .can_join_all_spaces()
            .into(),
    );

    let handler = PanelEventHandler::new();

    handler.window_did_become_key(move |notification| {
        println!("window_did_become_key {:?}", notification);
    });

    handler.window_did_resign_key(move |notification| {
        println!("window_did_resign_key {:?}", notification);
    });

    handler.window_did_move(move |notification| {
        println!("window_did_move {:?}", notification);
    });

    handler.window_did_resize(move |notification| {
        println!("window_did_resize {:?}", notification);
    });
}
