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
    ModelCatalogEntry, ModelImportProgress, ModelImportStage, ModelStore, ModelStoreDiagnostic,
    ModelStoreError, ModelStoreRecovery,
};

pub const INDEX_SCHEMA_VERSION: u32 = 1;
const MOTION_TIME_TOLERANCE: f32 = 0.000_001;

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
            && !value.ends_with('.')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && !is_windows_reserved_name(&value);
        if !valid {
            return Err(ModelError::new(
                ModelDiagnostic::InvalidModelId,
                None,
                "model id must be a portable 1-64 character ASCII store key",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_windows_reserved_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
        && matches!(bytes[3], b'1'..=b'9')
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
            behaviors: self
                .index
                .motion_groups
                .iter()
                .flat_map(|group| {
                    (0..group.motions.len()).map(|index| ModelBehaviorSnapshot::Motion {
                        group: group.name.clone(),
                        index,
                    })
                })
                .chain(self.index.expressions.iter().map(|expression| {
                    ModelBehaviorSnapshot::Expression {
                        name: expression.name.clone(),
                    }
                }))
                .collect(),
        }
    }
}

/// A model-declared behavior that is safe to expose to settings and shortcut
/// configuration. It contains an identifier only, never a package path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ModelBehaviorSnapshot {
    Motion { group: String, index: usize },
    Expression { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSnapshot {
    pub id: ModelId,
    pub entry: String,
    pub texture_count: usize,
    pub expression_count: usize,
    pub motion_count: usize,
    pub behaviors: Vec<ModelBehaviorSnapshot>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplayInfo {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Parameters", default)]
    parameters: Vec<RawDisplayInfoParameter>,
    #[serde(rename = "ParameterGroups", default)]
    parameter_groups: Vec<RawDisplayInfoParameterGroup>,
    #[serde(rename = "Parts", default)]
    parts: Vec<RawDisplayInfoPart>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplayInfoParameter {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "GroupId")]
    group_id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplayInfoParameterGroup {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "GroupId")]
    _group_id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplayInfoPart {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpressionResource {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f32>,
    #[serde(rename = "Parameters")]
    parameters: Vec<RawExpressionParameter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpressionParameter {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Value")]
    value: f32,
    #[serde(rename = "Blend", default)]
    blend: Option<RawExpressionBlend>,
}

#[derive(Debug, Deserialize)]
enum RawExpressionBlend {
    Add,
    Multiply,
    Overwrite,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMotionResourceFile {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Meta")]
    meta: RawMotionResourceMeta,
    #[serde(rename = "Curves")]
    curves: Vec<RawMotionResourceCurve>,
    #[serde(rename = "UserData", default)]
    user_data: Vec<RawMotionResourceUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMotionResourceMeta {
    #[serde(rename = "Duration")]
    duration: f32,
    #[serde(rename = "Fps")]
    fps: f32,
    #[serde(rename = "Loop")]
    _looping: bool,
    #[serde(rename = "AreBeziersRestricted")]
    _are_beziers_restricted: bool,
    #[serde(rename = "CurveCount")]
    curve_count: usize,
    #[serde(rename = "TotalSegmentCount")]
    _total_segment_count: usize,
    #[serde(rename = "TotalPointCount")]
    _total_point_count: usize,
    #[serde(rename = "UserDataCount")]
    user_data_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    total_user_data_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMotionResourceUserData {
    #[serde(rename = "Time")]
    time: f32,
    #[serde(rename = "Value")]
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMotionResourceCurve {
    #[serde(rename = "Target")]
    target: RawMotionResourceTarget,
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Segments")]
    segments: Vec<f32>,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoseResource {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "Groups")]
    groups: Vec<Vec<RawPosePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPosePart {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Link", default)]
    links: Vec<String>,
}

#[derive(Debug, Deserialize)]
enum RawMotionResourceTarget {
    Model,
    Parameter,
    PartOpacity,
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

    fn resolve_display_info(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_display_info_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    fn resolve_expression(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_expression_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    fn resolve_motion(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_motion_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    fn resolve_pose(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_pose_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    fn resolve_audio(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_flac_resource(&path, &normalized)?;
        Ok(normalized)
    }

    fn resolve_image(&mut self, reference: &str) -> Result<ImageResource, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelTextureMissing)?;
        let (width, height) = read_png_dimensions(&path, &normalized)?;
        validate_texture_dimensions(
            width,
            height,
            self.limits.maximum_texture_dimension,
            &normalized,
        )?;
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
        .map(|reference| reader.resolve_display_info(reference))
        .transpose()?;
    let expressions = model
        .files
        .expressions
        .into_iter()
        .map(|resource| {
            require_identifier(&resource.name, "expression name", &entry_name)?;
            let file = reader.resolve_expression(&resource.file)?;
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
                    let file = reader.resolve_motion(&motion.file)?;
                    let sound = motion
                        .sound
                        .as_deref()
                        .map(|reference| reader.resolve_audio(reference))
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
        .map(|reference| reader.resolve_pose(reference))
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
    parse_json_bytes(&bytes, reference, maximum_depth, diagnostic)
}

fn parse_json_bytes<T: DeserializeOwned>(
    bytes: &[u8],
    reference: &str,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
) -> Result<T, ModelError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
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

fn validate_display_info_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let display: RawDisplayInfo = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if display.version != 3 {
        return invalid_resource(reference, "cdi3 Version must be 3");
    }

    let parameter_groups = display
        .parameter_groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_groups.len() != display.parameter_groups.len()
        || display
            .parameter_groups
            .iter()
            .any(|group| group.id.trim().is_empty())
    {
        return invalid_resource(
            reference,
            "cdi3 ParameterGroups contain blank or duplicate Id",
        );
    }
    let parameters = display
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameters.len() != display.parameters.len()
        || display
            .parameters
            .iter()
            .any(|parameter| parameter.id.trim().is_empty())
    {
        return invalid_resource(reference, "cdi3 Parameters contain blank or duplicate Id");
    }
    if display.parameters.iter().any(|parameter| {
        !parameter.group_id.is_empty() && !parameter_groups.contains(parameter.group_id.as_str())
    }) {
        return invalid_resource(reference, "cdi3 Parameter GroupId is not declared");
    }
    let parts = display
        .parts
        .iter()
        .map(|part| part.id.as_str())
        .collect::<BTreeSet<_>>();
    if parts.len() != display.parts.len()
        || display.parts.iter().any(|part| part.id.trim().is_empty())
    {
        return invalid_resource(reference, "cdi3 Parts contain blank or duplicate Id");
    }
    Ok(())
}

fn validate_expression_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let expression: RawExpressionResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if expression.kind != "Live2D Expression" {
        return invalid_resource(reference, "exp3 Type must be Live2D Expression");
    }
    if [expression.fade_in_seconds, expression.fade_out_seconds]
        .into_iter()
        .flatten()
        .any(|seconds| !seconds.is_finite() || seconds < 0.0)
    {
        return invalid_resource(
            reference,
            "exp3 fade duration must be finite and non-negative",
        );
    }
    let parameter_ids = expression
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_ids.len() != expression.parameters.len()
        || expression
            .parameters
            .iter()
            .any(|parameter| parameter.id.trim().is_empty() || !parameter.value.is_finite())
    {
        return invalid_resource(reference, "exp3 Parameters contain an invalid Id or Value");
    }
    for parameter in expression.parameters {
        let _ = parameter.blend;
    }
    Ok(())
}

fn validate_motion_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let motion: RawMotionResourceFile = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if motion.version != 3 {
        return invalid_resource(reference, "motion3 Version must be 3");
    }
    if !motion.meta.duration.is_finite()
        || motion.meta.duration < 0.0
        || !motion.meta.fps.is_finite()
        || motion.meta.fps <= 0.0
    {
        return invalid_resource(
            reference,
            "motion3 Meta Duration/Fps must be finite and positive",
        );
    }
    if motion.meta.curve_count != motion.curves.len()
        || motion.meta.user_data_count != motion.user_data.len()
    {
        return invalid_resource(
            reference,
            "motion3 Meta counts do not match declared arrays",
        );
    }
    let user_data_size = motion.user_data.iter().try_fold(0_usize, |total, entry| {
        if !entry.time.is_finite()
            || entry.time < 0.0
            || entry.time > motion.meta.duration + MOTION_TIME_TOLERANCE
        {
            return Err(());
        }
        total.checked_add(entry.value.len()).ok_or(())
    });
    if user_data_size.ok() != Some(motion.meta.total_user_data_size) {
        return invalid_resource(
            reference,
            "motion3 UserData metadata contains an invalid time or size",
        );
    }
    for curve in motion.curves {
        let _ = curve.target;
        if curve.id.trim().is_empty()
            || curve.segments.len() < 2
            || curve.segments.iter().any(|value| !value.is_finite())
            || [curve.fade_in_seconds, curve.fade_out_seconds]
                .into_iter()
                .flatten()
                .any(|seconds| !seconds.is_finite() || seconds < 0.0)
        {
            return invalid_resource(reference, "motion3 curve contains invalid values");
        }
    }
    Ok(())
}

fn validate_pose_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let pose: RawPoseResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if pose.kind != "Live2D Pose" {
        return invalid_resource(reference, "pose3 Type must be Live2D Pose");
    }
    if pose
        .fade_in_seconds
        .is_some_and(|seconds| !seconds.is_finite() || seconds < 0.0)
    {
        return invalid_resource(
            reference,
            "pose3 FadeInTime must be finite and non-negative",
        );
    }
    if pose.groups.is_empty() {
        return invalid_resource(reference, "pose3 Groups must contain at least one group");
    }

    let mut part_ids = BTreeSet::new();
    for group in pose.groups {
        if group.is_empty() {
            return invalid_resource(reference, "pose3 groups must not be empty");
        }
        for part in group {
            if part.id.trim().is_empty() || !part_ids.insert(part.id.clone()) {
                return invalid_resource(reference, "pose3 part Id must be non-empty and unique");
            }
            let mut links = BTreeSet::new();
            for link in part.links {
                if link.trim().is_empty() || link == part.id || !links.insert(link) {
                    return invalid_resource(
                        reference,
                        "pose3 Link must be non-empty, unique, and not self-referential",
                    );
                }
            }
        }
    }
    Ok(())
}

fn invalid_resource(reference: &str, detail: &'static str) -> Result<(), ModelError> {
    Err(ModelError::new(
        ModelDiagnostic::ModelResourceInvalid,
        Some(reference),
        detail,
    ))
}

fn validate_flac_resource(path: &Path, reference: &str) -> Result<(), ModelError> {
    if !reference
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("flac"))
    {
        return invalid_resource(reference, "motion audio must use the supported FLAC format");
    }

    let mut file = File::open(path).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("audio resource cannot be opened: {error}"),
        )
    })?;
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC resource is missing its signature",
        )
    })?;
    if signature != *b"fLaC" {
        return invalid_resource(reference, "motion audio does not have a FLAC signature");
    }

    let mut block_header = [0_u8; 4];
    file.read_exact(&mut block_header).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC resource is missing a STREAMINFO block",
        )
    })?;
    let mut block_is_last = block_header[0] & 0x80 != 0;
    let block_kind = block_header[0] & 0x7f;
    let block_length = u32::from_be_bytes([0, block_header[1], block_header[2], block_header[3]]);
    if block_kind != 0 || block_length != 34 {
        return invalid_resource(
            reference,
            "FLAC resource must begin with a 34-byte STREAMINFO block",
        );
    }

    let mut stream_info = [0_u8; 34];
    file.read_exact(&mut stream_info).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC STREAMINFO block is truncated",
        )
    })?;
    let audio_properties = u64::from_be_bytes([
        stream_info[10],
        stream_info[11],
        stream_info[12],
        stream_info[13],
        stream_info[14],
        stream_info[15],
        stream_info[16],
        stream_info[17],
    ]);
    let sample_rate = audio_properties >> 44;
    let channels = ((audio_properties >> 41) & 0x7) + 1;
    let bits_per_sample = ((audio_properties >> 36) & 0x1f) + 1;
    if sample_rate == 0 || channels > 8 || bits_per_sample > 32 {
        return invalid_resource(
            reference,
            "FLAC STREAMINFO contains invalid audio properties",
        );
    }
    while !block_is_last {
        file.read_exact(&mut block_header).map_err(|_| {
            ModelError::new(
                ModelDiagnostic::ModelResourceInvalid,
                Some(reference),
                "FLAC metadata block is truncated",
            )
        })?;
        block_is_last = block_header[0] & 0x80 != 0;
        let block_length =
            u32::from_be_bytes([0, block_header[1], block_header[2], block_header[3]]);
        consume_flac_metadata_block(&mut file, block_length, reference)?;
    }
    if file.read(&mut [0_u8; 1]).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("FLAC resource cannot be read: {error}"),
        )
    })? == 0
    {
        return invalid_resource(reference, "FLAC resource has no audio frames");
    }
    Ok(())
}

