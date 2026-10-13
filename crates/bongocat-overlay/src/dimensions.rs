//! How large a window the model needs, and what it may not exceed.
//!
//! The overlay window is sized from the model's aspect ratio rather than from a
//! fixed box, so a model that is not square is not stretched. The height also
//! carries a fixed headroom above the canvas (see [`CANVAS_HEADROOM_FRACTION`])
//! so effects a model draws above its own canvas are not clipped away. A model
//! switch recomputes the height from the width the window already has: resizing
//! by the new scale's ratio as well would shrink the overlay twice for one
//! switch.

use super::*;

pub(crate) fn cover_window_dimension(value: f64) -> u32 {
    let value = if value.is_finite() {
        value.ceil()
    } else {
        f64::from(MIN_OVERLAY_WINDOW_DIMENSION)
    };
    value.clamp(
        f64::from(MIN_OVERLAY_WINDOW_DIMENSION),
        f64::from(MAX_OVERLAY_WINDOW_DIMENSION),
    ) as u32
}

/// Extra window height kept above the model canvas, as a fraction of the
/// canvas height.
///
/// A model may draw effects above the canvas box it declares - the preset
/// cat's ascend halo and its light glow sit up to about 22% of the canvas
/// height above the canvas top - and with an aspect-tight window those are
/// clipped away by the window edge. The render transform keeps the canvas
/// against the window's bottom edge, so this margin only extends the window
/// upward and the model stays where the user placed it. The value stays below
/// the ~23% at which the same model parks its hidden thug-life props above the
/// canvas, so a wider window does not reveal them.
///
/// On Windows every sizing entry point folds this in through `WindowSizing`,
/// so the created window, a right-button resize drag and a saved-box restore
/// all agree on the margin.
pub(crate) const CANVAS_HEADROOM_FRACTION: f64 = 0.20;

#[cfg(any(target_os = "macos", test))]
pub(crate) fn model_window_height_for_width(canvas: CanvasInfo, width: u32) -> u32 {
    let canvas_width = canvas.width.max(MIN_OVERLAY_WINDOW_DIMENSION);
    let canvas_height = canvas.height.max(MIN_OVERLAY_WINDOW_DIMENSION);
    let canvas_height_for_width =
        f64::from(width) * f64::from(canvas_height) / f64::from(canvas_width);
    cover_window_dimension(canvas_height_for_width * (1.0 + CANVAS_HEADROOM_FRACTION))
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn model_window_dimensions(canvas: CanvasInfo, scale_percent: u16) -> (u32, u32) {
    let width = cover_window_dimension(
        f64::from(DEFAULT_OVERLAY_WINDOW_WIDTH) * f64::from(scale_percent) / 100.0,
    );
    let height = model_window_height_for_width(canvas, width);
    (width, height)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn default_overlay_window_dimensions(canvas: CanvasInfo) -> (u32, u32) {
    model_window_dimensions(canvas, 100)
}
