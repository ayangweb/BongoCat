//! The bytes handed to the native library, and the ones it hands back.
//!
//! Cubism allocates the moc and the model itself and frees them itself, but
//! everything between — the moc bytes, and the vertex and index buffers Cubism
//! fills — is ours. The wrapper exists so that ownership is one value with one
//! `Drop`: an allocation that outlived its model, or a model whose allocation
//! went first, is a use-after-free that only shows up on a machine that happens
//! to reuse the address.

use super::*;

pub(crate) struct AlignedMemory {
    pub(crate) pointer: NonNull<u8>,
    pub(crate) layout: Layout,
}

impl AlignedMemory {
    pub(crate) fn zeroed(size: usize, alignment: usize) -> Result<Self, Live2dError> {
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

    pub(crate) fn from_bytes(bytes: &[u8], alignment: usize) -> Result<Self, Live2dError> {
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

    pub(crate) fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
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
