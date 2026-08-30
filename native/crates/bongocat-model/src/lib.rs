#![forbid(unsafe_code)]

mod store;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub use store::{
    ModelCatalogEntry, ModelStore, ModelStoreDiagnostic, ModelStoreError, ModelStoreRecovery,
};

pub const INDEX_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelPackageLimits {
    pub maximum_texture_dimension: u32,
    pub maximum_json_bytes: u64,
    pub maximum_json_depth: usize,
    pub maximum_file_bytes: u64,
    pub maximum_package_bytes: u64,
    pub maximum_file_count: usize,
    pub maximum_directory_depth: usize,
}

impl Default for ModelPackageLimits {
    fn default() -> Self {
        Self {
            maximum_texture_dimension: 8_192,
            maximum_json_bytes: 16 * 1024 * 1024,
            maximum_json_depth: 64,
            maximum_file_bytes: 512 * 1024 * 1024,
            maximum_package_bytes: 1024 * 1024 * 1024,
            maximum_file_count: 4_096,
            maximum_directory_depth: 32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && !value.starts_with('.')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid || matches!(value.as_str(), "." | "..") {
            return Err(ModelError::new(
                ModelDiagnostic::InvalidModelId,
                None,
                "model id must be 1-64 ASCII letters, digits, dots, dashes, or underscores and cannot start with a dot",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiagnostic {
    InvalidModelId,
    ModelEntryAmbiguous,
    ModelEntryMissing,
    ModelFileCountExceeded,
    ModelFileTooLarge,
    ModelIoError,
    ModelJsonInvalid,
    ModelJsonTooLarge,
    ModelMocMissing,
    ModelPackageDepthExceeded,
    ModelPackageSizeExceeded,
    ModelReferenceEscapesRoot,
    ModelReferenceInvalid,
    ModelReferenceSymlinkEscape,
    ModelResourceInvalid,
    ModelResourceMissing,
    ModelResourceNotFile,
    ModelSymlinkDirectoryUnsupported,
    ModelTextureDimensionExceeded,
    ModelTextureInvalidPng,
    ModelTextureMissing,
    ModelUnsupportedVersion,
}

impl ModelDiagnostic {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidModelId => "invalid_model_id",
            Self::ModelEntryAmbiguous => "model_entry_ambiguous",
            Self::ModelEntryMissing => "model_entry_missing",
            Self::ModelFileCountExceeded => "model_file_count_exceeded",
            Self::ModelFileTooLarge => "model_file_too_large",
            Self::ModelIoError => "model_io_error",
            Self::ModelJsonInvalid => "model_json_invalid",
            Self::ModelJsonTooLarge => "model_json_too_large",
            Self::ModelMocMissing => "model_moc_missing",
            Self::ModelPackageDepthExceeded => "model_package_depth_exceeded",
            Self::ModelPackageSizeExceeded => "model_package_size_exceeded",
            Self::ModelReferenceEscapesRoot => "model_reference_escapes_root",
            Self::ModelReferenceInvalid => "model_reference_invalid",
            Self::ModelReferenceSymlinkEscape => "model_reference_symlink_escape",
            Self::ModelResourceInvalid => "model_resource_invalid",
            Self::ModelResourceMissing => "model_resource_missing",
            Self::ModelResourceNotFile => "model_resource_not_file",
            Self::ModelSymlinkDirectoryUnsupported => "model_symlink_directory_unsupported",
            Self::ModelTextureDimensionExceeded => "model_texture_dimension_exceeded",
            Self::ModelTextureInvalidPng => "model_texture_invalid_png",
            Self::ModelTextureMissing => "model_texture_missing",
            Self::ModelUnsupportedVersion => "model_unsupported_version",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelError {
    pub code: ModelDiagnostic,
    pub resource: Option<String>,
    pub detail: String,
}

impl ModelError {
    fn new(code: ModelDiagnostic, resource: Option<&str>, detail: impl Into<String>) -> Self {
        Self {
            code,
            resource: resource.map(str::to_owned),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.resource {
            Some(resource) => write!(
                formatter,
                "{} ({resource}): {}",
                self.code.as_str(),
                self.detail
            ),
            None => write!(formatter, "{}: {}", self.code.as_str(), self.detail),
        }
    }
}

impl std::error::Error for ModelError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelPackageIndex {
    pub schema_version: u32,
    pub model_version: u32,
    pub entry: String,
    pub moc: String,
    pub textures: Vec<ImageResource>,
    pub display_info: Option<String>,
    pub expressions: Vec<NamedResource>,
    pub motion_groups: Vec<MotionGroup>,
    pub groups: Vec<ModelGroup>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub user_data: Option<String>,
    pub package_file_count: usize,
    pub package_total_bytes: u64,
    pub unreferenced_files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImageResource {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NamedResource {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MotionGroup {
    pub name: String,
    pub motions: Vec<MotionResource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MotionResource {
    pub file: String,
    pub sound: Option<String>,
    pub fade_in_seconds: Option<FiniteSeconds>,
    pub fade_out_seconds: Option<FiniteSeconds>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelGroup {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FiniteSeconds(f32);

impl Eq for FiniteSeconds {}

impl FiniteSeconds {
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedModel {
    id: ModelId,
    canonical_root: PathBuf,
    index: ModelPackageIndex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledModel {
    prepared: PreparedModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    Preset,
    Installed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedModel {
    prepared: PreparedModel,
    origin: ModelOrigin,
}

#[derive(Clone, Debug)]
pub struct PresetModelCatalog {
    root: PathBuf,
    limits: ModelPackageLimits,
}

impl InstalledModel {
    pub(crate) fn from_prepared(prepared: PreparedModel) -> Self {
        Self { prepared }
    }

    pub fn id(&self) -> &ModelId {
        self.prepared.id()
    }

    pub fn root(&self) -> &Path {
        self.prepared.root()
    }

    pub fn index(&self) -> &ModelPackageIndex {
        self.prepared.index()
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        self.prepared.snapshot()
    }
}

impl From<InstalledModel> for CommittedModel {
    fn from(installed: InstalledModel) -> Self {
        Self {
            prepared: installed.prepared,
            origin: ModelOrigin::Installed,
        }
    }
}

impl CommittedModel {
    pub fn origin(&self) -> ModelOrigin {
        self.origin
    }

    pub fn id(&self) -> &ModelId {
        self.prepared.id()
    }

    pub fn root(&self) -> &Path {
        self.prepared.root()
    }

    pub fn index(&self) -> &ModelPackageIndex {
        self.prepared.index()
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        self.prepared.snapshot()
    }
}

impl PresetModelCatalog {
    pub fn open(root: impl AsRef<Path>, limits: ModelPackageLimits) -> Result<Self, ModelError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog root cannot be opened: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelSymlinkDirectoryUnsupported,
                None,
                "preset catalog root must be a real directory",
            ));
        }
        let root = root.canonicalize().map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog root cannot be resolved: {error}"),
            )
        })?;
        Ok(Self { root, limits })
    }

    pub fn load(&self, id: &ModelId) -> Result<CommittedModel, ModelError> {
        let candidate = self.root.join(id.as_str());
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                Some(id.as_str()),
                format!("preset model cannot be opened: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelSymlinkDirectoryUnsupported,
                Some(id.as_str()),
                "preset model must be a real directory",
            ));
        }
        let prepared = PreparedModel::prepare(id.clone(), candidate, self.limits)?;
        if prepared.root().parent() != Some(self.root.as_path()) {
            return Err(ModelError::new(
                ModelDiagnostic::ModelReferenceEscapesRoot,
                Some(id.as_str()),
                "preset model resolves outside the catalog root",
            ));
        }
        Ok(CommittedModel {
            prepared,
            origin: ModelOrigin::Preset,
        })
    }

    pub fn list(&self) -> Result<Vec<ModelCatalogEntry>, ModelError> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog cannot be listed: {error}"),
            )
        })? {
            let entry = entry.map_err(|error| {
                ModelError::new(
                    ModelDiagnostic::ModelIoError,
                    None,
                    format!("preset catalog entry cannot be read: {error}"),
                )
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                ModelError::new(
                    ModelDiagnostic::InvalidModelId,
                    None,
                    "preset catalog contains a non-UTF-8 entry",
                )
            })?;
            let id = ModelId::parse(name)?;
            let catalog_entry = match self.load(&id) {
                Ok(model) => ModelCatalogEntry::Ready {
                    origin: ModelOrigin::Preset,
                    snapshot: model.snapshot(),
                },
                Err(error) => ModelCatalogEntry::Invalid {
                    origin: ModelOrigin::Preset,
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

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl PreparedModel {
    pub fn prepare(
        id: ModelId,
        root: impl AsRef<Path>,
        limits: ModelPackageLimits,
    ) -> Result<Self, ModelError> {
        let (canonical_root, index) = inspect_model_package(root, limits)?;
        Ok(Self {
            id,
            canonical_root,
            index,
        })
    }

    pub fn id(&self) -> &ModelId {
        &self.id
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn index(&self) -> &ModelPackageIndex {
        &self.index
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        ModelSnapshot {
            id: self.id.clone(),
            entry: self.index.entry.clone(),
            texture_count: self.index.textures.len(),
            expression_count: self.index.expressions.len(),
            motion_count: self
                .index
                .motion_groups
                .iter()
                .map(|group| group.motions.len())
                .sum(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSnapshot {
    pub id: ModelId,
    pub entry: String,
    pub texture_count: usize,
    pub expression_count: usize,
    pub motion_count: usize,
}

#[derive(Debug, Deserialize)]
struct ModelDefinition {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "FileReferences")]
    files: FileReferences,
    #[serde(rename = "Groups", default)]
    groups: Vec<RawModelGroup>,
}

#[derive(Debug, Deserialize)]
struct FileReferences {
    #[serde(rename = "Moc")]
    moc: String,
    #[serde(rename = "Textures", default)]
    textures: Vec<String>,
    #[serde(rename = "DisplayInfo", default)]
    display_info: Option<String>,
    #[serde(rename = "Expressions", default)]
    expressions: Vec<RawNamedResource>,
    #[serde(rename = "Motions", default)]
    motions: BTreeMap<String, Vec<RawMotionResource>>,
    #[serde(rename = "Physics", default)]
    physics: Option<String>,
    #[serde(rename = "Pose", default)]
    pose: Option<String>,
    #[serde(rename = "UserData", default)]
    user_data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNamedResource {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "File")]
    file: String,
}

#[derive(Debug, Deserialize)]
struct RawMotionResource {
    #[serde(rename = "File")]
    file: String,
    #[serde(rename = "Sound", default)]
    sound: Option<String>,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct RawModelGroup {
    #[serde(rename = "Target")]
    target: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Ids", default)]
    ids: Vec<String>,
}

struct PackageReader {
    canonical_root: PathBuf,
    limits: ModelPackageLimits,
    referenced_files: BTreeSet<String>,
}

impl PackageReader {
    fn new(root: &Path, limits: ModelPackageLimits) -> Result<Self, ModelError> {
        let canonical_root = root.canonicalize().map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("package root cannot be opened: {error}"),
            )
        })?;
        if !canonical_root.is_dir() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                "package root is not a directory",
            ));
        }
        Ok(Self {
            canonical_root,
            limits,
            referenced_files: BTreeSet::new(),
        })
    }

    fn resolve_file(
        &mut self,
        reference: &str,
        missing: ModelDiagnostic,
    ) -> Result<(String, PathBuf), ModelError> {
        let normalized = normalize_reference(reference)?;
        let candidate = self.canonical_root.join(path_from_reference(&normalized));
        let canonical = candidate
            .canonicalize()
            .map_err(|_| ModelError::new(missing, Some(&normalized), "resource does not exist"))?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(ModelError::new(
                ModelDiagnostic::ModelReferenceSymlinkEscape,
                Some(&normalized),
                "resource resolves outside the package root",
            ));
        }
        let metadata = canonical.metadata().map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                Some(&normalized),
                format!("resource metadata cannot be read: {error}"),
            )
        })?;
        if !metadata.is_file() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelResourceNotFile,
                Some(&normalized),
                "resource is not a regular file",
            ));
        }
        if metadata.len() > self.limits.maximum_file_bytes {
            return Err(ModelError::new(
                ModelDiagnostic::ModelFileTooLarge,
                Some(&normalized),
                format!("resource is {} bytes", metadata.len()),
            ));
        }
        self.referenced_files.insert(normalized.clone());
        Ok((normalized, canonical))
    }

    fn resolve_json(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_json_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    fn resolve_image(&mut self, reference: &str) -> Result<ImageResource, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelTextureMissing)?;
        let (width, height) = read_png_dimensions(&path, &normalized)?;
        if width > self.limits.maximum_texture_dimension
            || height > self.limits.maximum_texture_dimension
        {
            return Err(ModelError::new(
                ModelDiagnostic::ModelTextureDimensionExceeded,
                Some(&normalized),
                format!(
                    "texture is {width}x{height}; maximum side is {}",
                    self.limits.maximum_texture_dimension
                ),
            ));
        }
        Ok(ImageResource {
            file: normalized,
            width,
            height,
        })
    }

    fn inventory(&self) -> Result<PackageInventory, ModelError> {
        let mut files = Vec::new();
        collect_package_files(
            &self.canonical_root,
            &self.canonical_root,
            0,
            self.limits,
            &mut files,
        )?;
        files.sort_by(|left, right| left.0.cmp(&right.0));
        let total_bytes = files.iter().try_fold(0_u64, |total, (_, size)| {
            total.checked_add(*size).ok_or_else(|| {
                ModelError::new(
                    ModelDiagnostic::ModelPackageSizeExceeded,
                    None,
                    "package byte count overflowed",
                )
            })
        })?;
        if total_bytes > self.limits.maximum_package_bytes {
            return Err(ModelError::new(
                ModelDiagnostic::ModelPackageSizeExceeded,
                None,
                format!("package is {total_bytes} bytes"),
            ));
        }
        let unreferenced_files = files
            .iter()
            .map(|(path, _)| path)
            .filter(|path| !self.referenced_files.contains(*path))
            .cloned()
            .collect();
        Ok(PackageInventory {
            file_count: files.len(),
            total_bytes,
            unreferenced_files,
        })
    }
}

