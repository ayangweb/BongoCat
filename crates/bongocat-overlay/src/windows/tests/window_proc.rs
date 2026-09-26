//! Which message means hit test, resize drag or context menu.

use super::*;

#[test]
fn click_through_style_pairs_layered_and_transparent_hit_testing() {
    let canvas = CanvasInfo {
        width: 2048.0,
        height: 2048.0,
        origin_x: 1024.0,
        origin_y: 1024.0,
        pixels_per_unit: 1024.0,
    };
    let window = OverlayWindow::create(
        OverlaySessionOptions {
            click_through: true,
            ..OverlaySessionOptions::default()
        },
        canvas,
        None,
        None,
        None,
    )
    .expect("create click-through overlay window");
    let transparent = WS_EX_TRANSPARENT.0 as isize;
    let layered = WS_EX_LAYERED.0 as isize;
    // SAFETY: the test owns the live HWND and reads its extended style on
    // the creation thread.
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) };
    assert_ne!(style & transparent, 0);
    assert_ne!(style & layered, 0);

    window
        .set_click_through(false)
        .expect("disable overlay click-through");
    // SAFETY: the test owns the live HWND and reads its extended style on
    // the creation thread.
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) };
    assert_eq!(style & transparent, 0);
    assert_eq!(style & layered, 0);

    window
        .set_click_through(true)
        .expect("enable overlay click-through");
    // SAFETY: the test owns the live HWND and reads its extended style on
    // the creation thread.
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) };
    assert_ne!(style & transparent, 0);
    assert_ne!(style & layered, 0);
}

#[test]
fn context_menu_messages_cover_client_and_nonclient_right_click() {
    assert!(requests_context_menu(WM_CONTEXTMENU));
    assert!(!requests_context_menu(WM_CLOSE));
    // The right-button release is the resize drag's end, not a menu click:
    // it decides between the two from the drag state it was given.
    assert!(!requests_context_menu(WM_NCRBUTTONUP));
}

#[test]
fn right_button_messages_split_into_begin_move_and_end() {
    assert!(begins_resize_drag(WM_NCRBUTTONDOWN));
    assert!(begins_resize_drag(WM_RBUTTONDOWN));
    assert!(moves_resize_drag(WM_NCMOUSEMOVE));
    assert!(moves_resize_drag(WM_MOUSEMOVE));
    assert!(ends_resize_drag(WM_NCRBUTTONUP));
    assert!(ends_resize_drag(WM_RBUTTONUP));

    // A left click still drags the window through the system move loop, so
    // the resize drag must not claim its messages.
    assert!(!begins_resize_drag(WM_MOUSEMOVE));
    assert!(!ends_resize_drag(WM_CONTEXTMENU));
    assert!(!begins_resize_drag(WM_CLOSE));
    assert!(!ends_resize_drag(WM_CAPTURECHANGED));
}
