use crate::key_names::normalize_legacy_key_image_names;
use crate::mver::{self, ModelSourceContent, MverInputMode, MverSource};
use crate::{InstalledModel, ModelError, ModelId, ModelPackageLimits, PreparedModel};
use bongocat_storage::{set_private_directory, set_private_file};
use std::{
    fmt, fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const IMPORTING_PREFIX: &str = ".importing-";
const DELETING_PREFIX: &str = ".deleting-";

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
            // source was read as a package and failed package validation — this
            // one never became a package at all.
            Self::SourceConversionFailed => "model_store_source_conversion_failed",
            Self::SourceSymlinkUnsupported => "model_store_source_symlink_unsupported",
            Self::SourceEntryUnsupported => "model_store_source_entry_unsupported",
            Self::StoreBusy => "model_store_busy",
            Self::StoreEntryUnsupported => "model_store_entry_unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelImportStage {
    Preparing,
    Copying,
    Validating,
    Committing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelImportProgress {
    pub stage: ModelImportStage,
    pub files_copied: u64,
    pub bytes_copied: u64,
}

#[derive(Debug)]
pub struct ModelStoreError {
    pub code: ModelStoreDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
    source: Option<ModelError>,
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

    fn package(error: ModelError) -> Self {
        Self {
            code: ModelStoreDiagnostic::InvalidPackage,
            resource: error.resource.clone(),
            detail: error.to_string(),
            source: Some(error),
        }
    }
}

impl fmt::Display for ModelStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.resource {
            Some(resource) => write!(formatter, "{:?} ({resource}): {}", self.code, self.detail),
            None => write!(formatter, "{:?}: {}", self.code, self.detail),
        }
    }
}

impl std::error::Error for ModelStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

