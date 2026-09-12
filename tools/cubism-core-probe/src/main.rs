#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

#[allow(dead_code)]
mod sys {
    include!(env!("BONGOCAT_CUBISM_BINDINGS"));
}

use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    env, fs,
    io::Write,
    path::PathBuf,
    ptr::NonNull,
};

const EXPECTED_CORE_VERSION: u32 = 0x0600_0001;
const EXPECTED_LATEST_MOC_VERSION: u32 = 6;

#[derive(Clone, Debug, PartialEq)]
struct Observation {
    moc_version: u32,
    parameter_count: usize,
    part_count: usize,
    drawable_count: usize,
    canvas_width: f32,
    canvas_height: f32,
    origin_x: f32,
    origin_y: f32,
    pixels_per_unit: f32,
    vertex_count: usize,
    index_count: usize,
    mask_reference_count: usize,
    offscreen_count: usize,
    offscreen_mask_reference_count: usize,
}

struct AlignedMemory {
    pointer: NonNull<u8>,
    layout: Layout,
}

impl AlignedMemory {
    fn zeroed(size: usize, alignment: usize) -> Result<Self, String> {
        let layout = Layout::from_size_align(size, alignment)
            .map_err(|error| format!("invalid allocation layout: {error}"))?;
        // SAFETY: `layout` has non-zero size and valid power-of-two alignment. The allocation is
        // owned by this value and released exactly once with the identical layout in `Drop`.
        let pointer = unsafe { NonNull::new(alloc_zeroed(layout)) }
            .ok_or_else(|| format!("failed to allocate {size} bytes aligned to {alignment}"))?;
        Ok(Self { pointer, layout })
    }

    fn from_bytes(bytes: &[u8], alignment: usize) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("Moc file is empty".to_owned());
        }
        let memory = Self::zeroed(bytes.len(), alignment)?;
        // SAFETY: both pointers are valid for `bytes.len()` bytes, the destination allocation is
        // uniquely owned, and the source and destination do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory.pointer.as_ptr(), bytes.len());
        }
        Ok(memory)
    }

    fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
        self.pointer.as_ptr().cast()
    }
}

impl Drop for AlignedMemory {
    fn drop(&mut self) {
        // SAFETY: `pointer` came from `alloc_zeroed` with this exact layout and has not been freed.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) }
    }
}

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "Cubism Core probe failed: {error}"
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (cycles, models) = parse_arguments(env::args().skip(1))?;
    // SAFETY: these functions take no pointers and are loaded from the Core selected explicitly at
    // link/runtime. Their values are checked before any model memory crosses the FFI boundary.
    let (core_version, latest_moc_version) =
        unsafe { (sys::csmGetVersion(), sys::csmGetLatestMocVersion()) };
    if core_version != EXPECTED_CORE_VERSION {
        return Err(format!(
            "Core version mismatch: expected 0x{EXPECTED_CORE_VERSION:08x}, got 0x{core_version:08x}"
        ));
    }
    if latest_moc_version != EXPECTED_LATEST_MOC_VERSION {
        return Err(format!(
            "latest Moc version mismatch: expected {EXPECTED_LATEST_MOC_VERSION}, got {latest_moc_version}"
        ));
    }
    // SAFETY: the callback has the exact Core C ABI, never dereferences its borrowed pointer, and
    // cannot panic. It remains valid for the lifetime of this process.
    unsafe { sys::csmSetLogFunction(Some(discard_core_log)) };

    println!(
        "{{\"core_version\":\"0x{core_version:08x}\",\"latest_moc_version\":{latest_moc_version},\"cycles\":{cycles}}}"
    );
    for (id, path) in models {
        let bytes = fs::read(&path).map_err(|error| format!("cannot read model {id}: {error}"))?;
        let mut expected = None;
        for cycle in 0..cycles {
            let known_moc_version = expected
                .as_ref()
                .map(|observation: &Observation| observation.moc_version);
            let observation = inspect_model(&bytes, known_moc_version)?;
            if let Some(expected) = &expected {
                if expected != &observation {
                    return Err(format!("model {id} changed at lifecycle cycle {cycle}"));
                }
            } else {
                expected = Some(observation);
            }
        }
        let observation = expected.expect("cycles is validated as non-zero");
        println!(
            concat!(
                "{{\"id\":\"{}\",\"moc_version\":{},\"parameter_count\":{},",
                "\"part_count\":{},\"drawable_count\":{},\"canvas\":{{\"width\":{},",
                "\"height\":{},\"origin_x\":{},\"origin_y\":{},\"pixels_per_unit\":{}}},",
                "\"vertex_count\":{},\"index_count\":{},\"mask_reference_count\":{},",
                "\"offscreen_count\":{},\"offscreen_mask_reference_count\":{}}}"
            ),
            id,
            observation.moc_version,
            observation.parameter_count,
            observation.part_count,
            observation.drawable_count,
            observation.canvas_width,
            observation.canvas_height,
            observation.origin_x,
            observation.origin_y,
            observation.pixels_per_unit,
            observation.vertex_count,
            observation.index_count,
            observation.mask_reference_count,
            observation.offscreen_count,
            observation.offscreen_mask_reference_count,
        );
    }
    Ok(())
}

