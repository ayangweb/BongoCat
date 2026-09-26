//! Every refusal the service can report, and how it reads.
//!
//! The codes are stable and the text is a placeholder: the window maps a code to
//! a localized message by name, so a code that is added is a message that has to
//! be added, and a code that is renamed is a message that silently goes missing.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsErrorCode {
    ServiceUnavailable,
    SnapshotOutdated,
    RuntimeUnavailable,
    InvalidShortcutBindings,
    ConfigPersistFailed,
    ConfigPermissionDenied,
    ConfigStorageFull,
    ConfigTargetOccupied,
    BackupLocationOpenFailed,
    ModelUnavailable,
    ModelSwitchFailed,
    ModelBehaviorPreviewUnavailable,
    ModelBehaviorPreviewFailed,
    ModelTitleInvalid,
    ModelCoverInvalid,
    ModelCoverUpdateFailed,
    ModelSourcePickerUnavailable,
    ModelLocationOpenFailed,
    InvalidModelId,
    ModelAlreadyInstalled,
    ModelImportInvalidPackage,
    ModelImportDropInvalid,
    ModelImportSourceInvalid,
    ModelImportSourceChanged,
    ModelImportSourceUnsupported,
    ModelImportCancelled,
    ModelStoreBusy,
    ModelImportFailed,
    PresetModelCannotBeDeleted,
    ModelNotFound,
    ModelDeleteFailed,
    DiagnosticsExportFailed,
    SoftwareInfoCopyFailed,
    ExternalLinkOpenFailed,
    LogLocationOpenFailed,
    StartupItemUpdateFailed,
    StatusIconUpdateFailed,
    TaskbarIconUpdateFailed,
    WindowHideFailed,
    WindowStatePersistFailed,
    ShutdownFailed,
}

impl SettingsErrorCode {
    pub const ALL: [Self; 41] = [
        Self::ServiceUnavailable,
        Self::SnapshotOutdated,
        Self::RuntimeUnavailable,
        Self::InvalidShortcutBindings,
        Self::ConfigPersistFailed,
        Self::ConfigPermissionDenied,
        Self::ConfigStorageFull,
        Self::ConfigTargetOccupied,
        Self::BackupLocationOpenFailed,
        Self::ModelUnavailable,
        Self::ModelSwitchFailed,
        Self::ModelBehaviorPreviewUnavailable,
        Self::ModelBehaviorPreviewFailed,
        Self::ModelTitleInvalid,
        Self::ModelCoverInvalid,
        Self::ModelCoverUpdateFailed,
        Self::ModelSourcePickerUnavailable,
        Self::ModelLocationOpenFailed,
        Self::InvalidModelId,
        Self::ModelAlreadyInstalled,
        Self::ModelImportInvalidPackage,
        Self::ModelImportDropInvalid,
        Self::ModelImportSourceInvalid,
        Self::ModelImportSourceChanged,
        Self::ModelImportSourceUnsupported,
        Self::ModelImportCancelled,
        Self::ModelStoreBusy,
        Self::ModelImportFailed,
        Self::PresetModelCannotBeDeleted,
        Self::ModelNotFound,
        Self::ModelDeleteFailed,
        Self::DiagnosticsExportFailed,
        Self::SoftwareInfoCopyFailed,
        Self::ExternalLinkOpenFailed,
        Self::LogLocationOpenFailed,
        Self::StartupItemUpdateFailed,
        Self::StatusIconUpdateFailed,
        Self::TaskbarIconUpdateFailed,
        Self::WindowHideFailed,
        Self::WindowStatePersistFailed,
        Self::ShutdownFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceUnavailable => "service_unavailable",
            Self::SnapshotOutdated => "snapshot_outdated",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::InvalidShortcutBindings => "invalid_shortcut_bindings",
            Self::ConfigPersistFailed => "config_persist_failed",
            Self::ConfigPermissionDenied => "config_permission_denied",
            Self::ConfigStorageFull => "config_storage_full",
            Self::ConfigTargetOccupied => "config_target_occupied",
            Self::BackupLocationOpenFailed => "backup_location_open_failed",
            Self::ModelUnavailable => "model_unavailable",
            Self::ModelSwitchFailed => "model_switch_failed",
            Self::ModelBehaviorPreviewUnavailable => "model_behavior_preview_unavailable",
            Self::ModelBehaviorPreviewFailed => "model_behavior_preview_failed",
            Self::ModelTitleInvalid => "model_title_invalid",
            Self::ModelCoverInvalid => "model_cover_invalid",
            Self::ModelCoverUpdateFailed => "model_cover_update_failed",
            Self::ModelSourcePickerUnavailable => "model_source_picker_unavailable",
            Self::ModelLocationOpenFailed => "model_location_open_failed",
            Self::InvalidModelId => "invalid_model_id",
            Self::ModelAlreadyInstalled => "model_already_installed",
            Self::ModelImportInvalidPackage => "model_import_invalid_package",
            Self::ModelImportDropInvalid => "model_import_drop_invalid",
            Self::ModelImportSourceInvalid => "model_import_source_invalid",
            Self::ModelImportSourceChanged => "model_import_source_changed",
            Self::ModelImportSourceUnsupported => "model_import_source_unsupported",
            Self::ModelImportCancelled => "model_import_cancelled",
            Self::ModelStoreBusy => "model_store_busy",
            Self::ModelImportFailed => "model_import_failed",
            Self::PresetModelCannotBeDeleted => "preset_model_cannot_be_deleted",
            Self::ModelNotFound => "model_not_found",
            Self::ModelDeleteFailed => "model_delete_failed",
            Self::DiagnosticsExportFailed => "diagnostics_export_failed",
            Self::SoftwareInfoCopyFailed => "software_info_copy_failed",
            Self::ExternalLinkOpenFailed => "external_link_open_failed",
            Self::LogLocationOpenFailed => "log_location_open_failed",
            Self::StartupItemUpdateFailed => "startup_item_update_failed",
            Self::StatusIconUpdateFailed => "status_icon_update_failed",
            Self::TaskbarIconUpdateFailed => "taskbar_icon_update_failed",
            Self::WindowHideFailed => "window_hide_failed",
            Self::WindowStatePersistFailed => "window_state_persist_failed",
            Self::ShutdownFailed => "shutdown_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{message}", message = self.message())]
