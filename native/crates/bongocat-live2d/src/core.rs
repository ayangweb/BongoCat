use crate::{
    BlendMode, CUBISM_CORE_VERSION, CUBISM_LATEST_MOC_VERSION, CanvasInfo, DrawableSnapshot,
    Live2dError, Live2dErrorCode, RenderSnapshot, Vertex, sys,
};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    fs,
    mem::ManuallyDrop,
    path::Path,
    ptr::NonNull,
};

struct AlignedMemory {
    pointer: NonNull<u8>,
    layout: Layout,
}

impl AlignedMemory {
    fn zeroed(size: usize, alignment: usize) -> Result<Self, Live2dError> {
        let layout = Layout::from_size_align(size, alignment).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::ModelMemoryInvalid,
                format!("invalid allocation layout: {error}"),
            )
        })?;
        // SAFETY: the validated non-zero layout is retained by this owner and
        // passed unchanged to dealloc exactly once.
        let pointer = unsafe { NonNull::new(alloc_zeroed(layout)) }.ok_or_else(|| {
            Live2dError::new(
                Live2dErrorCode::ModelMemoryInvalid,
                format!("cannot allocate {size} bytes aligned to {alignment}"),
            )
        })?;
        Ok(Self { pointer, layout })
    }

    fn from_bytes(bytes: &[u8], alignment: usize) -> Result<Self, Live2dError> {
        if bytes.is_empty() {
            return Err(Live2dError::new(
                Live2dErrorCode::EmptyMoc,
                "Moc resource is empty",
            ));
        }
        let memory = Self::zeroed(bytes.len(), alignment)?;
        // SAFETY: both regions are valid for bytes.len(), uniquely owned, and
        // cannot overlap because the destination is a new allocation.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory.pointer.as_ptr(), bytes.len())
        };
        Ok(memory)
    }

    fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
        self.pointer.as_ptr().cast()
    }
}

