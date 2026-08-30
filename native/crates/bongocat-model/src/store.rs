use crate::{InstalledModel, ModelError, ModelId, ModelPackageLimits, PreparedModel};
use std::{
    fmt, fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const IMPORTING_PREFIX: &str = ".importing-";
const DELETING_PREFIX: &str = ".deleting-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStoreDiagnostic {
    AlreadyExists,
    InvalidPackage,
    IoError,
    NotFound,
    SourceContainsStore,
    SourceChanged,
    SourceSymlinkUnsupported,
    SourceEntryUnsupported,
    StoreBusy,
    StoreEntryUnsupported,
}

#[derive(Debug)]
pub struct ModelStoreError {
    pub code: ModelStoreDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
    source: Option<ModelError>,
}

impl ModelStoreError {
    fn new(
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
    Ready(crate::ModelSnapshot),
    Invalid {
        id: ModelId,
        code: crate::ModelDiagnostic,
        resource: Option<String>,
        detail: String,
    },
}

impl ModelCatalogEntry {
    pub fn id(&self) -> &ModelId {
        match self {
            Self::Ready(snapshot) => &snapshot.id,
            Self::Invalid { id, .. } => id,
        }
    }
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

    pub fn list(&self) -> Result<Vec<ModelCatalogEntry>, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let mut entries = Vec::new();
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
            let file_type = entry.file_type().map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("model store entry type cannot be read: {error}"),
                )
            })?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::StoreEntryUnsupported,
                    None,
                    "model store contains an entry not owned by the catalog",
                ));
            }
            let name = entry.file_name().into_string().map_err(|_| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::StoreEntryUnsupported,
                    None,
                    "model store contains a non-UTF-8 entry",
                )
            })?;
            let id = ModelId::parse(name).map_err(|_| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::StoreEntryUnsupported,
                    None,
                    "model store contains a directory with an invalid model id",
                )
            })?;
            let catalog_entry = match PreparedModel::prepare(id.clone(), entry.path(), self.limits)
            {
                Ok(prepared) => ModelCatalogEntry::Ready(prepared.snapshot()),
                Err(error) => ModelCatalogEntry::Invalid {
                    id,
                    code: error.code,
                    resource: error.resource,
                    detail: error.detail,
                },
            };
            entries.push(catalog_entry);
        }
        entries.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(entries)
    }

    pub fn load(&self, id: &ModelId) -> Result<InstalledModel, ModelStoreError> {
        let _lock = self.acquire_lock()?;
        let path = self.installed_path(id)?;
        PreparedModel::prepare(id.clone(), path, self.limits)
            .map(InstalledModel::from_prepared)
            .map_err(ModelStoreError::package)
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

    pub fn import(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
    ) -> Result<InstalledModel, ModelStoreError> {
        let _lock = self.acquire_lock()?;
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
        let prepared_source = PreparedModel::prepare(id.clone(), &canonical_source, self.limits)
            .map_err(ModelStoreError::package)?;
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
        copy_package(
            prepared_source.root(),
            prepared_source.root(),
            &staging,
            0,
            self.limits,
            &mut CopyStatistics::default(),
        )?;
        let mut prepared_staging = PreparedModel::prepare(id.clone(), &staging, self.limits)
            .map_err(ModelStoreError::package)?;

        if destination.exists() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::AlreadyExists,
                Some(id.as_str().to_owned()),
                "a model with this id was installed concurrently",
            ));
        }

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
        prepared_staging.canonical_root = destination;
        Ok(InstalledModel::from_prepared(prepared_staging))
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
struct CopyStatistics {
    file_count: usize,
    total_bytes: u64,
}

fn copy_package(
    source_root: &Path,
    source_directory: &Path,
    destination_directory: &Path,
    depth: usize,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
) -> Result<(), ModelStoreError> {
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
            copy_package(
                source_root,
                &source,
                &destination,
                depth + 1,
                limits,
                statistics,
            )?;
        } else if file_type.is_file() {
            copy_file(
                source_root,
                &source,
                &destination,
                &resource,
                limits,
                statistics,
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

fn copy_file(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    resource: &str,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
) -> Result<(), ModelStoreError> {
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
    let copied = io::copy(&mut input, &mut output).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be copied: {error}"),
        )
    })?;
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
    use std::fs::TryLockError;
    use std::path::Path;
    use tempfile::tempdir;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
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
        assert!(store.list().expect("empty catalog").is_empty());
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
        assert!(store.list().expect("empty catalog").is_empty());
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

        assert_eq!(
            store.list().expect_err("catalog rejects symlink").code,
            ModelStoreDiagnostic::StoreEntryUnsupported
        );
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
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].id().as_str(), "alpha");
        assert!(matches!(catalog[0], ModelCatalogEntry::Invalid { .. }));
        assert_eq!(catalog[1].id().as_str(), "zeta");
        assert!(matches!(catalog[1], ModelCatalogEntry::Ready(_)));
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
        assert_eq!(reopened.list().expect("persistent catalog").len(), 2);
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
        assert_eq!(store.list().expect("catalog").len(), 1);
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
        assert!(store.list().expect("empty catalog").is_empty());
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
}