pub struct SettingsError {
    pub(crate) code: SettingsErrorCode,
}

impl SettingsError {
    pub const fn new(code: SettingsErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> SettingsErrorCode {
        self.code
    }
}

impl SettingsError {
    pub(crate) fn message(&self) -> &'static str {
        match self.code {
            SettingsErrorCode::ServiceUnavailable => "Settings service is unavailable",
            SettingsErrorCode::SnapshotOutdated => {
                "Settings changed elsewhere. Review the latest settings and try again."
            }
            SettingsErrorCode::RuntimeUnavailable => "The setting did not take effect",
            SettingsErrorCode::InvalidShortcutBindings => {
                "Shortcut bindings are invalid or conflict"
            }
            SettingsErrorCode::ConfigPersistFailed => "The setting could not be saved",
            SettingsErrorCode::ConfigPermissionDenied => {
                "The configuration file cannot be written; check permissions and retry"
            }
            SettingsErrorCode::ConfigStorageFull => {
                "The disk holding the configuration is full; free space and retry"
            }
            SettingsErrorCode::ConfigTargetOccupied => {
                "The configuration location is in use; close the program using it and retry"
            }
            SettingsErrorCode::BackupLocationOpenFailed => {
                "The configuration backup folder could not be opened"
            }
            SettingsErrorCode::ModelUnavailable => "Selected model is unavailable",
            SettingsErrorCode::ModelSwitchFailed => "The selected model could not be activated",
            SettingsErrorCode::ModelBehaviorPreviewUnavailable => {
                "That action or expression belongs to a model that is no longer in use"
            }
            SettingsErrorCode::ModelBehaviorPreviewFailed => {
                "The action or expression could not be played"
            }
            SettingsErrorCode::ModelTitleInvalid => "The model name is not usable",
            SettingsErrorCode::ModelCoverInvalid => "Cover image must be a PNG file",
            SettingsErrorCode::ModelCoverUpdateFailed => "Model cover could not be updated",
            SettingsErrorCode::ModelSourcePickerUnavailable => {
                "The file dialog could not be completed. Try again."
            }
            SettingsErrorCode::ModelLocationOpenFailed => "The model folder could not be opened",
            SettingsErrorCode::InvalidModelId => "The model ID is invalid",
            SettingsErrorCode::ModelAlreadyInstalled => "This model is already installed",
            SettingsErrorCode::ModelImportInvalidPackage => "Model package is invalid",
            SettingsErrorCode::ModelImportDropInvalid => "Drop one valid model folder at a time",
            SettingsErrorCode::ModelImportSourceInvalid => {
                "The selected folder contains BongoCat's model storage. Choose an individual model folder instead."
            }
            SettingsErrorCode::ModelImportSourceChanged => "Model source changed during import",
            SettingsErrorCode::ModelImportSourceUnsupported => {
                "The model source contains an unsupported file"
            }
            SettingsErrorCode::ModelImportCancelled => "Model import was cancelled",
            SettingsErrorCode::ModelStoreBusy => {
                "Another model operation is in progress. Try again in a moment."
            }
            SettingsErrorCode::ModelImportFailed => "Model could not be imported",
            SettingsErrorCode::PresetModelCannotBeDeleted => "Built-in models cannot be deleted",
            SettingsErrorCode::ModelNotFound => "The model was not found",
            SettingsErrorCode::ModelDeleteFailed => "The imported model could not be deleted",
            SettingsErrorCode::DiagnosticsExportFailed => "Diagnostics could not be exported",
            SettingsErrorCode::SoftwareInfoCopyFailed => "Software information could not be copied",
            SettingsErrorCode::ExternalLinkOpenFailed => "The link could not be opened",
            SettingsErrorCode::LogLocationOpenFailed => {
                "The application log folder could not be opened"
            }
            SettingsErrorCode::StartupItemUpdateFailed => {
                "The login startup setting could not be updated"
            }
            SettingsErrorCode::StatusIconUpdateFailed => "Could not update the system icon.",
            SettingsErrorCode::TaskbarIconUpdateFailed => "Could not update the taskbar icon.",
            SettingsErrorCode::WindowHideFailed => "Settings window could not be hidden",
            SettingsErrorCode::WindowStatePersistFailed => "The window layout could not be saved",
            SettingsErrorCode::ShutdownFailed => "BongoCat could not close completely",
        }
    }
}
