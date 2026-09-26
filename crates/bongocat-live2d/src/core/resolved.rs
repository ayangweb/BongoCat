//! Resolving the model's own parameter and part tables into the shapes the
//! adapter reads.
//!
//! Cubism identifies parameters by index and parts by index, while the product
//! identifies them by the ids the model was authored with. Resolution happens
//! once at load: a product parameter a model does not have resolves to nothing
//! rather than to a wrong index, because driving the wrong parameter moves
//! something the user did not ask to move.

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct ResolvedParameter {
    pub(crate) index: usize,
    pub(crate) range: ParameterRange,
}

pub(crate) struct ResolvedParameters {
    pub(crate) product: [Option<ResolvedParameter>; ProductParameter::COUNT],
    pub(crate) by_id: BTreeMap<String, ResolvedParameter>,
}

pub(crate) struct ResolvedParts {
    pub(crate) by_id: BTreeMap<String, usize>,
    pub(crate) opacity_defaults: Vec<f32>,
}

pub(crate) unsafe fn resolve_parameters(
    model: *mut sys::csmModel,
) -> Result<ResolvedParameters, Live2dError> {
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
        if id.is_empty() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned an empty parameter id at index {index}"),
            ));
        }
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

pub(crate) unsafe fn resolve_parts(
    model: *mut sys::csmModel,
) -> Result<ResolvedParts, Live2dError> {
    // SAFETY: model is freshly initialized and remains owned by CoreModel.
    let count = nonnegative(unsafe { sys::csmGetPartCount(model) }, "part count")?;
    // SAFETY: the pointer/count pair comes from the same live Model.
    let ids = unsafe { checked_slice(sys::csmGetPartIds(model), count, "part ids")? };
    let opacity_defaults = unsafe {
        checked_slice(
            sys::csmGetPartOpacities(model),
            count,
            "initial part opacities",
        )?
    }
    .iter()
    .copied()
    .map(|opacity| {
        if opacity.is_finite() {
            Ok(opacity)
        } else {
            Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                "Core returned a non-finite initial part opacity",
            ))
        }
    })
    .collect::<Result<Vec<_>, _>>()?;
    let parent_indices = unsafe {
        checked_slice(
            sys::csmGetPartParentPartIndices(model),
            count,
            "part parent indices",
        )?
    };
    let offscreen_count = nonnegative(
        unsafe { sys::csmGetOffscreenCount(model) },
        "offscreen count",
    )?;
    let offscreen_indices = unsafe {
        checked_slice(
            sys::csmGetPartOffscreenIndices(model),
            count,
            "part offscreen indices",
        )?
    };
    for index in 0..count {
        let parent = parent_indices[index];
        if parent < -1 || usize::try_from(parent).is_ok_and(|parent| parent >= count) {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("part {index} has an out-of-range parent index {parent}"),
            ));
        }
        let offscreen = offscreen_indices[index];
        if offscreen < -1
            || usize::try_from(offscreen).is_ok_and(|offscreen| offscreen >= offscreen_count)
        {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("part {index} has an out-of-range offscreen index {offscreen}"),
            ));
        }
    }
    let mut by_id = BTreeMap::new();
    for (index, &pointer) in ids.iter().enumerate() {
        if pointer.is_null() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreArray,
                format!("Core returned a null part id at index {index}"),
            ));
        }
        // SAFETY: Core documents part IDs as NUL-terminated strings that live
        // for the Model lifetime.
        let id = unsafe { CStr::from_ptr(pointer) }.to_str().map_err(|_| {
            Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned a non-UTF-8 part id at index {index}"),
            )
        })?;
        if id.is_empty() || by_id.insert(id.to_owned(), index).is_some() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("Core returned an invalid or duplicate part id at index {index}"),
            ));
        }
    }
    Ok(ResolvedParts {
        by_id,
        opacity_defaults,
    })
}
