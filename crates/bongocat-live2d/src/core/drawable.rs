//! Reading a drawable's flags and blend mode back out of Cubism's own encoding.
//!
//! The flags are a bit field Cubism sets and clears, and each bit means the
//! drawable changed in one specific way. A mode the table does not name is
//! refused rather than defaulted: a wrong blend factor produces a picture that
//! looks almost right, which is the failure a user reports as "the model is
//! broken" rather than as "one part is shaded wrongly".

use super::*;

pub(crate) fn decode_dynamic_flags(flags: u8) -> DrawableDynamicFlags {
    DrawableDynamicFlags {
        visibility_changed: flags & sys::csmVisibilityDidChange as u8 != 0,
        opacity_changed: flags & sys::csmOpacityDidChange as u8 != 0,
        draw_order_changed: flags & sys::csmDrawOrderDidChange as u8 != 0,
        render_order_changed: flags & sys::csmRenderOrderDidChange as u8 != 0,
        vertex_positions_changed: flags & sys::csmVertexPositionsDidChange as u8 != 0,
        blend_color_changed: flags & sys::csmBlendColorDidChange as u8 != 0,
    }
}

pub(crate) unsafe fn validate_drawable_ids(model: *const sys::csmModel) -> Result<(), Live2dError> {
    // SAFETY: model is freshly initialized and remains owned by CoreModel.
    let count = nonnegative(unsafe { sys::csmGetDrawableCount(model) }, "drawable count")?;
    // SAFETY: the pointer/count pair comes from the same live Model.
    let ids = unsafe { checked_slice(sys::csmGetDrawableIds(model), count, "drawable ids")? };
    let mut unique_ids = BTreeSet::new();
    for (index, &pointer) in ids.iter().enumerate() {
        if pointer.is_null() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreArray,
                format!("Core returned a null drawable id at index {index}"),
            ));
        }
        // SAFETY: Core documents drawable IDs as NUL-terminated strings that
        // remain valid while the Model is alive.
        let id = unsafe { CStr::from_ptr(pointer) }.to_str().map_err(|_| {
            Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned a non-UTF-8 drawable id at index {index}"),
            )
        })?;
        if id.is_empty() || !unique_ids.insert(id) {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned an invalid or duplicate drawable id at index {index}"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn decode_blend_mode(mode: i32) -> Result<BlendMode, Live2dError> {
    match mode {
        0 => Ok(BlendMode::Normal),
        1 => Ok(BlendMode::Additive),
        2 => Ok(BlendMode::Multiplicative),
        value => Err(Live2dError::new(
            Live2dErrorCode::UnsupportedBlendMode,
            format!("Core returned unsupported drawable blend flags {value}"),
        )),
    }
}

pub(crate) fn validate_vertices(vertices: &[Vertex]) -> Result<(), Live2dError> {
    if vertices.iter().any(|vertex| {
        vertex
            .position
            .iter()
            .chain(&vertex.uv)
            .any(|value| !value.is_finite())
    }) {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            "Core returned a non-finite vertex",
        ));
    }
    Ok(())
}