pub struct ModelStore {
    canonical_root: PathBuf,
    lock_path: PathBuf,
    limits: ModelPackageLimits,
    recovery: ModelStoreRecovery,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelStoreRecovery {
    pub abandoned_imports_removed: usize,
    pub abandoned_deletions_removed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelCatalogEntry {
    Ready {
        origin: crate::ModelOrigin,
        snapshot: crate::ModelSnapshot,
    },
    Invalid {
        origin: crate::ModelOrigin,
        id: ModelId,
        code: crate::ModelDiagnostic,
        resource: Option<String>,
        detail: String,
    },
}

impl ModelCatalogEntry {
    pub const fn origin(&self) -> crate::ModelOrigin {
        match self {
            Self::Ready { origin, .. } | Self::Invalid { origin, .. } => *origin,
        }
    }

    pub fn id(&self) -> &ModelId {
        match self {
            Self::Ready { snapshot, .. } => &snapshot.id,
            Self::Invalid { id, .. } => id,
        }
    }

    pub fn snapshot(&self) -> Option<&crate::ModelSnapshot> {
        match self {
            Self::Ready { snapshot, .. } => Some(snapshot),
            Self::Invalid { .. } => None,
        }
    }
}

/// Outcome of scanning the installed model store.
///
/// The store root is application-owned, but it is still an ordinary directory
/// on the user's disk: file managers drop metadata beside the model folders and
/// a user can leave unrelated files behind. A single unrecognized entry must
/// never make the whole catalog unavailable, so such entries are dropped during
/// the scan and only counted here instead of failing it. Platform metadata is
/// not even counted: the operating system or file manager owns it and it can
/// never be a model.
///
/// The count is internal store state. Filtering is silent by design, so it is
/// never projected into the settings snapshot, user-facing text or logging.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstalledModelCatalog {
    pub entries: Vec<ModelCatalogEntry>,
    pub skipped_entries: usize,
}

/// File-manager and operating-system metadata that legitimately appears in the
/// store root without being owned by the catalog.
pub(crate) fn is_platform_metadata_name(name: &str) -> bool {
    // AppleDouble sidecars are written next to files on non-native volumes.
    name.starts_with("._")
        || matches!(
            name,
            ".DS_Store" | ".localized" | "Thumbs.db" | "desktop.ini"
        )
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
                    origin: crate::ModelOrigin::Installed,
                    snapshot: prepared.snapshot(),
                },
                Err(error) => ModelCatalogEntry::Invalid {
                    origin: crate::ModelOrigin::Installed,
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

    fn is_id_vacant(&self, id: &ModelId) -> bool {
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
        let resources = root.join(crate::PACKAGE_RESOURCES_DIRECTORY);
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

        let cover = resources.join(crate::PACKAGE_COVER_FILE);
        let staging = resources.join(format!(".{}.new", crate::PACKAGE_COVER_FILE));
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

    fn describe_source(&self, source: &MverSource) -> Result<ModelSourceContent, ModelStoreError> {
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

    fn convert_mver_mode<Observe, IsCancelled>(
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
        self.commit_installed_staging(&id, staging, &mut cleanup, &statistics, &mut observation)
    }

    pub fn import_with_observer<Observe, IsCancelled>(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
        mut observe: Observe,
        mut is_cancelled: IsCancelled,
    ) -> Result<InstalledModel, ModelStoreError>
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
        self.commit_installed_staging(&id, staging, &mut cleanup, &statistics, &mut observation)
    }

    /// Validate the materialized staging tree and commit it as the installed
    /// model.
    ///
    /// Whatever produced the bytes — a folder copy or a BongoCatMver conversion
    /// — goes through the *same* package validation and the *same* single atomic
    /// rename, so no source format can become a second, weaker parser.
    fn commit_installed_staging<Observe, IsCancelled>(
        &self,
        id: &ModelId,
        staging: PathBuf,
        cleanup: &mut StagingCleanup,
        statistics: &CopyStatistics,
        observation: &mut ImportObservation<'_, Observe, IsCancelled>,
    ) -> Result<InstalledModel, ModelStoreError>
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
        let mut prepared = PreparedModel::prepare(id.clone(), &staging, self.limits)
            .map_err(ModelStoreError::package)?;
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
        prepared.canonical_root = destination;
        Ok(InstalledModel::from_prepared(prepared))
    }

    fn create_staging_directory(&self, id: &ModelId) -> Result<PathBuf, ModelStoreError> {
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

    fn unique_operation_path(
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

    fn acquire_lock(&self) -> Result<ModelStoreLock, ModelStoreError> {
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

    fn installed_path(&self, id: &ModelId) -> Result<PathBuf, ModelStoreError> {
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

    fn recover_abandoned_operations(&self) -> Result<ModelStoreRecovery, ModelStoreError> {
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

fn is_owned_operation_name(name: &str, prefix: &str) -> bool {
    let Some(remainder) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some((id_and_process, sequence)) = remainder.rsplit_once('-') else {
        return false;
    };
    let Some((id, process)) = id_and_process.rsplit_once('-') else {
        return false;
    };
    ModelId::parse(id).is_ok() && process.parse::<u32>().is_ok() && sequence.parse::<u64>().is_ok()
}

struct ModelStoreLock {
    _file: File,
}

#[derive(Default)]
pub(crate) struct CopyStatistics {
    pub(crate) file_count: usize,
    pub(crate) total_bytes: u64,
}

pub(crate) const COPY_BUFFER_BYTES: usize = 64 * 1024;

pub(crate) struct ImportObservation<'a, Observe, IsCancelled> {
    observe: &'a mut Observe,
    is_cancelled: &'a mut IsCancelled,
}

impl<Observe, IsCancelled> ImportObservation<'_, Observe, IsCancelled>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    pub(crate) fn new<'a>(
        observe: &'a mut Observe,
        is_cancelled: &'a mut IsCancelled,
    ) -> ImportObservation<'a, Observe, IsCancelled> {
        ImportObservation {
            observe,
            is_cancelled,
        }
    }

    pub(crate) fn check_cancelled(&mut self) -> Result<(), ModelStoreError> {
        if (self.is_cancelled)() {
            Err(ModelStoreError::new(
                ModelStoreDiagnostic::Cancelled,
                None,
                "model import was cancelled",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn report(&mut self, progress: ModelImportProgress) {
        (self.observe)(progress);
    }
}

pub(crate) fn file_count_for_progress(file_count: usize) -> u64 {
    u64::try_from(file_count).unwrap_or(u64::MAX)
}

fn copy_package<Observe, IsCancelled>(
    source_root: &Path,
    source_directory: &Path,
    destination_directory: &Path,
    depth: usize,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
    if depth > limits.maximum_directory_depth {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            None,
            "source directory depth changed after validation",
        ));
    }
    for entry in fs::read_dir(source_directory).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("source directory cannot be listed: {error}"),
        )
    })? {
        observation.check_cancelled()?;
        let entry = entry.map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("source directory entry cannot be read: {error}"),
            )
        })?;
        let source = entry.path();
        let relative = source.strip_prefix(source_root).map_err(|_| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                None,
                "source entry escaped the validated package",
            )
        })?;
        let resource = relative.to_str().map(str::to_owned).ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceEntryUnsupported,
                None,
                "source path is not valid UTF-8",
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.clone()),
                format!("source entry type cannot be read: {error}"),
            )
        })?;
        if file_type.is_symlink() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                Some(resource),
                "model imports do not follow symbolic links",
            ));
        }
        let destination = destination_directory.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir(&destination).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(resource.clone()),
                    format!("staging directory cannot be created: {error}"),
                )
            })?;
            set_private_directory(&destination).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(resource.clone()),
                    format!("staging directory permissions cannot be set: {error}"),
                )
            })?;
            copy_package(
                source_root,
                &source,
                &destination,
                depth + 1,
                limits,
                statistics,
                observation,
            )?;
        } else if file_type.is_file() {
            copy_file(
                source_root,
                &source,
                &destination,
                &resource,
                limits,
                statistics,
                observation,
            )?;
        } else {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceEntryUnsupported,
                Some(resource),
                "source entry is not a regular file or directory",
            ));
        }
    }
    Ok(())
}

fn copy_file<Observe, IsCancelled>(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    resource: &str,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
    let canonical = source.canonicalize().map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be resolved: {error}"),
        )
    })?;
    if !canonical.starts_with(source_root) {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source file resolved outside the validated package",
        ));
    }
    let mut input = File::open(&canonical).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be opened: {error}"),
        )
    })?;
    let size = input
        .metadata()
        .map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file metadata cannot be read: {error}"),
            )
        })?
        .len();
    if size > limits.maximum_file_bytes {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source file exceeded its validated size limit",
        ));
    }
    let next_file_count = statistics.file_count.saturating_add(1);
    let next_total_bytes = statistics.total_bytes.checked_add(size).ok_or_else(|| {
        ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source package byte count overflowed",
        )
    })?;
    if next_file_count > limits.maximum_file_count
        || next_total_bytes > limits.maximum_package_bytes
    {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source package exceeded its validated limits",
        ));
    }

    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("staging file cannot be created: {error}"),
            )
        })?;
    set_private_file(&output).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("staging file permissions cannot be set: {error}"),
        )
    })?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        observation.check_cancelled()?;
        let read = input.read(&mut buffer).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file cannot be read: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file cannot be copied: {error}"),
            )
        })?;
        copied = copied.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if copied > size {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(resource.to_owned()),
                "source file grew while copying",
            ));
        }
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Copying,
            files_copied: file_count_for_progress(statistics.file_count),
            bytes_copied: statistics.total_bytes.saturating_add(copied),
        });
    }
    if copied != size {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            format!("source size changed while copying: expected {size}, copied {copied}"),
        ));
    }
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("staging file cannot be flushed: {error}"),
            )
        })?;
    statistics.file_count = next_file_count;
    statistics.total_bytes = next_total_bytes;
    observation.report(ModelImportProgress {
        stage: ModelImportStage::Copying,
        files_copied: file_count_for_progress(statistics.file_count),
        bytes_copied: statistics.total_bytes,
    });
    Ok(())
}