struct PackageInventory {
    file_count: usize,
    total_bytes: u64,
    unreferenced_files: Vec<String>,
}

fn inspect_model_package(
    root: impl AsRef<Path>,
    limits: ModelPackageLimits,
) -> Result<(PathBuf, ModelPackageIndex), ModelError> {
    let root = root.as_ref();
    let entry = discover_entry(root)?;
    let mut reader = PackageReader::new(root, limits)?;
    let entry_name = entry
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ModelError::new(
                ModelDiagnostic::ModelReferenceInvalid,
                None,
                "model entry filename is not valid UTF-8",
            )
        })?;
    let (entry_name, entry_path) =
        reader.resolve_file(entry_name, ModelDiagnostic::ModelResourceMissing)?;
    let model: ModelDefinition = read_json(
        &entry_path,
        &entry_name,
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
        ModelDiagnostic::ModelJsonInvalid,
    )?;
    if model.version != 3 {
        return Err(ModelError::new(
            ModelDiagnostic::ModelUnsupportedVersion,
            Some(&entry_name),
            format!("model3 version {} is not supported", model.version),
        ));
    }

    let (moc, _) = reader.resolve_file(&model.files.moc, ModelDiagnostic::ModelMocMissing)?;
    let textures = model
        .files
        .textures
        .iter()
        .map(|reference| reader.resolve_image(reference))
        .collect::<Result<Vec<_>, _>>()?;
    let display_info = model
        .files
        .display_info
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let expressions = model
        .files
        .expressions
        .into_iter()
        .map(|resource| {
            require_identifier(&resource.name, "expression name", &entry_name)?;
            let file = reader.resolve_json(&resource.file)?;
            Ok(NamedResource {
                name: resource.name,
                file,
            })
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    let motion_groups = model
        .files
        .motions
        .into_iter()
        .map(|(name, motions)| {
            require_identifier(&name, "motion group name", &entry_name)?;
            let motions = motions
                .into_iter()
                .map(|motion| {
                    let file = reader.resolve_json(&motion.file)?;
                    let sound = motion
                        .sound
                        .as_deref()
                        .map(|reference| {
                            reader
                                .resolve_file(reference, ModelDiagnostic::ModelResourceMissing)
                                .map(|(normalized, _)| normalized)
                        })
                        .transpose()?;
                    for (label, value) in [
                        ("FadeInTime", motion.fade_in_seconds),
                        ("FadeOutTime", motion.fade_out_seconds),
                    ] {
                        if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
                            return Err(ModelError::new(
                                ModelDiagnostic::ModelJsonInvalid,
                                Some(&file),
                                format!("motion {label} must be finite and non-negative"),
                            ));
                        }
                    }
                    Ok(MotionResource {
                        file,
                        sound,
                        fade_in_seconds: motion.fade_in_seconds.map(FiniteSeconds),
                        fade_out_seconds: motion.fade_out_seconds.map(FiniteSeconds),
                    })
                })
                .collect::<Result<Vec<_>, ModelError>>()?;
            Ok(MotionGroup { name, motions })
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    let groups = model
        .groups
        .into_iter()
        .map(|group| {
            require_identifier(&group.target, "group target", &entry_name)?;
            require_identifier(&group.name, "group name", &entry_name)?;
            for id in &group.ids {
                require_identifier(id, "group parameter id", &entry_name)?;
            }
            Ok(ModelGroup {
                target: group.target,
                name: group.name,
                ids: group.ids,
            })
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    let physics = model
        .files
        .physics
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let pose = model
        .files
        .pose
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let user_data = model
        .files
        .user_data
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let inventory = reader.inventory()?;

    Ok((
        reader.canonical_root.clone(),
        ModelPackageIndex {
            schema_version: INDEX_SCHEMA_VERSION,
            model_version: model.version,
            entry: entry_name,
            moc,
            textures,
            display_info,
            expressions,
            motion_groups,
            groups,
            physics,
            pose,
            user_data,
            package_file_count: inventory.file_count,
            package_total_bytes: inventory.total_bytes,
            unreferenced_files: inventory.unreferenced_files,
        },
    ))
}

fn discover_entry(root: &Path) -> Result<PathBuf, ModelError> {
    let entries = fs::read_dir(root).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            None,
            format!("package root cannot be listed: {error}"),
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("package entry cannot be read: {error}"),
            )
        })?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(".model3.json"))
        {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    match candidates.len() {
        0 => Err(ModelError::new(
            ModelDiagnostic::ModelEntryMissing,
            None,
            "package root has no .model3.json entry",
        )),
        1 => Ok(candidates.remove(0)),
        count => Err(ModelError::new(
            ModelDiagnostic::ModelEntryAmbiguous,
            None,
            format!("package root has {count} .model3.json entries"),
        )),
    }
}

fn normalize_reference(reference: &str) -> Result<String, ModelError> {
    let normalized = reference.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || normalized.contains('\0')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ModelError::new(
            ModelDiagnostic::ModelReferenceEscapesRoot,
            Some(reference),
            "resource path is absolute, empty, or traverses outside the package",
        ));
    }
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| {
            if part.contains(':') {
                Err(ModelError::new(
                    ModelDiagnostic::ModelReferenceEscapesRoot,
                    Some(reference),
                    "resource path contains a platform path prefix",
                ))
            } else {
                Ok(part)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err(ModelError::new(
            ModelDiagnostic::ModelReferenceInvalid,
            Some(reference),
            "resource path does not name a file",
        ));
    }
    Ok(parts.join("/"))
}

