//! How large a window the model needs, and what it may not exceed.
//!
//! The overlay window is sized from the model's aspect ratio rather than from a
//! fixed box, so a model that is not square is not stretched. A model switch
//! recomputes the height from the width the window already has: resizing by the
//! new scale's ratio as well would shrink the overlay twice for one switch.

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

pub(crate) fn model_window_height_for_width(canvas: CanvasInfo, width: u32) -> u32 {
    let canvas_width = canvas.width.max(MIN_OVERLAY_WINDOW_DIMENSION);
    let canvas_height = canvas.height.max(MIN_OVERLAY_WINDOW_DIMENSION);
    cover_window_dimension(f64::from(width) * f64::from(canvas_height) / f64::from(canvas_width))
}

pub(crate) fn model_window_dimensions(canvas: CanvasInfo, scale_percent: u16) -> (u32, u32) {
    let width = cover_window_dimension(
        f64::from(DEFAULT_OVERLAY_WINDOW_WIDTH) * f64::from(scale_percent) / 100.0,
    );
    let height = model_window_height_for_width(canvas, width);
    (width, height)
}

pub(crate) fn default_overlay_window_dimensions(canvas: CanvasInfo) -> (u32, u32) {
    model_window_dimensions(canvas, 100)
}
