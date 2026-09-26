//! Importing a model package or a legacy BongoCatMver source, and removing a
//! model the user no longer wants.

use super::Application;
use crate::ApplicationError;
use crate::app_log::{ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent};
use crate::import_progress::ImportProgressAccumulator;
use crate::model_listing::{
    STANDARD_PRESET_MODEL_ID, model_input_mode_from_mver, model_input_mode_from_store,
};
use crate::model_titles::{installed_model_title, legacy_mode_label, legacy_model_title};
use crate::shortcut_config::without_removed_model_targets;
use bongocat_config::{ImportedModelMetadata, ModelSource};
use bongocat_model::{InstalledModel, ModelId, ModelOrigin};
use bongocat_model_store::{
    ModelImportProgress, ModelSourceContent, ModelStoreError, MverInputMode,
};
use std::path::Path;

impl Application {
    pub fn delete_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let result = (|| {
            let id = ModelId::parse(id)?;
            if origin == ModelOrigin::Preset {
                return Err(ApplicationError::PresetModelDeletion(id));
            }
            // Deleting the active installed model first switches to the standard
            // preset, so the runtime never keeps ownership of removed files.
            if self.is_selected_installed_model(&id) {
                self.select_model(ModelOrigin::Preset, STANDARD_PRESET_MODEL_ID)?;
            }
            self.model_store
                .delete(&id)
                .map_err(ApplicationError::ModelStore)?;
            // The "last model used" memory is session state, so a removed model
            // only has to be forgotten here: keeping it would make the next
            // gamepad transition ask for files that no longer exist.
            for remembered in [&mut self.last_gamepad_model, &mut self.last_other_model] {
                if remembered
                    .as_ref()
                    .is_some_and(|live| live.id == id.as_str())
                {
                    *remembered = None;
                }
            }
            let (auto_switch, targets_changed) =
                without_removed_model_targets(&self.config.model.gamepad_auto_switch, &id);
            let mut installed_models = self.config.model.imported_models.clone();
            let before = installed_models.len();
            installed_models.retain(|metadata| metadata.id != id.as_str());
            if installed_models.len() != before || targets_changed {
                self.commit_model_metadata(installed_models, auto_switch)?;
            }
            Ok(())
        })();
        match &result {
            Ok(()) => self.application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::ModelOperationCompleted)
                    .with_context(ApplicationLogContext::Operation("delete"))
                    .with_context(ApplicationLogContext::Count(1)),
            ),
            Err(error) => self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::ModelOperationFailed)
                    .with_context(ApplicationLogContext::Operation("delete"))
                    .with_context(ApplicationLogContext::Reason(error.stable_code())),
            ),
        }
        result
    }

    /// Whether `id` is the installed model the application is showing or has
    /// selected.
    ///
    /// The overlay's identity and the recorded selection are two separate
    /// facts: a fresh configuration records no selection while the startup
    /// model is already live. Either one pointing at `id` means deleting it
    /// would remove the model the user is currently looking at, which is the
    /// case [`Application::delete_model`] has to switch away from first.
    fn is_selected_installed_model(&self, id: &ModelId) -> bool {
        let shown = self.active_model_origin == Some(ModelOrigin::Installed)
            && self
                .runtime
                .client()
                .snapshot()
                .active_model
                .as_ref()
                .is_some_and(|active| active.id.as_str() == id.as_str());
        let configured = self
            .config
            .model
            .selected_model
            .as_ref()
            .is_some_and(|selected| {
                selected.source == ModelSource::Imported && selected.id == id.as_str()
            });
        shown || configured
    }

    /// Import every model a source describes.
    ///
    /// A BongoCat package installs one model. Its mode is resolved from the
    /// committed key artwork before the store atomically renames it into place;
    /// a package with no classifiable key artwork is rejected. A BongoCatMver
    /// source installs one *converted* model per input mode it carries, each
    /// with its own generated UUID store key and metadata record, so the three
    /// modes of a legacy model become three ordinary entries in the model list
    /// that can be activated, renamed and deleted independently.
    ///
    /// Each model is committed on its own: a source whose second mode fails
    /// still leaves the first installed and titled. That is deliberate — the
    /// models are independent, and silently discarding a mode that converted
    /// correctly would be worse than reporting a failure the user can act on by
    /// fixing that one mode.
    pub fn import_models(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
    ) -> Result<Vec<InstalledModel>, ApplicationError> {
        self.import_models_with_observer(title_hint, source_root, |_| {}, || false)
    }

    /// Inspect what a user-picked source is without installing anything.
    ///
    /// The settings page calls this to decide whether to ask the user about a
    /// BongoCat Mver conversion, and which modes to offer.
    pub fn inspect_model_source(
        &self,
        source_root: impl AsRef<Path>,
    ) -> Result<bongocat_ui_protocol::SettingsModelSourceContent, ApplicationError> {
        self.model_store
            .inspect_source(source_root)
            .map(settings_model_source_content)
            .map_err(ApplicationError::ModelStore)
    }

    /// Import every model a source carries, converting `selected_modes`.
    ///
    /// A BongoCat package installs one model and the selection is ignored. A
    /// BongoCat Mver source converts the intersection of `selected_modes` and
    /// the modes the source actually carries, taken in [`MverInputMode::ALL`]
    /// order; an empty intersection after an Mver source is a named
    /// `ModelImportSourceUnsupported` rather than a silent no-op.
    ///
    /// Kept for every caller that wants the whole source. The observer
    /// variant is the one the settings page uses.
    pub fn import_models_with_observer<Observe, IsCancelled>(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
        observe: Observe,
        is_cancelled: IsCancelled,
    ) -> Result<Vec<InstalledModel>, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        // No preference means "convert everything the source carries": reuse the
        // selection filter with all modes requested.
        self.import_models_with_selected_modes_with_observer(
            title_hint,
            source_root,
            MverInputMode::ALL.to_vec(),
            observe,
            is_cancelled,
        )
    }

    /// Import every model a source carries, converting only `selected_modes`.
    ///
    /// `selected_modes` names the BongoCat Mver modes to convert. A package
    /// source ignores them; an Mver source filters to the selected modes it
    /// actually carries, in [`MverInputMode::ALL`] order, so a request never
    /// duplicates a mode and the report order stays the one the settings
    /// contract declares. An Mver source whose selection matches no carried
    /// mode is reported as `ModelImportSourceUnsupported`.
    pub fn import_models_with_selected_modes_with_observer<Observe, IsCancelled>(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
        selected_modes: Vec<MverInputMode>,
        observe: Observe,
        mut is_cancelled: IsCancelled,
    ) -> Result<Vec<InstalledModel>, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let result = (|| {
            let title_hint = title_hint.into();
            let source_root = source_root.as_ref();
            let language = self.effective_language();
            let mut aggregate = ImportProgressAccumulator::new(observe);

            // Which models the source describes is decided from its own bytes, not
            // from anything the caller selected: the folder is inspected for a
            // legacy key table. The store only reports a legacy source once a mode
            // really carries a usable model, so an empty mode list cannot occur —
            // treating it as a package keeps the loop below total and still reports
            // a real diagnostic from the package path.
            let content = self
                .model_store
                .inspect_source(source_root)
                .map_err(ApplicationError::ModelStore)?;
            let modes = match content {
                // A legacy source keeps the selected modes it really carries, in
                // the declared mode order.
                ModelSourceContent::Mver { modes } if !modes.is_empty() => {
                    let selected = MverInputMode::ALL
                        .into_iter()
                        .filter(|mode| modes.contains(mode) && selected_modes.contains(mode))
                        .collect::<Vec<_>>();
                    if selected.is_empty() {
                        return Err(ApplicationError::ModelStore(
                            ModelStoreError::source_conversion_failed(
                                "none of the selected BongoCatMver modes are present",
                            ),
                        ));
                    }
                    Some(selected)
                }
                // An Mver source with no convertible mode falls back to the
                // package path, exactly as before.
                ModelSourceContent::Mver { modes } => {
                    debug_assert!(modes.is_empty());
                    None
                }
                ModelSourceContent::Package => None,
            };

            let mut installed = Vec::new();
            let mut installed_models = self.config.model.imported_models.clone();
            let count = modes.as_ref().map_or(1, Vec::len);
            for index in 0..count {
                let id = self
                    .model_store
                    .allocate_unique_id()
                    .map_err(ApplicationError::ModelStore)?;
                let fallback = id.as_str().to_owned();
                let (model, input_mode) = match modes.as_ref() {
                    None => self
                        .model_store
                        .import_with_observer_and_input_mode(
                            id,
                            source_root,
                            |update| aggregate.report(update),
                            &mut is_cancelled,
                        )
                        .map(|(model, mode)| (model, model_input_mode_from_store(mode)))
                        .map_err(ApplicationError::ModelStore)?,
                    Some(modes) => self
                        .model_store
                        .import_mver_with_observer(
                            id,
                            modes[index],
                            source_root,
                            |update| aggregate.report(update),
                            &mut is_cancelled,
                        )
                        .map(|model| (model, model_input_mode_from_mver(modes[index])))
                        .map_err(ApplicationError::ModelStore)?,
                };
                let title = match modes.as_ref() {
                    None => installed_model_title(&title_hint, source_root, &fallback),
                    Some(modes) => legacy_model_title(
                        &title_hint,
                        source_root,
                        &fallback,
                        legacy_mode_label(language, modes[index]),
                    ),
                };
                installed_models.push(ImportedModelMetadata {
                    id: model.id().as_str().to_owned(),
                    title,
                    input_mode,
                });
                self.commit_installed_model_metadata(installed_models.clone())?;
                installed.push(model);
            }
            Ok(installed)
        })();
        match &result {
            Ok(installed) => self.application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::ModelOperationCompleted)
                    .with_context(ApplicationLogContext::Operation("import"))
                    .with_context(ApplicationLogContext::Count(installed.len() as u64)),
            ),
            Err(error) => self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::ModelOperationFailed)
                    .with_context(ApplicationLogContext::Operation("import"))
                    .with_context(ApplicationLogContext::Reason(error.stable_code())),
            ),
        }
        result
    }

    /// Drop metadata records whose installed model directory no longer
    /// exists. The record list stays consistent with the store even when a
    /// model was removed by hand outside the application.
    ///
    /// Preset records are deliberately not pruned by anything: a preset is
    /// shipped with the build rather than found on disk, so a record whose model
    /// this build no longer carries is not evidence of a stale customisation —
    /// it is simply not shown, and it names the model again if a later build
    /// ships it.
    pub(crate) fn prune_missing_installed_metadata(&mut self) {
        let catalog = match self.model_store.list() {
            Ok(catalog) => catalog,
            Err(_) => {
                self.application_log.record_once(
                    ApplicationLogEvent::new(ApplicationLogCode::ModelOperationFailed)
                        .with_context(ApplicationLogContext::Operation("metadata_prune"))
                        .with_context(ApplicationLogContext::Reason("model_catalog_unavailable")),
                );
                return;
            }
        };
        let present = catalog
            .entries
            .iter()
            .map(|entry| entry.id().as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let kept = self
            .config
            .model
            .imported_models
            .iter()
            .filter(|metadata| present.contains(&metadata.id))
            .cloned()
            .collect::<Vec<_>>();
        if kept.len() == self.config.model.imported_models.len() {
            return;
        }
        if self.commit_installed_model_metadata(kept).is_err() {
            self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                    .with_context(ApplicationLogContext::State("config"))
                    .with_context(ApplicationLogContext::Operation("metadata_prune"))
                    .with_context(ApplicationLogContext::Reason("config_commit_failed")),
            );
        }
    }
}
/// Project one model-crate source description onto the settings boundary enum.
///
/// The model crate is the one that read the bytes; the UI enum is the only
/// shape the settings page knows. Keeping the mapping here means the settings
/// page never names a model-crate type, and a later mode added upstream is a
/// compile error here rather than a silent gap in the dialog.
pub(crate) fn settings_model_source_content(
    content: ModelSourceContent,
) -> bongocat_ui_protocol::SettingsModelSourceContent {
    match content {
        ModelSourceContent::Package => bongocat_ui_protocol::SettingsModelSourceContent::Package,
        ModelSourceContent::Mver { modes } => {
            bongocat_ui_protocol::SettingsModelSourceContent::Mver {
                modes: modes
                    .into_iter()
                    .map(|mode| match mode {
                        MverInputMode::Standard => bongocat_ui_protocol::SettingsMverMode::Standard,
                        MverInputMode::Keyboard => bongocat_ui_protocol::SettingsMverMode::Keyboard,
                        MverInputMode::Gamepad => bongocat_ui_protocol::SettingsMverMode::Gamepad,
                    })
                    .collect(),
            }
        }
    }
}