fn path_from_reference(reference: &str) -> PathBuf {
    reference.split('/').collect()
}

fn read_json<T: DeserializeOwned>(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
) -> Result<T, ModelError> {
    let bytes = read_bounded(path, reference, maximum_bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        ModelError::new(
            diagnostic,
            Some(reference),
            format!("invalid JSON: {error}"),
        )
    })?;
    validate_json_depth(&value, maximum_depth, diagnostic, reference)?;
    serde_json::from_value(value).map_err(|error| {
        ModelError::new(
            diagnostic,
            Some(reference),
            format!("invalid JSON structure: {error}"),
        )
    })
}

fn validate_json_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let value: serde_json::Value = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if !value.is_object() {
        return Err(ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "resource JSON root must be an object",
        ));
    }
    Ok(())
}

fn validate_json_depth(
    value: &serde_json::Value,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
    reference: &str,
) -> Result<(), ModelError> {
    fn visit(value: &serde_json::Value, depth: usize, maximum_depth: usize) -> bool {
        if depth > maximum_depth {
            return false;
        }
        match value {
            serde_json::Value::Array(values) => values
                .iter()
                .all(|value| visit(value, depth + 1, maximum_depth)),
            serde_json::Value::Object(values) => values
                .values()
                .all(|value| visit(value, depth + 1, maximum_depth)),
            _ => true,
        }
    }

    if maximum_depth == 0 || !visit(value, 1, maximum_depth) {
        return Err(ModelError::new(
            diagnostic,
            Some(reference),
            format!("JSON nesting exceeds {maximum_depth} levels"),
        ));
    }
    Ok(())
}

