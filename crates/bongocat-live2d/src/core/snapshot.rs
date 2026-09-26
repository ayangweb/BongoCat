//! What one frame of a model looks like.
//!
//! The snapshot is the boundary the renderer reads: the library is stepped and
//! read here, and nothing above this module touches a Cubism pointer. That is
//! what lets the renderer be tested against a value rather than against a GPU.

use super::*;

impl CoreModel {
    pub(crate) fn update_and_snapshot(&mut self) -> Result<RenderSnapshot, Live2dError> {
        // SAFETY: the model pointer targets self.model_memory and both it and
        // the revived Moc remain alive and uniquely owned for this call.
        unsafe {
            sys::csmUpdateModel(self.model.as_ptr());
            let snapshot = self.snapshot()?;
            sys::csmResetDrawableDynamicFlags(self.model.as_ptr());
            Ok(snapshot)
        }
    }
}

impl CoreModel {
    pub(crate) unsafe fn snapshot(&self) -> Result<RenderSnapshot, Live2dError> {
        let model = self.model.as_ptr();
        // SAFETY: all pointer/count pairs below come from the same live Model.
        let count = unsafe { self.drawable_count()? };
        // SAFETY: checked_slice rejects null for non-zero counts.
        let render_orders =
            unsafe { checked_slice(sys::csmGetRenderOrders(model), count, "render orders")? };
        let texture_indices = unsafe {
            checked_slice(
                sys::csmGetDrawableTextureIndices(model),
                count,
                "texture indices",
            )?
        };
        let opacities =
            unsafe { checked_slice(sys::csmGetDrawableOpacities(model), count, "opacities")? };
        let constant_flags = unsafe {
            checked_slice(
                sys::csmGetDrawableConstantFlags(model),
                count,
                "constant flags",
            )?
        };
        let dynamic_flags = unsafe {
            checked_slice(
                sys::csmGetDrawableDynamicFlags(model),
                count,
                "dynamic flags",
            )?
        };
        let blend_modes =
            unsafe { checked_slice(sys::csmGetDrawableBlendModes(model), count, "blend modes")? };
        let vertex_counts = unsafe {
            checked_slice(
                sys::csmGetDrawableVertexCounts(model),
                count,
                "vertex counts",
            )?
        };
        let positions = unsafe {
            checked_slice(
                sys::csmGetDrawableVertexPositions(model),
                count,
                "positions",
            )?
        };
        let uvs = unsafe { checked_slice(sys::csmGetDrawableVertexUvs(model), count, "UVs")? };
        let index_counts =
            unsafe { checked_slice(sys::csmGetDrawableIndexCounts(model), count, "index counts")? };
        let indices =
            unsafe { checked_slice(sys::csmGetDrawableIndices(model), count, "indices")? };
        let mask_counts =
            unsafe { checked_slice(sys::csmGetDrawableMaskCounts(model), count, "mask counts")? };
        let masks = unsafe { checked_slice(sys::csmGetDrawableMasks(model), count, "masks")? };
        let multiply_colors = unsafe {
            checked_slice(
                sys::csmGetDrawableMultiplyColors(model),
                count,
                "multiply colors",
            )?
        };
        let screen_colors = unsafe {
            checked_slice(
                sys::csmGetDrawableScreenColors(model),
                count,
                "screen colors",
            )?
        };

        let mut drawables = Vec::with_capacity(count);
        for source_index in 0..count {
            let visible = dynamic_flags[source_index] & sys::csmIsVisible as u8 != 0;
            let vertex_count = nonnegative(vertex_counts[source_index], "vertex count")?;
            let index_count = nonnegative(index_counts[source_index], "index count")?;
            let mask_count = nonnegative(mask_counts[source_index], "mask count")?;
            // SAFETY: each nested pointer is paired with the per-drawable
            // count returned by this same live Model.
            let positions = unsafe {
                checked_slice(positions[source_index], vertex_count, "drawable positions")?
            };
            let uvs = unsafe { checked_slice(uvs[source_index], vertex_count, "drawable UVs")? };
            let drawable_indices =
                unsafe { checked_slice(indices[source_index], index_count, "drawable indices")? };
            let drawable_masks =
                unsafe { checked_slice(masks[source_index], mask_count, "drawable masks")? };
            let vertices = positions
                .iter()
                .zip(uvs)
                .map(|(position, uv)| Vertex {
                    position: [position.X, position.Y],
                    uv: [uv.X, uv.Y],
                })
                .collect::<Vec<_>>();
            validate_vertices(&vertices)?;
            if drawable_indices
                .iter()
                .any(|index| usize::from(*index) >= vertex_count)
            {
                return Err(Live2dError::new(
                    Live2dErrorCode::InvalidCoreValue,
                    format!("drawable {source_index} has an out-of-range triangle index"),
                ));
            }
            let masks = drawable_masks
                .iter()
                .map(|index| {
                    usize::try_from(*index).map_err(|_| {
                        Live2dError::new(
                            Live2dErrorCode::InvalidCoreValue,
                            format!("drawable {source_index} has a negative mask index"),
                        )
                    })
                })
                .map(|result| result.map(DrawableId::new))
                .collect::<Result<Vec<_>, _>>()?;
            if masks.iter().any(|id| id.index() >= count) {
                return Err(Live2dError::new(
                    Live2dErrorCode::InvalidCoreValue,
                    format!("drawable {source_index} has an out-of-range mask index"),
                ));
            }
            let texture_index = usize::try_from(texture_indices[source_index]).map_err(|_| {
                Live2dError::new(
                    Live2dErrorCode::TextureIndexInvalid,
                    format!("drawable {source_index} has a negative texture index"),
                )
            })?;
            let opacity = opacities[source_index];
            if !opacity.is_finite() {
                return Err(Live2dError::new(
                    Live2dErrorCode::InvalidCoreValue,
                    format!("drawable {source_index} has invalid opacity {opacity}"),
                ));
            }
            let opacity = opacity.clamp(0.0, 1.0);
            drawables.push(DrawableSnapshot {
                id: DrawableId::new(source_index),
                dynamic_flags: decode_dynamic_flags(dynamic_flags[source_index]),
                render_order: render_orders[source_index],
                visible,
                texture_id: TextureId::new(texture_index),
                opacity,
                blend_mode: decode_blend_mode(blend_modes[source_index])?,
                double_sided: constant_flags[source_index] & sys::csmIsDoubleSided as u8 != 0,
                inverted_mask: constant_flags[source_index] & sys::csmIsInvertedMask as u8 != 0,
                multiply_color: vector4(multiply_colors[source_index])?,
                screen_color: vector4(screen_colors[source_index])?,
                masks,
                vertices,
                indices: drawable_indices.to_vec(),
            });
        }
        drawables.sort_by_key(|drawable| (drawable.render_order, drawable.id));
        let canvas = unsafe { read_canvas(model)? };
        let bounds = ModelBounds::from_canvas(canvas);
        Ok(RenderSnapshot {
            canvas,
            bounds,
            active_keys: Vec::new(),
            model_opacity: 1.0,
            mirror_horizontal: false,
            drawables,
        })
    }
}