unsafe extern "C" fn discard_core_log(_message: *const core::ffi::c_char) {}

fn parse_arguments(
    arguments: impl Iterator<Item = String>,
) -> Result<(usize, Vec<(String, PathBuf)>), String> {
    let mut arguments = arguments.peekable();
    let cycles = if arguments.peek().map(String::as_str) == Some("--cycles") {
        arguments.next();
        arguments
            .next()
            .ok_or_else(|| "missing value for --cycles".to_owned())?
            .parse::<usize>()
            .map_err(|_| "--cycles must be a positive integer".to_owned())?
    } else {
        1
    };
    if cycles == 0 {
        return Err("--cycles must be greater than zero".to_owned());
    }

    let mut models = Vec::new();
    for argument in arguments {
        let (id, path) = argument
            .split_once('=')
            .ok_or_else(|| "model arguments must use id=/absolute/path.moc3".to_owned())?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(
                "model id must contain only ASCII letters, digits, dashes, or underscores"
                    .to_owned(),
            );
        }
        let path = PathBuf::from(path);
        if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("moc3")
        {
            return Err(format!("model {id} must use an absolute .moc3 path"));
        }
        models.push((id.to_owned(), path));
    }
    if models.is_empty() {
        return Err("at least one model argument is required".to_owned());
    }
    Ok((cycles, models))
}

