//! The model catalog: what the Model library page shows, and the user-editable
//! metadata and cover art recorded for each entry.

use super::Application;
use crate::model_listing::{
    installed_model_order, model_input_mode_from_store, preset_model_input_mode, preset_model_order,
};
use crate::model_titles::normalize_model_title;
use crate::{ApplicationError, PNG_SIGNATURE};
use bongocat_config::{
    BuiltInModelMetadata, GamepadAutoSwitchConfig, ImportedModelMetadata, ModelInputMode,
};
use bongocat_model::{ModelCatalogEntry, ModelId, ModelOrigin, ModelPackageLimits};
use bongocat_model_store::preset_cover_exists;
use std::{
    fs,
    path::{Path, PathBuf},
};

impl Application {
    /// Every model the Model library page can show, in the order the page shows them:
    /// the build's presets first, then the models the user imported.
    ///
    /// The presets are the three input modes the build ships, and the page
    /// lists them in mode order — Standard, Keyboard, Gamepad. Their ids are
    /// `standard`, `keyboard` and `gamepad`, so sorting them by id would run
    /// the page backwards. See
    /// [`preset_model_order`](crate::model_listing::preset_model_order).
    ///
    /// The imported models follow, in the order they were imported: the
    /// configuration's record list is append-only, so a newly imported model
    /// joins the end of the page and stays where it landed. See
    /// [`installed_model_order`](crate::model_listing::installed_model_order).
    ///
    /// A model id present in both halves appears twice, once per origin, and
    /// the preset one always comes first because the whole preset half does.
    pub fn model_catalog(&self) -> Result<Vec<ModelCatalogEntry>, ApplicationError> {
        // Unrecognized store entries are filtered inside the store scan; the
        // merged catalog only exposes real models.
        let mut presets = self.preset_models.list()?;
        presets.sort_by(|left, right| {
            preset_model_order(left.id().as_str())
                .cmp(&preset_model_order(right.id().as_str()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        let records = &self.config.model.imported_models;
        let mut installed = self.model_store.list()?.entries;
        installed.sort_by(|left, right| {
            installed_model_order(records, left.id().as_str())
                .cmp(&installed_model_order(records, right.id().as_str()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        presets.extend(installed);
        Ok(presets)
    }

    pub const fn active_model_origin(&self) -> Option<ModelOrigin> {
        self.active_model_origin
    }

    /// Where a model's own files live, when the model is actually present.
    ///
    /// The settings catalog needs this twice: to offer "open model folder", and
    /// to find the cover image the package may ship. Nothing in the runtime or
    /// renderer path uses it, and a missing directory is reported as `None`
    /// rather than an error, because a catalog entry and the directory behind it
    /// are re-read independently.
    pub fn model_directory(&self, origin: ModelOrigin, id: &str) -> Option<PathBuf> {
        let Ok(id) = ModelId::parse(id) else {
            return None;
        };
        let root = match origin {
            ModelOrigin::Preset => self.preset_models.root(),
            ModelOrigin::Installed => self.model_store.root(),
        };
        let directory = root.join(id.as_str());
        directory.is_dir().then_some(directory)
    }

    /// Rename a model, whichever origin it came from.
    ///
    /// A title is user-editable metadata in the configuration, so this is the
    /// only model fact that lives outside the model directory. Both origins are
    /// renamed the same way, into their own list: a preset's package belongs to
    /// the build and is never written to, so its name is a customisation the
    /// configuration records instead of a property of the package.
    pub fn set_model_title(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let id = ModelId::parse(id)?;
        let title =
            normalize_model_title(&title.into()).ok_or(ApplicationError::ModelTitleInvalid)?;
        if self.model_directory(origin, id.as_str()).is_none() {
            return Err(ApplicationError::ModelNotFound(id));
        }
        self.record_model_title(origin, &id, title)
    }

    /// Replace a model's cover image with a user-chosen PNG.
    ///
    /// The cover is display artwork for the settings catalog, so the check here
    /// is the file contract the package layout implies — a PNG within the
    /// package's own per-file limit — and the bytes are installed verbatim,
    /// exactly as the BongoCatMver conversion installs a legacy cover.
    pub fn set_model_cover(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        source: impl AsRef<Path>,
    ) -> Result<PathBuf, ApplicationError> {
        let id = ModelId::parse(id)?;
        let source = source.as_ref();
        let metadata = fs::metadata(source).map_err(|_| ApplicationError::ModelCoverInvalid)?;
        if !metadata.is_file() || metadata.len() > ModelPackageLimits::default().maximum_file_bytes
        {
            return Err(ApplicationError::ModelCoverInvalid);
        }
        let bytes = fs::read(source).map_err(|_| ApplicationError::ModelCoverInvalid)?;
        self.install_model_cover(origin, &id, &bytes)
    }

    /// Replace a model's cover image with PNG bytes the product made.
    ///
    /// A captured cover never exists as a file until it is installed, so the cover
    /// the renderer just produced arrives here directly. It passes the same contract
    /// as a user-chosen one: a real PNG, within the package's per-file limit.
    pub fn set_model_cover_bytes(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        bytes: &[u8],
    ) -> Result<PathBuf, ApplicationError> {
        let id = ModelId::parse(id)?;
        self.install_model_cover(origin, &id, bytes)
    }

    /// The contract every cover source shares, once its bytes are in hand.
    ///
    /// Where the bytes land is the one thing the origins do not share: an
    /// installed model's cover goes into its own package, and a preset's goes to
    /// the user side, because the package it belongs to sits inside the
    /// application bundle and may not be written to.
    fn install_model_cover(
        &self,
        origin: ModelOrigin,
        id: &ModelId,
        bytes: &[u8],
    ) -> Result<PathBuf, ApplicationError> {
        if bytes.len() as u64 > ModelPackageLimits::default().maximum_file_bytes
            || !bytes.starts_with(&PNG_SIGNATURE)
        {
            return Err(ApplicationError::ModelCoverInvalid);
        }
        match origin {
            ModelOrigin::Preset => {
                // The preset cover store creates the directory it writes into,
                // so unlike the model store it cannot report a missing model by
                // itself. The package has to be there before it may be
                // customised, exactly as for an installed model.
                if self.model_directory(origin, id.as_str()).is_none() {
                    return Err(ApplicationError::ModelNotFound(id.clone()));
                }
                self.preset_covers
                    .replace_cover(id, bytes)
                    .map_err(ApplicationError::ModelStore)
            }
            ModelOrigin::Installed => self
                .model_store
                .replace_cover(id, bytes)
                .map_err(ApplicationError::ModelStore),
        }
    }

    /// The display name recorded for a model, if the user ever changed it.
    ///
    /// `None` means the model has never been renamed, which is the ordinary
    /// state of a preset: its name is then the id the build shipped it under.
    pub fn recorded_model_title(&self, origin: ModelOrigin, id: &str) -> Option<&str> {
        match origin {
            ModelOrigin::Preset => self
                .config
                .model
                .built_in_models
                .iter()
                .find(|record| record.id == id)
                .map(|record| record.title.as_str()),
            ModelOrigin::Installed => self
                .config
                .model
                .imported_models
                .iter()
                .find(|record| record.id == id)
                .map(|record| record.title.as_str()),
        }
    }

    /// The input mode shown for one model.
    ///
    /// Presets derive it from the stable ids owned by the build. Imported models
    /// read the value resolved before their store commit. A hand-copied store
    /// directory is classified on demand only when it is otherwise valid; an
    /// invalid directory has no mode and therefore no mode badge.
    pub fn model_input_mode(&self, origin: ModelOrigin, id: &str) -> Option<ModelInputMode> {
        match origin {
            ModelOrigin::Preset => preset_model_input_mode(id),
            ModelOrigin::Installed => self
                .config
                .model
                .imported_models
                .iter()
                .find(|record| record.id == id)
                .map(|record| record.input_mode)
                .or_else(|| {
                    let id = ModelId::parse(id).ok()?;
                    self.model_store
                        .classify_installed_input_mode(&id)
                        .ok()
                        .map(model_input_mode_from_store)
                }),
        }
    }

    /// The cover the settings page should draw for a model, if it has one.
    ///
    /// A replacement wins over the artwork a preset's package ships: the bundle
    /// is read-only, so a replacement is the only cover that can reflect what
    /// the user chose. An installed model needs no such preference — its cover
    /// lives in the package either way.
    pub fn model_cover_path(&self, origin: ModelOrigin, id: &str) -> Option<PathBuf> {
        let directory = self.model_directory(origin, id)?;
        if origin == ModelOrigin::Preset {
            let replacement = self.preset_covers.cover_path(&ModelId::parse(id).ok()?);
            if preset_cover_exists(&replacement) {
                return Some(replacement);
            }
        }
        let cover = bongocat_model::package_cover_path(&directory);
        cover.is_file().then_some(cover)
    }

    /// Write one model's display name into the list that owns its lifecycle.
    fn record_model_title(
        &mut self,
        origin: ModelOrigin,
        id: &ModelId,
        title: String,
    ) -> Result<(), ApplicationError> {
        match origin {
            ModelOrigin::Preset => {
                let mut records = self.config.model.built_in_models.clone();
                match records.iter_mut().find(|record| record.id == id.as_str()) {
                    Some(record) => record.title = title,
                    None => records.push(BuiltInModelMetadata {
                        id: id.as_str().to_owned(),
                        title,
                    }),
                }
                self.commit_preset_model_metadata(records)
            }
            ModelOrigin::Installed => {
                let mut records = self.config.model.imported_models.clone();
                match records.iter_mut().find(|record| record.id == id.as_str()) {
                    Some(record) => record.title = title,
                    // A package copied into the store by hand can legitimately
                    // exist without metadata. Classify it once before creating
                    // the record, using the same import-time rule; an invalid
                    // package cannot be given a title record.
                    None => {
                        let input_mode = self
                            .model_store
                            .classify_installed_input_mode(id)
                            .map(model_input_mode_from_store)
                            .map_err(ApplicationError::ModelStore)?;
                        records.push(ImportedModelMetadata {
                            id: id.as_str().to_owned(),
                            title,
                            input_mode,
                        });
                    }
                }
                self.commit_installed_model_metadata(records)
            }
        }
    }

    /// Persist build-shipped model metadata. The typed validation in
    /// `bongocat-config` rejects duplicate ids, blank titles, and over-long
    /// values before anything is written.
    fn commit_preset_model_metadata(
        &mut self,
        records: Vec<BuiltInModelMetadata>,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.built_in_models = records;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    /// Persist imported model metadata under the same transactional guarantees
    /// as every other configuration write.
    pub(crate) fn commit_installed_model_metadata(
        &mut self,
        records: Vec<ImportedModelMetadata>,
    ) -> Result<(), ApplicationError> {
        let gamepad_auto_switch = self.config.model.gamepad_auto_switch.clone();
        self.commit_model_metadata(records, gamepad_auto_switch)
    }

    /// Persist imported model metadata and the gamepad auto switch in one commit.
    ///
    /// A model that has just been removed must not stay configured as an
    /// automatic target, and splitting the two writes would leave a window where
    /// the configuration names a model the store no longer has.
    pub(crate) fn commit_model_metadata(
        &mut self,
        records: Vec<ImportedModelMetadata>,
        gamepad_auto_switch: GamepadAutoSwitchConfig,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.imported_models = records;
        next_config.model.gamepad_auto_switch = gamepad_auto_switch;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }
}
