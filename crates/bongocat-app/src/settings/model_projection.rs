//! The model half of the settings snapshot: the catalog the Model library page
//! shows, and the progress one import action reports.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

use super::error_mapping::*;

pub(super) const fn settings_import_progress(
    progress: ModelImportProgress,
) -> SettingsModelImportProgress {
    SettingsModelImportProgress {
        stage: match progress.stage {
            ModelImportStage::Preparing => SettingsModelImportStage::Preparing,
            ModelImportStage::Copying => SettingsModelImportStage::Copying,
            ModelImportStage::Validating => SettingsModelImportStage::Validating,
            ModelImportStage::Committing => SettingsModelImportStage::Committing,
        },
        files_copied: progress.files_copied,
        bytes_copied: progress.bytes_copied,
    }
}

pub(super) fn settings_model_catalog(application: &Application) -> SettingsModelCatalog {
    match application.model_catalog() {
        Ok(entries) => SettingsModelCatalog {
            entries: entries
                .into_iter()
                .map(|entry| settings_model_entry(application, entry))
                .collect(),
            error: None,
        },
        Err(_) => SettingsModelCatalog {
            entries: Vec::new(),
            error: Some(SettingsModelCatalogError::Unavailable),
        },
    }
}

pub(super) fn configured_model_key(application: &Application) -> Option<SettingsModelKey> {
    let selected = application.config().model.selected_model.as_ref()?;
    Some(SettingsModelKey {
        id: selected.id.clone(),
        origin: settings_origin_from_config(selected.source),
    })
}

/// The configured gamepad-connection model switch.
///
/// A `None` target is the default and stays `None` in the snapshot: it means "the
/// last model activated for this input family", which the product resolves from
/// what happened rather than from configuration. The settings window therefore
/// shows it as its own choice instead of an empty control.
pub(super) fn settings_gamepad_auto_switch(
    switch: &GamepadAutoSwitchConfig,
) -> SettingsGamepadAutoSwitch {
    SettingsGamepadAutoSwitch {
        enabled: switch.enabled,
        connected_model: switch
            .connected_model
            .as_ref()
            .map(settings_key_from_config),
        disconnected_model: switch
            .disconnected_model
            .as_ref()
            .map(settings_key_from_config),
    }
}

pub(super) fn model_mver_input_mode(mode: bongocat_ui_protocol::SettingsMverMode) -> MverInputMode {
    match mode {
        bongocat_ui_protocol::SettingsMverMode::Standard => MverInputMode::Standard,
        bongocat_ui_protocol::SettingsMverMode::Keyboard => MverInputMode::Keyboard,
        bongocat_ui_protocol::SettingsMverMode::Gamepad => MverInputMode::Gamepad,
    }
}

pub(super) fn settings_model_mode(mode: ModelInputMode) -> SettingsModelMode {
    match mode {
        ModelInputMode::Standard => SettingsModelMode::Standard,
        ModelInputMode::Keyboard => SettingsModelMode::Keyboard,
        ModelInputMode::Gamepad => SettingsModelMode::Gamepad,
    }
}

pub(super) fn settings_model_entry(
    application: &Application,
    entry: ModelCatalogEntry,
) -> SettingsModelEntry {
    let id = entry.id().as_str().to_owned();
    let model_origin = entry.origin();
    let origin = settings_origin_from_model(model_origin);
    // The title is user-editable metadata; a model that was never renamed —
    // which is every preset the user has not customised — displays the stable
    // id instead of inventing a name.
    let title = application
        .recorded_model_title(model_origin, &id)
        .map(str::to_owned)
        .unwrap_or_else(|| id.clone());
    let input_mode = application
        .model_input_mode(model_origin, &id)
        .map(settings_model_mode);
    let availability = match entry {
        ModelCatalogEntry::Ready { snapshot, .. } => SettingsModelAvailability::Ready {
            behaviors: snapshot
                .behaviors
                .into_iter()
                .map(settings_model_behavior)
                .collect(),
        },
        ModelCatalogEntry::Invalid { code, .. } => SettingsModelAvailability::Invalid {
            diagnostic: settings_model_diagnostic(code),
        },
    };
    // The directory and the cover are read here rather than in the page: the
    // page only ever displays a path, and a model with no cover at all is
    // reported as `None` instead of a path that does not resolve. The cover a
    // preset ships lives in the bundle, so this is also where the user's
    // replacement gets its say.
    let directory = application.model_directory(model_origin, &id);
    let cover = application.model_cover_path(model_origin, &id);
    SettingsModelEntry {
        id,
        title,
        input_mode,
        origin,
        availability,
        directory,
        cover,
    }
}

