//! How much of a model there is.
//!
//! Three numbers, and they are read rather than cached because Cubism is the only
//! thing that knows them: a model that answered a different count after load is a
//! model whose buffers and tables disagree, and every read above would be wrong.

use super::*;

impl CoreModel {
    pub(crate) unsafe fn drawable_count(&self) -> Result<usize, Live2dError> {
        // SAFETY: self.model points into the live model allocation.
        nonnegative(
            unsafe { sys::csmGetDrawableCount(self.model.as_ptr()) },
            "drawable count",
        )
    }
}

impl CoreModel {
    pub(crate) unsafe fn parameter_count(&self) -> Result<usize, Live2dError> {
        // SAFETY: self.model points into the live model allocation.
        nonnegative(
            unsafe { sys::csmGetParameterCount(self.model.as_ptr()) },
            "parameter count",
        )
    }
}

impl CoreModel {
    pub(crate) unsafe fn part_count(&self) -> Result<usize, Live2dError> {
        // SAFETY: self.model points into the live model allocation.
        nonnegative(
            unsafe { sys::csmGetPartCount(self.model.as_ptr()) },
            "part count",
        )
    }
}
