//! Loading a model, and checking that what came back is usable.
//!
//! Load is the only point where the library is asked about the model as a whole,
//! so it is where its own tables are validated: the texture indices it reports
//! have to be inside the resources it was given, because a drawable that names a
//! texture that is not there is a read past the end of the array on the first
//! frame.

use super::*;

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
            let parts = resolve_parts(model.as_ptr())?;
            validate_drawable_ids(model.as_ptr())?;
            Ok(Self {
                model,
                parameters: parameters.product,
                parameters_by_id: parameters.by_id,
                parts_by_id: parts.by_id,
                part_opacity_defaults: parts.opacity_defaults,
                model_memory: ManuallyDrop::new(model_memory),
                moc_memory: ManuallyDrop::new(moc_memory),
            })
        }
    }
}

impl CoreModel {
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
}
