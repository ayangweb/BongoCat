//! Restoring the configured model at startup, and the fallback that keeps a
//! selection which no longer loads from becoming a startup that never presents a
//! model at all.

use super::Application;
use crate::ApplicationError;
use crate::app_log::{ApplicationLogContext, ApplicationLogEvent};
use crate::model_identity::{config_source_from_model, model_origin_from_config};
use crate::model_listing::STANDARD_PRESET_MODEL_ID;
use bongocat_config::ModelIdentity;
use bongocat_model::{ModelId, ModelOrigin};

impl Application {
    /// Restore the model selection at startup. The configured selection is
    /// activated when it still loads; a selection whose resources were
    /// deleted by hand or are otherwise unusable never blocks startup — the
    /// application records an anonymous fallback event, persists the
    /// always-available standard preset as the corrected selection, and
    /// activates it. Without a configured selection the standard preset is
    /// the default model. Metadata records for model directories that no
    /// longer exist are pruned while configuration is operational.
    ///
    /// The runtime keeps at most one unresolved model activation: a pending
    /// activation is only committed once the overlay frame source consumes
    /// its first prepared frame. Startup therefore prepares exactly one
    /// model and never issues a second activation while the first may still
    /// be pending.
    pub fn restore_startup_model(&mut self) -> Result<(), ApplicationError> {
        self.prune_missing_installed_metadata();
        let configured = self.config.model.selected_model.as_ref().map(|selected| {
            (
                selected.id.clone(),
                model_origin_from_config(selected.source),
            )
        });
        let Some((id, origin)) = configured else {
            // No configured selection: the standard preset is the default model.
            return self
                .prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
                .map(|_| ());
        };
        let configured_selection = match ModelId::parse(id) {
            Ok(id) => (origin, id),
            Err(_) => {
                self.fallback_to_standard_preset("model_id_invalid");
                return self
                    .prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
                    .map(|_| ());
            }
        };
        let (origin, id) = configured_selection;
        match self.prepare_model(origin, id.as_str()) {
            Ok(_) => return Ok(()),
            Err(error) => self.fallback_to_standard_preset(error.stable_code()),
        }
        self.prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
            .map(|_| ())
    }

    /// Record the anonymous fallback event and persist the standard preset
    /// as the corrected selection. A failed commit keeps the stale selection
    /// on disk; the next startup simply retries the fallback.
    fn fallback_to_standard_preset(&mut self, reason: &'static str) {
        self.application_log.record(
            ApplicationLogEvent::model_selection_fallback()
                .with_context(ApplicationLogContext::Reason(reason)),
        );
        self.persist_model_selection(ModelOrigin::Preset, &self.standard_preset_id());
    }

    fn standard_preset_id(&self) -> ModelId {
        ModelId::parse(STANDARD_PRESET_MODEL_ID).expect("standard preset model id is valid")
    }

    /// Persist a corrected model selection without touching the runtime.
    /// A failed commit keeps the stale selection on disk; the next startup
    /// simply retries the fallback.
    fn persist_model_selection(&mut self, origin: ModelOrigin, id: &ModelId) {
        let mut next_config = self.config.clone();
        next_config.model.selected_model = Some(ModelIdentity {
            id: id.as_str().to_owned(),
            source: config_source_from_model(origin),
        });
        let Ok(expected_revision) = self.ready_config_revision() else {
            return;
        };
        let Ok(next_revision) = self
            .config_store
            .commit_if_revision(&next_config, expected_revision)
        else {
            return;
        };
        self.config = next_config;
        self.config_revision = Some(next_revision);
    }
}