struct StagingCleanup {
    path: Option<PathBuf>,
}

impl StagingCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::fs::TryLockError;
    use std::path::Path;
    use tempfile::tempdir;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root")
            .to_owned()
    }

    fn fixture(name: &str) -> PathBuf {
        repository_root()
            .join("shared/fixtures/model-fixtures/cases")
            .join(name)
    }

    fn model_store(base: &Path) -> ModelStore {
        ModelStore::new(
            base.join("models"),
            base.join("locks/models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("model store")
    }

    #[test]
    fn a_cover_replacement_lands_on_the_package_cover_and_leaves_no_staging_file() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let id = ModelId::parse("cover").expect("model id");
        let installed = store
            .import(id.clone(), fixture("非 ASCII 模型"))
            .expect("import model");

        let replacement = b"\x89PNG\r\n\x1a\nreplacement".to_vec();
        let cover = store
            .replace_cover(&id, &replacement)
            .expect("replace cover");

        assert_eq!(cover, installed.root().join("resources/cover.png"));
        assert_eq!(fs::read(&cover).expect("stored cover"), replacement);
        let leftovers = fs::read_dir(installed.root().join("resources"))
            .expect("resources directory")
            .map(|entry| entry.expect("resource entry").file_name())
            .filter(|name| name.to_string_lossy().ends_with(".new"))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn a_cover_cannot_be_replaced_on_a_model_that_is_not_installed() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let error = store
            .replace_cover(&ModelId::parse("absent").expect("model id"), b"bytes")
            .expect_err("missing model");
        assert_eq!(error.code, ModelStoreDiagnostic::NotFound);
    }

    #[test]
    fn imports_valid_package_without_modifying_source() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let source = fixture("非 ASCII 模型");
        let source_moc = fs::read(source.join("模型 数据.moc3")).expect("source moc");
        let installed = store
            .import(ModelId::parse("unicode").expect("model id"), &source)
            .expect("import model");

        assert_eq!(installed.root(), store.root().join("unicode"));
        assert_eq!(installed.index().moc, "模型 数据.moc3");
        assert_eq!(
            fs::read(source.join("模型 数据.moc3")).expect("source unchanged"),
            source_moc
        );
        assert!(installed.root().join("猫.model3.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn model_store_data_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let installed = store
            .import(
                ModelId::parse("private").expect("model id"),
                fixture("非 ASCII 模型"),
            )
            .expect("import model");

        assert_eq!(
            fs::metadata(store.root())
                .expect("model root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(installed.root())
                .expect("installed model metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(installed.root().join("猫.model3.json"))
                .expect("installed model file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(data.path().join("locks"))
                .expect("lock directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(data.path().join("locks/models.writer.lock"))
                .expect("lock file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn successful_import_reports_all_stages_and_final_copy_totals() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let progress = RefCell::new(Vec::new());

        store
            .import_with_observer(
                ModelId::parse("observed").expect("model id"),
                fixture("非 ASCII 模型"),
                |update| progress.borrow_mut().push(update),
                || false,
            )
            .expect("observed import");

        let progress = progress.into_inner();
        assert_eq!(
            progress.first().map(|update| update.stage),
            Some(ModelImportStage::Preparing)
        );
        assert_eq!(
            progress.last().map(|update| update.stage),
            Some(ModelImportStage::Committing)
        );
        assert!(
            progress
                .iter()
                .any(|update| update.stage == ModelImportStage::Copying)
        );
        assert!(
            progress
                .iter()
                .any(|update| update.stage == ModelImportStage::Validating)
        );
        let final_progress = progress.last().expect("final progress");
        assert!(final_progress.files_copied > 0);
        assert!(final_progress.bytes_copied > 0);
        for updates in progress.windows(2) {
            assert!(updates[0].stage <= updates[1].stage);
            assert!(updates[0].files_copied <= updates[1].files_copied);
            assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
        }
    }

    #[test]
    fn import_progress_is_monotonic_and_cancellation_removes_partial_state() {
        let source = tempdir().expect("source");
        fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        fs::write(source.path().join("payload.bin"), vec![0_u8; 512 * 1024]).expect("payload");
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let cancelled = Cell::new(false);
        let progress = RefCell::new(Vec::new());

        let error = store
            .import_with_observer(
                ModelId::parse("cancelled").expect("model id"),
                source.path(),
                |update| {
                    progress.borrow_mut().push(update);
                    if update.stage == ModelImportStage::Copying && update.bytes_copied >= 65_536 {
                        cancelled.set(true);
                    }
                },
                || cancelled.get(),
            )
            .expect_err("cancelled import");

        assert_eq!(error.code, ModelStoreDiagnostic::Cancelled);
        assert!(store.list().expect("empty catalog").entries.is_empty());
        assert!(!store.root().join("cancelled").exists());
        assert!(
            fs::read_dir(store.root())
                .expect("store entries")
                .all(|entry| !entry
                    .expect("store entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(IMPORTING_PREFIX))
        );
        let progress = progress.into_inner();
        assert!(progress.len() >= 3);
        for updates in progress.windows(2) {
            assert!(updates[0].stage <= updates[1].stage);
            assert!(updates[0].files_copied <= updates[1].files_copied);
            assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
        }
    }

    #[test]
    fn duplicate_id_never_overwrites_installed_model() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let id = ModelId::parse("unicode").expect("model id");
        let installed = store
            .import(id.clone(), fixture("非 ASCII 模型"))
            .expect("first import");
        fs::write(installed.root().join("user-marker"), b"keep").expect("marker");

        let error = store
            .import(id, fixture("非 ASCII 模型"))
            .expect_err("duplicate import");
        assert_eq!(error.code, ModelStoreDiagnostic::AlreadyExists);
        assert_eq!(
            fs::read(installed.root().join("user-marker")).expect("marker preserved"),
            b"keep"
        );
    }

    #[test]
    fn invalid_package_leaves_no_destination_or_staging_directory() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let error = store
            .import(
                ModelId::parse("broken").expect("model id"),
                fixture("missing-moc"),
            )
            .expect_err("invalid import");
        assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
        assert!(store.list().expect("empty catalog").entries.is_empty());
    }

    /// The store's only source shape is the folder a user picked, so a path that
    /// is not one has to fail cleanly rather than half-import or panic.
    ///
    /// This is the property the archive source's removal left behind: the
    /// picker never returns a file, but the store is a public API and a caller
    /// that hands it one still has to get a stable diagnostic and an untouched
    /// store — not a panic from code that assumed a directory, and not a
    /// half-written staging tree.
    #[test]
    fn a_source_that_is_not_a_folder_fails_without_touching_the_store() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let file = sources.path().join("model.package");
        fs::write(&file, b"not a model folder").expect("source file");

        let error = store
            .import(ModelId::parse("a-file").expect("model id"), &file)
            .expect_err("a file is not a model folder");
        assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
        assert_store_holds_no_entries(&store);

        // Describing it is not an error either: the answer is "a package", and
        // the import above is what reports the real diagnostic.
        assert_eq!(
            store.inspect_source(&file).expect("describe a file"),
            ModelSourceContent::Package
        );
        assert_store_holds_no_entries(&store);
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_even_internal_symbolic_links() {
        use std::os::unix::fs::symlink;

        let source = tempdir().expect("source");
        fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        symlink("model.moc3", source.path().join("alias.moc3")).expect("symlink");
        fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"alias.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        let data = tempdir().expect("data root");
        let store = model_store(data.path());

        let error = store
            .import(ModelId::parse("linked").expect("model id"), source.path())
            .expect_err("symlink import");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceSymlinkUnsupported);
        assert!(store.list().expect("empty catalog").entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_and_delete_never_follow_an_installed_symlink() {
        use std::os::unix::fs::symlink;

        let data = tempdir().expect("data root");
        let outside = tempdir().expect("outside root");
        fs::write(outside.path().join("keep"), b"outside").expect("outside marker");
        let store = model_store(data.path());
        symlink(outside.path(), store.root().join("linked")).expect("installed symlink");
        let id = ModelId::parse("linked").expect("model id");

        let catalog = store.list().expect("catalog");
        assert!(catalog.entries.is_empty());
        assert_eq!(catalog.skipped_entries, 1);
        assert_eq!(
            store.delete(&id).expect_err("delete rejects symlink").code,
            ModelStoreDiagnostic::StoreEntryUnsupported
        );
        assert_eq!(
            fs::read(outside.path().join("keep")).expect("outside marker preserved"),
            b"outside"
        );
    }

    #[test]
    fn catalog_survives_platform_metadata_in_the_store_root() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let fixture = fixture("非 ASCII 模型");
        store
            .import(ModelId::parse("alpha").expect("model id"), &fixture)
            .expect("import alpha");
        // Browsing the store root in Finder (for example to delete an installed
        // model by hand) drops `.DS_Store` next to the model directories. It is
        // file-manager state, never a model, and must not make the catalog
        // unavailable.
        fs::write(store.root().join(".DS_Store"), b"finder metadata").expect("finder metadata");

        let catalog = store.list().expect("catalog");
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].id().as_str(), "alpha");
        assert_eq!(catalog.skipped_entries, 0);
    }

    #[test]
    fn catalog_skips_foreign_entries_without_hiding_the_remaining_models() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let fixture = fixture("非 ASCII 模型");
        store
            .import(ModelId::parse("alpha").expect("model id"), &fixture)
            .expect("import alpha");
        fs::write(store.root().join("notes.txt"), b"user note").expect("foreign file");
        // A directory whose name is not a portable model id can never be an
        // installed model, so it is skipped instead of entering the catalog.
        fs::create_dir(store.root().join("not a model id")).expect("foreign directory");
        fs::write(store.root().join("Thumbs.db"), b"windows metadata").expect("windows metadata");
        // A directory carrying a valid model id stays visible even when it holds
        // no usable package; only entries that cannot be models are skipped.
        fs::create_dir(store.root().join("empty-model")).expect("empty model directory");

        let catalog = store.list().expect("catalog");
        assert_eq!(catalog.entries.len(), 2);
        assert_eq!(catalog.entries[0].id().as_str(), "alpha");
        assert!(matches!(
            catalog.entries[1],
            ModelCatalogEntry::Invalid { .. }
        ));
        assert_eq!(catalog.entries[1].id().as_str(), "empty-model");
        assert_eq!(catalog.skipped_entries, 2);
    }

    #[test]
    fn catalog_is_sorted_and_reports_corrupt_packages_without_hiding_valid_models() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let fixture = fixture("非 ASCII 模型");
        store
            .import(ModelId::parse("zeta").expect("model id"), &fixture)
            .expect("import zeta");
        let alpha = store
            .import(ModelId::parse("alpha").expect("model id"), &fixture)
            .expect("import alpha");
        fs::remove_file(alpha.root().join("模型 数据.moc3")).expect("corrupt alpha");

        let catalog = store.list().expect("catalog");
        assert_eq!(catalog.entries.len(), 2);
        assert_eq!(catalog.entries[0].id().as_str(), "alpha");
        assert!(matches!(
            catalog.entries[0],
            ModelCatalogEntry::Invalid { .. }
        ));
        assert_eq!(catalog.entries[0].origin(), crate::ModelOrigin::Installed);
        assert!(catalog.entries[0].snapshot().is_none());
        assert_eq!(catalog.entries[1].id().as_str(), "zeta");
        assert!(matches!(
            catalog.entries[1],
            ModelCatalogEntry::Ready { .. }
        ));
        assert_eq!(catalog.entries[1].origin(), crate::ModelOrigin::Installed);
        assert!(catalog.entries[1].snapshot().is_some());
        assert_eq!(
            store
                .load(&ModelId::parse("zeta").expect("model id"))
                .expect("load installed model")
                .id()
                .as_str(),
            "zeta"
        );

        drop(store);
        let reopened = model_store(data.path());
        assert_eq!(
            reopened.list().expect("persistent catalog").entries.len(),
            2
        );
    }

    #[test]
    fn delete_retires_only_the_selected_installed_model() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let fixture = fixture("非 ASCII 模型");
        let alpha = ModelId::parse("alpha").expect("model id");
        let beta = ModelId::parse("beta").expect("model id");
        store.import(alpha.clone(), &fixture).expect("import alpha");
        store.import(beta.clone(), &fixture).expect("import beta");

        store.delete(&alpha).expect("delete alpha");
        assert_eq!(store.list().expect("catalog").entries.len(), 1);
        assert_eq!(
            store.load(&alpha).expect_err("alpha removed").code,
            ModelStoreDiagnostic::NotFound
        );
        assert_eq!(store.load(&beta).expect("beta preserved").id(), &beta);
    }

    #[test]
    fn startup_recovers_only_well_formed_owned_operation_directories() {
        let data = tempdir().expect("data root");
        let root = data.path().join("models");
        fs::create_dir_all(root.join(".importing-alpha-10-20")).expect("import staging");
        fs::create_dir_all(root.join(".deleting-beta-10-21")).expect("delete staging");
        fs::create_dir_all(root.join(".importing-not-owned")).expect("unowned directory");

        let store = ModelStore::new(
            &root,
            data.path().join("locks/models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("model store");
        assert_eq!(
            store.recovery(),
            ModelStoreRecovery {
                abandoned_imports_removed: 1,
                abandoned_deletions_removed: 1,
            }
        );
        assert!(root.join(".importing-not-owned").is_dir());
    }

    #[test]
    fn import_rejects_a_source_that_contains_the_destination_store() {
        let source = tempdir().expect("source");
        fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        let store = model_store(source.path());

        let error = store
            .import(
                ModelId::parse("recursive").expect("model id"),
                source.path(),
            )
            .expect_err("recursive source must fail");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceContainsStore);
        assert!(store.list().expect("empty catalog").entries.is_empty());
    }

    #[test]
    fn store_lock_makes_contention_observable() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(data.path().join("locks/models.writer.lock"))
            .expect("open store lock");
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => panic!("test lock unexpectedly busy"),
            Err(TryLockError::Error(error)) => panic!("test lock failed: {error}"),
        }

        let error = store.list().expect_err("contended store must fail");
        assert_eq!(error.code, ModelStoreDiagnostic::StoreBusy);
    }

    #[test]
    fn model_store_diagnostic_codes_are_stable_and_unique() {
        let mut codes = ModelStoreDiagnostic::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| code.starts_with("model_store_")));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), ModelStoreDiagnostic::ALL.len());
        assert_eq!(
            ModelStoreDiagnostic::SourceChanged.as_str(),
            "model_store_source_changed"
        );
    }

    #[test]
    fn allocate_unique_id_generates_distinct_portable_ids() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..32 {
            let id = store.allocate_unique_id().expect("allocate");
            assert_eq!(id.as_str().len(), 36, "hyphenated UUID v4 length");
            assert!(ModelId::parse(id.as_str()).is_ok());
            assert!(seen.insert(id.as_str().to_owned()), "ids must be unique");
        }
    }

    // A model source is the folder a user picked. The archive cases this module
    // used to carry were removed with the archive source itself (ADR-0036 已撤回);
    // what is left exercises the folder path end to end.

    const SAMPLE_MODEL_JSON: &[u8] = br#"{
      "Version": 3,
      "FileReferences": {
        "Moc": "model.moc3",
        "Textures": ["textures/texture_00.png"]
      },
      "Groups": [
        {"Target": "Parameter", "Name": "EyeBlink", "Ids": ["ParamEyeLOpen"]}
      ]
    }"#;

    /// The 24 bytes the package parser inspects: the PNG signature and an IHDR
    /// chunk carrying the declared dimensions.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::from(*b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    fn sample_package_entries() -> Vec<(String, Vec<u8>)> {
        vec![
            ("cat.model3.json".to_owned(), SAMPLE_MODEL_JSON.to_vec()),
            ("model.moc3".to_owned(), b"moc3".to_vec()),
            ("textures/texture_00.png".to_owned(), png_header(1024, 1024)),
        ]
    }

    fn write_package_directory(root: &Path) {
        for (reference, bytes) in sample_package_entries() {
            let path = root.join(&reference);
            fs::create_dir_all(path.parent().expect("reference parent"))
                .expect("create package directory");
            fs::write(path, bytes).expect("write package file");
        }
    }

    fn assert_store_holds_no_entries(store: &ModelStore) {
        assert!(
            fs::read_dir(store.root())
                .expect("store entries")
                .next()
                .is_none(),
            "a rejected import must not leave staging or destination entries"
        );
    }

    /// A model authored against the old `rdev` naming must keep working after
    /// import: the installed package is rewritten to the canonical names the
    /// runtime resolves, and the user's source is left exactly as it was.
    #[test]
    fn legacy_alt_key_images_are_renamed_on_import_without_touching_the_source() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let source = tempdir().expect("source");
        write_package_directory(source.path());
        let legacy = [
            ("resources/left-keys/Alt.png", b"alt".as_slice()),
            ("resources/left-keys/AltGr.png", b"altgr".as_slice()),
            ("resources/right-keys/Alt.png", b"hand alt".as_slice()),
        ];
        for (reference, bytes) in legacy {
            let path = source.path().join(reference);
            fs::create_dir_all(path.parent().expect("reference parent")).expect("key directory");
            fs::write(&path, bytes).expect("write legacy key image");
        }

        let installed = store
            .import(ModelId::parse("legacy").expect("model id"), source.path())
            .expect("import legacy model");

        for (reference, bytes) in [
            ("resources/left-keys/AltLeft.png", b"alt".as_slice()),
            ("resources/left-keys/AltRight.png", b"altgr".as_slice()),
            ("resources/right-keys/AltLeft.png", b"hand alt".as_slice()),
        ] {
            assert_eq!(
                fs::read(installed.root().join(reference)).expect("canonical key image"),
                bytes,
                "{reference}"
            );
        }
        for legacy in [
            "resources/left-keys/Alt.png",
            "resources/left-keys/AltGr.png",
            "resources/right-keys/Alt.png",
        ] {
            assert!(
                !installed.root().join(legacy).exists(),
                "{legacy} must not survive the import"
            );
        }
        // Renaming is the only change: the package the source described is the
        // package that was installed.
        assert_eq!(installed.index().moc, "model.moc3");
        assert_eq!(installed.index().textures.len(), 1);

        for (reference, bytes) in legacy {
            assert_eq!(
                fs::read(source.path().join(reference)).expect("source key image"),
                bytes,
                "the source keeps its own names and bytes: {reference}"
            );
        }
    }

    /// The rewrite never resolves a conflict by guessing: a package that ships
    /// both spellings keeps the canonical file's bytes and the legacy file.
    #[test]
    fn an_imported_package_that_ships_both_spellings_keeps_the_canonical_image() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let source = tempdir().expect("source");
        write_package_directory(source.path());
        for (reference, bytes) in [
            ("resources/left-keys/Alt.png", b"legacy".as_slice()),
            ("resources/left-keys/AltLeft.png", b"canonical".as_slice()),
        ] {
            let path = source.path().join(reference);
            fs::create_dir_all(path.parent().expect("reference parent")).expect("key directory");
            fs::write(&path, bytes).expect("write key image");
        }

        let installed = store
            .import(ModelId::parse("both").expect("model id"), source.path())
            .expect("import model");

        assert_eq!(
            fs::read(installed.root().join("resources/left-keys/AltLeft.png"))
                .expect("canonical key image"),
            b"canonical"
        );
        assert_eq!(
            fs::read(installed.root().join("resources/left-keys/Alt.png"))
                .expect("legacy key image"),
            b"legacy"
        );
    }

    /// A BongoCatMver source is one user-picked folder that describes several
    /// models. The store must recognize it from its own bytes while leaving a
    /// genuine BongoCat package alone.
    #[test]
    fn legacy_sources_are_described_by_their_own_bytes() {
        use crate::mver::fixture;

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let legacy = sources.path().join("Bongo Cat Mver");
        fs::create_dir(&legacy).expect("legacy source");
        fixture::legacy_source(&legacy, &fixture::all_modes(), true);

        assert_eq!(
            store
                .inspect_source(&legacy)
                .expect("describe legacy directory"),
            ModelSourceContent::Mver {
                modes: MverInputMode::ALL.to_vec(),
            }
        );

        // A genuine package is still a package.
        assert_eq!(
            store
                .inspect_source(fixture("非 ASCII 模型"))
                .expect("describe package"),
            ModelSourceContent::Package
        );
        let directory = sources.path().join("looks-like-a-package");
        write_package_directory(&directory);
        assert_eq!(
            store
                .inspect_source(&directory)
                .expect("describe package directory"),
            ModelSourceContent::Package
        );
    }

    /// Installing a legacy source produces one installed model per mode, each
    /// with the structure the runtime and the settings page expect.
    #[test]
    fn legacy_import_installs_one_model_per_configured_mode() {
        use crate::mver::fixture;

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let legacy = sources.path().join("Bongo Cat Mver");
        fs::create_dir(&legacy).expect("legacy source");
        fixture::legacy_source(&legacy, &fixture::all_modes(), true);

        let mut installed = Vec::new();
        for mode in MverInputMode::ALL {
            let id = store.allocate_unique_id().expect("allocate id");
            installed.push(
                store
                    .import_mver_with_observer(id, mode, &legacy, |_| {}, || false)
                    .unwrap_or_else(|error| panic!("{} failed: {error:?}", mode.as_str())),
            );
        }

        assert_eq!(store.list().expect("catalog").entries.len(), 3);
        assert_ne!(installed[0].id(), installed[1].id());
        for (model, mode) in installed.iter().zip(MverInputMode::ALL) {
            assert_eq!(model.index().entry, "cat.model3.json");
            assert_eq!(model.index().moc, "model.moc3");
            assert!(
                model
                    .root()
                    .join(format!("resources/left-keys/{}.png", left_key_name(mode)))
                    .is_file(),
                "{} must install a composed left key image",
                mode.as_str()
            );
            assert!(
                model.root().join("resources/background.png").is_file(),
                "{} must install its background",
                mode.as_str()
            );
            assert_eq!(
                model.root().join("resources/right-keys").is_dir(),
                mode != MverInputMode::Standard,
                "only the split modes expose a right hand ({})",
                mode.as_str()
            );
        }
        // The legacy source itself is never modified.
        assert!(legacy.join("config.json").is_file());
        assert!(legacy.join("img").is_dir());
        // A conversion leaves no staging directory behind.
        assert!(
            fs::read_dir(store.root())
                .expect("store entries")
                .all(|entry| !entry
                    .expect("store entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(IMPORTING_PREFIX))
        );
    }

    fn left_key_name(mode: MverInputMode) -> &'static str {
        match mode {
            MverInputMode::Standard | MverInputMode::Keyboard => "KeyA",
            MverInputMode::Gamepad => "DPadLeft",
        }
    }

    /// A conversion is cancellable and, like every other import, leaves nothing
    /// behind when it is.
    #[test]
    fn legacy_import_reports_monotonic_progress_and_cancels_without_partial_state() {
        use crate::mver::fixture;

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let legacy = sources.path().join("Bongo Cat Mver");
        fs::create_dir(&legacy).expect("legacy source");
        fixture::legacy_source(&legacy, &fixture::all_modes(), true);

        let progress = RefCell::new(Vec::new());
        let id = store.allocate_unique_id().expect("allocate id");
        store
            .import_mver_with_observer(
                id.clone(),
                MverInputMode::Standard,
                &legacy,
                |update| progress.borrow_mut().push(update),
                || false,
            )
            .expect("observed conversion");
        let progress = progress.into_inner();
        assert_eq!(
            progress.first().map(|update| update.stage),
            Some(ModelImportStage::Preparing)
        );
        assert_eq!(
            progress.last().map(|update| update.stage),
            Some(ModelImportStage::Committing)
        );
        assert!(
            progress
                .iter()
                .any(|update| update.stage == ModelImportStage::Copying)
        );
        assert!(
            progress
                .iter()
                .any(|update| update.stage == ModelImportStage::Validating)
        );
        for updates in progress.windows(2) {
            assert!(updates[0].stage <= updates[1].stage);
            assert!(updates[0].files_copied <= updates[1].files_copied);
            assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
        }

        let cancelled = Cell::new(false);
        let error = store
            .import_mver_with_observer(
                store.allocate_unique_id().expect("allocate id"),
                MverInputMode::Keyboard,
                &legacy,
                |update| {
                    if update.stage == ModelImportStage::Copying && update.files_copied > 0 {
                        cancelled.set(true);
                    }
                },
                || cancelled.get(),
            )
            .expect_err("cancelled conversion");
        assert_eq!(error.code, ModelStoreDiagnostic::Cancelled);
        let catalog = store.list().expect("catalog");
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].id(), &id);
        assert!(
            fs::read_dir(store.root())
                .expect("store entries")
                .all(|entry| !entry
                    .expect("store entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(IMPORTING_PREFIX))
        );
    }

    /// Asking for a mode the source does not carry is a stable diagnostic, not
    /// an installed model.
    #[test]
    fn legacy_import_rejects_a_mode_the_source_does_not_carry() {
        use crate::mver::fixture;

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let legacy = sources.path().join("Bongo Cat Mver");
        fs::create_dir(&legacy).expect("legacy source");
        fixture::legacy_source(
            &legacy,
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );

        let error = store
            .import_mver_with_observer(
                store.allocate_unique_id().expect("allocate id"),
                MverInputMode::Gamepad,
                &legacy,
                |_| {},
                || false,
            )
            .expect_err("missing input mode");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceConversionFailed);
        assert_store_holds_no_entries(&store);
    }

    /// Read a source folder's key images without importing anything, so a test
    /// can compare them with what an import installed. Keys are package-relative
    /// references.
    fn source_key_images(source: &Path) -> std::collections::BTreeMap<(String, String), Vec<u8>> {
        use std::collections::BTreeMap;

        const DIRECTORIES: [&str; 2] = ["resources/left-keys", "resources/right-keys"];
        let mut images = BTreeMap::new();
        for directory in DIRECTORIES {
            let Ok(entries) = fs::read_dir(source.join(directory)) else {
                continue;
            };
            for entry in entries {
                let entry = entry.expect("source key image");
                if entry.file_type().expect("source entry type").is_dir() {
                    continue;
                }
                images.insert(
                    (
                        directory.to_owned(),
                        entry.file_name().into_string().expect("key image name"),
                    ),
                    fs::read(entry.path()).expect("read source key image"),
                );
            }
        }
        images
    }

    /// Import the community BongoCat model the maintainer points at.
    ///
    /// Models downloaded for the old `rdev`-based BongoCat carry third-party
    /// artwork and cannot be committed to the repository, so the real-world case
    /// is covered by pointing `BONGOCAT_PACKAGE_SAMPLE` at one — the folder the
    /// model was exported as. The test is skipped when the variable is unset,
    /// which keeps it out of the default `cargo test` line.
    ///
    /// What it asserts is the whole point of the rewrite: every pre-rename key
    /// image the source shipped is installed under its canonical name with the
    /// same bytes, no pre-rename name survives, every other key image is copied
    /// verbatim, and the source the user picked is not written to.
    #[test]
    fn imports_the_bongo_cat_sample_named_by_the_environment() {
        use std::collections::BTreeMap;

        let Some(source) = std::env::var_os("BONGOCAT_PACKAGE_SAMPLE") else {
            return;
        };
        let source = PathBuf::from(source);
        let data = tempdir().expect("data root");
        let store = model_store(data.path());

        let source_images = source_key_images(&source);
        assert!(
            !source_images.is_empty(),
            "{} ships no key images",
            source.display()
        );
        let mut expected: BTreeMap<(String, String), Vec<u8>> = BTreeMap::new();
        for ((directory, name), bytes) in source_images.clone() {
            let canonical = match name.as_str() {
                "Alt.png" => "AltLeft.png",
                "AltGr.png" => "AltRight.png",
                "Return.png" => "Enter.png",
                _ => {
                    expected.insert((directory, name), bytes);
                    continue;
                }
            };
            expected.insert((directory, canonical.to_owned()), bytes);
        }

        let installed = store
            .import(store.allocate_unique_id().expect("allocate id"), &source)
            .expect("import sample");

        for directory in ["resources/left-keys", "resources/right-keys"] {
            let mut actual = BTreeMap::new();
            if let Ok(entries) = fs::read_dir(installed.root().join(directory)) {
                for entry in entries {
                    let entry = entry.expect("installed key image");
                    if entry.file_type().expect("installed entry type").is_dir() {
                        continue;
                    }
                    actual.insert(
                        entry.file_name().into_string().expect("key image name"),
                        fs::read(entry.path()).expect("read installed key image"),
                    );
                }
            }
            let want = expected
                .iter()
                .filter(|((expected_directory, _), _)| expected_directory == directory)
                .map(|((_, name), bytes)| (name.clone(), bytes.clone()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(actual, want, "{directory} contents after import");
        }

        // The user's source is an input, not a working copy: it keeps its own
        // names and bytes.
        assert_eq!(
            source_key_images(&source),
            source_images,
            "the source must not be rewritten by the import"
        );
    }

    /// Convert the legacy application folder the maintainer points at.
    ///
    /// A real BongoCatMver installation bundles third-party model artwork and
    /// the legacy application itself, so it cannot be committed to the
    /// repository; the real-world case is covered by pointing
    /// `BONGOCAT_MVER_SAMPLE` at one instead. The test is skipped when the
    /// variable is unset, which keeps it out of the default `cargo test` line.
    /// What it asserts is what makes a conversion usable: every mode the
    /// inspector found installs as a real package with a resolved entry, moc and
    /// texture, its overlays are decodable PNGs of one size, and the legacy
    /// source is left untouched.
    #[test]
    fn converts_the_legacy_sample_named_by_the_environment() {
        use image::ImageReader;
        use std::collections::BTreeSet;

        let Some(source) = std::env::var_os("BONGOCAT_MVER_SAMPLE") else {
            return;
        };
        let source = PathBuf::from(source);
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let content = store.inspect_source(&source).expect("describe sample");
        let ModelSourceContent::Mver { modes } = &content else {
            panic!("sample {} is not a BongoCatMver source", source.display());
        };
        assert!(!modes.is_empty(), "sample has no convertible mode");

        for mode in modes {
            let id = store.allocate_unique_id().expect("allocate id");
            let installed = store
                .import_mver_with_observer(id, *mode, &source, |_| {}, || false)
                .unwrap_or_else(|error| panic!("{} failed: {error:?}", mode.as_str()));
            assert_eq!(installed.index().entry, "cat.model3.json");
            assert!(installed.root().join(&installed.index().moc).is_file());
            assert!(!installed.index().textures.is_empty());
            for texture in &installed.index().textures {
                assert!(installed.root().join(&texture.file).is_file());
            }

            let left_keys = installed.root().join("resources/left-keys");
            assert!(
                left_keys.is_dir(),
                "{} has no left key images",
                mode.as_str()
            );
            let mut sizes = BTreeSet::new();
            for entry in fs::read_dir(&left_keys).expect("left key images") {
                let path = entry.expect("left key image").path();
                let image = ImageReader::open(&path)
                    .expect("open key image")
                    .decode()
                    .unwrap_or_else(|error| panic!("{} is not decodable: {error}", path.display()));
                sizes.insert((image.width(), image.height()));
            }
            assert_eq!(sizes.len(), 1, "every overlay shares one canvas size");
        }

        // The legacy folder is a reference source, never an install target.
        assert!(source.join("config.json").is_file());
        assert!(source.join("img").is_dir());
    }
}
