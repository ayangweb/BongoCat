//! Why a package was refused.
//!
//! A refusal has to say what was wrong with the package, not just that
//! something was: the settings window shows the diagnostic next to the model,
//! and a user cannot act on "invalid model". Every code here is stable, because
//! the settings protocol maps it to a localized message by name.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiagnostic {
    InvalidModelId,
    ModelEntryAmbiguous,
    ModelEntryMissing,
    ModelFileCountExceeded,
    ModelFileTooLarge,
    ModelIoError,
    ModelJsonInvalid,
    ModelJsonTooLarge,
    ModelMocMissing,
    ModelPackageDepthExceeded,
    ModelPackageSizeExceeded,
    ModelReferenceEscapesRoot,
    ModelReferenceInvalid,
    ModelReferenceSymlinkEscape,
    ModelResourceInvalid,
    ModelResourceMissing,
    ModelResourceNotFile,
    ModelSymlinkDirectoryUnsupported,
    ModelTextureDimensionExceeded,
    ModelTextureInvalidPng,
    ModelTextureMissing,
    ModelUnsupportedVersion,
}

impl ModelDiagnostic {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidModelId => "invalid_model_id",
            Self::ModelEntryAmbiguous => "model_entry_ambiguous",
            Self::ModelEntryMissing => "model_entry_missing",
            Self::ModelFileCountExceeded => "model_file_count_exceeded",
            Self::ModelFileTooLarge => "model_file_too_large",
            Self::ModelIoError => "model_io_error",
            Self::ModelJsonInvalid => "model_json_invalid",
            Self::ModelJsonTooLarge => "model_json_too_large",
            Self::ModelMocMissing => "model_moc_missing",
            Self::ModelPackageDepthExceeded => "model_package_depth_exceeded",
            Self::ModelPackageSizeExceeded => "model_package_size_exceeded",
            Self::ModelReferenceEscapesRoot => "model_reference_escapes_root",
            Self::ModelReferenceInvalid => "model_reference_invalid",
            Self::ModelReferenceSymlinkEscape => "model_reference_symlink_escape",
            Self::ModelResourceInvalid => "model_resource_invalid",
            Self::ModelResourceMissing => "model_resource_missing",
            Self::ModelResourceNotFile => "model_resource_not_file",
            Self::ModelSymlinkDirectoryUnsupported => "model_symlink_directory_unsupported",
            Self::ModelTextureDimensionExceeded => "model_texture_dimension_exceeded",
            Self::ModelTextureInvalidPng => "model_texture_invalid_png",
            Self::ModelTextureMissing => "model_texture_missing",
            Self::ModelUnsupportedVersion => "model_unsupported_version",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}", message = self.message())]
pub struct ModelError {
    pub code: ModelDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
}

impl ModelError {
    pub(crate) fn new(
        code: ModelDiagnostic,
        resource: Option<&str>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            resource: resource.map(str::to_owned),
            detail: detail.into(),
        }
    }

    pub(crate) fn message(&self) -> String {
        match &self.resource {
            Some(resource) => {
                format!("{} ({resource}): {}", self.code.as_str(), self.detail)
            }
            None => format!("{}: {}", self.code.as_str(), self.detail),
        }
    }
}
