//! A Cubism failure, as a runtime category.
//!
//! The runtime's codes are stable and the diagnostics window maps them to localized
//! text, so a Cubism error is translated once here rather than carrying the
//! vendor's message up through four layers.

use super::*;

pub(crate) fn map_live2d_error(
    _error: Live2dError,
    fallback: RuntimeRenderErrorCode,
) -> RuntimeRenderErrorCode {
    fallback
}
