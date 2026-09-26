//! The model store: what is installed, and how a source becomes installed.
//!
//! `ModelStore` owns the filesystem lifecycle a package goes through to become
//! an installed model, and its methods are one sequence — inspect the source,
//! stage it beside the models, copy it, commit it, and recover whatever an
//! interrupted run left behind. That sequence stays whole here; the parts it is
//! made of are the modules beside it, and the test tree under `tests` follows
//! the same split.

mod catalog;
mod copy;
mod diagnostic;
mod input_mode;
mod progress;
mod staging;
#[cfg(test)]
mod tests;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the store's items are one vocabulary, and a list per
// module would be the same list six times.
pub(crate) use catalog::*;
pub(crate) use copy::*;
pub(crate) use diagnostic::*;
pub(crate) use input_mode::*;
pub(crate) use progress::*;
pub(crate) use staging::*;

use crate::key_names::normalize_legacy_key_image_names;
use crate::mver::{self, ModelSourceContent, MverInputMode, MverSource};
use bongocat_model::{
    InstalledModel, ModelCatalogEntry, ModelError, ModelId, ModelOrigin, ModelPackageLimits,
    PACKAGE_COVER_FILE, PACKAGE_RESOURCES_DIRECTORY, PreparedModel,
};
use bongocat_storage::{set_private_directory, set_private_file};
use std::{
    collections::BTreeSet,
    fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

// The public surface the crate root re-exports. The `pub(crate) use` lines above
// already bring the rest of these modules' items into scope here and below.
pub use catalog::InstalledModelCatalog;
pub use diagnostic::{ModelStoreDiagnostic, ModelStoreError};
pub use input_mode::ModelStoreInputMode;
pub use progress::{ModelImportProgress, ModelImportStage};
pub use staging::ModelStoreRecovery;

pub struct ModelStore {
    pub(crate) canonical_root: PathBuf,
    pub(crate) lock_path: PathBuf,
    pub(crate) limits: ModelPackageLimits,
    pub(crate) recovery: ModelStoreRecovery,
}

impl ModelStore {
    pub fn new(
        root: impl AsRef<Path>,
        lock_path: impl AsRef<Path>,
        limits: ModelPackageLimits,
    ) -> Result<Self, ModelStoreError> {
        fs::create_dir_all(root.as_ref()).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store cannot be created: {error}"),
            )
        })?;
        set_private_directory(root.as_ref()).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store permissions cannot be set: {error}"),
            )
        })?;
        let canonical_root = root.as_ref().canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store cannot be opened: {error}"),
            )
        })?;
        if !canonical_root.is_dir() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                "model store is not a directory",
            ));
        }
        let lock_parent = lock_path.as_ref().parent().ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                "model store lock path has no parent directory",
            )
        })?;
        fs::create_dir_all(lock_parent).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store lock directory cannot be created: {error}"),
            )
        })?;
        set_private_directory(lock_parent).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store lock directory permissions cannot be set: {error}"),
            )
        })?;
        let canonical_lock_parent = lock_parent.canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store lock directory cannot be opened: {error}"),
            )
        })?;
        let lock_name = lock_path.as_ref().file_name().ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                "model store lock path has no file name",
            )
        })?;
        let mut store = Self {
            canonical_root,
            lock_path: canonical_lock_parent.join(lock_name),
            limits,
            recovery: ModelStoreRecovery::default(),
        };
        let _lock = store.acquire_lock()?;
        store.recovery = store.recover_abandoned_operations()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub const fn recovery(&self) -> ModelStoreRecovery {
        self.recovery
    }

    /// Scan the store root for installed models.
    ///
    /// Only the root directory itself is required to be readable. Entries that
    /// the catalog does not own are dropped silently and counted, and entries
    /// that carry a valid model id but fail package validation are reported as
    /// [`ModelCatalogEntry::Invalid`] so they stay visible next to the valid
    /// models. This never follows symbolic links.
    pub fn list(&self) -> Result<InstalledModelCatalog, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let mut catalog = InstalledModelCatalog::default();
        for entry in fs::read_dir(&self.canonical_root).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store cannot be listed: {error}"),
            )
        })? {
            let entry = entry.map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("model store entry cannot be read: {error}"),
                )
            })?;
            let Ok(name) = entry.file_name().into_string() else {
                catalog.skipped_entries += 1;
                continue;
            };
            if is_platform_metadata_name(&name) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                catalog.skipped_entries += 1;
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                catalog.skipped_entries += 1;
                continue;
            }
            let Ok(id) = ModelId::parse(name) else {
                catalog.skipped_entries += 1;
                continue;
            };
            let catalog_entry = match PreparedModel::prepare(id.clone(), entry.path(), self.limits)
            {
                Ok(prepared) => ModelCatalogEntry::Ready {
                    origin: ModelOrigin::Installed,
                    snapshot: prepared.snapshot(),
                },
                Err(error) => ModelCatalogEntry::Invalid {
                    origin: ModelOrigin::Installed,
                    id,
                    code: error.code,
                    resource: error.resource,
                    detail: error.detail,
                },
            };
            catalog.entries.push(catalog_entry);
        }
        catalog
            .entries
            .sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(catalog)
    }

    pub fn load(&self, id: &ModelId) -> Result<InstalledModel, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let path = self.installed_path(id)?;
        PreparedModel::prepare(id.clone(), path, self.limits)
            .map(InstalledModel::from_prepared)
            .map_err(ModelStoreError::package)
    }

    /// Resolve the input family of an already-installed ordinary package.
    ///
    /// This is used only for a store entry that predates metadata (for example a
    /// folder copied in by hand). Normal imports resolve the mode before their
    /// atomic commit and persist it in application config.
    pub fn classify_installed_input_mode(
        &self,
        id: &ModelId,
    ) -> Result<ModelStoreInputMode, ModelStoreError> {
        let model = self.load(id)?;
        classify_input_mode(model.root())
    }

    /// Generate the portable store key for a newly imported model. Identity
    /// is a random UUID v4, deliberately independent of the user-visible
    /// title and of the source folder name, so titles can be edited freely
    /// and identical folder names never collide. The store is never
    /// overwritten by this call; `import` still rejects a concurrently
    /// occupied destination as the final guard.
    pub fn allocate_unique_id(&self) -> Result<ModelId, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        for _ in 0..16 {
            let id = ModelId::parse(uuid::Uuid::new_v4().to_string())
                .expect("hyphenated UUID v4 is a portable model id");
            if self.is_id_vacant(&id) {
                return Ok(id);
            }
        }
        Err(ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            "no unique model id was available",
        ))
    }

    pub(crate) fn is_id_vacant(&self, id: &ModelId) -> bool {
        !self.canonical_root.join(id.as_str()).exists()
    }

    pub fn delete(&self, id: &ModelId) -> Result<(), ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let source = self.installed_path(id)?;
        let deleting = self.unique_operation_path(DELETING_PREFIX, id)?;
        fs::rename(&source, &deleting).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("installed model cannot be retired: {error}"),
            )
        })?;
        fs::remove_dir_all(&deleting).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("retired model cannot be removed: {error}"),
            )
        })
    }

    /// Replace an installed model's cover image.
    ///
    /// The cover is display-only artwork, so this is a plain atomic file
    /// replacement inside the model's own directory: a torn write could only
    /// ever produce a wrong picture, never a broken model. The caller owns the
    /// format decision, and the bytes are stored exactly as handed over, which
    /// is also what the BongoCatMver conversion does with a legacy `cat.png`.
    pub fn replace_cover(&self, id: &ModelId, bytes: &[u8]) -> Result<PathBuf, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let root = self.installed_path(id)?;
        let resources = root.join(PACKAGE_RESOURCES_DIRECTORY);
        fs::create_dir_all(&resources).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("model resources directory cannot be created: {error}"),
            )
        })?;
        set_private_directory(&resources).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("model resources directory cannot be secured: {error}"),
            )
        })?;

        let cover = resources.join(PACKAGE_COVER_FILE);
        let staging = resources.join(format!(".{}.new", PACKAGE_COVER_FILE));
        let write = || -> io::Result<()> {
            let mut file = File::create(&staging)?;
            set_private_file(&file)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&staging, &cover)
        };
        write().map_err(|error| {
            // A failed replace must not leave a half-written cover behind: the
            // previous one is still in place until the rename above succeeds.
            let _ = fs::remove_file(&staging);
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("model cover cannot be replaced: {error}"),
            )
        })?;
        Ok(cover)
    }

    /// Import a BongoCat model package from the folder a user picked.
    ///
    /// An ordinary package must carry at least one key PNG. Its input family is
    /// resolved from the key directories before the atomic commit; a package
    /// with no classifiable key artwork is rejected as [`ModelStoreDiagnostic::InvalidPackage`].
    /// A BongoCatMver source is not a package: ask [`ModelStore::inspect_source`]
    /// what a user-picked source is first, and install each of its modes with
    /// [`ModelStore::import_mver_with_observer`].
    ///
    /// The source is a folder and nothing else — the picker only returns
    /// directories, and a path that is not one fails through the package
    /// validation below rather than reaching a second source shape. That is
    /// deliberate: there is one import path, so there is one set of rules to
    /// keep correct. `tests::a_source_that_is_not_a_folder_fails_without_touching_the_store`
    /// pins the outcome.
    pub fn import(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
    ) -> Result<InstalledModel, ModelStoreError> {
        self.import_with_observer(id, source_root, |_| {}, || false)
    }

    /// Describe what a user-picked source is, without installing anything.
    ///
    /// The answer comes from the source itself rather than from the button the
    /// user pressed: the folder is read in place and then asked whether it
    /// carries a BongoCatMver key table. A source that is neither a package nor
    /// a legacy model is still reported as [`ModelSourceContent::Package`], so
    /// the ordinary import reports the real diagnostic instead of this call
    /// guessing.
    pub fn inspect_source(
        &self,
        source_root: impl AsRef<Path>,
    ) -> Result<ModelSourceContent, ModelStoreError> {
        let canonical_source = source_root.as_ref().canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model source cannot be opened: {error}"),
            )
        })?;
        self.describe_source(&MverSource::directory(&canonical_source)?)
    }

    pub(crate) fn describe_source(
        &self,
        source: &MverSource,
    ) -> Result<ModelSourceContent, ModelStoreError> {
        Ok(match mver::inspect(source, self.limits)? {
            Some(plan) => ModelSourceContent::Mver {
                modes: plan.modes().collect(),
            },
            None => ModelSourceContent::Package,
        })
    }

    /// Import one input mode of a BongoCatMver source as its own model.
    ///
    /// The conversion writes into the store's own staging directory and the
    /// result is committed by the same single rename as any other import, so a
    /// legacy source is not a second, weaker way into the store: it cannot skip
    /// package validation, and a conversion that fails part way leaves nothing
    /// behind. One legacy source describes several models, so the caller
    /// allocates one id per mode and calls this once per mode; each call is
    /// independently atomic, which is what lets a source that only partly
    /// converts still deliver the modes that do.
    pub fn import_mver_with_observer<Observe, IsCancelled>(
        &self,
        id: ModelId,
        mode: MverInputMode,
        source_root: impl AsRef<Path>,
        mut observe: Observe,
        mut is_cancelled: IsCancelled,
    ) -> Result<InstalledModel, ModelStoreError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let canonical_source = source_root.as_ref().canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model source cannot be opened: {error}"),
            )
        })?;
        if self.canonical_root.starts_with(&canonical_source) {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceContainsStore,
                None,
                "model source cannot contain the destination store",
            ));
        }
        self.convert_mver_mode(
            id,
            mode,
            MverSource::directory(&canonical_source)?,
            &mut observe,
            &mut is_cancelled,
        )
    }

    pub(crate) fn convert_mver_mode<Observe, IsCancelled>(
        &self,
        id: ModelId,
        mode: MverInputMode,
        source: MverSource,
        observe: &mut Observe,
        is_cancelled: &mut IsCancelled,
    ) -> Result<InstalledModel, ModelStoreError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let mut observation = ImportObservation::new(observe, is_cancelled);
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Preparing,
            files_copied: 0,
            bytes_copied: 0,
        });
        observation.check_cancelled()?;
        let _lock = self.acquire_lock()?;
        observation.check_cancelled()?;

        let plan = mver::inspect(&source, self.limits)?.ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceConversionFailed,
                None,
                "the selected source is not a BongoCatMver model",
            )
        })?;
        let mode_plan = plan.mode(mode).ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceConversionFailed,
                Some(mode.as_str().to_owned()),
                "the source does not carry this BongoCatMver input mode",
            )
        })?;

        let staging = self.create_staging_directory(&id)?;
        let mut cleanup = StagingCleanup::new(staging.clone());
        let mut statistics = CopyStatistics::default();
        mver::convert_mode(
            &source,
            mode_plan,
            &staging,
            self.limits,
            &mut statistics,
            &mut observation,
        )?;
        observation.check_cancelled()?;
        self.commit_installed_staging(
            &id,
            staging,
            Some(ModelStoreInputMode::from(mode)),
            &mut cleanup,
            &statistics,
            &mut observation,
        )
        .map(|(model, _)| model)
    }

    pub fn import_with_observer<Observe, IsCancelled>(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
        observe: Observe,
        is_cancelled: IsCancelled,
    ) -> Result<InstalledModel, ModelStoreError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        self.import_with_observer_and_input_mode(id, source_root, observe, is_cancelled)
            .map(|(model, _)| model)
    }

    /// Import one ordinary package and return the mode resolved from its key
    /// artwork before the destination is committed.
    pub fn import_with_observer_and_input_mode<Observe, IsCancelled>(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
        mut observe: Observe,
        mut is_cancelled: IsCancelled,
    ) -> Result<(InstalledModel, ModelStoreInputMode), ModelStoreError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let mut observation = ImportObservation::new(&mut observe, &mut is_cancelled);
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Preparing,
            files_copied: 0,
            bytes_copied: 0,
        });
        observation.check_cancelled()?;
        let _lock = self.acquire_lock()?;
        observation.check_cancelled()?;
        let source_root = source_root.as_ref();
        let canonical_source = source_root.canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model source cannot be opened: {error}"),
            )
        })?;
        if self.canonical_root.starts_with(&canonical_source) {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceContainsStore,
                None,
                "model source cannot contain the destination store",
            ));
        }
        // The source is the folder a user exported, validated as far as its
        // format allows *before* the store creates a staging directory, so a
        // source that can never be imported leaves the store untouched.
        let prepared_source = PreparedModel::prepare(id.clone(), &canonical_source, self.limits)
            .map_err(ModelStoreError::package)?;
        observation.check_cancelled()?;
        let destination = self.canonical_root.join(id.as_str());
        if destination.exists() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::AlreadyExists,
                Some(id.as_str().to_owned()),
                "a model with this id is already installed",
            ));
        }

        let staging = self.create_staging_directory(&id)?;
        let mut cleanup = StagingCleanup::new(staging.clone());
        let mut statistics = CopyStatistics::default();
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Copying,
            files_copied: 0,
            bytes_copied: 0,
        });
        copy_package(
            prepared_source.root(),
            prepared_source.root(),
            &staging,
            0,
            self.limits,
            &mut statistics,
            &mut observation,
        )?;
        // The staged tree now speaks the product's key vocabulary. This runs on
        // the store's own copy, before the shared validation tail, so the user's
        // source is never written to.
        normalize_legacy_key_image_names(&staging)?;
        self.commit_installed_staging(
            &id,
            staging,
            None,
            &mut cleanup,
            &statistics,
            &mut observation,
        )
    }

    /// Validate the materialized staging tree and commit it as the installed
    /// model.
    ///
    /// Whatever produced the bytes — a folder copy or a BongoCatMver conversion
    /// — goes through the *same* package validation and the *same* single atomic
    /// rename, so no source format can become a second, weaker parser.
    pub(crate) fn commit_installed_staging<Observe, IsCancelled>(
        &self,
        id: &ModelId,
        staging: PathBuf,
        expected_mode: Option<ModelStoreInputMode>,
        cleanup: &mut StagingCleanup,
        statistics: &CopyStatistics,
        observation: &mut ImportObservation<'_, Observe, IsCancelled>,
    ) -> Result<(InstalledModel, ModelStoreInputMode), ModelStoreError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        observation.check_cancelled()?;
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Validating,
            files_copied: file_count_for_progress(statistics.file_count),
            bytes_copied: statistics.total_bytes,
        });
        let prepared = PreparedModel::prepare(id.clone(), &staging, self.limits)
            .map_err(ModelStoreError::package)?;
        let input_mode = match expected_mode {
            Some(mode) => mode,
            None => classify_input_mode(prepared.root())?,
        };
        observation.check_cancelled()?;

        let destination = self.canonical_root.join(id.as_str());
        if destination.exists() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::AlreadyExists,
                Some(id.as_str().to_owned()),
                "a model with this id was installed concurrently",
            ));
        }

        observation.report(ModelImportProgress {
            stage: ModelImportStage::Committing,
            files_copied: file_count_for_progress(statistics.file_count),
            bytes_copied: statistics.total_bytes,
        });
        observation.check_cancelled()?;
        fs::rename(&staging, &destination).map_err(|error| {
            let (code, detail) = if destination.exists() {
                (
                    ModelStoreDiagnostic::AlreadyExists,
                    "a model with this id was installed concurrently".to_owned(),
                )
            } else {
                (
                    ModelStoreDiagnostic::IoError,
                    format!("staged model cannot be committed: {error}"),
                )
            };
            ModelStoreError::new(code, Some(id.as_str().to_owned()), detail)
        })?;
        cleanup.disarm();
        let prepared = prepared.relocate(destination);
        Ok((InstalledModel::from_prepared(prepared), input_mode))
    }

    pub(crate) fn create_staging_directory(
        &self,
        id: &ModelId,
    ) -> Result<PathBuf, ModelStoreError> {
        let path = self.unique_operation_path(IMPORTING_PREFIX, id)?;
        fs::create_dir(&path).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model staging directory cannot be created: {error}"),
            )
        })?;
        set_private_directory(&path).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("model staging directory permissions cannot be set: {error}"),
            )
        })?;
        Ok(path)
    }

    pub(crate) fn unique_operation_path(
        &self,
        prefix: &str,
        id: &ModelId,
    ) -> Result<PathBuf, ModelStoreError> {
        for _ in 0..128 {
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!("{prefix}{}-{}-{sequence}", id.as_str(), std::process::id());
            let path = self.canonical_root.join(name);
            if !path.exists() {
                return Ok(path);
            }
        }
        Err(ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            "no unique model operation path was available",
        ))
    }

    pub(crate) fn acquire_lock(&self) -> Result<ModelStoreLock, ModelStoreError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("model store lock cannot be opened: {error}"),
                )
            })?;
        set_private_file(&file).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store lock permissions cannot be set: {error}"),
            )
        })?;
        match file.try_lock() {
            Ok(()) => Ok(ModelStoreLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ModelStoreError::new(
                ModelStoreDiagnostic::StoreBusy,
                None,
                "model store is busy",
            )),
            Err(TryLockError::Error(error)) => Err(ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store cannot be locked: {error}"),
            )),
        }
    }

    pub(crate) fn installed_path(&self, id: &ModelId) -> Result<PathBuf, ModelStoreError> {
        let path = self.canonical_root.join(id.as_str());
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ModelStoreError::new(
                    ModelStoreDiagnostic::NotFound,
                    Some(id.as_str().to_owned()),
                    "installed model was not found",
                )
            } else {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(id.as_str().to_owned()),
                    format!("installed model cannot be inspected: {error}"),
                )
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::StoreEntryUnsupported,
                Some(id.as_str().to_owned()),
                "installed model is not an owned directory",
            ));
        }
        Ok(path)
    }

    pub(crate) fn recover_abandoned_operations(
        &self,
    ) -> Result<ModelStoreRecovery, ModelStoreError> {
        let mut recovery = ModelStoreRecovery::default();
        for entry in fs::read_dir(&self.canonical_root).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("model store cannot be scanned for recovery: {error}"),
            )
        })? {
            let entry = entry.map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("model recovery entry cannot be read: {error}"),
                )
            })?;
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let operation = if is_owned_operation_name(&name, IMPORTING_PREFIX) {
                Some(true)
            } else if is_owned_operation_name(&name, DELETING_PREFIX) {
                Some(false)
            } else {
                None
            };
            let Some(importing) = operation else {
                continue;
            };
            let file_type = entry.file_type().map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("model recovery entry type cannot be read: {error}"),
                )
            })?;
            if file_type.is_symlink() || !file_type.is_dir() {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::StoreEntryUnsupported,
                    None,
                    "model operation entry is not an owned directory",
                ));
            }
            fs::remove_dir_all(entry.path()).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("abandoned model operation cannot be removed: {error}"),
                )
            })?;
            if importing {
                recovery.abandoned_imports_removed += 1;
            } else {
                recovery.abandoned_deletions_removed += 1;
            }
        }
        Ok(recovery)
    }
}

pub(crate) struct ModelStoreLock {
    pub(crate) _file: File,
}