fn read_bounded(path: &Path, reference: &str, maximum_bytes: u64) -> Result<Vec<u8>, ModelError> {
    let metadata = path.metadata().map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("resource metadata cannot be read: {error}"),
        )
    })?;
    if metadata.len() > maximum_bytes {
        return Err(ModelError::new(
            ModelDiagnostic::ModelJsonTooLarge,
            Some(reference),
            format!(
                "resource is {} bytes; limit is {maximum_bytes}",
                metadata.len()
            ),
        ));
    }
    fs::read(path).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("resource cannot be read: {error}"),
        )
    })
}

fn read_png_dimensions(path: &Path, reference: &str) -> Result<(u32, u32), ModelError> {
    let mut header = [0_u8; 24];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelTextureInvalidPng,
                Some(reference),
                format!("PNG header cannot be read: {error}"),
            )
        })?;
    if header[..8] != *b"\x89PNG\r\n\x1a\n" || header[12..16] != *b"IHDR" {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureInvalidPng,
            Some(reference),
            "texture does not have a PNG IHDR header",
        ));
    }
    let width = u32::from_be_bytes(header[16..20].try_into().expect("fixed PNG width slice"));
    let height = u32::from_be_bytes(header[20..24].try_into().expect("fixed PNG height slice"));
    if width == 0 || height == 0 {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureInvalidPng,
            Some(reference),
            "texture dimensions must be non-zero",
        ));
    }
    Ok((width, height))
}