pub(super) fn settings_model_behavior(behavior: ModelBehaviorSnapshot) -> SettingsModelBehavior {
    match behavior {
        ModelBehaviorSnapshot::Motion { group, index } => {
            SettingsModelBehavior::Motion { group, index }
        }
        ModelBehaviorSnapshot::Expression { name } => SettingsModelBehavior::Expression { name },
    }
}

/// Play one behavior of the model the runtime is actually running.
///
/// The request names the model it was rendered for, because a shortcut row
/// belongs to one model's behavior list and the page keeps rows for whichever
/// model is live. The runtime only plays the active model's own motions and
/// expressions, so a request whose model has since been switched away from is
/// answered with [`SettingsErrorCode::ModelBehaviorPreviewUnavailable`] instead
/// of being played against whatever is loaded now. Nothing is persisted, and
/// the failure of a preview never changes the model in use.
pub(super) fn preview_model_behavior(
    application: &Application,
    model: &SettingsModelKey,
    behavior: SettingsModelBehavior,
) -> Result<(), SettingsError> {
    let runtime = application.runtime_client().snapshot();
    let active_matches = runtime.active_model.is_some_and(|active| {
        active.id.as_str() == model.id
            && application.active_model_origin() == Some(model_origin_from_settings(model.origin))
    });
    if !active_matches {
        return Err(SettingsError::new(
            SettingsErrorCode::ModelBehaviorPreviewUnavailable,
        ));
    }

    let result = match behavior {
        SettingsModelBehavior::Motion { group, index } => application.preview_motion(group, index),
        SettingsModelBehavior::Expression { name } => application.set_expression(name),
    };
    result.map(|_| ()).map_err(map_preview_error)
}

pub(super) const fn settings_model_diagnostic(
    diagnostic: ModelDiagnostic,
) -> SettingsModelDiagnostic {
    match diagnostic {
        ModelDiagnostic::InvalidModelId => SettingsModelDiagnostic::InvalidModelId,
        ModelDiagnostic::ModelEntryAmbiguous => SettingsModelDiagnostic::ModelEntryAmbiguous,
        ModelDiagnostic::ModelEntryMissing => SettingsModelDiagnostic::ModelEntryMissing,
        ModelDiagnostic::ModelFileCountExceeded => SettingsModelDiagnostic::ModelFileCountExceeded,
        ModelDiagnostic::ModelFileTooLarge => SettingsModelDiagnostic::ModelFileTooLarge,
        ModelDiagnostic::ModelIoError => SettingsModelDiagnostic::ModelIoError,
        ModelDiagnostic::ModelJsonInvalid => SettingsModelDiagnostic::ModelJsonInvalid,
        ModelDiagnostic::ModelJsonTooLarge => SettingsModelDiagnostic::ModelJsonTooLarge,
        ModelDiagnostic::ModelMocMissing => SettingsModelDiagnostic::ModelMocMissing,
        ModelDiagnostic::ModelPackageDepthExceeded => {
            SettingsModelDiagnostic::ModelPackageDepthExceeded
        }
        ModelDiagnostic::ModelPackageSizeExceeded => {
            SettingsModelDiagnostic::ModelPackageSizeExceeded
        }
        ModelDiagnostic::ModelReferenceEscapesRoot => {
            SettingsModelDiagnostic::ModelReferenceEscapesRoot
        }
        ModelDiagnostic::ModelReferenceInvalid => SettingsModelDiagnostic::ModelReferenceInvalid,
        ModelDiagnostic::ModelReferenceSymlinkEscape => {
            SettingsModelDiagnostic::ModelReferenceSymlinkEscape
        }
        ModelDiagnostic::ModelResourceInvalid => SettingsModelDiagnostic::ModelResourceInvalid,
        ModelDiagnostic::ModelResourceMissing => SettingsModelDiagnostic::ModelResourceMissing,
        ModelDiagnostic::ModelResourceNotFile => SettingsModelDiagnostic::ModelResourceNotFile,
        ModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
            SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported
        }
        ModelDiagnostic::ModelTextureDimensionExceeded => {
            SettingsModelDiagnostic::ModelTextureDimensionExceeded
        }
        ModelDiagnostic::ModelTextureInvalidPng => SettingsModelDiagnostic::ModelTextureInvalidPng,
        ModelDiagnostic::ModelTextureMissing => SettingsModelDiagnostic::ModelTextureMissing,
        ModelDiagnostic::ModelUnsupportedVersion => {
            SettingsModelDiagnostic::ModelUnsupportedVersion
        }
    }
}
