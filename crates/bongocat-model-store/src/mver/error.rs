//! Why a conversion was refused.
//!
//! Distinct from a package error on purpose: the source was not a model package
//! at all, so the message the settings window shows has to be about the legacy
//! layout rather than about a model this product could not read.

use super::*;

pub(crate) fn conversion_error(
    resource: Option<&str>,
    detail: impl Into<String>,
) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::SourceConversionFailed,
        resource.map(str::to_owned),
        detail,
    )
}
