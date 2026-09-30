//! Reading the window's messages.
//!
//! The procedure is the product's only view of the desktop: it turns a message
//! into a decision — hit test, resize drag, context menu — and hands the
//! decision to the window, which owns the state. Messages it does not recognise
//! are forwarded to `DefWindowProcW` with the exact parameters user32 supplied,
//! because swallowing them is how a window stops closing.

use super::*;

pub(crate) fn pump_window_messages() {
    let mut message = MSG::default();
    // SAFETY: message storage is valid for each synchronous call and dispatch
    // remains on the HWND owner thread.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The pointer position the resize math measures against, in screen pixels.
///
/// The screen origin is the top-left of the virtual desktop, which is the same
/// downwards-positive axis the drag math assumes.
pub(crate) fn resize_pointer() -> (f64, f64) {
    let point = current_cursor_position();
    (f64::from(point.x), f64::from(point.y))
}

/// Whether the message starts a right-button resize drag.
pub(crate) fn begins_resize_drag(message: u32) -> bool {
    // The overlay answers `HTCAPTION` from `WM_NCHITTEST`, so a right click
    // arrives as a non-client message; the client form is kept for the paths
    // that hit-test differently.
    matches!(message, WM_NCRBUTTONDOWN | WM_RBUTTONDOWN)
}

/// Whether the message reports the pointer moving while a drag is active.
pub(crate) fn moves_resize_drag(message: u32) -> bool {
    matches!(message, WM_NCMOUSEMOVE | WM_MOUSEMOVE)
}

/// Whether the message ends a right-button resize drag.
pub(crate) fn ends_resize_drag(message: u32) -> bool {
    matches!(message, WM_NCRBUTTONUP | WM_RBUTTONUP)
}

/// Apply one drag step to the window, keeping its top-left corner where it is.
///
/// The origin is what the user grabbed, and the legacy implementation resized
/// from a fixed top-left corner, so the drag grows down and to the right.
pub(crate) unsafe fn apply_resize(hwnd: HWND, outcome: ResizeOutcome) {
    // SAFETY: the HWND is live on its owner thread and the size is bounded by
    // the resize state machine.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            outcome.width as i32,
            outcome.height as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

pub(crate) fn requests_context_menu(message: u32) -> bool {
    // The non-client form is what the overlay's `HTCAPTION` hit test produces,
    // but that message is consumed by the resize drag first; what is left here
    // is the client form and the non-client path that never saw a press.
    matches!(message, WM_CONTEXTMENU)
}

/// Whether the message begins a left-button press on the client area.
///
/// The client form only, and that is what makes it a reliable signal: `WM_NCHITTEST`
/// answers `HTCLIENT` for a point inside a layer and `HTCAPTION` for a point outside
/// one, so a press that landed on a panel arrives as `WM_LBUTTONDOWN` and a press
/// anywhere else is promoted to `WM_NCLBUTTONDOWN` and becomes a window drag before
/// it ever reaches the dispatch below. Its `lparam` is therefore a client point.
pub(crate) fn begins_layer_press(message: u32) -> bool {
    matches!(message, WM_LBUTTONDOWN)
}

/// Where a message's packed `lparam` point is expressed.
///
/// The two callers do not agree, and treating them as if they did is what makes a
/// Windows press land somewhere other than where the panel was drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointSpace {
    /// `WM_NCHITTEST` carries a **screen** point, so it must be converted.
    Screen,
    /// `WM_LBUTTONDOWN` and the other client-area mouse messages already carry a
    /// **client** point, so converting it subtracts the window origin a second time.
    Client,
}

/// The press a left-button message carries, in the window's own client pixels.
///
/// `lparam` is read as the space `space` names. For a client message the packed pair
/// is already relative to the client area and is used as it stands; for a
/// non-client message it is a screen point and is converted with `ScreenToClient`
/// rather than by subtracting the window origin, because a DirectComposition visual
/// and the window's frame can disagree by a rounding error at some display scales
/// and the hit test measures against the client rect the frame loop just drew.
///
/// The packed pair is signed 16-bit and read as signed rather than discarded, so a
/// point left of or above the window is still refused by the hit test for being
/// outside the drawable, rather than not reported at all.
pub(crate) fn client_click(hwnd: HWND, lparam: LPARAM, space: PointSpace) -> Option<(f32, f32)> {
    let packed = lparam.0 as i32;
    let raw = (
        (packed & 0xFFFF) as i16 as f32,
        ((packed >> 16) & 0xFFFF) as i16 as f32,
    );
    let (x, y) = match space {
        PointSpace::Client => raw,
        PointSpace::Screen => {
            let mut point = POINT {
                x: raw.0 as i32,
                y: raw.1 as i32,
            };
            // SAFETY: the HWND is live and read on its owner thread, and `point` is a
            // live POINT the call writes into.
            if unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
                (point.x as f32, point.y as f32)
            } else {
                raw
            }
        }
    };
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The press a point in a window message lands on, if it lands on one at all.
///
/// The client rect is measured and the point is read in the same call so the two
/// call sites — the hit test and the click itself — cannot disagree about which
/// space the answer is in, or a press would be accepted for one point and drawn for
/// another. `space` is the caller's to name, because only the caller knows which
/// message it is answering.
///
/// SAFETY: the caller holds the live boxed state for this HWND on its owner thread.
pub(crate) unsafe fn layer_press(
    state: &OverlayWindowState,
    hwnd: HWND,
    lparam: LPARAM,
    space: PointSpace,
) -> Option<bongocat_render::OverlayLayerPointer> {
    // SAFETY: the HWND is live and read on its owner thread.
    let mut rect = RECT::default();
    // SAFETY: `rect` is live storage for the call to write into.
    let measured = unsafe { GetClientRect(hwnd, &mut rect) }.is_ok();
    if !measured {
        return None;
    }
    let (width, height) = (
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    );
    let (x, y) = client_click(hwnd, lparam, space)?;
    state.placed_layers.press(width, height, x, y)
}

pub(crate) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE carries the pointer supplied by CreateWindowExW.
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        // SAFETY: the owner keeps this boxed state alive until after DestroyWindow.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    // SAFETY: userdata is either null before WM_NCCREATE or the live boxed state above.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindowState };
    if message == WM_NCDESTROY {
        // SAFETY: clearing userdata prevents later messages from observing the stale pointer.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        // SAFETY: forwarding uses the exact user32 callback arguments.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    match message {
        WM_NCHITTEST => {
            // SAFETY: the callback receives a live HWND from user32 and only
            // reads its current extended style on the dispatch thread.
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
            if style & WS_EX_TRANSPARENT.0 as isize != 0 {
                return LRESULT(HTTRANSPARENT as isize);
            }
            // A point inside a layer belongs to that layer, so the window answers
            // `HTCLIENT` for it: a click on a panel's button then arrives as a
            // client message and does not start a window drag. Everything else
            // stays `HTCAPTION`, which is what makes the model window movable by a
            // left click at all.
            // SAFETY: userdata is the live boxed state for this HWND, and the hit
            // test only reads it on the dispatch thread.
            if !state.is_null()
                && unsafe { layer_press(&*state, hwnd, lparam, PointSpace::Screen) }.is_some()
            {
                return LRESULT(HTCLIENT as isize);
            }
            return LRESULT(HTCAPTION as isize);
        }
        WM_CLOSE => {
            // SAFETY: WM_CLOSE is delivered to this owned top-level window and
            // destruction stays on the same UI thread.
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        _ => {}
    }
    if !state.is_null() {
        // SAFETY: the state belongs to this HWND and remains live while it is dispatched.
        let state = unsafe { &mut *state };
        if begins_resize_drag(message) {
            // The base is converted with the window's current DPI rather than
            // the one it was created with, because a window that has moved to
            // another display has to scale from that display's pixels.
            // SAFETY: the HWND is live and read on its owner thread.
            let base = resize_base_for_dpi(
                state.resize_base_logical.0,
                state.resize_base_logical.1,
                unsafe { GetDpiForWindow(hwnd) },
            );
            let mut rect = RECT::default();
            // SAFETY: the HWND is live and read on its owner thread.
            let measured = unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok();
            if let Some(base) = base
                && measured
            {
                // The drag starts from the size the window actually has, so a
                // box that drifted from the stored scale does not jump on the
                // first pointer move.
                let width = (rect.right - rect.left).max(0) as u32;
                state.drag = Some(ResizeDrag::begin(
                    resize_pointer(),
                    base,
                    base.scale_percent_for_width(width),
                ));
                // SAFETY: the HWND is live on its owner thread. Capture is what
                // keeps pointer messages arriving once the drag leaves the box.
                let _ = unsafe { SetCapture(hwnd) };
            }
            return LRESULT(0);
        }
        if state.drag.is_some() && moves_resize_drag(message) {
            let mut drag = state.drag.take().expect("checked resize drag state");
            let outcome = drag.observe(resize_pointer());
            state.drag = Some(drag);
            if let Some(outcome) = outcome {
                // SAFETY: the HWND is live and belongs to this thread.
                unsafe { apply_resize(hwnd, outcome) };
            }
            return LRESULT(0);
        }
        if ends_resize_drag(message) {
            // The drag is taken before the capture is released: releasing it
            // delivers `WM_CAPTURECHANGED` synchronously, which clears the drag
            // state, and the release must not turn the drag into a menu click.
            let drag = state.drag.take();
            // SAFETY: the capture was taken by this window when the drag began.
            let _ = unsafe { ReleaseCapture() };
            match drag {
                Some(drag) if drag.dragging() => {
                    if let Some(scale_percent) = drag.finish()
                        && let Some(sender) = &state.resize_sender
                    {
                        let _ = sender.try_send(OverlayResizeOutcome { scale_percent });
                    }
                }
                _ => {
                    if let Some(sender) = &state.context_menu_sender {
                        let _ = sender.try_send(OverlayContextMenuRequest);
                    }
                }
            }
            return LRESULT(0);
        }
        if message == WM_CAPTURECHANGED {
            // Losing the capture (another window took it, the system cancelled
            // the mode) ends the drag rather than leaving it stuck. A release
            // that still arrives afterwards is then treated as a plain right
            // click, which is the same outcome as a click without movement.
            state.drag = None;
        }
        if requests_context_menu(message) {
            if let Some(sender) = &state.context_menu_sender {
                let _ = sender.try_send(OverlayContextMenuRequest);
            }
            return LRESULT(0);
        }
        if state.drag.is_none() && begins_layer_press(message) {
            // Consumed: `WM_NCHITTEST` only answers `HTCLIENT` for a point inside a
            // layer, so reaching here means the press was on one. Letting it reach
            // `DefWindowProcW` would give the window a capture it never gives back
            // for the rest of the click.
            // SAFETY: the state belongs to this HWND on its owner thread.
            if let Some(press) = unsafe { layer_press(state, hwnd, lparam, PointSpace::Client) }
                && let Some(sink) = &state.press_sink
            {
                sink.press(press.layer_id, press.x, press.y);
            }
            return LRESULT(0);
        }
    }
    // SAFETY: unhandled messages are forwarded with the exact parameters
    // supplied by user32, as required by the window procedure contract.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