fn inspect_model(bytes: &[u8], known_moc_version: Option<u32>) -> Result<Observation, String> {
    let size =
        u32::try_from(bytes.len()).map_err(|_| "Moc is larger than the Core ABI".to_owned())?;
    let mut moc_memory = AlignedMemory::from_bytes(bytes, sys::csmAlignofMoc as usize)?;

    // SAFETY: the Moc and Model allocations satisfy the SDK-reported size/alignment, remain alive
    // for every pointer read below, and are uniquely owned by this call. Each count and base pointer
    // is validated before constructing a slice or dereferencing an element pointer.
    unsafe {
        let moc_version = known_moc_version
            .unwrap_or_else(|| sys::csmGetMocVersion(moc_memory.as_mut_ptr(), size));
        if known_moc_version.is_none()
            && sys::csmHasMocConsistency(moc_memory.as_mut_ptr(), size) != 1
        {
            return Err("Moc consistency check failed".to_owned());
        }
        let moc = sys::csmReviveMocInPlace(moc_memory.as_mut_ptr(), size);
        if moc.is_null() {
            return Err("Moc revive returned null".to_owned());
        }
        let model_size = sys::csmGetSizeofModel(moc);
        if model_size == 0 {
            return Err("Core reported a zero-sized Model".to_owned());
        }
        let mut model_memory =
            AlignedMemory::zeroed(model_size as usize, sys::csmAlignofModel as usize)?;
        let model = sys::csmInitializeModelInPlace(moc, model_memory.as_mut_ptr(), model_size);
        if model.is_null() {
            return Err("Model initialization returned null".to_owned());
        }
        sys::csmUpdateModel(model);

        let parameter_count = nonnegative_count(sys::csmGetParameterCount(model), "parameter")?;
        validate_ids(sys::csmGetParameterIds(model), parameter_count, "parameter")?;
        require_array(
            sys::csmGetParameterTypes(model),
            parameter_count,
            "parameter types",
        )?;
        validate_parameters(
            sys::csmGetParameterMinimumValues(model),
            sys::csmGetParameterMaximumValues(model),
            sys::csmGetParameterDefaultValues(model),
            sys::csmGetParameterValues(model),
            parameter_count,
        )?;

        let part_count = nonnegative_count(sys::csmGetPartCount(model), "part")?;
        let offscreen_count = nonnegative_count(sys::csmGetOffscreenCount(model), "offscreen")?;
        validate_ids(sys::csmGetPartIds(model), part_count, "part")?;
        validate_finite_values(
            sys::csmGetPartOpacities(model),
            part_count,
            "part opacities",
        )?;
        validate_index_array(
            sys::csmGetPartOffscreenIndices(model),
            part_count,
            offscreen_count,
            true,
            "part offscreen indices",
        )?;

        let drawable_count = nonnegative_count(sys::csmGetDrawableCount(model), "drawable")?;
        validate_ids(sys::csmGetDrawableIds(model), drawable_count, "drawable")?;
        require_array(
            sys::csmGetDrawableConstantFlags(model),
            drawable_count,
            "constant flags",
        )?;
        require_array(
            sys::csmGetDrawableDynamicFlags(model),
            drawable_count,
            "dynamic flags",
        )?;
        validate_blend_modes(
            sys::csmGetDrawableBlendModes(model),
            drawable_count,
            "drawable blend modes",
        )?;
        require_array(
            sys::csmGetDrawableTextureIndices(model),
            drawable_count,
            "texture indices",
        )?;
        require_array(
            sys::csmGetDrawableDrawOrders(model),
            drawable_count,
            "draw orders",
        )?;
        require_array(
            sys::csmGetRenderOrders(model),
            drawable_count,
            "render orders",
        )?;
        validate_finite_values(
            sys::csmGetDrawableOpacities(model),
            drawable_count,
            "drawable opacities",
        )?;
        validate_vectors(
            sys::csmGetDrawableMultiplyColors(model),
            drawable_count,
            "multiply colors",
        )?;
        validate_vectors(
            sys::csmGetDrawableScreenColors(model),
            drawable_count,
            "screen colors",
        )?;
        validate_index_array(
            sys::csmGetDrawableParentPartIndices(model),
            drawable_count,
            part_count,
            true,
            "drawable parent part indices",
        )?;

        let mask_counts = checked_slice(
            sys::csmGetDrawableMaskCounts(model),
            drawable_count,
            "mask counts",
        )?;
        let masks = checked_slice(sys::csmGetDrawableMasks(model), drawable_count, "masks")?;
        let vertex_counts = checked_slice(
            sys::csmGetDrawableVertexCounts(model),
            drawable_count,
            "vertex counts",
        )?;
        let positions = checked_slice(
            sys::csmGetDrawableVertexPositions(model),
            drawable_count,
            "vertex positions",
        )?;
        let uvs = checked_slice(
            sys::csmGetDrawableVertexUvs(model),
            drawable_count,
            "vertex UVs",
        )?;
        let index_counts = checked_slice(
            sys::csmGetDrawableIndexCounts(model),
            drawable_count,
            "index counts",
        )?;
        let indices = checked_slice(sys::csmGetDrawableIndices(model), drawable_count, "indices")?;

        let mut vertex_count = 0usize;
        let mut index_count = 0usize;
        let mut mask_reference_count = 0usize;
        for index in 0..drawable_count {
            let drawable_vertices = nonnegative_count(vertex_counts[index], "drawable vertex")?;
            let drawable_indices = nonnegative_count(index_counts[index], "drawable index")?;
            let drawable_masks = nonnegative_count(mask_counts[index], "drawable mask")?;
            validate_vector2_values(positions[index], drawable_vertices, "drawable positions")?;
            validate_vector2_values(uvs[index], drawable_vertices, "drawable UVs")?;
            validate_triangle_indices(
                indices[index],
                drawable_indices,
                drawable_vertices,
                "drawable indices",
            )?;
            validate_index_array(
                masks[index],
                drawable_masks,
                drawable_count,
                false,
                "drawable masks",
            )?;
            vertex_count = vertex_count
                .checked_add(drawable_vertices)
                .ok_or_else(|| "vertex count overflow".to_owned())?;
            index_count = index_count
                .checked_add(drawable_indices)
                .ok_or_else(|| "index count overflow".to_owned())?;
            mask_reference_count = mask_reference_count
                .checked_add(drawable_masks)
                .ok_or_else(|| "mask count overflow".to_owned())?;
        }

        validate_blend_modes(
            sys::csmGetOffscreenBlendModes(model),
            offscreen_count,
            "offscreen blend modes",
        )?;
        validate_finite_values(
            sys::csmGetOffscreenOpacities(model),
            offscreen_count,
            "offscreen opacities",
        )?;
        validate_index_array(
            sys::csmGetOffscreenOwnerIndices(model),
            offscreen_count,
            part_count,
            false,
            "offscreen owner indices",
        )?;
        validate_vectors(
            sys::csmGetOffscreenMultiplyColors(model),
            offscreen_count,
            "offscreen multiply colors",
        )?;
        validate_vectors(
            sys::csmGetOffscreenScreenColors(model),
            offscreen_count,
            "offscreen screen colors",
        )?;
        require_array(
            sys::csmGetOffscreenConstantFlags(model),
            offscreen_count,
            "offscreen constant flags",
        )?;
        let offscreen_mask_counts = checked_slice(
            sys::csmGetOffscreenMaskCounts(model),
            offscreen_count,
            "offscreen mask counts",
        )?;
        let offscreen_masks = checked_slice(
            sys::csmGetOffscreenMasks(model),
            offscreen_count,
            "offscreen masks",
        )?;
        let mut offscreen_mask_reference_count = 0usize;
        for index in 0..offscreen_count {
            let count = nonnegative_count(offscreen_mask_counts[index], "offscreen mask")?;
            validate_index_array(
                offscreen_masks[index],
                count,
                drawable_count,
                false,
                "offscreen masks",
            )?;
            offscreen_mask_reference_count = offscreen_mask_reference_count
                .checked_add(count)
                .ok_or_else(|| "offscreen mask count overflow".to_owned())?;
        }

        let mut canvas_size = sys::csmVector2 { X: 0.0, Y: 0.0 };
        let mut canvas_origin = sys::csmVector2 { X: 0.0, Y: 0.0 };
        let mut pixels_per_unit = 0.0;
        sys::csmReadCanvasInfo(
            model,
            &mut canvas_size,
            &mut canvas_origin,
            &mut pixels_per_unit,
        );
        for (name, value) in [
            ("canvas width", canvas_size.X),
            ("canvas height", canvas_size.Y),
            ("origin x", canvas_origin.X),
            ("origin y", canvas_origin.Y),
            ("pixels per unit", pixels_per_unit),
        ] {
            if !value.is_finite() {
                return Err(format!("{name} is not finite"));
            }
        }
        if canvas_size.X <= 0.0 || canvas_size.Y <= 0.0 || pixels_per_unit <= 0.0 {
            return Err("canvas dimensions and pixels per unit must be positive".to_owned());
        }
        sys::csmResetDrawableDynamicFlags(model);

        Ok(Observation {
            moc_version,
            parameter_count,
            part_count,
            drawable_count,
            canvas_width: canvas_size.X,
            canvas_height: canvas_size.Y,
            origin_x: canvas_origin.X,
            origin_y: canvas_origin.Y,
            pixels_per_unit,
            vertex_count,
            index_count,
            mask_reference_count,
            offscreen_count,
            offscreen_mask_reference_count,
        })
    }
}

