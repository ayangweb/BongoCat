//! The window class, its taskbar button, and correcting a saved box that left
//! the desktop.

use super::*;
use bongocat_runtime::OverlaySettings;

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

/// The `show_taskbar_icon` preference owns the model window's taskbar button,
/// and the window is created with the saved value rather than being corrected
/// after its first frame.
#[test]
fn overlay_window_owns_its_taskbar_button_at_creation() {
    let canvas = CanvasInfo {
        width: 2048.0,
        height: 2048.0,
        origin_x: 1024.0,
        origin_y: 1024.0,
        pixels_per_unit: 1024.0,
    };
    let shown = OverlayWindow::create(
        OverlaySessionOptions {
            taskbar_icon_visible: true,
            ..OverlaySessionOptions::default()
        },
        canvas,
        None,
        None,
        None,
    )
    .expect("create overlay window with a taskbar button");
    assert!(shown.taskbar_icon_is_visible());

    let hidden = OverlayWindow::create(
        OverlaySessionOptions {
            taskbar_icon_visible: false,
            ..OverlaySessionOptions::default()
        },
        canvas,
        None,
        None,
        None,
    )
    .expect("create overlay window without a taskbar button");
    assert!(!hidden.taskbar_icon_is_visible());
}

/// Toggling the taskbar button in place must survive a model switch, which
/// replaces the HWND, so the applied value is recorded on the session options
/// the replacement window is created from.
#[test]
fn taskbar_toggle_preserves_unrelated_extended_styles_and_is_idempotent() {
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
    .expect("create overlay window");
    let click_through = WS_EX_LAYERED.0 as isize | WS_EX_TRANSPARENT.0 as isize;
    // SAFETY: the test owns the live HWND and reads its extended style on the
    // creation thread.
    let unrelated = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) } & click_through;
    assert_eq!(unrelated, click_through);

    window
        .set_taskbar_icon_visible(true)
        .expect("give the model window a taskbar button");
    assert!(window.taskbar_icon_is_visible());
    // A repeat is a no-op rather than a second style write.
    window
        .set_taskbar_icon_visible(true)
        .expect("repeat the taskbar button write");
    // SAFETY: the test owns the live HWND and reads its extended style on the
    // creation thread.
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) };
    assert_eq!(style & click_through, click_through);
    assert_eq!(
        style & WS_EX_NOACTIVATE.0 as isize,
        WS_EX_NOACTIVATE.0 as isize
    );
    assert_eq!(
        style & WS_EX_NOREDIRECTIONBITMAP.0 as isize,
        WS_EX_NOREDIRECTIONBITMAP.0 as isize
    );

    window
        .set_taskbar_icon_visible(false)
        .expect("take the model window's taskbar button away");
    assert!(!window.taskbar_icon_is_visible());
    // SAFETY: the test owns the live HWND and reads its extended style on the
    // creation thread.
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) };
    assert_eq!(style & click_through, click_through);
}

/// The session keeps the applied value in its options, which is what a
/// replacement window is created from, so a model switch cannot bring back a
/// taskbar button the user turned off.
#[test]
fn session_options_carry_the_taskbar_button_into_a_replacement_window() {
    // The shipped default leaves the button off, so the fallback options must
    // not hand out a taskbar button no configuration asked for.
    assert!(!OverlaySessionOptions::default().taskbar_icon_visible);
    let options = OverlaySessionOptions {
        taskbar_icon_visible: false,
        ..OverlaySessionOptions::default()
    };
    // A runtime overlay settings change carries no taskbar field, so the
    // applied value has to survive the projection onto the options.
    let projected = options.with_runtime_settings(OverlaySettings::default());
    assert!(!projected.taskbar_icon_visible);
    // The taskbar button is applied in place, so it never forces the window to
    // be replaced.
    assert!(!options.requires_window_recreation(projected));
}
