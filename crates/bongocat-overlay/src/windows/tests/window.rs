//! The window class, and correcting a saved box that left the desktop.

use super::*;

#[test]
fn overlay_window_class_supports_overlapping_replacement_windows() {
    let canvas = CanvasInfo {
        width: 2048.0,
        height: 2048.0,
        origin_x: 1024.0,
        origin_y: 1024.0,
        pixels_per_unit: 1024.0,
    };
    let first = OverlayWindow::create(OverlaySessionOptions::default(), canvas, None, None, None)
        .expect("create first overlay window");
    let second = OverlayWindow::create(OverlaySessionOptions::default(), canvas, None, None, None)
        .expect("reuse class for replacement overlay window");

    assert_ne!(first.hwnd, second.hwnd);
    drop(first);
    drop(second);
}

#[test]
fn overlay_window_creation_corrects_a_saved_box_off_the_desktop() {
    let screens = screen_bounds_all();
    let left_most = screens
        .iter()
        .min_by_key(|screen| screen.x)
        .expect("at least one display");
    let canvas = CanvasInfo {
        width: 2_048.0,
        height: 2_048.0,
        origin_x: 1_024.0,
        origin_y: 1_024.0,
        pixels_per_unit: 1_024.0,
    };
    let candidate = OverlayWindowBounds::new(left_most.x - 10_000, left_most.y, 350, 350);
    let expected = correction_for_screens(&screens, candidate).expect("a display to correct into");
    let window = OverlayWindow::create(
        OverlaySessionOptions::default(),
        canvas,
        Some(candidate),
        None,
        None,
    )
    .expect("create constrained overlay window");
    let created = window.bounds().expect("constrained bounds");
    assert_eq!(created, expected);
    assert!(bounds_inside_screens(&screen_bounds_all(), created));
}
