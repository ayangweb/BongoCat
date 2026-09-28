//! Where the overlay sits on the desktop.
//!
//! AppKit measures the primary display from its bottom-left corner and
//! CoreGraphics from its top-left, so every position that crosses between them
//! is mirrored here rather than at each call site: a window frame compared with
//! a cursor sample that was never flipped is a panel the user cannot see.
//! Placement also has to survive a display being unplugged, so a box that no
//! longer lands on a screen is corrected rather than restored off the edge.

use super::*;

pub(crate) fn main_window_level(always_on_top: bool) -> NSWindowLevel {
    if always_on_top {
        NSMainMenuWindowLevel
    } else {
        NSNormalWindowLevel
    }
}

/// Convert a cursor sample into the AppKit screen space used by `NSScreen` and
/// by the overlay panel's frame.
///
/// CoreGraphics measures the primary display from its top-left corner while
/// AppKit measures it from its bottom-left corner, so the vertical axis has to
/// be mirrored about the primary display's height before a cursor position can
/// be compared with a window frame. Both spaces use the same unit, so no
/// scaling is involved. The primary display is the one whose AppKit frame
/// origin is `(0, 0)`, which is also the display whose CoreGraphics bounds
/// start at `(0, 0)`.
///
/// Returns `None` when the primary display cannot be identified, which makes
/// the caller treat the pointer as unknown instead of guessing a position.
pub(crate) fn appkit_cursor_position(
    sample: CursorSample,
    mtm: MainThreadMarker,
) -> Option<CursorPosition> {
    let primary_height = NSScreen::screens(mtm).iter().find_map(|screen| {
        let frame = screen.frame();
        (frame.origin.x == 0.0 && frame.origin.y == 0.0).then_some(frame.size.height)
    })?;
    Some(CursorPosition {
        x: sample.position.x,
        y: primary_height - sample.position.y,
    })
}

pub(crate) fn centered_origin(mtm: MainThreadMarker, width: f64, height: f64) -> NSPoint {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            mouse.x >= frame.origin.x
                && mouse.x < frame.origin.x + frame.size.width
                && mouse.y >= frame.origin.y
                && mouse.y < frame.origin.y + frame.size.height
        })
        .map(|screen| screen.frame())
        .or_else(|| NSScreen::mainScreen(mtm).map(|screen| screen.frame()));
    screen.map_or(NSPoint::new(80.0, 80.0), |screen| {
        NSPoint::new(
            screen.origin.x + (screen.size.width - width) / 2.0,
            screen.origin.y + (screen.size.height - height) / 2.0,
        )
    })
}

/// Every display's full frame, including the strips the menu bar and the Dock
/// occupy, so the placement constraint allows the overlay over desktop chrome
/// while still keeping it on a screen.
pub(crate) fn screen_bounds_all(
    mtm: MainThreadMarker,
) -> Result<Vec<OverlayScreenBounds>, OverlayError> {
    let mut bounds = Vec::new();
    for screen in NSScreen::screens(mtm) {
        let frame = screen.frame();
        bounds.push(OverlayScreenBounds {
            x: rounded_i32(frame.origin.x)?,
            y: rounded_i32(frame.origin.y)?,
            width: rounded_u32(frame.size.width)?,
            height: rounded_u32(frame.size.height)?,
        });
    }
    Ok(bounds)
}

pub(crate) fn overlay_bounds_visible(mtm: MainThreadMarker, bounds: OverlayWindowBounds) -> bool {
    let left = f64::from(bounds.x);
    let bottom = f64::from(bounds.y);
    let right = left + f64::from(bounds.width);
    let top = bottom + f64::from(bounds.height);
    NSScreen::screens(mtm).iter().any(|screen| {
        let frame = screen.frame();
        left < frame.origin.x + frame.size.width
            && right > frame.origin.x
            && bottom < frame.origin.y + frame.size.height
            && top > frame.origin.y
    })
}

pub(crate) fn rounded_i32(value: f64) -> Result<i32, OverlayError> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(OverlayError::new("overlay window coordinate is invalid"));
    }
    Ok(value.round() as i32)
}

pub(crate) fn rounded_u32(value: f64) -> Result<u32, OverlayError> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(OverlayError::new("overlay window dimension is invalid"));
    }
    Ok(value.round() as u32)
}
