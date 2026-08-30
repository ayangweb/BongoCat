use crate::{
    CUBISM_CORE_VERSION, CUBISM_LATEST_MOC_VERSION, Live2dError, Live2dErrorCode, ParameterRange,
    ParameterUpdate, ProductParameter, sys,
};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableId, DrawableSnapshot, RenderSnapshot, TextureId, Vertex,
};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    collections::BTreeMap,
    ffi::CStr,
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
    parameters: [Option<ResolvedParameter>; ProductParameter::COUNT],
    parameters_by_id: BTreeMap<String, ResolvedParameter>,
    model_memory: ManuallyDrop<AlignedMemory>,
    moc_memory: ManuallyDrop<AlignedMemory>,
}

#[derive(Clone, Copy)]
struct ResolvedParameter {
    index: usize,
    range: ParameterRange,
}

struct ResolvedParameters {
    product: [Option<ResolvedParameter>; ProductParameter::COUNT],
    by_id: BTreeMap<String, ResolvedParameter>,
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
            let parameters = resolve_parameters(model.as_ptr())?;
            Ok(Self {
                model,
                parameters: parameters.product,
                parameters_by_id: parameters.by_id,
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

    pub(crate) fn parameter_range(&self, parameter: ProductParameter) -> Option<ParameterRange> {
        self.parameters[parameter.slot()].map(|resolved| resolved.range)
    }

    pub(crate) fn parameter_value(
        &self,
        parameter: ProductParameter,
    ) -> Result<Option<f32>, Live2dError> {
        let Some(resolved) = self.parameters[parameter.slot()] else {
            return Ok(None);
        };
        // SAFETY: the parameter table and values belong to the live Model.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index]
        };
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("{} has a non-finite current value", parameter.id()),
            ));
        }
        Ok(Some(value))
    }

    pub(crate) fn set_parameter(
        &mut self,
        parameter: ProductParameter,
        requested: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !requested.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{} received a non-finite value", parameter.id()),
            ));
        }
        let Some(resolved) = self.parameters[parameter.slot()] else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let value = requested.clamp(resolved.range.minimum, resolved.range.maximum);
        // SAFETY: self uniquely owns the live Model and the resolved index was
        // validated against this Model's parameter count during construction.
        unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index] = value;
        }
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }

    pub(crate) fn set_parameter_by_id(
        &mut self,
        id: &str,
        requested: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !requested.is_finite() || !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received an invalid motion value or weight"),
            ));
        }
        let Some(resolved) = self.parameters_by_id.get(id).copied() else {
            return Ok(ParameterUpdate::Unsupported);
        };
        // SAFETY: self uniquely owns the Model and the index was validated
        // against this Model's parameter count while building parameters_by_id.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            let current = values[resolved.index];
            let blended = current + (requested - current) * weight;
            let clamped = blended.clamp(resolved.range.minimum, resolved.range.maximum);
            values[resolved.index] = clamped;
            clamped
        };
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }

    pub(crate) fn parameter_value_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        let Some(resolved) = self.parameters_by_id.get(id).copied() else {
            return Ok(None);
        };
        // SAFETY: self owns the live Model and the resolved index was checked
        // against this Model's parameter array during construction.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index]
        };
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("{id} has a non-finite current value"),
            ));
        }
        Ok(Some(value))
    }

    pub(crate) fn restore_parameter_defaults(&mut self) -> Result<(), Live2dError> {
        // SAFETY: self uniquely owns the Model. Each parameters_by_id entry
        // was resolved against this exact parameter array during construction.
        unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            for resolved in self.parameters_by_id.values() {
                values[resolved.index] = resolved.range.default;
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
        Ok(RenderSnapshot {
            canvas: unsafe { read_canvas(model)? },
            model_opacity: 1.0,
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

    unsafe fn parameter_count(&self) -> Result<usize, Live2dError> {
        // SAFETY: self.model points into the live model allocation.
        nonnegative(
            unsafe { sys::csmGetParameterCount(self.model.as_ptr()) },
            "parameter count",
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

unsafe fn resolve_parameters(model: *mut sys::csmModel) -> Result<ResolvedParameters, Live2dError> {
    // SAFETY: model is freshly initialized and remains owned by CoreModel.
    let count = nonnegative(
        unsafe { sys::csmGetParameterCount(model) },
        "parameter count",
    )?;
    // SAFETY: all pointer/count pairs come from the same live Model.
    let ids = unsafe { checked_slice(sys::csmGetParameterIds(model), count, "parameter ids")? };
    let minimums = unsafe {
        checked_slice(
            sys::csmGetParameterMinimumValues(model),
            count,
            "parameter minimums",
        )?
    };
    let maximums = unsafe {
        checked_slice(
            sys::csmGetParameterMaximumValues(model),
            count,
            "parameter maximums",
        )?
    };
    let defaults = unsafe {
        checked_slice(
            sys::csmGetParameterDefaultValues(model),
            count,
            "parameter defaults",
        )?
    };
    let mut resolved = [None; ProductParameter::COUNT];
    let mut by_id = BTreeMap::new();
    for index in 0..count {
        let id_pointer = ids[index];
        if id_pointer.is_null() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreArray,
                format!("Core returned a null parameter id at index {index}"),
            ));
        }
        // SAFETY: Cubism Core parameter IDs are documented as NUL-terminated
        // strings whose storage remains valid for the Model lifetime.
        let id = unsafe { CStr::from_ptr(id_pointer) }
            .to_str()
            .map_err(|_| {
                Live2dError::new(
                    Live2dErrorCode::InvalidCoreValue,
                    format!("Core returned a non-UTF-8 parameter id at index {index}"),
                )
            })?;
        let range = ParameterRange {
            minimum: minimums[index],
            maximum: maximums[index],
            default: defaults[index],
        };
        if !range.minimum.is_finite()
            || !range.maximum.is_finite()
            || !range.default.is_finite()
            || range.minimum > range.maximum
            || !(range.minimum..=range.maximum).contains(&range.default)
        {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned an invalid range for {id}"),
            ));
        }
        let entry = ResolvedParameter { index, range };
        if by_id.insert(id.to_owned(), entry).is_some() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned duplicate parameter id {id}"),
            ));
        }
        if let Some(parameter) = ProductParameter::ALL
            .iter()
            .copied()
            .find(|parameter| parameter.id() == id)
        {
            resolved[parameter.slot()] = Some(entry);
        }
    }
    Ok(ResolvedParameters {
        product: resolved,
        by_id,
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

unsafe fn checked_slice_mut<'a, T>(
    pointer: *mut T,
    count: usize,
    name: &str,
) -> Result<&'a mut [T], Live2dError> {
    if count == 0 {
        return Ok(&mut []);
    }
    if pointer.is_null() {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreArray,
            format!("Core returned a null {name} array for {count} values"),
        ));
    }
    // SAFETY: the Core owns at least count uniquely writable elements for
    // this pointer/count pair while the caller's Model owner remains alive.
    Ok(unsafe { std::slice::from_raw_parts_mut(pointer, count) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};

    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    fn preset_model(id: &str) -> CommittedModel {
        PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model")
    }

    #[test]
    fn all_preset_models_produce_stable_drawable_snapshots() {
        for id in ["standard", "keyboard", "gamepad"] {
            let committed = preset_model(id);
            let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
            let first = model.update_and_snapshot().expect("first snapshot");
            assert!(!first.drawables.is_empty());
            assert!(first.drawables.iter().all(|drawable| {
                drawable.texture_id.index() < model.texture_assets().len()
                    && !drawable.vertices.is_empty()
                    && !drawable.indices.is_empty()
            }));
            for _ in 0..10 {
                assert_eq!(model.update_and_snapshot().expect("repeat snapshot"), first);
            }
        }
    }

    #[test]
    fn preset_product_parameters_resolve_and_drive_drawables() {
        for id in ["standard", "keyboard", "gamepad"] {
            let committed = preset_model(id);
            let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
            let expected_parameters: &[ProductParameter] = match id {
                "standard" => &[
                    ProductParameter::AngleX,
                    ProductParameter::AngleY,
                    ProductParameter::AngleZ,
                    ProductParameter::EyeBallX,
                    ProductParameter::EyeBallY,
                    ProductParameter::LeftHandDown,
                    ProductParameter::MouseX,
                    ProductParameter::MouseY,
                    ProductParameter::MouseLeftDown,
                    ProductParameter::MouseRightDown,
                ],
                "keyboard" => &[
                    ProductParameter::AngleX,
                    ProductParameter::AngleY,
                    ProductParameter::AngleZ,
                    ProductParameter::EyeBallX,
                    ProductParameter::EyeBallY,
                    ProductParameter::LeftHandDown,
                    ProductParameter::RightHandDown,
                ],
                "gamepad" => &[
                    ProductParameter::AngleX,
                    ProductParameter::AngleY,
                    ProductParameter::AngleZ,
                    ProductParameter::EyeBallX,
                    ProductParameter::EyeBallY,
                    ProductParameter::LeftHandDown,
                    ProductParameter::RightHandDown,
                    ProductParameter::StickLeftDown,
                    ProductParameter::StickRightDown,
                    ProductParameter::StickShowLeftHand,
                    ProductParameter::StickShowRightHand,
                    ProductParameter::StickLeftX,
                    ProductParameter::StickLeftY,
                    ProductParameter::StickRightX,
                    ProductParameter::StickRightY,
                ],
                _ => unreachable!("preset model list is fixed"),
            };
            for parameter in ProductParameter::ALL {
                assert_eq!(
                    model.parameter_range(parameter).is_some(),
                    expected_parameters.contains(&parameter),
                    "{id} support mismatch for {}",
                    parameter.id()
                );
            }
            let baseline = model.update_and_snapshot().expect("baseline snapshot");
            let range = model
                .parameter_range(ProductParameter::LeftHandDown)
                .expect("left hand parameter");
            let update = model
                .set_parameter(ProductParameter::LeftHandDown, f32::MAX)
                .expect("set parameter");
            assert_eq!(
                update,
                ParameterUpdate::Applied {
                    value: range.maximum,
                    clamped: true,
                }
            );
            assert_eq!(
                model
                    .parameter_value(ProductParameter::LeftHandDown)
                    .expect("read parameter"),
                Some(range.maximum)
            );
            let pressed = model.update_and_snapshot().expect("pressed snapshot");
            assert_ne!(pressed, baseline, "{id} left hand must affect drawables");
        }
    }

    #[test]
    fn parameter_updates_reject_non_finite_and_report_unsupported_ids() {
        let committed = preset_model("keyboard");
        let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
        let error = model
            .set_parameter(ProductParameter::LeftHandDown, f32::NAN)
            .expect_err("NaN must fail");
        assert_eq!(error.code, Live2dErrorCode::ParameterValueInvalid);
        assert_eq!(
            model
                .set_parameter(ProductParameter::MouseLeftDown, 1.0)
                .expect("unsupported is not corrupt"),
            ParameterUpdate::Unsupported
        );
        assert_eq!(model.parameter_range(ProductParameter::MouseLeftDown), None);
    }
}