fn require_identifier(value: &str, label: &str, resource: &str) -> Result<(), ModelError> {
    if value.trim().is_empty() {
        return Err(ModelError::new(
            ModelDiagnostic::ModelJsonInvalid,
            Some(resource),
            format!("{label} must not be blank"),
        ));
    }
    Ok(())
}

fn collect_package_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    limits: ModelPackageLimits,
    files: &mut Vec<(String, u64)>,
) -> Result<(), ModelError> {
    if depth > limits.maximum_directory_depth {
        return Err(ModelError::new(
            ModelDiagnostic::ModelPackageDepthExceeded,
            None,
            format!(
                "package directory depth exceeds {}",
                limits.maximum_directory_depth
            ),
        ));
    }
    for entry in fs::read_dir(directory).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            None,
            format!("package directory cannot be listed: {error}"),
        )
    })? {
        let entry = entry.map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("package directory entry cannot be read: {error}"),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("package entry type cannot be read: {error}"),
            )
        })?;
        let reference = relative_reference(root, &path)?;
        if file_type.is_symlink() {
            let canonical = path.canonicalize().map_err(|error| {
                ModelError::new(
                    ModelDiagnostic::ModelIoError,
                    Some(&reference),
                    format!("package symlink cannot be resolved: {error}"),
                )
            })?;
            if !canonical.starts_with(root) {
                return Err(ModelError::new(
                    ModelDiagnostic::ModelReferenceSymlinkEscape,
                    Some(&reference),
                    "package symlink resolves outside the package root",
                ));
            }
            if canonical.is_dir() {
                return Err(ModelError::new(
                    ModelDiagnostic::ModelSymlinkDirectoryUnsupported,
                    Some(&reference),
                    "symlinked directories are not supported",
                ));
            }
        }
        if path.is_dir() {
            collect_package_files(root, &path, depth + 1, limits, files)?;
        } else if path.is_file() {
            let size = path
                .metadata()
                .map_err(|error| {
                    ModelError::new(
                        ModelDiagnostic::ModelIoError,
                        Some(&reference),
                        format!("package file metadata cannot be read: {error}"),
                    )
                })?
                .len();
            if size > limits.maximum_file_bytes {
                return Err(ModelError::new(
                    ModelDiagnostic::ModelFileTooLarge,
                    Some(&reference),
                    format!("resource is {size} bytes"),
                ));
            }
            files.push((reference, size));
            if files.len() > limits.maximum_file_count {
                return Err(ModelError::new(
                    ModelDiagnostic::ModelFileCountExceeded,
                    None,
                    format!("package has more than {} files", limits.maximum_file_count),
                ));
            }
        } else {
            return Err(ModelError::new(
                ModelDiagnostic::ModelResourceNotFile,
                Some(&reference),
                "package entry is not a regular file or directory",
            ));
        }
    }
    Ok(())
}

