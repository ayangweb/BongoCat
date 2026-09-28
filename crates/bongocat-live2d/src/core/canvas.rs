//! The canvas the model reports, and the vector helper that reads it.
//!
//! The canvas is what the overlay sizes itself from, so it is read at load and
//! validated rather than read per frame: a canvas that changed under the window
//! would resize it mid-drag.

use super::*;

pub(crate) unsafe fn read_canvas(model: *const sys::csmModel) -> Result<CanvasInfo, Live2dError> {
    let mut size = sys::csmVector2 { X: 0.0, Y: 0.0 };
    let mut origin = sys::csmVector2 { X: 0.0, Y: 0.0 };
    let mut pixels_per_unit = 0.0;
    // SAFETY: output pointers refer to initialized stack values and model is
    // the live Core Model owned by the caller.
    unsafe { sys::csmReadCanvasInfo(model, &mut size, &mut origin, &mut pixels_per_unit) };
    let values = [size.X, size.Y, origin.X, origin.Y, pixels_per_unit];
    if values.iter().any(|value| !value.is_finite())
        || size.X <= 0.0
        || size.Y <= 0.0
        || pixels_per_unit <= 0.0
    {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            "Core returned invalid canvas dimensions",
        ));
    }
    Ok(CanvasInfo {
        width: size.X,
        height: size.Y,
        origin_x: origin.X,
        origin_y: origin.Y,
        pixels_per_unit,
    })
}

pub(crate) fn vector4(value: sys::csmVector4) -> Result<[f32; 4], Live2dError> {
    let result = [value.X, value.Y, value.Z, value.W];
    if result.iter().any(|component| !component.is_finite()) {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            "Core returned a non-finite drawable color",
        ));
    }
    Ok(result)
}