fn nonnegative_count(value: i32, name: &str) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| format!("Core returned an invalid {name} count"))
}

unsafe fn require_array<T>(pointer: *const T, count: usize, name: &str) -> Result<(), String> {
    // SAFETY: delegated to `checked_slice`, which validates null/count before constructing a slice.
    unsafe { checked_slice(pointer, count, name).map(|_| ()) }
}

unsafe fn validate_parameters(
    minimums: *const f32,
    maximums: *const f32,
    defaults: *const f32,
    values: *const f32,
    count: usize,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let minimums = unsafe { checked_slice(minimums, count, "parameter minimums")? };
    // SAFETY: same owner and count contract as above.
    let maximums = unsafe { checked_slice(maximums, count, "parameter maximums")? };
    // SAFETY: same owner and count contract as above.
    let defaults = unsafe { checked_slice(defaults, count, "parameter defaults")? };
    // SAFETY: same owner and count contract as above.
    let values = unsafe { checked_slice(values, count, "parameter values")? };
    for index in 0..count {
        let (minimum, maximum, default, value) = (
            minimums[index],
            maximums[index],
            defaults[index],
            values[index],
        );
        if ![minimum, maximum, default, value]
            .iter()
            .all(|value| value.is_finite())
            || minimum > maximum
            || !(minimum..=maximum).contains(&default)
            || !(minimum..=maximum).contains(&value)
        {
            return Err("Core returned invalid parameter bounds or values".to_owned());
        }
    }
    Ok(())
}

