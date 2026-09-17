use crate::archive::{self, ModelSourceKind};
use crate::mver::{self, ModelSourceContent, MverInputMode, MverSource};
use crate::{InstalledModel, ModelError, ModelId, ModelPackageLimits, PreparedModel};
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

// Installed models and their lock metadata are user-owned data. Unix modes
// enforce that boundary; Windows relies on the profile directory ACL.
pub(crate) fn set_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn set_private_file(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStoreDiagnostic {
    AlreadyExists,
    Cancelled,
    InvalidPackage,
    IoError,
    NotFound,
    SourceArchiveUnsupported,
    SourceContainsStore,
    SourceChanged,
    SourceConversionFailed,
    SourceSymlinkUnsupported,
    SourceEntryUnsupported,
    StoreBusy,
    StoreEntryUnsupported,
}

impl ModelStoreDiagnostic {
    pub const ALL: [Self; 13] = [
        Self::AlreadyExists,
        Self::Cancelled,
        Self::InvalidPackage,
        Self::IoError,
        Self::NotFound,
        Self::SourceArchiveUnsupported,
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
            Self::SourceArchiveUnsupported => "model_store_source_archive_unsupported",
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

    /// Import a BongoCat model package from a directory or a `.zip` archive.
    ///
    /// A BongoCatMver source is not a package: ask [`ModelStore::inspect_source`]
    /// what a user-picked source is first, and install each of its modes with
    /// [`ModelStore::import_mver_with_observer`].
    pub fn import(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
    ) -> Result<InstalledModel, ModelStoreError> {
        self.import_with_observer(id, source_root, |_| {}, || false)
    }

    /// Describe what a user-picked source is, without installing anything.
    ///
    /// The answer comes from the source's own bytes rather than from the
    /// button the user pressed, exactly like [`ModelStore::import`] deciding
    /// between a directory and an archive: a folder is read in place, an archive
    /// is planned but not unpacked, and both are then asked whether they carry a
    /// BongoCatMver key table. A source that is neither a package nor a legacy
    /// model is still reported as [`ModelSourceContent::Package`], so the
    /// ordinary import reports the real diagnostic instead of this call
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
        match archive::detect_source_kind(&canonical_source, self.limits)? {
            ModelSourceKind::Directory => {
                self.describe_source(&MverSource::directory(&canonical_source)?)
            }
            ModelSourceKind::ZipArchive => {
                let plan = archive::plan_archive(&canonical_source, self.limits)?;
                self.describe_source(&MverSource::archive(&canonical_source, &plan))
            }
        }
    }

    fn describe_source(
        &self,
        source: &MverSource<'_>,
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
        if archive::detect_source_kind(&canonical_source, self.limits)?
            == ModelSourceKind::ZipArchive
        {
            let plan = archive::plan_archive(&canonical_source, self.limits)?;
            return self.convert_mver_mode(
                id,
                mode,
                MverSource::archive(&canonical_source, &plan),
                &mut observe,
                &mut is_cancelled,
            );
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
        source: MverSource<'_>,
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
        // A source is a directory or a zip archive, recognized from the source
        // itself instead of from a caller-supplied flag or the file extension:
        // the user picks one thing and the store decides what it is. Both are
        // validated as far as their format allows *before* the store creates a
        // staging directory, so a source that can never be imported leaves the
        // store untouched; the archive's bounded decompression then takes the
        // place of the directory copy.
        let prepared_source = match archive::detect_source_kind(&canonical_source, self.limits)? {
            ModelSourceKind::Directory => Some(
                PreparedModel::prepare(id.clone(), &canonical_source, self.limits)
                    .map_err(ModelStoreError::package)?,
            ),
            ModelSourceKind::ZipArchive => None,
        };
        let archive_plan = match prepared_source {
            Some(_) => None,
            None => Some(archive::plan_archive(&canonical_source, self.limits)?),
        };
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
        match (&prepared_source, &archive_plan) {
            (Some(prepared_source), _) => copy_package(
                prepared_source.root(),
                prepared_source.root(),
                &staging,
                0,
                self.limits,
                &mut statistics,
                &mut observation,
            )?,
            (None, Some(archive_plan)) => archive::extract_archive(
                &canonical_source,
                archive_plan,
                &staging,
                self.limits,
                &mut statistics,
                &mut observation,
            )?,
            (None, None) => unreachable!("every model source is a directory or an archive"),
        }
        self.commit_installed_staging(&id, staging, &mut cleanup, &statistics, &mut observation)
    }

    /// Validate the materialized staging tree and commit it as the installed
    /// model.
    ///
    /// Every source kind shares this tail. Whatever produced the bytes — a
    /// directory copy, an archive extraction, or a BongoCatMver conversion —
    /// goes through the *same* package validation and the *same* single atomic
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
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

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

    // A model source is either the directory a user picked or the `.zip` archive
    // a model site handed out. The archive cases below build real deflate
    // streams with the same `zip` reader/writer the store itself uses, so the
    // tests exercise the actual decompressor instead of a hand-made central
    // directory.

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

    struct ArchiveBuilder {
        path: PathBuf,
        writer: ZipWriter<File>,
    }

    impl ArchiveBuilder {
        fn new(path: PathBuf) -> Self {
            Self {
                writer: ZipWriter::new(File::create(&path).expect("create archive")),
                path,
            }
        }

        fn deflated_file(&mut self, name: &str, bytes: &[u8]) {
            self.writer
                .start_file(
                    name,
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .unix_permissions(0o644),
                )
                .expect("start deflated archive file");
            self.writer.write_all(bytes).expect("write archive file");
        }

        fn stored_file(&mut self, name: &str, bytes: &[u8]) {
            self.writer
                .start_file(
                    name,
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Stored)
                        .unix_permissions(0o644),
                )
                .expect("start stored archive file");
            self.writer
                .write_all(bytes)
                .expect("write stored archive file");
        }

        fn directory(&mut self, name: &str) {
            self.writer
                .add_directory(name, SimpleFileOptions::default().unix_permissions(0o755))
                .expect("add archive directory");
        }

        fn symlink(&mut self, name: &str, target: &str) {
            self.writer
                .add_symlink(name, target, SimpleFileOptions::default())
                .expect("add archive symlink");
        }

        fn finish(self) -> PathBuf {
            self.writer.finish().expect("finish archive");
            self.path
        }
    }

    /// Add every file below `directory` to the archive under `prefix`.
    ///
    /// Listing a real folder beats maintaining the expected entry names twice:
    /// a source that gains or renames a resource must change the assertion it
    /// belongs to, not an unrelated archive-entry list.
    fn write_archive_tree(builder: &mut ArchiveBuilder, prefix: &str, directory: &Path) {
        for entry in fs::read_dir(directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let name = entry.file_name().into_string().expect("source entry name");
            let reference = format!("{prefix}/{name}");
            if entry.file_type().expect("source entry type").is_dir() {
                builder.directory(&format!("{reference}/"));
                write_archive_tree(builder, &reference, &entry.path());
            } else {
                builder.deflated_file(&reference, &fs::read(entry.path()).expect("source file"));
            }
        }
    }

    /// Write the sample package into an archive, optionally below one wrapper
    /// directory the way "compress this folder" does.
    fn write_package_archive(path: PathBuf, wrapper: Option<&str>) -> PathBuf {
        let mut builder = ArchiveBuilder::new(path);
        if let Some(wrapper) = wrapper {
            builder.directory(&format!("{wrapper}/"));
        }
        for (reference, bytes) in sample_package_entries() {
            let name = match wrapper {
                Some(wrapper) => format!("{wrapper}/{reference}"),
                None => reference,
            };
            builder.deflated_file(&name, &bytes);
        }
        builder.finish()
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

    #[test]
    fn zip_and_directory_sources_of_the_same_package_import_identically() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let directory_source = tempdir().expect("directory source");
        write_package_directory(directory_source.path());
        let archives = tempdir().expect("archive root");
        // The wrapper name deliberately differs from the archive file name, the
        // way an exported archive is usually named after the model release
        // while the archived folder keeps its own name.
        let archive = write_package_archive(
            archives.path().join("猫 · 标准模式.zip"),
            Some("图弟 · 标准模式"),
        );

        let from_directory = store
            .import(
                ModelId::parse("from-directory").expect("model id"),
                directory_source.path(),
            )
            .expect("import directory source");
        let from_archive = store
            .import(ModelId::parse("from-archive").expect("model id"), &archive)
            .expect("import archive source");

        // Both sources describe exactly the same package, so the archive must
        // not be a second, subtly different parser.
        assert_eq!(from_archive.index(), from_directory.index());
        assert_eq!(from_archive.index().entry, "cat.model3.json");
        assert_eq!(from_archive.index().moc, "model.moc3");
        assert_eq!(from_archive.index().textures.len(), 1);
        assert_eq!(from_archive.index().textures[0].width, 1024);
        // The wrapper directory is archive tooling, so the installed package has
        // its entry at the root just like the directory source does.
        assert!(from_archive.root().join("cat.model3.json").is_file());
        assert!(
            from_archive
                .root()
                .join("textures/texture_00.png")
                .is_file()
        );
        assert!(!from_archive.root().join("图弟 · 标准模式").exists());
        assert_eq!(store.list().expect("catalog").entries.len(), 2);
    }

    #[test]
    fn archive_sources_are_recognized_by_content_not_by_name() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        // An archive keeps working when the extension is missing or wrong.
        let unnamed = write_package_archive(sources.path().join("model.package"), None);
        store
            .import(ModelId::parse("unnamed").expect("model id"), &unnamed)
            .expect("extensionless archive");
        // A stored (uncompressed) archive is a valid source too.
        let mut builder = ArchiveBuilder::new(sources.path().join("stored.zip"));
        for (reference, bytes) in sample_package_entries() {
            builder.stored_file(&reference, &bytes);
        }
        store
            .import(
                ModelId::parse("stored").expect("model id"),
                builder.finish(),
            )
            .expect("stored archive");
        // A directory named like an archive stays a directory: the source type
        // comes from the filesystem entry, never from the suffix.
        let directory = sources.path().join("looks-like.zip");
        fs::create_dir(&directory).expect("directory named like an archive");
        write_package_directory(&directory);
        store
            .import(ModelId::parse("directory").expect("model id"), &directory)
            .expect("directory named like an archive");

        assert_eq!(store.list().expect("catalog").entries.len(), 3);
    }

    #[test]
    fn nested_wrapper_directories_are_stripped_and_deep_ones_are_rejected() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        let mut builder = ArchiveBuilder::new(sources.path().join("nested.zip"));
        builder.directory("models/");
        builder.directory("models/猫/");
        for (reference, bytes) in sample_package_entries() {
            builder.deflated_file(&format!("models/猫/{reference}"), &bytes);
        }
        let nested = builder.finish();
        let installed = store
            .import(ModelId::parse("nested").expect("model id"), &nested)
            .expect("nested wrapper directories");
        assert_eq!(installed.index().entry, "cat.model3.json");

        let mut builder = ArchiveBuilder::new(sources.path().join("too-deep.zip"));
        for (reference, bytes) in sample_package_entries() {
            builder.deflated_file(&format!("猫/子目录/{reference}"), &bytes);
        }
        let too_deep = builder.finish();
        let shallow = ModelStore::new(
            data.path().join("shallow"),
            data.path().join("locks/shallow.writer.lock"),
            ModelPackageLimits {
                maximum_directory_depth: 1,
                ..ModelPackageLimits::default()
            },
        )
        .expect("shallow model store");
        let error = shallow
            .import(ModelId::parse("too-deep").expect("model id"), &too_deep)
            .expect_err("archive entry nested past the depth limit");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceChanged);
        assert_store_holds_no_entries(&shallow);
    }

    #[test]
    fn archive_tooling_metadata_entries_are_ignored() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        let mut builder = ArchiveBuilder::new(sources.path().join("finder.zip"));
        builder.directory("__MACOSX/");
        builder.directory("__MACOSX/猫 · 标准模式/");
        builder.deflated_file("__MACOSX/猫 · 标准模式/._cat.model3.json", b"appledouble");
        builder.directory("猫 · 标准模式/");
        builder.deflated_file("猫 · 标准模式/.DS_Store", b"finder metadata");
        for (reference, bytes) in sample_package_entries() {
            builder.deflated_file(&format!("猫 · 标准模式/{reference}"), &bytes);
        }
        let archive = builder.finish();

        let installed = store
            .import(ModelId::parse("finder").expect("model id"), &archive)
            .expect("archive carrying file-manager metadata");
        assert_eq!(installed.index().entry, "cat.model3.json");
        assert_eq!(installed.index().package_file_count, 3);
        assert!(!installed.root().join(".DS_Store").exists());
        assert!(!installed.root().join("__MACOSX").exists());
        assert!(installed.index().unreferenced_files.is_empty());
    }

    #[test]
    fn archives_without_a_usable_package_are_rejected() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        let not_an_archive = sources.path().join("plain.zip");
        fs::write(&not_an_archive, b"this is not a zip archive").expect("plain file");
        let truncated = sources.path().join("truncated.zip");
        fs::write(&truncated, b"PK\x03\x04").expect("truncated archive");
        let empty = ArchiveBuilder::new(sources.path().join("empty.zip")).finish();
        let mut directories_only = ArchiveBuilder::new(sources.path().join("dirs.zip"));
        directories_only.directory("猫/");
        let directories_only = directories_only.finish();

        for (id, source) in [
            ("plain", not_an_archive),
            ("truncated", truncated),
            ("empty", empty),
            ("directories-only", directories_only),
        ] {
            let error = store
                .import(ModelId::parse(id).expect("model id"), &source)
                .expect_err("archive without a usable package");
            assert_eq!(
                error.code,
                ModelStoreDiagnostic::SourceArchiveUnsupported,
                "source {id}"
            );
            assert_store_holds_no_entries(&store);
        }
    }

    #[test]
    fn archive_entries_that_escape_or_conflict_are_rejected() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        for (id, name) in [
            ("parent", "../escape.moc3"),
            ("absolute", "/etc/passwd"),
            ("platform", r"C:\models\moc.moc3"),
        ] {
            let mut builder = ArchiveBuilder::new(sources.path().join(format!("{id}.zip")));
            builder.deflated_file(name, b"payload");
            let archive = builder.finish();
            let error = store
                .import(ModelId::parse(id).expect("model id"), &archive)
                .expect_err("escaping archive entry");
            assert_eq!(
                error.code,
                ModelStoreDiagnostic::SourceEntryUnsupported,
                "entry {name}"
            );
            assert_store_holds_no_entries(&store);
        }

        let mut duplicates = ArchiveBuilder::new(sources.path().join("duplicate.zip"));
        duplicates.deflated_file("猫/model.moc3", b"first");
        // Two archive names that differ only in path spelling collapse onto one
        // package reference: the archive is well formed, the package is not.
        duplicates.deflated_file("猫//model.moc3", b"second");
        let error = store
            .import(
                ModelId::parse("duplicate").expect("model id"),
                duplicates.finish(),
            )
            .expect_err("duplicate archive entry");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceEntryUnsupported);
        assert_store_holds_no_entries(&store);

        let mut conflicting = ArchiveBuilder::new(sources.path().join("conflict.zip"));
        conflicting.deflated_file("猫/model.moc3", b"file where a directory is needed");
        conflicting.deflated_file("猫/model.moc3/child.moc3", b"nested file");
        let error = store
            .import(
                ModelId::parse("conflict").expect("model id"),
                conflicting.finish(),
            )
            .expect_err("conflicting archive entries");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceEntryUnsupported);
        assert_store_holds_no_entries(&store);
    }

    #[test]
    fn archive_symbolic_links_are_never_followed() {
        let data = tempdir().expect("data root");
        let outside = tempdir().expect("outside root");
        fs::write(outside.path().join("secret.moc3"), b"outside").expect("outside file");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");

        let mut builder = ArchiveBuilder::new(sources.path().join("symlink.zip"));
        builder.deflated_file("猫/cat.model3.json", SAMPLE_MODEL_JSON);
        builder.symlink("猫/model.moc3", "../../outside/secret.moc3");
        let archive = builder.finish();

        let error = store
            .import(ModelId::parse("symlinked").expect("model id"), &archive)
            .expect_err("symlinked archive entry");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceSymlinkUnsupported);
        assert_store_holds_no_entries(&store);
    }

    #[test]
    fn archives_exceeding_the_package_limits_are_rejected_before_extraction() {
        let data = tempdir().expect("data root");
        let sources = tempdir().expect("sources");
        let archive = write_package_archive(sources.path().join("limited.zip"), None);

        for (label, limits) in [
            (
                "file count",
                ModelPackageLimits {
                    maximum_file_count: 2,
                    ..ModelPackageLimits::default()
                },
            ),
            (
                "package bytes",
                ModelPackageLimits {
                    maximum_package_bytes: 8,
                    ..ModelPackageLimits::default()
                },
            ),
            (
                "file bytes",
                ModelPackageLimits {
                    maximum_file_bytes: 8,
                    ..ModelPackageLimits::default()
                },
            ),
        ] {
            let store = ModelStore::new(
                data.path().join(label.replace(' ', "-")),
                data.path()
                    .join(format!("locks/{}.writer.lock", label.replace(' ', "-"))),
                limits,
            )
            .expect("limited model store");
            let error = store
                .import(ModelId::parse("limited").expect("model id"), &archive)
                .expect_err("archive over the package limits");
            assert_eq!(error.code, ModelStoreDiagnostic::SourceChanged, "{label}");
            assert_store_holds_no_entries(&store);
        }

        // The entry ceiling is a structural bound checked before the entries are
        // walked at all, so an archive declaring far more entries than it can
        // ever install is refused without being decompressed.
        let mut builder = ArchiveBuilder::new(sources.path().join("crowded.zip"));
        for index in 0..5 {
            builder.deflated_file(&format!("猫/file{index}.moc3"), b"payload");
        }
        let crowded = builder.finish();
        let store = ModelStore::new(
            data.path().join("crowded"),
            data.path().join("locks/crowded.writer.lock"),
            ModelPackageLimits {
                maximum_file_count: 1,
                ..ModelPackageLimits::default()
            },
        )
        .expect("crowded model store");
        assert_eq!(
            store
                .import(ModelId::parse("crowded").expect("model id"), &crowded)
                .expect_err("archive over the entry ceiling")
                .code,
            ModelStoreDiagnostic::SourceChanged
        );
        assert_store_holds_no_entries(&store);
    }

    #[test]
    fn archive_import_reports_monotonic_progress_and_cancels_without_partial_state() {
        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        let sources = tempdir().expect("sources");
        let archive = write_package_archive(sources.path().join("cat.zip"), Some("猫"));

        let progress = RefCell::new(Vec::new());
        store
            .import_with_observer(
                ModelId::parse("observed").expect("model id"),
                &archive,
                |update| progress.borrow_mut().push(update),
                || false,
            )
            .expect("observed archive import");
        let progress = progress.into_inner();
        assert_eq!(
            progress.first().map(|update| update.stage),
            Some(ModelImportStage::Preparing)
        );
        assert_eq!(
            progress.last().map(|update| update.stage),
            Some(ModelImportStage::Committing)
        );
        let final_progress = progress.last().expect("final progress");
        assert_eq!(final_progress.files_copied, 3);
        for updates in progress.windows(2) {
            assert!(updates[0].stage <= updates[1].stage);
            assert!(updates[0].files_copied <= updates[1].files_copied);
            assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
        }

        let cancelled = Cell::new(false);
        let error = store
            .import_with_observer(
                ModelId::parse("cancelled-archive").expect("model id"),
                &archive,
                |update| {
                    if update.stage == ModelImportStage::Copying && update.bytes_copied > 0 {
                        cancelled.set(true);
                    }
                },
                || cancelled.get(),
            )
            .expect_err("cancelled archive import");
        assert_eq!(error.code, ModelStoreDiagnostic::Cancelled);
        // The cancelled import left no staging directory behind and did not
        // disturb the model that was already installed.
        assert!(
            fs::read_dir(store.root())
                .expect("store entries")
                .all(|entry| !entry
                    .expect("store entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(IMPORTING_PREFIX))
        );
        let catalog = store.list().expect("catalog");
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].id().as_str(), "observed");
    }

    /// A BongoCatMver source is one user-picked thing that describes several
    /// models. The store must recognize it from its own bytes — a directory and
    /// the archive exported from it behave identically — while leaving a genuine
    /// BongoCat package alone.
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

        let mut builder = ArchiveBuilder::new(sources.path().join("legacy.zip"));
        builder.directory("Bongo Cat Mver/");
        write_archive_tree(&mut builder, "Bongo Cat Mver", &legacy);
        let archive = builder.finish();
        assert_eq!(
            store
                .inspect_source(&archive)
                .expect("describe legacy archive"),
            ModelSourceContent::Mver {
                modes: MverInputMode::ALL.to_vec(),
            },
            "the archive wrapped around the same folder describes the same models"
        );

        // A genuine package is still a package, whichever way it arrives.
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

    /// Import the archives the maintainer points at.
    ///
    /// Model archives exported from a model site cannot be committed to the
    /// repository, so the real-world case is covered by pointing
    /// `BONGOCAT_MODEL_ARCHIVE_SAMPLES` at a directory of `.zip` files instead.
    /// The test is skipped when the variable is unset, which keeps it out of the
    /// default `cargo test` line: everything it collects needs a model the
    /// repository does not hold. What it asserts is what makes an archive usable
    /// — the entry, the moc and every texture must resolve to a real file inside
    /// the installed package, and the package must land in the catalog.
    #[test]
    fn imports_the_archive_samples_named_by_the_environment() {
        let Some(directory) = std::env::var_os("BONGOCAT_MODEL_ARCHIVE_SAMPLES") else {
            return;
        };
        let mut archives = fs::read_dir(PathBuf::from(directory))
            .expect("sample directory")
            .map(|entry| entry.expect("sample entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
            })
            .collect::<Vec<_>>();
        archives.sort();
        assert!(!archives.is_empty(), "sample directory holds no .zip files");

        let data = tempdir().expect("data root");
        let store = model_store(data.path());
        for (index, archive) in archives.iter().enumerate() {
            let id = ModelId::parse(format!("sample-{index}")).expect("model id");
            let installed = store.import(id.clone(), archive).unwrap_or_else(|error| {
                panic!("sample {} was rejected: {error:?}", archive.display())
            });
            assert_eq!(installed.id(), &id);
            assert!(
                installed.root().join(&installed.index().moc).is_file(),
                "sample {} has no installed moc",
                archive.display()
            );
            assert!(
                !installed.index().textures.is_empty(),
                "sample {} declares no texture",
                archive.display()
            );
            for texture in &installed.index().textures {
                assert!(
                    installed.root().join(&texture.file).is_file(),
                    "sample {} is missing texture {}",
                    archive.display(),
                    texture.file
                );
            }
            assert!(
                installed.index().entry.ends_with(".model3.json"),
                "sample {} entry moved out of the package root",
                archive.display()
            );
        }
        assert_eq!(
            store.list().expect("catalog").entries.len(),
            archives.len(),
            "every sample must be installed and listed"
        );
    }
}
