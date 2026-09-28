//! Why a model could not be loaded, and what a code means to a reader.
//!
//! The codes are stable and the text is assembled from the code, the resource
//! and a detail: a failure has to name the file it happened to, or a user cannot
//! act on "the model is invalid".

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Live2dErrorCode {
    CoreVersionMismatch,
    EmptyMoc,
    InvalidCoreArray,
    InvalidCoreValue,
    MocConsistencyFailed,
    MocReviveFailed,
    ModelInitializeFailed,
    ModelMemoryInvalid,
    ResourceIo,
    TextureIndexInvalid,
    ParameterValueInvalid,
    MotionInvalid,
    MotionNotFound,
    ExpressionInvalid,
    ExpressionNotFound,
    UnsupportedBlendMode,
}

impl Live2dErrorCode {
    pub const ALL: [Self; 16] = [
        Self::CoreVersionMismatch,
        Self::EmptyMoc,
        Self::InvalidCoreArray,
        Self::InvalidCoreValue,
        Self::MocConsistencyFailed,
        Self::MocReviveFailed,
        Self::ModelInitializeFailed,
        Self::ModelMemoryInvalid,
        Self::ResourceIo,
        Self::TextureIndexInvalid,
        Self::ParameterValueInvalid,
        Self::MotionInvalid,
        Self::MotionNotFound,
        Self::ExpressionInvalid,
        Self::ExpressionNotFound,
        Self::UnsupportedBlendMode,
    ];

    /// Stable, path-free identifier for diagnostics and typed callers.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CoreVersionMismatch => "core_version_mismatch",
            Self::EmptyMoc => "empty_moc",
            Self::InvalidCoreArray => "invalid_core_array",
            Self::InvalidCoreValue => "invalid_core_value",
            Self::MocConsistencyFailed => "moc_consistency_failed",
            Self::MocReviveFailed => "moc_revive_failed",
            Self::ModelInitializeFailed => "model_initialize_failed",
            Self::ModelMemoryInvalid => "model_memory_invalid",
            Self::ResourceIo => "resource_io",
            Self::TextureIndexInvalid => "texture_index_invalid",
            Self::ParameterValueInvalid => "parameter_value_invalid",
            Self::MotionInvalid => "motion_invalid",
            Self::MotionNotFound => "motion_not_found",
            Self::ExpressionInvalid => "expression_invalid",
            Self::ExpressionNotFound => "expression_not_found",
            Self::UnsupportedBlendMode => "unsupported_blend_mode",
        }
    }
}

impl fmt::Display for Live2dErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct Live2dError {
    pub code: Live2dErrorCode,
    pub detail: String,
}

impl Live2dError {
    pub(crate) fn new(code: Live2dErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl From<PlaybackError> for Live2dError {
    fn from(error: PlaybackError) -> Self {
        let code = match error.code {
            PlaybackErrorCode::ExpressionInvalid => Live2dErrorCode::ExpressionInvalid,
            PlaybackErrorCode::MotionInvalid => Live2dErrorCode::MotionInvalid,
        };
        Self::new(code, error.detail)
    }
}
