//! Turning an application failure into a settings error.
//!
//! The settings window must never see an application, configuration or store
//! error, and a diagnostic must never name a path. Every mapping therefore ends at
//! a stable `SettingsErrorCode`, and the store's own diagnostics are translated
//! item by item rather than stringified.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

pub(super) fn check_revision(
    application: &Application,
    expected: u64,
) -> Result<(), SettingsError> {
    if application.config_revision() == Some(expected) {
        Ok(())
    } else {
        Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
    }
}

/// The error a failed preview reports.
///
/// A preview is the only path that reaches the runtime's motion and expression
/// commands with an id the page built, so the id errors are preview failures
/// rather than the generic "the setting did not take effect": the page that
/// sent one has to be able to say the behavior itself could not be played.
pub(super) fn map_preview_error(error: ApplicationError) -> SettingsError {
    match error {
        ApplicationError::MotionId(_)
        | ApplicationError::ExpressionId(_)
        | ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish => {
            SettingsError::new(SettingsErrorCode::ModelBehaviorPreviewFailed)
        }
        other => map_application_error(other),
    }
}

pub(super) fn map_application_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::PlatformStorage(_) => SettingsErrorCode::ConfigPersistFailed,
        ApplicationError::Config(error) | ApplicationError::ConfigRollback(error) => {
            settings_config_error_code(&error).unwrap_or(SettingsErrorCode::ConfigPersistFailed)
        }
        ApplicationError::WindowState(_) => SettingsErrorCode::WindowStatePersistFailed,
        ApplicationError::Model(_) | ApplicationError::ModelStore(_) => {
            SettingsErrorCode::ModelUnavailable
        }
        ApplicationError::Shutdown(_)
        | ApplicationError::MotionAudioShutdown(_)
        | ApplicationError::ShutdownAggregate(_) => SettingsErrorCode::ShutdownFailed,
        ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish
        | ApplicationError::RuntimeDidNotPrepareModel => SettingsErrorCode::ModelSwitchFailed,
        _ => SettingsErrorCode::RuntimeUnavailable,
    };
    SettingsError::new(code)
}

/// Map a rename failure to its own code.
///
/// The three outcomes a user can act on differently — a name the configuration
/// will not accept, and a model that is no longer on disk — each get their own
/// code instead of collapsing into the generic settings failure.
pub(super) fn map_model_metadata_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::ModelTitleInvalid => SettingsErrorCode::ModelTitleInvalid,
        ApplicationError::ModelNotFound(_) => SettingsErrorCode::ModelNotFound,
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

pub(super) fn map_model_cover_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::ModelCoverInvalid => SettingsErrorCode::ModelCoverInvalid,
        ApplicationError::ModelNotFound(_) => SettingsErrorCode::ModelNotFound,
        ApplicationError::ModelStore(error) if error.code == ModelStoreDiagnostic::NotFound => {
            SettingsErrorCode::ModelNotFound
        }
        ApplicationError::ModelStore(_) => SettingsErrorCode::ModelCoverUpdateFailed,
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

pub(super) fn settings_config_error_code(error: &ConfigError) -> Option<SettingsErrorCode> {
    if matches!(error, ConfigError::InvalidValue(field) if field.starts_with("shortcuts.")) {
        return Some(SettingsErrorCode::InvalidShortcutBindings);
    }
    match error.write_failure_reason()? {
        ConfigWriteFailureReason::PermissionDenied => {
            Some(SettingsErrorCode::ConfigPermissionDenied)
        }
        ConfigWriteFailureReason::StorageFull => Some(SettingsErrorCode::ConfigStorageFull),
        ConfigWriteFailureReason::TargetOccupied => Some(SettingsErrorCode::ConfigTargetOccupied),
    }
}

pub(super) fn map_model_import_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) => {
            if error.code == ModelDiagnostic::InvalidModelId {
                SettingsErrorCode::InvalidModelId
            } else {
                SettingsErrorCode::ModelImportInvalidPackage
            }
        }
        ApplicationError::ModelStore(error) => map_model_store_import_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

pub(super) const fn map_model_store_import_diagnostic(
    diagnostic: ModelStoreDiagnostic,
) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::AlreadyExists => SettingsErrorCode::ModelAlreadyInstalled,
        ModelStoreDiagnostic::Cancelled => SettingsErrorCode::ModelImportCancelled,
        ModelStoreDiagnostic::InvalidPackage => SettingsErrorCode::ModelImportInvalidPackage,
        ModelStoreDiagnostic::SourceContainsStore => SettingsErrorCode::ModelImportSourceInvalid,
        ModelStoreDiagnostic::SourceChanged => SettingsErrorCode::ModelImportSourceChanged,
        // A source the store cannot read at all — an entry that is not a regular
        // file or directory, a symbolic link, or a BongoCatMver source it cannot
        // convert — is the same user-facing outcome as any other unsupported
        // source entry: the chosen source cannot be imported as it stands. The
        // distinction between them stays in the diagnostic, which is what the
        // diagnostics bundle and the log carry.
        ModelStoreDiagnostic::SourceConversionFailed
        | ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported => {
            SettingsErrorCode::ModelImportSourceUnsupported
        }
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::NotFound
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelImportFailed,
    }
}

pub(super) fn map_model_delete_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::PresetModelDeletion(_) => SettingsErrorCode::PresetModelCannotBeDeleted,
        ApplicationError::ModelStore(error) => map_model_store_delete_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

pub(super) const fn map_model_store_delete_diagnostic(
    diagnostic: ModelStoreDiagnostic,
) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::NotFound => SettingsErrorCode::ModelNotFound,
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::AlreadyExists
        | ModelStoreDiagnostic::Cancelled
        | ModelStoreDiagnostic::InvalidPackage
        | ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::SourceConversionFailed
        | ModelStoreDiagnostic::SourceContainsStore
        | ModelStoreDiagnostic::SourceChanged
        | ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelDeleteFailed,
    }
}