unsafe fn validate_index_array(
    pointer: *const i32,
    count: usize,
    upper_bound: usize,
    allow_missing: bool,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let values = unsafe { checked_slice(pointer, count, name)? };
    for &value in values {
        if allow_missing && value == -1 {
            continue;
        }
        let value =
            usize::try_from(value).map_err(|_| format!("Core returned a negative {name} value"))?;
        if value >= upper_bound {
            return Err(format!("Core returned an out-of-range {name} value"));
        }
    }
    Ok(())
}

unsafe fn validate_triangle_indices(
    pointer: *const u16,
    count: usize,
    vertex_count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let values = unsafe { checked_slice(pointer, count, name)? };
    if values
        .iter()
        .any(|&value| usize::from(value) >= vertex_count)
    {
        return Err(format!("Core returned an out-of-range {name} value"));
    }
    Ok(())
}

unsafe fn validate_blend_modes(
    pointer: *const i32,
    count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let modes = unsafe { checked_slice(pointer, count, name)? };
    for &mode in modes {
        let color = mode & 0xff;
        let alpha = (mode >> 8) & 0xff;
        if !(0..=17).contains(&color) || !(0..=4).contains(&alpha) || mode & !0xffff != 0 {
            return Err(format!("Core returned an invalid {name} value"));
        }
    }
    Ok(())
}

unsafe fn validate_finite_values(
    pointer: *const f32,
    count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let values = unsafe { checked_slice(pointer, count, name)? };
    if values.iter().any(|value| !value.is_finite()) {
        return Err(format!("Core returned a non-finite {name} value"));
    }
    Ok(())
}

unsafe fn validate_vectors(
    pointer: *const sys::csmVector4,
    count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let values = unsafe { checked_slice(pointer, count, name)? };
    if values.iter().any(|value| {
        [value.X, value.Y, value.Z, value.W]
            .iter()
            .any(|v| !v.is_finite())
    }) {
        return Err(format!("Core returned a non-finite {name} value"));
    }
    Ok(())
}

unsafe fn validate_vector2_values(
    pointer: *const sys::csmVector2,
    count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let values = unsafe { checked_slice(pointer, count, name)? };
    if values
        .iter()
        .any(|value| !value.X.is_finite() || !value.Y.is_finite())
    {
        return Err(format!("Core returned a non-finite {name} value"));
    }
    Ok(())
}

unsafe fn checked_slice<'a, T>(
    pointer: *const T,
    count: usize,
    name: &str,
) -> Result<&'a [T], String> {
    if count == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(format!("Core returned null {name}"));
    }
    // SAFETY: the caller guarantees the Core owner remains alive, and this function validates a
    // non-null base pointer and uses the count returned for the corresponding Core array.
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

unsafe fn validate_ids(
    pointer: *mut *const core::ffi::c_char,
    count: usize,
    name: &str,
) -> Result<(), String> {
    // SAFETY: the caller keeps the Model alive and supplies the matching Core-reported count.
    let ids = unsafe { checked_slice(pointer, count, name)? };
    for id in ids {
        if id.is_null() {
            return Err(format!("Core returned a null {name} id"));
        }
        // SAFETY: Cubism Core documents every ID as a null-terminated ANSI string tied to Model
        // lifetime; null was rejected above and the caller keeps the Model alive.
        let value = unsafe { std::ffi::CStr::from_ptr(*id) };
        if value.to_bytes().is_empty() || value.to_str().is_err() {
            return Err(format!("Core returned an invalid {name} id"));
        }
    }
    Ok(())
}