fn consume_flac_metadata_block(
    file: &mut File,
    length: u32,
    reference: &str,
) -> Result<(), ModelError> {
    let mut remaining = length as usize;
    let mut buffer = [0_u8; 8 * 1024];
    while remaining > 0 {
        let read_length = remaining.min(buffer.len());
        let read = file.read(&mut buffer[..read_length]).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                Some(reference),
                format!("FLAC metadata cannot be read: {error}"),
            )
        })?;
        if read == 0 {
            return invalid_resource(reference, "FLAC metadata block is truncated");
        }
        remaining -= read;
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
    parse_png_dimensions(&header, reference)
}

fn parse_png_dimensions(header: &[u8], reference: &str) -> Result<(u32, u32), ModelError> {
    if header.len() < 24 || header[..8] != *b"\x89PNG\r\n\x1a\n" || header[12..16] != *b"IHDR" {
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

fn validate_texture_dimensions(
    width: u32,
    height: u32,
    maximum_dimension: u32,
    reference: &str,
) -> Result<(), ModelError> {
    if width > maximum_dimension || height > maximum_dimension {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureDimensionExceeded,
            Some(reference),
            format!("texture is {width}x{height}; maximum side is {maximum_dimension}"),
        ));
    }
    Ok(())
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
    use proptest::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    enum FixtureStage {
        PackageDiscovery,
        JsonParse,
        ReferenceResolution,
        TextureHeader,
    }

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    enum FixtureExpectation {
        Accept,
        Reject,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct FixtureLimits {
        maximum_texture_dimension: u32,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureEncoding {
        Hex,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureMaterialization {
        source: String,
        target: String,
        encoding: FixtureEncoding,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct FixtureCase {
        id: String,
        directory: String,
        stage: FixtureStage,
        entry_source: Option<String>,
        materialized_entry: Option<String>,
        #[serde(default)]
        materialize: Vec<FixtureMaterialization>,
        expected: FixtureExpectation,
        expected_diagnostics: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct FixtureManifest {
        schema_version: u32,
        limits: FixtureLimits,
        cases: Vec<FixtureCase>,
    }

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

    fn copy_fixture_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create fixture destination");
        for entry in fs::read_dir(source).expect("list fixture source") {
            let entry = entry.expect("read fixture entry");
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let file_type = entry.file_type().expect("read fixture file type");
            if file_type.is_dir() {
                copy_fixture_tree(&source_path, &destination_path);
            } else {
                assert!(
                    file_type.is_file(),
                    "fixture entries must be files or directories"
                );
                fs::copy(source_path, destination_path).expect("copy fixture file");
            }
        }
    }

    fn decode_fixture_hex(value: &str) -> Vec<u8> {
        let value = value.trim().as_bytes();
        assert!(
            value.len().is_multiple_of(2),
            "fixture hex must contain pairs"
        );
        value
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                    .expect("valid fixture hex")
            })
            .collect()
    }

    fn fixture_materialization_path(root: &Path, reference: &str) -> PathBuf {
        let normalized = normalize_reference(reference).expect("fixture materialization reference");
        assert_eq!(
            normalized, reference,
            "fixture references must be normalized"
        );
        root.join(path_from_reference(&normalized))
    }

    fn materialize_fixture_case(case: &FixtureCase) -> tempfile::TempDir {
        let temporary = tempdir().expect("fixture package");
        copy_fixture_tree(&fixture(&case.directory), temporary.path());
        match (&case.entry_source, &case.materialized_entry) {
            (Some(source), Some(target)) => {
                fs::copy(
                    fixture_materialization_path(temporary.path(), source),
                    fixture_materialization_path(temporary.path(), target),
                )
                .expect("materialize fixture entry");
            }
            (None, None) => {}
            _ => panic!("fixture entry source and target must be paired"),
        }
        for materialization in &case.materialize {
            let source = fixture_materialization_path(temporary.path(), &materialization.source);
            let target = fixture_materialization_path(temporary.path(), &materialization.target);
            fs::create_dir_all(target.parent().expect("materialization parent"))
                .expect("create materialization parent");
            let bytes = match materialization.encoding {
                FixtureEncoding::Hex => decode_fixture_hex(
                    &fs::read_to_string(source).expect("read fixture materialization"),
                ),
            };
            fs::write(target, bytes).expect("write fixture materialization");
        }
        temporary
    }

    fn snapshot_fixture_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(directory).expect("list fixture snapshot") {
                let entry = entry.expect("read fixture snapshot entry");
                let path = entry.path();
                let file_type = entry.file_type().expect("read fixture snapshot type");
                if file_type.is_dir() {
                    visit(root, &path, snapshot);
                } else {
                    assert!(
                        file_type.is_file(),
                        "fixture snapshot must not contain links"
                    );
                    let reference = relative_reference(root, &path).expect("fixture reference");
                    snapshot.insert(
                        reference,
                        fs::read(path).expect("read fixture snapshot file"),
                    );
                }
            }
        }

        let mut snapshot = BTreeMap::new();
        visit(root, root, &mut snapshot);
        snapshot
    }

    fn stage_accepts_diagnostic(stage: FixtureStage, diagnostic: ModelDiagnostic) -> bool {
        match stage {
            FixtureStage::PackageDiscovery => matches!(
                diagnostic,
                ModelDiagnostic::ModelEntryAmbiguous | ModelDiagnostic::ModelEntryMissing
            ),
            FixtureStage::JsonParse => matches!(
                diagnostic,
                ModelDiagnostic::ModelJsonInvalid
                    | ModelDiagnostic::ModelJsonTooLarge
                    | ModelDiagnostic::ModelUnsupportedVersion
            ),
            FixtureStage::ReferenceResolution => matches!(
                diagnostic,
                ModelDiagnostic::ModelMocMissing
                    | ModelDiagnostic::ModelReferenceEscapesRoot
                    | ModelDiagnostic::ModelReferenceInvalid
                    | ModelDiagnostic::ModelReferenceSymlinkEscape
                    | ModelDiagnostic::ModelResourceInvalid
                    | ModelDiagnostic::ModelResourceMissing
                    | ModelDiagnostic::ModelResourceNotFile
                    | ModelDiagnostic::ModelSymlinkDirectoryUnsupported
                    | ModelDiagnostic::ModelTextureMissing
            ),
            FixtureStage::TextureHeader => matches!(
                diagnostic,
                ModelDiagnostic::ModelTextureDimensionExceeded
                    | ModelDiagnostic::ModelTextureInvalidPng
            ),
        }
    }

    #[test]
    fn shared_custom_model_fixtures_match_product_parser_and_store_contract() {
        let fixture_root = repository_root().join("shared/fixtures/model-fixtures");
        let manifest: FixtureManifest = serde_json::from_slice(
            &fs::read(fixture_root.join("cases.json")).expect("read fixture manifest"),
        )
        .expect("strict fixture manifest");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(
            manifest.limits.maximum_texture_dimension,
            ModelPackageLimits::default().maximum_texture_dimension
        );
        assert!(!manifest.cases.is_empty());

        let mut case_ids = BTreeSet::new();
        let mut registered_directories = BTreeSet::new();
        for (index, case) in manifest.cases.iter().enumerate() {
            assert!(case_ids.insert(case.id.as_str()), "duplicate fixture id");
            assert!(
                registered_directories.insert(case.directory.as_str()),
                "duplicate fixture directory"
            );
            assert_eq!(
                Path::new(&case.directory).components().collect::<Vec<_>>(),
                [Component::Normal(case.directory.as_ref())],
                "fixture directory must be one normal component"
            );
            let unique_diagnostics = case
                .expected_diagnostics
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert_eq!(unique_diagnostics.len(), case.expected_diagnostics.len());
            assert_eq!(
                case.expected == FixtureExpectation::Accept,
                case.expected_diagnostics.is_empty(),
                "fixture accept/reject contract must match diagnostics"
            );

            let package = materialize_fixture_case(case);
            let source_before = snapshot_fixture_tree(package.path());
            let id = ModelId::parse(format!("fixture-{index}")).expect("fixture model id");
            let prepared =
                PreparedModel::prepare(id.clone(), package.path(), ModelPackageLimits::default());
            match (&case.expected, &prepared) {
                (FixtureExpectation::Accept, Ok(model)) => {
                    assert_eq!(model.id(), &id, "fixture {} model id", case.id);
                }
                (FixtureExpectation::Reject, Err(error)) => {
                    assert_eq!(
                        case.expected_diagnostics,
                        [error.code.as_str()],
                        "fixture {} diagnostic",
                        case.id
                    );
                    assert!(
                        stage_accepts_diagnostic(case.stage, error.code),
                        "fixture {} diagnostic must belong to declared stage",
                        case.id
                    );
                }
                (FixtureExpectation::Accept, Err(error)) => {
                    panic!("fixture {} unexpectedly rejected: {error}", case.id)
                }
                (FixtureExpectation::Reject, Ok(_)) => {
                    panic!("fixture {} unexpectedly accepted", case.id)
                }
            }

            let data = tempdir().expect("fixture store root");
            let store = ModelStore::new(
                data.path().join("models"),
                data.path().join("locks/models.writer.lock"),
                ModelPackageLimits::default(),
            )
            .expect("fixture model store");
            let imported = store.import(id.clone(), package.path());
            match case.expected {
                FixtureExpectation::Accept => {
                    let installed = imported.expect("accepted fixture import");
                    assert_eq!(installed.id(), &id);
                    let catalog = store.list().expect("fixture catalog");
                    assert_eq!(catalog.len(), 1);
                    assert_eq!(catalog[0].origin(), ModelOrigin::Installed);
                }
                FixtureExpectation::Reject => {
                    assert_eq!(
                        imported.expect_err("rejected fixture import").code,
                        ModelStoreDiagnostic::InvalidPackage,
                        "fixture {} store diagnostic",
                        case.id
                    );
                    assert!(store.list().expect("empty fixture catalog").is_empty());
                    assert!(
                        fs::read_dir(store.root())
                            .expect("fixture store entries")
                            .next()
                            .is_none(),
                        "fixture {} must not leave staging or destination entries",
                        case.id
                    );
                }
            }
            assert_eq!(
                snapshot_fixture_tree(package.path()),
                source_before,
                "fixture {} source package changed",
                case.id
            );
        }

        let actual_directories = fs::read_dir(fixture_root.join("cases"))
            .expect("list fixture directories")
            .map(|entry| {
                entry
                    .expect("read fixture directory")
                    .file_name()
                    .into_string()
                    .expect("UTF-8 fixture directory")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_directories,
            registered_directories
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
            "every custom model fixture directory must be registered"
        );
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
    fn sidecar_contracts_reject_invalid_display_expression_and_motion_resources() {
        let package = tempdir().expect("package");
        let limits = ModelPackageLimits::default();
        let display = package.path().join("display.cdi3.json");
        fs::write(
            &display,
            r#"{"Version":3,"Parameters":[{"Id":"ParamA","GroupId":"missing","Name":"A"}]}"#,
        )
        .expect("display resource");
        let error = validate_display_info_resource(
            &display,
            "display.cdi3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("undeclared display group must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

        let expression = package.path().join("expression.exp3.json");
        fs::write(
            &expression,
            r#"{"Type":"Live2D Expression","Parameters":[{"Id":"ParamA","Value":1},{"Id":"ParamA","Value":2}]}"#,
        )
        .expect("expression resource");
        let error = validate_expression_resource(
            &expression,
            "expression.exp3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("duplicate expression parameter must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

        let motion = package.path().join("motion.motion3.json");
        fs::write(
            &motion,
            r#"{
              "Version":3,
              "Meta":{"Duration":1,"Fps":30,"Loop":false,"AreBeziersRestricted":true,"CurveCount":1,"TotalSegmentCount":0,"TotalPointCount":0,"UserDataCount":0,"TotalUserDataSize":0},
              "Curves":[]
            }"#,
        )
        .expect("motion resource");
        let error = validate_motion_resource(
            &motion,
            "motion.motion3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("mismatched motion count must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

        fs::write(
            &motion,
            r#"{
              "Version":3,
              "Meta":{"Duration":1,"Fps":30,"Loop":false,"AreBeziersRestricted":true,"CurveCount":0,"TotalSegmentCount":0,"TotalPointCount":0,"UserDataCount":1,"TotalUserDataSize":1},
              "Curves":[],
              "UserData":[{"Time":2,"Value":"x"}]
            }"#,
        )
        .expect("invalid user data resource");
        let error = validate_motion_resource(
            &motion,
            "motion.motion3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("out-of-range motion user data must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
    }

    #[test]
    fn pose_contract_rejects_invalid_groups_parts_and_links() {
        let package = tempdir().expect("package");
        let limits = ModelPackageLimits::default();
        let pose = package.path().join("model.pose3.json");
        fs::write(
            &pose,
            r#"{
              "Type":"Live2D Pose",
              "FadeInTime":0.5,
              "Groups":[[
                {"Id":"PartArmA","Link":["PartArmB"]},
                {"Id":"PartArmB"}
              ]]
            }"#,
        )
        .expect("valid pose resource");
        validate_pose_resource(
            &pose,
            "model.pose3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect("valid pose resource must be accepted");

        fs::write(
            &pose,
            r#"{
              "Type":"Live2D Pose",
              "Groups":[[
                {"Id":"PartArmA","Link":["PartArmA"]}
              ]]
            }"#,
        )
        .expect("invalid pose resource");
        let error = validate_pose_resource(
            &pose,
            "model.pose3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("self-referential pose link must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
        assert!(error.detail.contains("not self-referential"));
    }

    #[test]
    fn audio_contract_rejects_unsupported_or_malformed_flac_resources() {
        let package = tempdir().expect("package");
        let unsupported = package.path().join("sound.wav");
        fs::write(&unsupported, b"RIFF").expect("unsupported audio resource");
        let error = validate_flac_resource(&unsupported, "sound.wav")
            .expect_err("unsupported audio format must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

        let malformed = package.path().join("sound.flac");
        fs::write(&malformed, b"not-a-flac").expect("malformed audio resource");
        let error = validate_flac_resource(&malformed, "sound.flac")
            .expect_err("malformed FLAC must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
    }

    #[test]
    fn preset_sidecars_pass_product_contract_validation() {
        for mode in ["standard", "keyboard", "gamepad"] {
            let model = PreparedModel::prepare(
                ModelId::parse(mode).expect("model id"),
                repository_root().join("native/resources/models").join(mode),
                ModelPackageLimits::default(),
            )
            .expect("preset sidecars must be valid");
            assert!(model.index().display_info.is_some());
            assert!(!model.index().expressions.is_empty());
            assert!(
                model
                    .index()
                    .motion_groups
                    .iter()
                    .all(|group| !group.motions.is_empty())
            );
        }
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
    fn model_snapshot_exposes_only_declared_behavior_identities() {
        let catalog = PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog");
        let snapshot = catalog
            .load(&ModelId::parse("standard").expect("model id"))
            .expect("committed preset")
            .snapshot();

        assert_eq!(snapshot.motion_count, 4);
        assert_eq!(snapshot.expression_count, 3);
        assert_eq!(snapshot.behaviors.len(), 7);
        assert!(snapshot.behaviors.contains(&ModelBehaviorSnapshot::Motion {
            group: "CAT_motion".to_owned(),
            index: 0,
        }));
        assert!(
            snapshot
                .behaviors
                .contains(&ModelBehaviorSnapshot::Expression {
                    name: "live2d_expression2.exp3.json".to_owned(),
                })
        );
        assert!(snapshot.behaviors.iter().all(|behavior| match behavior {
            ModelBehaviorSnapshot::Motion { group, .. } =>
                !group.contains('/') && !group.contains('\\'),
            ModelBehaviorSnapshot::Expression { name } =>
                !name.contains('/') && !name.contains('\\'),
        }));
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
        for invalid in [
            "",
            "..",
            ".hidden",
            "trailing.",
            "CON",
            "nul.custom",
            "Com1",
            "LPT9.backup",
            "cat/model",
            "猫",
            "model id",
        ] {
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

    #[test]
    fn package_byte_and_file_limits_fail_before_unbounded_loading() {
        let package = tempdir().expect("package");
        let model = r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#;
        fs::write(package.path().join("cat.model3.json"), model).expect("model3");
        fs::write(package.path().join("model.moc3"), b"moc").expect("moc");
        fs::write(package.path().join("unreferenced.bin"), b"extra").expect("extra file");

        let json_limited = PreparedModel::prepare(
            ModelId::parse("limited").expect("model id"),
            package.path(),
            ModelPackageLimits {
                maximum_json_bytes: 16,
                ..ModelPackageLimits::default()
            },
        )
        .expect_err("JSON byte limit");
        assert_eq!(json_limited.code, ModelDiagnostic::ModelJsonTooLarge);

        let file_limited = PreparedModel::prepare(
            ModelId::parse("limited").expect("model id"),
            package.path(),
            ModelPackageLimits {
                maximum_file_count: 2,
                ..ModelPackageLimits::default()
            },
        )
        .expect_err("file count limit");
        assert_eq!(file_limited.code, ModelDiagnostic::ModelFileCountExceeded);

        let package_limited = PreparedModel::prepare(
            ModelId::parse("limited").expect("model id"),
            package.path(),
            ModelPackageLimits {
                maximum_package_bytes: 1,
                ..ModelPackageLimits::default()
            },
        )
        .expect_err("package byte limit");
        assert_eq!(
            package_limited.code,
            ModelDiagnostic::ModelPackageSizeExceeded
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn arbitrary_model_json_bytes_never_escape_the_bounded_parser(
            bytes in proptest::collection::vec(any::<u8>(), 0..8_192),
            maximum_depth in 1_usize..=64,
        ) {
            let result: Result<ModelDefinition, ModelError> = parse_json_bytes(
                &bytes,
                "generated.model3.json",
                maximum_depth,
                ModelDiagnostic::ModelJsonInvalid,
            );
            if let Err(error) = result {
                prop_assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
                prop_assert_eq!(error.resource.as_deref(), Some("generated.model3.json"));
            }
        }

        #[test]
        fn generated_model_array_indices_round_trip_without_loss(
            textures in proptest::collection::vec(any::<String>(), 0..64),
            group_ids in proptest::collection::vec(any::<String>(), 0..64),
        ) {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "Version": 3,
                "FileReferences": {
                    "Moc": "model.moc3",
                    "Textures": textures,
                },
                "Groups": [{
                    "Target": "Parameter",
                    "Name": "Generated",
                    "Ids": group_ids,
                }],
            }))
            .expect("generated model JSON");
            let parsed: ModelDefinition = parse_json_bytes(
                &bytes,
                "generated.model3.json",
                16,
                ModelDiagnostic::ModelJsonInvalid,
            )?;

            prop_assert_eq!(parsed.files.textures, textures);
            prop_assert_eq!(parsed.groups.len(), 1);
            prop_assert_eq!(&parsed.groups[0].ids, &group_ids);
        }

        #[test]
        fn accepted_model_ids_exactly_match_the_portable_store_key_contract(value in any::<String>()) {
            let expected = !value.is_empty()
                && value.len() <= 64
                && !value.starts_with('.')
                && !value.ends_with('.')
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                })
                && !is_windows_reserved_name(&value);
            prop_assert_eq!(ModelId::parse(value).is_ok(), expected);
        }

        #[test]
        fn normalized_references_are_idempotent_relative_paths(reference in any::<String>()) {
            if let Ok(normalized) = normalize_reference(&reference) {
                let renormalized = normalize_reference(&normalized);
                prop_assert_eq!(renormalized.as_deref(), Ok(normalized.as_str()));
                prop_assert!(!normalized.chars().any(|character| matches!(character, '\\' | '\0' | ':')));
                prop_assert!(!Path::new(&normalized).is_absolute());
                prop_assert!(normalized.split('/').all(|part| !part.is_empty() && part != "." && part != ".."));
                prop_assert!(path_from_reference(&normalized)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))));
            }
        }

        #[test]
        fn png_dimensions_are_parsed_and_limited_without_pixel_allocation(
            width in any::<u32>(),
            height in any::<u32>(),
            maximum_dimension in any::<u32>(),
        ) {
            let mut header = [0_u8; 24];
            header[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
            header[12..16].copy_from_slice(b"IHDR");
            header[16..20].copy_from_slice(&width.to_be_bytes());
            header[20..24].copy_from_slice(&height.to_be_bytes());

            let parsed = parse_png_dimensions(&header, "generated.png");
            if width == 0 || height == 0 {
                prop_assert_eq!(parsed.expect_err("zero dimension").code, ModelDiagnostic::ModelTextureInvalidPng);
            } else {
                prop_assert_eq!(parsed?, (width, height));
                let limited = validate_texture_dimensions(
                    width,
                    height,
                    maximum_dimension,
                    "generated.png",
                );
                prop_assert_eq!(limited.is_ok(), width <= maximum_dimension && height <= maximum_dimension);
                if let Err(error) = limited {
                    prop_assert_eq!(error.code, ModelDiagnostic::ModelTextureDimensionExceeded);
                }
            }
        }

        #[test]
        fn arbitrary_png_headers_cannot_produce_unrelated_dimensions(
            header in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            if let Ok((width, height)) = parse_png_dimensions(&header, "generated.png") {
                prop_assert!(header.len() >= 24);
                prop_assert_eq!(&header[..8], b"\x89PNG\r\n\x1a\n");
                prop_assert_eq!(&header[12..16], b"IHDR");
                prop_assert_eq!(width, u32::from_be_bytes(header[16..20].try_into().expect("width slice")));
                prop_assert_eq!(height, u32::from_be_bytes(header[20..24].try_into().expect("height slice")));
                prop_assert!(width > 0 && height > 0);
            }
        }
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