fn relative_reference(root: &Path, path: &Path) -> Result<String, ModelError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelReferenceSymlinkEscape,
            None,
            "package entry is outside the package root",
        )
    })?;
    relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| {
                    ModelError::new(
                        ModelDiagnostic::ModelReferenceInvalid,
                        None,
                        "package path is not valid UTF-8",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn prepares_all_three_preset_packages() {
        for mode in ["standard", "keyboard", "gamepad"] {
            let root = repository_root().join("src-tauri/assets/models").join(mode);
            let prepared = PreparedModel::prepare(
                ModelId::parse(mode).expect("model id"),
                &root,
                ModelPackageLimits::default(),
            )
            .expect("prepare preset package");
            assert_eq!(prepared.index().model_version, 3);
            assert_eq!(prepared.index().schema_version, INDEX_SCHEMA_VERSION);
            assert!(!prepared.index().moc.is_empty());
            assert!(!prepared.index().textures.is_empty());
            let eye_blink = prepared
                .index()
                .groups
                .iter()
                .find(|group| group.target == "Parameter" && group.name == "EyeBlink")
                .expect("EyeBlink parameter group");
            assert_eq!(
                eye_blink.ids,
                ["ParamEyeLOpen".to_owned(), "ParamEyeROpen".to_owned()]
            );
            assert_eq!(
                prepared.root(),
                root.canonicalize().expect("canonical root")
            );
        }
    }

    #[test]
    fn fixture_contract_rejects_missing_ambiguous_and_escaping_packages() {
        for (name, expected) in [
            ("missing-moc", ModelDiagnostic::ModelMocMissing),
            ("multiple-model3", ModelDiagnostic::ModelEntryAmbiguous),
            ("path-traversal", ModelDiagnostic::ModelReferenceEscapesRoot),
        ] {
            let error = PreparedModel::prepare(
                ModelId::parse("fixture").expect("model id"),
                fixture(name),
                ModelPackageLimits::default(),
            )
            .expect_err("fixture must be rejected");
            assert_eq!(error.code, expected, "fixture {name}");
        }
    }

    #[test]
    fn malformed_model3_is_rejected_before_resource_resolution() {
        let source = fixture("malformed-model3-json");
        let package = tempdir().expect("temporary package");
        fs::copy(
            source.join("cat.model3.json.invalid"),
            package.path().join("cat.model3.json"),
        )
        .expect("materialize malformed entry");

        let error = PreparedModel::prepare(
            ModelId::parse("malformed").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect_err("malformed model3 must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
    }

    #[test]
    fn accepts_non_ascii_resource_paths() {
        let prepared = PreparedModel::prepare(
            ModelId::parse("unicode").expect("model id"),
            fixture("非 ASCII 模型"),
            ModelPackageLimits::default(),
        )
        .expect("non-ASCII package");
        assert_eq!(prepared.index().moc, "模型 数据.moc3");
    }

    #[test]
    fn model_groups_are_retained_and_blank_identifiers_are_rejected() {
        let package = tempdir().expect("package");
        fs::write(package.path().join("model.moc3"), b"moc").expect("moc");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
              "Version":3,
              "FileReferences":{"Moc":"model.moc3","Textures":[]},
              "Groups":[
                {"Target":"Parameter","Name":"LipSync","Ids":["ParamMouthOpenY"]},
                {"Target":"FutureTarget","Name":"Metadata","Ids":[]}
              ]
            }"#,
        )
        .expect("model3");
        let prepared = PreparedModel::prepare(
            ModelId::parse("groups").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect("grouped model");
        assert_eq!(
            prepared.index().groups,
            [
                ModelGroup {
                    target: "Parameter".to_owned(),
                    name: "LipSync".to_owned(),
                    ids: vec!["ParamMouthOpenY".to_owned()],
                },
                ModelGroup {
                    target: "FutureTarget".to_owned(),
                    name: "Metadata".to_owned(),
                    ids: vec![],
                },
            ]
        );

        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
              "Version":3,
              "FileReferences":{"Moc":"model.moc3","Textures":[]},
              "Groups":[{"Target":"Parameter","Name":"LipSync","Ids":[" "]}]
            }"#,
        )
        .expect("invalid model3");
        let error = PreparedModel::prepare(
            ModelId::parse("groups").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect_err("blank group parameter id");
        assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
        assert!(error.detail.contains("group parameter id"));
    }

    #[test]
    fn preset_catalog_is_the_only_path_from_bundled_resources_to_committed_models() {
        let catalog = PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog");
        for id in ["standard", "keyboard", "gamepad"] {
            let model = catalog
                .load(&ModelId::parse(id).expect("model id"))
                .expect("committed preset");
            assert_eq!(model.origin(), ModelOrigin::Preset);
            assert_eq!(model.id().as_str(), id);
            assert_eq!(model.root().parent(), Some(catalog.root()));
            assert!(!model.index().textures.is_empty());
        }
    }

    #[test]
    fn preset_catalog_is_sorted_and_retains_invalid_entries() {
        let root = tempdir().expect("preset catalog");
        for id in ["zeta", "alpha"] {
            let model = root.path().join(id);
            fs::create_dir(&model).expect("preset directory");
            fs::write(model.join("model.moc3"), b"moc").expect("preset moc");
            fs::write(
                model.join("cat.model3.json"),
                r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
            )
            .expect("preset model3");
        }
        fs::remove_file(root.path().join("alpha/model.moc3")).expect("corrupt alpha preset");

        let catalog = PresetModelCatalog::open(root.path(), ModelPackageLimits::default())
            .expect("open preset catalog")
            .list()
            .expect("list preset catalog");
        assert_eq!(
            catalog
                .iter()
                .map(|entry| entry.id().as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(catalog[0].origin(), ModelOrigin::Preset);
        assert!(matches!(
            catalog[0],
            ModelCatalogEntry::Invalid {
                code: ModelDiagnostic::ModelMocMissing,
                ..
            }
        ));
        assert!(catalog[0].snapshot().is_none());
        assert_eq!(catalog[1].origin(), ModelOrigin::Preset);
        assert!(catalog[1].snapshot().is_some());
    }

    #[test]
    fn rejects_declared_texture_dimensions_before_decode() {
        let source = fixture("oversized-texture");
        let package = tempdir().expect("temporary package");
        fs::create_dir(package.path().join("textures")).expect("texture directory");
        for file in ["cat.model3.json", "placeholder.moc3"] {
            fs::copy(source.join(file), package.path().join(file)).expect("copy fixture file");
        }
        let hex =
            fs::read_to_string(source.join("textures/huge.png.hex")).expect("read encoded texture");
        let bytes = hex
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                    .expect("hex byte")
            })
            .collect::<Vec<_>>();
        fs::write(package.path().join("textures/huge.png"), bytes).expect("write texture");

        let error = PreparedModel::prepare(
            ModelId::parse("oversized").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect_err("oversized texture must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelTextureDimensionExceeded);
    }

    #[test]
    fn model_ids_are_portable_store_keys() {
        assert!(ModelId::parse("keyboard-v2_1").is_ok());
        for invalid in ["", "..", ".hidden", "cat/model", "猫", "model id"] {
            assert_eq!(
                ModelId::parse(invalid).expect_err("invalid id").code,
                ModelDiagnostic::InvalidModelId
            );
        }
    }

    #[test]
    fn absolute_and_platform_prefixed_references_are_rejected() {
        for reference in ["/tmp/model.moc3", r"C:\models\model.moc3", "../model.moc3"] {
            assert_eq!(
                normalize_reference(reference)
                    .expect_err("escaping reference")
                    .code,
                ModelDiagnostic::ModelReferenceEscapesRoot
            );
        }
    }

    #[test]
    fn json_nesting_limit_is_applied_before_typed_deserialization() {
        let package = tempdir().expect("package");
        let nesting = "[".repeat(65) + &"]".repeat(65);
        fs::write(
            package.path().join("cat.model3.json"),
            format!(
                r#"{{"Version":3,"FileReferences":{{"Moc":"model.moc3","Textures":[]}},"Nested":{nesting}}}"#
            ),
        )
        .expect("model3");
        fs::write(package.path().join("model.moc3"), b"moc").expect("moc");

        let error = PreparedModel::prepare(
            ModelId::parse("deep").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect_err("deep JSON must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
        assert!(error.detail.contains("nesting"));
    }

    #[cfg(unix)]
    #[test]
    fn referenced_symlink_cannot_escape_package_root() {
        use std::os::unix::fs::symlink;

        let package = tempdir().expect("package");
        let outside = tempdir().expect("outside");
        fs::write(outside.path().join("model.moc3"), b"moc").expect("outside moc");
        symlink(
            outside.path().join("model.moc3"),
            package.path().join("model.moc3"),
        )
        .expect("symlink");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");

        let error = PreparedModel::prepare(
            ModelId::parse("escape").expect("model id"),
            package.path(),
            ModelPackageLimits::default(),
        )
        .expect_err("escaping symlink");
        assert_eq!(error.code, ModelDiagnostic::ModelReferenceSymlinkEscape);
    }

    #[cfg(unix)]
    #[test]
    fn preset_catalog_rejects_a_symlinked_model_entry() {
        use std::os::unix::fs::symlink;

        let catalog_root = tempdir().expect("catalog");
        symlink(
            repository_root().join("native/resources/models/standard"),
            catalog_root.path().join("standard"),
        )
        .expect("symlink preset");
        let catalog = PresetModelCatalog::open(catalog_root.path(), ModelPackageLimits::default())
            .expect("catalog root");
        let error = catalog
            .load(&ModelId::parse("standard").expect("model id"))
            .expect_err("symlinked preset must fail");
        assert_eq!(
            error.code,
            ModelDiagnostic::ModelSymlinkDirectoryUnsupported
        );
    }
}
