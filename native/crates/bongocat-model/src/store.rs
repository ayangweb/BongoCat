use crate::{ModelError, ModelId, ModelPackageLimits, PreparedModel};
use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelImportDiagnostic {
    AlreadyExists,
    InvalidPackage,
    IoError,
    SourceChanged,
    SourceSymlinkUnsupported,
    SourceEntryUnsupported,
}

#[derive(Debug)]
pub struct ModelImportError {
    pub code: ModelImportDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
    source: Option<ModelError>,
}

impl ModelImportError {
    fn new(
        code: ModelImportDiagnostic,
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
            code: ModelImportDiagnostic::InvalidPackage,
            resource: error.resource.clone(),
            detail: error.to_string(),
            source: Some(error),
        }
    }
}

impl fmt::Display for ModelImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.resource {
            Some(resource) => write!(formatter, "{:?} ({resource}): {}", self.code, self.detail),
            None => write!(formatter, "{:?}: {}", self.code, self.detail),
        }
    }
}

impl std::error::Error for ModelImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

pub struct ModelStore {
    canonical_root: PathBuf,
    limits: ModelPackageLimits,
}

impl ModelStore {
    pub fn new(
        root: impl AsRef<Path>,
        limits: ModelPackageLimits,
    ) -> Result<Self, ModelImportError> {
        fs::create_dir_all(root.as_ref()).map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                None,
                format!("model store cannot be created: {error}"),
            )
        })?;
        let canonical_root = root.as_ref().canonicalize().map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                None,
                format!("model store cannot be opened: {error}"),
            )
        })?;
        if !canonical_root.is_dir() {
            return Err(ModelImportError::new(
                ModelImportDiagnostic::IoError,
                None,
                "model store is not a directory",
            ));
        }
        Ok(Self {
            canonical_root,
            limits,
        })
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn import(
        &self,
        id: ModelId,
        source_root: impl AsRef<Path>,
    ) -> Result<PreparedModel, ModelImportError> {
        let prepared_source = PreparedModel::prepare(id.clone(), source_root, self.limits)
            .map_err(ModelImportError::package)?;
        let destination = self.canonical_root.join(id.as_str());
        if destination.exists() {
            return Err(ModelImportError::new(
                ModelImportDiagnostic::AlreadyExists,
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
            .map_err(ModelImportError::package)?;

        fs::rename(&staging, &destination).map_err(|error| {
            let (code, detail) = if destination.exists() {
                (
                    ModelImportDiagnostic::AlreadyExists,
                    "a model with this id was installed concurrently".to_owned(),
                )
            } else {
                (
                    ModelImportDiagnostic::IoError,
                    format!("staged model cannot be committed: {error}"),
                )
            };
            ModelImportError::new(code, Some(id.as_str().to_owned()), detail)
        })?;
        cleanup.disarm();
        prepared_staging.canonical_root = destination;
        Ok(prepared_staging)
    }

    fn create_staging_directory(&self, id: &ModelId) -> Result<PathBuf, ModelImportError> {
        for _ in 0..128 {
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                ".importing-{}-{}-{sequence}",
                id.as_str(),
                std::process::id()
            );
            let path = self.canonical_root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(ModelImportError::new(
                        ModelImportDiagnostic::IoError,
                        None,
                        format!("model staging directory cannot be created: {error}"),
                    ));
                }
            }
        }
        Err(ModelImportError::new(
            ModelImportDiagnostic::IoError,
            None,
            "no unique model staging directory was available",
        ))
    }
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
) -> Result<(), ModelImportError> {
    if depth > limits.maximum_directory_depth {
        return Err(ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            None,
            "source directory depth changed after validation",
        ));
    }
    for entry in fs::read_dir(source_directory).map_err(|error| {
        ModelImportError::new(
            ModelImportDiagnostic::IoError,
            None,
            format!("source directory cannot be listed: {error}"),
        )
    })? {
        let entry = entry.map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                None,
                format!("source directory entry cannot be read: {error}"),
            )
        })?;
        let source = entry.path();
        let relative = source.strip_prefix(source_root).map_err(|_| {
            ModelImportError::new(
                ModelImportDiagnostic::SourceChanged,
                None,
                "source entry escaped the validated package",
            )
        })?;
        let resource = relative.to_str().map(str::to_owned).ok_or_else(|| {
            ModelImportError::new(
                ModelImportDiagnostic::SourceEntryUnsupported,
                None,
                "source path is not valid UTF-8",
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                Some(resource.clone()),
                format!("source entry type cannot be read: {error}"),
            )
        })?;
        if file_type.is_symlink() {
            return Err(ModelImportError::new(
                ModelImportDiagnostic::SourceSymlinkUnsupported,
                Some(resource),
                "model imports do not follow symbolic links",
            ));
        }
        let destination = destination_directory.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir(&destination).map_err(|error| {
                ModelImportError::new(
                    ModelImportDiagnostic::IoError,
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
            return Err(ModelImportError::new(
                ModelImportDiagnostic::SourceEntryUnsupported,
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
) -> Result<(), ModelImportError> {
    let canonical = source.canonicalize().map_err(|error| {
        ModelImportError::new(
            ModelImportDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be resolved: {error}"),
        )
    })?;
    if !canonical.starts_with(source_root) {
        return Err(ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source file resolved outside the validated package",
        ));
    }
    let mut input = File::open(&canonical).map_err(|error| {
        ModelImportError::new(
            ModelImportDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be opened: {error}"),
        )
    })?;
    let size = input
        .metadata()
        .map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file metadata cannot be read: {error}"),
            )
        })?
        .len();
    if size > limits.maximum_file_bytes {
        return Err(ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source file exceeded its validated size limit",
        ));
    }
    let next_file_count = statistics.file_count.saturating_add(1);
    let next_total_bytes = statistics.total_bytes.checked_add(size).ok_or_else(|| {
        ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source package byte count overflowed",
        )
    })?;
    if next_file_count > limits.maximum_file_count
        || next_total_bytes > limits.maximum_package_bytes
    {
        return Err(ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            "source package exceeded its validated limits",
        ));
    }

    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("staging file cannot be created: {error}"),
            )
        })?;
    let copied = io::copy(&mut input, &mut output).map_err(|error| {
        ModelImportError::new(
            ModelImportDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("source file cannot be copied: {error}"),
        )
    })?;
    if copied != size {
        return Err(ModelImportError::new(
            ModelImportDiagnostic::SourceChanged,
            Some(resource.to_owned()),
            format!("source size changed while copying: expected {size}, copied {copied}"),
        ));
    }
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(|error| {
            ModelImportError::new(
                ModelImportDiagnostic::IoError,
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

    #[test]
    fn imports_valid_package_without_modifying_source() {
        let data = tempdir().expect("data root");
        let store = ModelStore::new(data.path().join("models"), ModelPackageLimits::default())
            .expect("model store");
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
        let store = ModelStore::new(data.path().join("models"), ModelPackageLimits::default())
            .expect("model store");
        let id = ModelId::parse("unicode").expect("model id");
        let installed = store
            .import(id.clone(), fixture("非 ASCII 模型"))
            .expect("first import");
        fs::write(installed.root().join("user-marker"), b"keep").expect("marker");

        let error = store
            .import(id, fixture("非 ASCII 模型"))
            .expect_err("duplicate import");
        assert_eq!(error.code, ModelImportDiagnostic::AlreadyExists);
        assert_eq!(
            fs::read(installed.root().join("user-marker")).expect("marker preserved"),
            b"keep"
        );
    }

    #[test]
    fn invalid_package_leaves_no_destination_or_staging_directory() {
        let data = tempdir().expect("data root");
        let store = ModelStore::new(data.path().join("models"), ModelPackageLimits::default())
            .expect("model store");
        let error = store
            .import(
                ModelId::parse("broken").expect("model id"),
                fixture("missing-moc"),
            )
            .expect_err("invalid import");
        assert_eq!(error.code, ModelImportDiagnostic::InvalidPackage);
        assert_eq!(
            fs::read_dir(store.root()).expect("store entries").count(),
            0
        );
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
        let store = ModelStore::new(data.path().join("models"), ModelPackageLimits::default())
            .expect("model store");

        let error = store
            .import(ModelId::parse("linked").expect("model id"), source.path())
            .expect_err("symlink import");
        assert_eq!(error.code, ModelImportDiagnostic::SourceSymlinkUnsupported);
        assert_eq!(
            fs::read_dir(store.root()).expect("store entries").count(),
            0
        );
    }
}
