//! Why an operation was refused.
//!
//! The store's own failures are distinct from the model's: an import can be
//! refused because the package is invalid, because the source moved under it,
//! or because another writer holds the lock, and the settings window shows a
//! different message for each. Every code is stable, because the settings
//! protocol maps it to a localized message by name.

use super::*;

pub(crate) static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStoreDiagnostic {
    AlreadyExists,
    Cancelled,
    InvalidPackage,
    IoError,
    NotFound,
    SourceContainsStore,
    SourceChanged,
    SourceConversionFailed,
    SourceSymlinkUnsupported,
    SourceEntryUnsupported,
    StoreBusy,
    StoreEntryUnsupported,
}

impl ModelStoreDiagnostic {
    pub const ALL: [Self; 12] = [
        Self::AlreadyExists,
        Self::Cancelled,
        Self::InvalidPackage,
        Self::IoError,
        Self::NotFound,
        Self::SourceContainsStore,
        Self::SourceChanged,
        Self::SourceConversionFailed,
        Self::SourceSymlinkUnsupported,
        Self::SourceEntryUnsupported,
        Self::StoreBusy,
        Self::StoreEntryUnsupported,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyExists => "model_store_already_exists",
            Self::Cancelled => "model_store_cancelled",
            Self::InvalidPackage => "model_store_invalid_package",
            Self::IoError => "model_store_io_error",
            Self::NotFound => "model_store_not_found",
            Self::SourceContainsStore => "model_store_source_contains_store",
            Self::SourceChanged => "model_store_source_changed",
            // A source this product recognizes as a BongoCatMver model but
            // cannot turn into a BongoCat package: an unreadable key image, a
            // missing layer, metadata that does not describe a key table. It is
            // deliberately distinct from `InvalidPackage`, which means the
            // source was read as a package and failed package validation
            // (including the ordinary-package key-mode contract) — this one
            // never became a package at all.
            Self::SourceConversionFailed => "model_store_source_conversion_failed",
            Self::SourceSymlinkUnsupported => "model_store_source_symlink_unsupported",
            Self::SourceEntryUnsupported => "model_store_source_entry_unsupported",
            Self::StoreBusy => "model_store_busy",
            Self::StoreEntryUnsupported => "model_store_entry_unsupported",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}", message = self.message())]
pub struct ModelStoreError {
    pub code: ModelStoreDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
    #[source]
    pub(crate) source: Option<ModelError>,
}

impl ModelStoreError {
    pub(crate) fn new(
        code: ModelStoreDiagnostic,
        resource: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            resource,
            detail: detail.into(),
            source: None,
        }
    }

    pub fn source_conversion_failed(detail: impl Into<String>) -> Self {
        Self::new(ModelStoreDiagnostic::SourceConversionFailed, None, detail)
    }

    pub(crate) fn package(error: ModelError) -> Self {
        Self {
            code: ModelStoreDiagnostic::InvalidPackage,
            resource: error.resource.clone(),
            detail: error.to_string(),
            source: Some(error),
        }
    }

    pub(crate) fn message(&self) -> String {
        match &self.resource {
            Some(resource) => {
                format!("{:?} ({resource}): {}", self.code, self.detail)
            }
            None => format!("{:?}: {}", self.code, self.detail),
        }
    }
}
