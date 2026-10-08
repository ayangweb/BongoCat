//! Which message means hit test, resize drag or context menu.

use super::*;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_ENTERSIZEMOVE, WMSZ_BOTTOMRIGHT};

#[test]
fn window_sizing_normalizes_programmatic_native_and_dpi_geometry() {
    let canvas = CanvasInfo {
        width: 612.0,
        height: 354.0,
        origin_x: 306.0,
        origin_y: 177.0,
        pixels_per_unit: 354.0,
    };
    for window_mode in [false, true] {
        let mut window = OverlayWindow::create(
            OverlaySessionOptions {
                window_mode,
                keep_inside_screen: false,
                ..Default::default()
            },
            canvas,
            Some(OverlayWindowBounds::new(100, 100, 720, 308)),
            None,
            None,
        )
        .unwrap();
        for width in [88, 111, 525, 720, 840] {
            let requested = OverlayWindowBounds::new(100, 100, width, 308);
            window.resize(requested).unwrap();
            let programmatic = window.bounds().unwrap();
            let expected_height = (f64::from(programmatic.width) * 354.0 / 612.0).ceil() as u32;
            assert_eq!(
                programmatic.height, expected_height,
                "mode {window_mode}, requested width {width}, actual {programmatic:?}"
            );
            // SAFETY: native resize and synthetic DPI messages target the
            // owned HWND on this thread, with live RECT storage throughout.
            unsafe {
                let frame = window_frame_rect(window.hwnd, width, 308).unwrap();
                SetWindowPos(
                    window.hwnd,
                    None,
                    100 + frame.left,
                    100 + frame.top,
                    frame.right - frame.left,
                    frame.bottom - frame.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
                .unwrap();
                assert_eq!(window.bounds().unwrap(), programmatic);
                let mut suggested = RECT {
                    left: 100 + frame.left,
                    top: 100 + frame.top,
                    right: 100 + frame.right,
                    bottom: 100 + frame.bottom,
                };
                SendMessageW(
                    window.hwnd,
                    WM_DPICHANGED,
                    None,
                    Some(LPARAM((&mut suggested as *mut RECT) as isize)),
                );
            }
            assert_eq!(window.bounds().unwrap(), programmatic);
        }
        if !window_mode {
            // Right-button drag outcomes reach the native window before the
            // runtime acknowledges their percentage. They must match numeric sizing.
            for scale_percent in [25, 50, 100, 125, 400] {
                // SAFETY: DPI reads and native drag resizing stay on the owner thread.
                unsafe {
                    let base = window
                        ._state
                        .sizing
                        .resize_base(GetDpiForWindow(window.hwnd))
                        .unwrap();
                    let (width, height) = base.dimensions(scale_percent);
                    apply_resize(
                        window.hwnd,
                        ResizeOutcome {
                            scale_percent,
                            width,
                            height,
                        },
                    );
                }
                let actual = window.bounds().unwrap();
                let acknowledged = window.bounds_for_scale(scale_percent).unwrap();
                assert_eq!(actual, acknowledged, "right-button resize {scale_percent}%");
            }
        }
    }
}

fn window_mode_resize_test_window() -> (OverlayWindow, Receiver<OverlayResizeOutcome>) {
    let (sender, receiver) = sync_channel(1);
    let window = OverlayWindow::create(
        OverlaySessionOptions {
            window_mode: true,
            keep_inside_screen: false,
            ..OverlaySessionOptions::default()
        },
        CanvasInfo {
            width: 2048.0,
            height: 2048.0,
            origin_x: 1024.0,
            origin_y: 1024.0,
            pixels_per_unit: 1024.0,
        },
        // Saved geometry can differ from the configured scale after a model
        // switch. Moving it must not reinterpret that geometry as a new scale.
        Some(OverlayWindowBounds::new(100, 100, 420, 420)),
        None,
        Some(sender),
    )
    .expect("create ordinary window with saved geometry");
    (window, receiver)
}

#[test]
fn window_mode_moving_saved_geometry_never_publishes_scale() {
    let (window, receiver) = window_mode_resize_test_window();
    let initial = window.bounds().unwrap();
    for offset in [20, 40, 60] {
        // SAFETY: messages target the test's live HWND on its owner thread.
        unsafe {
            SendMessageW(window.hwnd, WM_ENTERSIZEMOVE, None, None);
        }
        let moved = OverlayWindowBounds {
            x: initial.x + offset,
            y: initial.y + offset,
            ..initial
        };
        window.set_origin(moved).unwrap();
        unsafe {
            SendMessageW(window.hwnd, WM_EXITSIZEMOVE, None, None);
        }
        assert_eq!(window.bounds().unwrap(), moved);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }
}

#[test]
fn window_mode_resize_publishes_once_and_cancelled_resize_publishes_nothing() {
    let (mut window, receiver) = window_mode_resize_test_window();
    let initial = window.bounds().unwrap();
    // SAFETY: the HWND and writable RECT belong to this test's thread; the
    // RECT remains valid throughout each synchronous WM_SIZING dispatch.
    unsafe {
        SendMessageW(window.hwnd, WM_ENTERSIZEMOVE, None, None);
        let mut rect = RECT::default();
        GetWindowRect(window.hwnd, &mut rect).unwrap();
        rect.right += 80;
        rect.bottom += 80;
        SendMessageW(
            window.hwnd,
            WM_SIZING,
            Some(WPARAM(WMSZ_BOTTOMRIGHT as usize)),
            Some(LPARAM((&mut rect as *mut RECT) as isize)),
        );
        let frame = window_frame_rect(window.hwnd, 0, 0).unwrap();
        window
            .resize(OverlayWindowBounds {
                width: (rect.right - rect.left - (frame.right - frame.left)) as u32,
                height: (rect.bottom - rect.top - (frame.bottom - frame.top)) as u32,
                ..initial
            })
            .unwrap();
        SendMessageW(window.hwnd, WM_EXITSIZEMOVE, None, None);
    }
    let resized = window.bounds().unwrap();
    assert_ne!(resized.width, initial.width);
    // SAFETY: DPI is read from the owned HWND on its creation thread.
    let base = window
        ._state
        .sizing
        .resize_base(unsafe { GetDpiForWindow(window.hwnd) })
        .unwrap();
    assert_eq!(
        receiver.try_recv().unwrap().scale_percent,
        base.scale_percent_for_width(resized.width)
    );
    // A resize acknowledgement must not apply the scale ratio a second time.
    assert_eq!(
        (resized.width, resized.height),
        window
            .bounds_for_scale(base.scale_percent_for_width(resized.width))
            .map(|bounds| (bounds.width, bounds.height))
            .unwrap()
    );
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));

    for apply_proposed_size in [false, true] {
        // SAFETY: all geometry and synchronous message operations remain on
        // the owner thread, with a live writable RECT for WM_SIZING.
        unsafe {
            SendMessageW(window.hwnd, WM_ENTERSIZEMOVE, None, None);
            let mut rect = RECT::default();
            GetWindowRect(window.hwnd, &mut rect).unwrap();
            rect.right += 80;
            SendMessageW(
                window.hwnd,
                WM_SIZING,
                Some(WPARAM(WMSZ_BOTTOMRIGHT as usize)),
                Some(LPARAM((&mut rect as *mut RECT) as isize)),
            );
        }
        if apply_proposed_size {
            window
                .resize(OverlayWindowBounds {
                    width: resized.width + 80,
                    height: resized.height + 80,
                    ..resized
                })
                .unwrap();
            window.resize(resized).unwrap();
        }
        // SAFETY: end and subsequent move messages target the owned HWND.
        unsafe {
            SendMessageW(window.hwnd, WM_EXITSIZEMOVE, None, None);
            SendMessageW(window.hwnd, WM_ENTERSIZEMOVE, None, None);
        }
        window
            .set_origin(OverlayWindowBounds {
                x: resized.x + 20,
                ..resized
            })
            .unwrap();
        unsafe {
            SendMessageW(window.hwnd, WM_EXITSIZEMOVE, None, None);
        }
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }
}

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
