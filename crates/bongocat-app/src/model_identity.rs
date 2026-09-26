//! The model identity the application passes across crate boundaries.
//!
//! One model is named three ways: `bongocat-config` persists it as a
//! [`ModelSource`], `bongocat-model` and the runtime address it as a
//! [`ModelOrigin`], and `bongocat-ui-protocol` shows it to the settings window as
//! a [`SettingsModelOrigin`]. The three vocabularies are parallel, and a site
//! that picks the wrong pair of names compiles fine but reports a model from the
//! wrong catalog.
//!
//! This module is the only place the three are mapped onto each other, in all
//! six directions, so a fourth origin is one match arm per direction rather than
//! a spelling repeated at every call site. Nothing here reads configuration,
//! touches the filesystem or holds state: the mapping is total, so a caller only
//! has to choose the direction it needs.

use bongocat_config::{ModelIdentity, ModelSource};
use bongocat_model::ModelOrigin;
use bongocat_ui_protocol::{SettingsModelKey, SettingsModelOrigin};

/// The persisted identity of a model the runtime or the model store owns.
pub(crate) const fn config_source_from_model(origin: ModelOrigin) -> ModelSource {
    match origin {
        ModelOrigin::Preset => ModelSource::BuiltIn,
        ModelOrigin::Installed => ModelSource::Imported,
    }
}

/// The model or store identity a persisted selection names.
pub(crate) const fn model_origin_from_config(source: ModelSource) -> ModelOrigin {
    match source {
        ModelSource::BuiltIn => ModelOrigin::Preset,
        ModelSource::Imported => ModelOrigin::Installed,
    }
}

/// The identity the settings window displays for a model the runtime owns.
pub(crate) const fn settings_origin_from_model(origin: ModelOrigin) -> SettingsModelOrigin {
    match origin {
        ModelOrigin::Preset => SettingsModelOrigin::BuiltIn,
        ModelOrigin::Installed => SettingsModelOrigin::Imported,
    }
}

/// The model or store identity a settings-window key names.
pub(crate) const fn model_origin_from_settings(origin: SettingsModelOrigin) -> ModelOrigin {
    match origin {
        SettingsModelOrigin::BuiltIn => ModelOrigin::Preset,
        SettingsModelOrigin::Imported => ModelOrigin::Installed,
    }
}

/// The persisted identity a settings-window key records into a binding.
pub(crate) const fn config_source_from_settings(origin: SettingsModelOrigin) -> ModelSource {
    match origin {
        SettingsModelOrigin::BuiltIn => ModelSource::BuiltIn,
        SettingsModelOrigin::Imported => ModelSource::Imported,
    }
}

/// The identity the settings window displays for a persisted selection.
pub(crate) const fn settings_origin_from_config(source: ModelSource) -> SettingsModelOrigin {
    match source {
        ModelSource::BuiltIn => SettingsModelOrigin::BuiltIn,
        ModelSource::Imported => SettingsModelOrigin::Imported,
    }
}

/// The persisted identity a settings-window key names.
///
/// Used by the fields that store an optional model identity, so a target the
/// settings window shows and the target the configuration keeps can never be
/// spelled two different ways.
pub(crate) fn config_identity_from_settings(model: &SettingsModelKey) -> ModelIdentity {
    ModelIdentity {
        id: model.id.clone(),
        source: config_source_from_settings(model.origin),
    }
}

/// The settings-window key a persisted identity names.
pub(crate) fn settings_key_from_config(identity: &ModelIdentity) -> SettingsModelKey {
    SettingsModelKey {
        id: identity.id.clone(),
        origin: settings_origin_from_config(identity.source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every direction has to agree with the two it can be reached through, or
    /// the same model is spelled three different ways depending on which layer
    /// asked for it.
    #[test]
    fn the_three_vocabularies_name_the_same_two_origins() {
        for source in [ModelSource::BuiltIn, ModelSource::Imported] {
            let model = model_origin_from_config(source);
            let settings = settings_origin_from_config(source);
            assert_eq!(config_source_from_model(model), source);
            assert_eq!(config_source_from_settings(settings), source);
            assert_eq!(settings_origin_from_model(model), settings);
            assert_eq!(model_origin_from_settings(settings), model);
        }
    }
}