impl Drop for AlignedMemory {
    fn drop(&mut self) {
        // SAFETY: pointer was allocated with this exact layout and ownership
        // has not escaped this value.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

pub(crate) struct CoreModel {
    model: NonNull<sys::csmModel>,
    model_memory: ManuallyDrop<AlignedMemory>,
    moc_memory: ManuallyDrop<AlignedMemory>,
}

impl CoreModel {
    pub(crate) fn load(path: &Path) -> Result<Self, Live2dError> {
        let bytes = fs::read(path).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::ResourceIo,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        let size = u32::try_from(bytes.len()).map_err(|_| {
            Live2dError::new(
                Live2dErrorCode::ModelMemoryInvalid,
                "Moc resource exceeds the Core ABI size limit",
            )
        })?;
        let mut moc_memory = AlignedMemory::from_bytes(&bytes, sys::csmAlignofMoc as usize)?;

        // SAFETY: version calls do not borrow application memory and are made
        // against the statically linked Core selected by build.rs.
        let (core_version, latest_moc_version) =
            unsafe { (sys::csmGetVersion(), sys::csmGetLatestMocVersion()) };
        if core_version != CUBISM_CORE_VERSION || latest_moc_version != CUBISM_LATEST_MOC_VERSION {
            return Err(Live2dError::new(
                Live2dErrorCode::CoreVersionMismatch,
                format!(
                    "expected Core 0x{CUBISM_CORE_VERSION:08x}/Moc {CUBISM_LATEST_MOC_VERSION}, got 0x{core_version:08x}/Moc {latest_moc_version}"
                ),
            ));
        }

        // SAFETY: Moc allocation has the SDK-required size and alignment and
        // remains uniquely owned for the entire consistency/revive sequence.
        unsafe {
            if sys::csmHasMocConsistency(moc_memory.as_mut_ptr(), size) != 1 {
                return Err(Live2dError::new(
                    Live2dErrorCode::MocConsistencyFailed,
                    "Cubism Core rejected Moc consistency",
                ));
            }
            let moc = NonNull::new(sys::csmReviveMocInPlace(moc_memory.as_mut_ptr(), size))
                .ok_or_else(|| {
                    Live2dError::new(
                        Live2dErrorCode::MocReviveFailed,
                        "Cubism Core returned a null Moc",
                    )
                })?;
            let model_size = sys::csmGetSizeofModel(moc.as_ptr());
            if model_size == 0 {
                return Err(Live2dError::new(
                    Live2dErrorCode::ModelMemoryInvalid,
                    "Cubism Core returned a zero Model size",
                ));
            }
            let mut model_memory =
                AlignedMemory::zeroed(model_size as usize, sys::csmAlignofModel as usize)?;
            let model = NonNull::new(sys::csmInitializeModelInPlace(
                moc.as_ptr(),
                model_memory.as_mut_ptr(),
                model_size,
            ))
            .ok_or_else(|| {
                Live2dError::new(
                    Live2dErrorCode::ModelInitializeFailed,
                    "Cubism Core returned a null Model",
                )
            })?;
            Ok(Self {
                model,
                model_memory: ManuallyDrop::new(model_memory),
                moc_memory: ManuallyDrop::new(moc_memory),
            })
        }
    }

    pub(crate) fn validate_texture_indices(&self, texture_count: usize) -> Result<(), Live2dError> {
        // SAFETY: self owns the live Model and all Core-reported arrays. The
        // helper validates null/count before creating the slice.
        unsafe {
            let count = self.drawable_count()?;
            let indices = checked_slice(
                sys::csmGetDrawableTextureIndices(self.model.as_ptr()),
                count,
                "texture indices",
            )?;
            for &index in indices {
                let index = usize::try_from(index).map_err(|_| {
                    Live2dError::new(
                        Live2dErrorCode::TextureIndexInvalid,
                        "Core returned a negative texture index",
                    )
                })?;
                if index >= texture_count {
                    return Err(Live2dError::new(
                        Live2dErrorCode::TextureIndexInvalid,
                        format!("Core texture index {index} exceeds {texture_count} assets"),
                    ));
                }
            }
        }
        Ok(())
    }

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

    unsafe fn snapshot(&self) -> Result<RenderSnapshot, Live2dError> {
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
                .collect::<Result<Vec<_>, _>>()?;
            if masks.iter().any(|index| *index >= count) {
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
                source_index,
                render_order: render_orders[source_index],
                visible,
                texture_index,
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
        drawables.sort_by_key(|drawable| (drawable.render_order, drawable.source_index));
        Ok(RenderSnapshot {
            canvas: unsafe { read_canvas(model)? },
            drawables,
        })
    }

    unsafe fn drawable_count(&self) -> Result<usize, Live2dError> {
        // SAFETY: self.model points into the live model allocation.
        nonnegative(
            unsafe { sys::csmGetDrawableCount(self.model.as_ptr()) },
            "drawable count",
        )
    }
}

impl Drop for CoreModel {
    fn drop(&mut self) {
        // Model contains pointers into Moc-owned tables, so its allocation is
        // always released first. Neither pointer is observable after this.
        unsafe {
            ManuallyDrop::drop(&mut self.model_memory);
            ManuallyDrop::drop(&mut self.moc_memory);
        }
    }
}

unsafe fn read_canvas(model: *const sys::csmModel) -> Result<CanvasInfo, Live2dError> {
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

fn decode_blend_mode(mode: i32) -> Result<BlendMode, Live2dError> {
    match mode & 0xff {
        0 => Ok(BlendMode::Normal),
        1 => Ok(BlendMode::Additive),
        2 => Ok(BlendMode::Multiplicative),
        value => Err(Live2dError::new(
            Live2dErrorCode::UnsupportedBlendMode,
            format!("Core color blend mode {value} is not implemented yet"),
        )),
    }
}

fn vector4(value: sys::csmVector4) -> Result<[f32; 4], Live2dError> {
    let result = [value.X, value.Y, value.Z, value.W];
    if result.iter().any(|component| !component.is_finite()) {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            "Core returned a non-finite drawable color",
        ));
    }
    Ok(result)
}

fn validate_vertices(vertices: &[Vertex]) -> Result<(), Live2dError> {
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

fn nonnegative(value: i32, name: &str) -> Result<usize, Live2dError> {
    usize::try_from(value).map_err(|_| {
        Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            format!("Core returned a negative {name}"),
        )
    })
}

unsafe fn checked_slice<'a, T>(
    pointer: *const T,
    count: usize,
    name: &str,
) -> Result<&'a [T], Live2dError> {
    if count == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreArray,
            format!("Core returned a null {name} array for {count} values"),
        ));
    }
    // SAFETY: the Core owns at least count elements for this pointer/count
    // pair while the caller's Model owner remains alive.
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{ModelId, ModelPackageLimits, PreparedModel};

    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    #[test]
    fn all_preset_models_produce_stable_drawable_snapshots() {
        for id in ["standard", "keyboard", "gamepad"] {
            let prepared = PreparedModel::prepare(
                ModelId::parse(id).expect("model id"),
                repository_root().join("native/resources/models").join(id),
                ModelPackageLimits::default(),
            )
            .expect("prepare preset");
            let mut model = crate::Live2dModel::load(&prepared).expect("load Cubism model");
            let first = model.update_and_snapshot().expect("first snapshot");
            assert!(!first.drawables.is_empty());
            assert!(first.drawables.iter().all(|drawable| {
                drawable.texture_index < model.texture_assets().len()
                    && !drawable.vertices.is_empty()
                    && !drawable.indices.is_empty()
            }));
            for _ in 0..10 {
                assert_eq!(model.update_and_snapshot().expect("repeat snapshot"), first);
            }
        }
    }
}
