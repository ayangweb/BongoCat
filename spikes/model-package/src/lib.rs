#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const INDEX_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelPackageLimits {
    pub maximum_texture_dimension: u32,
    pub maximum_json_bytes: u64,
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
            maximum_file_bytes: 512 * 1024 * 1024,
            maximum_package_bytes: 1024 * 1024 * 1024,
            maximum_file_count: 4_096,
            maximum_directory_depth: 32,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    ModelEntryAmbiguous,
    ModelEntryMissing,
    ModelDisplayInfoInvalid,
    ModelExpressionInvalid,
    ModelFileCountExceeded,
    ModelFileTooLarge,
    ModelIoError,
    ModelJsonInvalid,
    ModelJsonTooLarge,
    ModelMocMissing,
    ModelMotionInvalid,
    ModelPackageDepthExceeded,
    ModelPackageSizeExceeded,
    ModelPhysicsInvalid,
    ModelPoseInvalid,
    ModelReferenceEscapesRoot,
    ModelReferenceInvalid,
    ModelReferenceSymlinkEscape,
    ModelResourceJsonInvalid,
    ModelResourceMissing,
    ModelResourceNotFile,
    ModelSymlinkDirectoryUnsupported,
    ModelTextureDimensionExceeded,
    ModelTextureInvalidPng,
    ModelTextureMissing,
    ModelUnsupportedVersion,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelEntryAmbiguous => "model_entry_ambiguous",
            Self::ModelEntryMissing => "model_entry_missing",
            Self::ModelDisplayInfoInvalid => "model_display_info_invalid",
            Self::ModelExpressionInvalid => "model_expression_invalid",
            Self::ModelFileCountExceeded => "model_file_count_exceeded",
            Self::ModelFileTooLarge => "model_file_too_large",
            Self::ModelIoError => "model_io_error",
            Self::ModelJsonInvalid => "model_json_invalid",
            Self::ModelJsonTooLarge => "model_json_too_large",
            Self::ModelMocMissing => "model_moc_missing",
            Self::ModelMotionInvalid => "model_motion_invalid",
            Self::ModelPackageDepthExceeded => "model_package_depth_exceeded",
            Self::ModelPackageSizeExceeded => "model_package_size_exceeded",
            Self::ModelPhysicsInvalid => "model_physics_invalid",
            Self::ModelPoseInvalid => "model_pose_invalid",
            Self::ModelReferenceEscapesRoot => "model_reference_escapes_root",
            Self::ModelReferenceInvalid => "model_reference_invalid",
            Self::ModelReferenceSymlinkEscape => "model_reference_symlink_escape",
            Self::ModelResourceJsonInvalid => "model_resource_json_invalid",
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
pub struct ModelPackageError {
    pub code: DiagnosticCode,
    pub resource: Option<String>,
    pub detail: String,
}

impl ModelPackageError {
    fn new(code: DiagnosticCode, resource: Option<&str>, detail: impl Into<String>) -> Self {
        Self {
            code,
            resource: resource.map(str::to_owned),
            detail: detail.into(),
        }
    }
}

impl Display for ModelPackageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(resource) = &self.resource {
            write!(
                formatter,
                "{} ({resource}): {}",
                self.code.as_str(),
                self.detail
            )
        } else {
            write!(formatter, "{}: {}", self.code.as_str(), self.detail)
        }
    }
}

impl Error for ModelPackageError {}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelPackageIndex {
    pub schema_version: u32,
    pub model_version: u32,
    pub entry: String,
    pub moc: String,
    pub textures: Vec<ImageResource>,
    pub display_info: Option<DisplayInfoResource>,
    pub expressions: Vec<NamedResource>,
    pub motion_groups: Vec<MotionGroup>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub user_data: Option<String>,
    pub groups: Vec<ModelGroup>,
    pub hit_areas: Vec<HitArea>,
    pub companion_resources: CompanionResources,
    pub package_file_count: usize,
    pub package_total_bytes: u64,
    pub unreferenced_files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImageResource {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct NamedImageResource {
    pub id: String,
    pub image: ImageResource,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct NamedResource {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DisplayInfoResource {
    pub file: String,
    pub parameter_count: usize,
    pub parameter_group_count: usize,
    pub part_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhysicsResourceSummary {
    pub version: u32,
    pub fps: f64,
    pub setting_count: usize,
    pub input_count: usize,
    pub output_count: usize,
    pub vertex_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PoseResourceSummary {
    pub fade_in_seconds: Option<f64>,
    pub group_count: usize,
    pub part_count: usize,
    pub link_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MotionGroup {
    pub name: String,
    pub motions: Vec<MotionResource>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MotionResource {
    pub file: String,
    pub sound: Option<String>,
    pub fade_in_seconds: Option<f64>,
    pub fade_out_seconds: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ModelGroup {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct HitArea {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelModeHint {
    Standard,
    Keyboard,
    Gamepad,
    Unclassified,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CompanionResources {
    pub mode_hint: ModelModeHint,
    pub background: Option<ImageResource>,
    pub cover: Option<ImageResource>,
    pub left_keys: Vec<NamedImageResource>,
    pub right_keys: Vec<NamedImageResource>,
}

#[derive(Debug, Deserialize)]
struct ModelDefinition {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "FileReferences")]
    file_references: FileReferences,
    #[serde(rename = "Groups", default)]
    groups: Vec<RawGroup>,
    #[serde(rename = "HitAreas", default)]
    hit_areas: Vec<RawHitArea>,
}

#[derive(Debug, Deserialize)]
struct FileReferences {
    #[serde(rename = "Moc")]
    moc: String,
    #[serde(rename = "Textures")]
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
    fade_in_seconds: Option<f64>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawGroup {
    #[serde(rename = "Target")]
    target: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Ids")]
    ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawHitArea {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayInfoDefinition {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Parameters")]
    parameters: Vec<DisplayInfoItem>,
    #[serde(rename = "ParameterGroups")]
    parameter_groups: Vec<DisplayInfoItem>,
    #[serde(rename = "Parts")]
    parts: Vec<DisplayInfoPart>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayInfoItem {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "GroupId")]
    group_id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayInfoPart {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DisplayInfoInspection {
    resource: DisplayInfoResource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionDefinition {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Meta")]
    meta: MotionMeta,
    #[serde(rename = "Curves")]
    curves: Vec<MotionCurve>,
    #[serde(rename = "UserData", default)]
    user_data: Vec<MotionUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionMeta {
    #[serde(rename = "Duration")]
    duration: f64,
    #[serde(rename = "Fps")]
    fps: f64,
    #[serde(rename = "Loop")]
    _looping: bool,
    #[serde(rename = "AreBeziersRestricted")]
    _are_beziers_restricted: bool,
    #[serde(rename = "CurveCount")]
    curve_count: usize,
    #[serde(rename = "TotalSegmentCount")]
    total_segment_count: usize,
    #[serde(rename = "TotalPointCount")]
    total_point_count: usize,
    #[serde(rename = "UserDataCount")]
    user_data_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    _total_user_data_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionCurve {
    #[serde(rename = "Target")]
    target: MotionCurveTarget,
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Segments")]
    segments: Vec<f64>,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f64>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum MotionCurveTarget {
    Model,
    Parameter,
    PartOpacity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionUserData {
    #[serde(rename = "Time")]
    time: f64,
    #[serde(rename = "Value")]
    value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MotionSummary {
    curve_count: usize,
    segment_count: usize,
    point_count: usize,
    user_data_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpressionDefinition {
    #[serde(rename = "Type")]
    expression_type: String,
    #[serde(rename = "Parameters")]
    parameters: Vec<ExpressionParameter>,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f64>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpressionParameter {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Value")]
    value: f64,
    #[serde(rename = "Blend", default)]
    blend: ExpressionBlend,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum ExpressionBlend {
    #[default]
    Add,
    Multiply,
    Overwrite,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ExpressionSummary {
    parameter_count: usize,
    blend_counts: BTreeMap<ExpressionBlend, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoseDefinition {
    #[serde(rename = "Type")]
    pose_type: String,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f64>,
    #[serde(rename = "Groups")]
    groups: Vec<Vec<PosePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PosePart {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Link", default)]
    links: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsDefinition {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Meta")]
    meta: PhysicsMeta,
    #[serde(rename = "PhysicsSettings")]
    settings: Vec<PhysicsSetting>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsMeta {
    #[serde(rename = "PhysicsSettingCount")]
    setting_count: usize,
    #[serde(rename = "TotalInputCount")]
    input_count: usize,
    #[serde(rename = "TotalOutputCount")]
    output_count: usize,
    #[serde(rename = "VertexCount")]
    vertex_count: usize,
    #[serde(rename = "Fps")]
    fps: f64,
    #[serde(rename = "EffectiveForces")]
    effective_forces: PhysicsForces,
    #[serde(rename = "PhysicsDictionary")]
    dictionary: Vec<PhysicsDictionaryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsForces {
    #[serde(rename = "Gravity")]
    gravity: PhysicsVector,
    #[serde(rename = "Wind")]
    wind: PhysicsVector,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsVector {
    #[serde(rename = "X")]
    x: f64,
    #[serde(rename = "Y")]
    y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsDictionaryEntry {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsSetting {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Input")]
    inputs: Vec<PhysicsInput>,
    #[serde(rename = "Output")]
    outputs: Vec<PhysicsOutput>,
    #[serde(rename = "Vertices")]
    vertices: Vec<PhysicsVertex>,
    #[serde(rename = "Normalization")]
    normalization: PhysicsNormalization,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsInput {
    #[serde(rename = "Source")]
    source: PhysicsTarget,
    #[serde(rename = "Weight")]
    weight: f64,
    #[serde(rename = "Type")]
    _kind: PhysicsChannel,
    #[serde(rename = "Reflect")]
    _reflect: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsOutput {
    #[serde(rename = "Destination")]
    destination: PhysicsTarget,
    #[serde(rename = "VertexIndex")]
    vertex_index: usize,
    #[serde(rename = "Scale")]
    scale: f64,
    #[serde(rename = "Weight")]
    weight: f64,
    #[serde(rename = "Type")]
    _kind: PhysicsChannel,
    #[serde(rename = "Reflect")]
    _reflect: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum PhysicsChannel {
    X,
    Y,
    Angle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsTarget {
    #[serde(rename = "Target")]
    _target: PhysicsTargetKind,
    #[serde(rename = "Id")]
    id: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum PhysicsTargetKind {
    Parameter,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsNormalization {
    #[serde(rename = "Position")]
    position: PhysicsRange,
    #[serde(rename = "Angle")]
    angle: PhysicsRange,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsRange {
    #[serde(rename = "Minimum")]
    minimum: f64,
    #[serde(rename = "Default")]
    default: f64,
    #[serde(rename = "Maximum")]
    maximum: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicsVertex {
    #[serde(rename = "Position")]
    position: PhysicsVector,
    #[serde(rename = "Mobility")]
    mobility: f64,
    #[serde(rename = "Delay")]
    delay: f64,
    #[serde(rename = "Acceleration")]
    acceleration: f64,
    #[serde(rename = "Radius")]
    radius: f64,
}

#[derive(Clone, Copy)]
enum ResourceKind {
    Moc,
    Texture,
    Other,
}

struct PackageReader {
    canonical_root: PathBuf,
    limits: ModelPackageLimits,
    referenced_files: BTreeSet<String>,
}

impl PackageReader {
    fn new(root: &Path, limits: ModelPackageLimits) -> Result<Self, ModelPackageError> {
        let canonical_root = root.canonicalize().map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                None,
                format!("package root cannot be opened: {error}"),
            )
        })?;
        if !canonical_root.is_dir() {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelIoError,
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
        kind: ResourceKind,
    ) -> Result<(String, PathBuf), ModelPackageError> {
        let normalized = normalize_reference(reference)?;
        let candidate = self.canonical_root.join(path_from_normalized(&normalized));
        let canonical = candidate.canonicalize().map_err(|_| {
            ModelPackageError::new(
                missing_code(kind),
                Some(&normalized),
                "resource does not exist",
            )
        })?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelReferenceSymlinkEscape,
                Some(&normalized),
                "resource resolves outside the package root",
            ));
        }
        let metadata = canonical.metadata().map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                Some(&normalized),
                format!("resource metadata cannot be read: {error}"),
            )
        })?;
        if !metadata.is_file() {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelResourceNotFile,
                Some(&normalized),
                "resource is not a regular file",
            ));
        }
        if metadata.len() > self.limits.maximum_file_bytes {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelFileTooLarge,
                Some(&normalized),
                format!(
                    "resource is {} bytes; limit is {} bytes",
                    metadata.len(),
                    self.limits.maximum_file_bytes
                ),
            ));
        }
        self.referenced_files.insert(normalized.clone());
        Ok((normalized, canonical))
    }

    fn resolve_json(&mut self, reference: &str) -> Result<String, ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        read_json_object(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            DiagnosticCode::ModelResourceJsonInvalid,
        )?;
        Ok(normalized)
    }

    fn resolve_physics(&mut self, reference: &str) -> Result<String, ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        inspect_physics_file(&path, &normalized, self.limits.maximum_json_bytes)?;
        Ok(normalized)
    }

    fn resolve_pose(&mut self, reference: &str) -> Result<String, ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        inspect_pose_file(&path, &normalized, self.limits.maximum_json_bytes)?;
        Ok(normalized)
    }

    fn resolve_display_info(
        &mut self,
        reference: &str,
    ) -> Result<DisplayInfoInspection, ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        inspect_display_info_file(&path, &normalized, self.limits.maximum_json_bytes)
    }

    fn resolve_motion(
        &mut self,
        reference: &str,
    ) -> Result<(String, MotionSummary), ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        let summary = inspect_motion_file(&path, &normalized, self.limits.maximum_json_bytes)?;
        Ok((normalized, summary))
    }

    fn resolve_expression(
        &mut self,
        reference: &str,
    ) -> Result<(String, ExpressionSummary), ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Other)?;
        let summary = inspect_expression_file(&path, &normalized, self.limits.maximum_json_bytes)?;
        Ok((normalized, summary))
    }

    fn resolve_image(&mut self, reference: &str) -> Result<ImageResource, ModelPackageError> {
        let (normalized, path) = self.resolve_file(reference, ResourceKind::Texture)?;
        let (width, height) = read_png_dimensions(&path, &normalized)?;
        if width > self.limits.maximum_texture_dimension
            || height > self.limits.maximum_texture_dimension
        {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelTextureDimensionExceeded,
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

    fn optional_image(
        &mut self,
        reference: &str,
    ) -> Result<Option<ImageResource>, ModelPackageError> {
        let normalized = normalize_reference(reference)?;
        let candidate = self.canonical_root.join(path_from_normalized(&normalized));
        match fs::symlink_metadata(&candidate) {
            Ok(_) => self.resolve_image(&normalized).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                Some(&normalized),
                format!("optional image metadata cannot be read: {error}"),
            )),
        }
    }

    fn image_directory(
        &mut self,
        reference: &str,
    ) -> Result<Vec<NamedImageResource>, ModelPackageError> {
        let normalized_directory = normalize_reference(reference)?;
        let candidate = self
            .canonical_root
            .join(path_from_normalized(&normalized_directory));
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelIoError,
                    Some(&normalized_directory),
                    format!("resource directory metadata cannot be read: {error}"),
                ));
            }
        }
        let canonical = candidate.canonicalize().map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                Some(&normalized_directory),
                format!("resource directory cannot be opened: {error}"),
            )
        })?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelReferenceSymlinkEscape,
                Some(&normalized_directory),
                "resource directory resolves outside the package root",
            ));
        }
        if !canonical.is_dir() {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelResourceNotFile,
                Some(&normalized_directory),
                "key image path is not a directory",
            ));
        }

        let mut references = Vec::new();
        for entry in fs::read_dir(&canonical).map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                Some(&normalized_directory),
                format!("resource directory cannot be listed: {error}"),
            )
        })? {
            let entry = entry.map_err(|error| {
                ModelPackageError::new(
                    DiagnosticCode::ModelIoError,
                    Some(&normalized_directory),
                    format!("resource directory entry cannot be read: {error}"),
                )
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                ModelPackageError::new(
                    DiagnosticCode::ModelReferenceInvalid,
                    Some(&normalized_directory),
                    "resource filename is not valid UTF-8",
                )
            })?;
            if name.to_ascii_lowercase().ends_with(".png") {
                references.push(format!("{normalized_directory}/{name}"));
            }
        }
        references.sort();

        references
            .into_iter()
            .map(|reference| {
                let id = Path::new(&reference)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| {
                        ModelPackageError::new(
                            DiagnosticCode::ModelReferenceInvalid,
                            Some(&reference),
                            "key image name has no UTF-8 stem",
                        )
                    })?
                    .to_owned();
                Ok(NamedImageResource {
                    id,
                    image: self.resolve_image(&reference)?,
                })
            })
            .collect()
    }

    fn inventory(&self) -> Result<PackageInventory, ModelPackageError> {
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
                ModelPackageError::new(
                    DiagnosticCode::ModelPackageSizeExceeded,
                    None,
                    "package byte count overflowed",
                )
            })
        })?;
        if total_bytes > self.limits.maximum_package_bytes {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelPackageSizeExceeded,
                None,
                format!(
                    "package is {total_bytes} bytes; limit is {} bytes",
                    self.limits.maximum_package_bytes
                ),
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

pub fn inspect_model_package(
    root: impl AsRef<Path>,
    limits: ModelPackageLimits,
) -> Result<ModelPackageIndex, ModelPackageError> {
    let root = root.as_ref();
    let entry = discover_entry(root)?;
    let mut reader = PackageReader::new(root, limits)?;
    let entry_name = entry
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ModelPackageError::new(
                DiagnosticCode::ModelReferenceInvalid,
                None,
                "model entry filename is not valid UTF-8",
            )
        })?
        .to_owned();
    let (entry_name, entry_path) = reader.resolve_file(&entry_name, ResourceKind::Other)?;
    let model: ModelDefinition = read_typed_json(
        &entry_path,
        &entry_name,
        limits.maximum_json_bytes,
        DiagnosticCode::ModelJsonInvalid,
    )?;
    if model.version != 3 {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelUnsupportedVersion,
            Some(&entry_name),
            format!("model3 version {} is not supported", model.version),
        ));
    }

    let (moc, _) = reader.resolve_file(&model.file_references.moc, ResourceKind::Moc)?;
    let textures = model
        .file_references
        .textures
        .iter()
        .map(|reference| reader.resolve_image(reference))
        .collect::<Result<Vec<_>, _>>()?;
    let display_info_inspection = model
        .file_references
        .display_info
        .as_deref()
        .map(|reference| reader.resolve_display_info(reference))
        .transpose()?;
    let display_info = display_info_inspection
        .as_ref()
        .map(|inspection| inspection.resource.clone());
    let expressions = model
        .file_references
        .expressions
        .into_iter()
        .map(|resource| {
            require_identifier(&resource.name, "expression name")?;
            let (file, _summary) = reader.resolve_expression(&resource.file)?;
            Ok(NamedResource {
                name: resource.name,
                file,
            })
        })
        .collect::<Result<Vec<_>, ModelPackageError>>()?;
    let motion_groups = model
        .file_references
        .motions
        .into_iter()
        .map(|(name, motions)| {
            require_identifier(&name, "motion group name")?;
            let motions = motions
                .into_iter()
                .map(|motion| {
                    validate_fade(motion.fade_in_seconds, "FadeInTime")?;
                    validate_fade(motion.fade_out_seconds, "FadeOutTime")?;
                    let (file, _summary) = reader.resolve_motion(&motion.file)?;
                    Ok(MotionResource {
                        file,
                        sound: motion
                            .sound
                            .as_deref()
                            .map(|reference| {
                                reader
                                    .resolve_file(reference, ResourceKind::Other)
                                    .map(|(normalized, _)| normalized)
                            })
                            .transpose()?,
                        fade_in_seconds: motion.fade_in_seconds,
                        fade_out_seconds: motion.fade_out_seconds,
                    })
                })
                .collect::<Result<Vec<_>, ModelPackageError>>()?;
            Ok(MotionGroup { name, motions })
        })
        .collect::<Result<Vec<_>, ModelPackageError>>()?;
    let physics = model
        .file_references
        .physics
        .as_deref()
        .map(|reference| reader.resolve_physics(reference))
        .transpose()?;
    let pose = model
        .file_references
        .pose
        .as_deref()
        .map(|reference| reader.resolve_pose(reference))
        .transpose()?;
    let user_data = model
        .file_references
        .user_data
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;

    let groups = model
        .groups
        .into_iter()
        .map(|group| {
            require_identifier(&group.target, "group target")?;
            require_identifier(&group.name, "group name")?;
            if group.ids.iter().any(|id| id.trim().is_empty()) {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelJsonInvalid,
                    Some(&entry_name),
                    "group ids must not contain blank values",
                ));
            }
            Ok(ModelGroup {
                target: group.target,
                name: group.name,
                ids: group.ids,
            })
        })
        .collect::<Result<Vec<_>, ModelPackageError>>()?;
    let hit_areas = model
        .hit_areas
        .into_iter()
        .map(|area| {
            require_identifier(&area.id, "hit area id")?;
            require_identifier(&area.name, "hit area name")?;
            Ok(HitArea {
                id: area.id,
                name: area.name,
            })
        })
        .collect::<Result<Vec<_>, ModelPackageError>>()?;

    let background = reader.optional_image("resources/background.png")?;
    let cover = reader.optional_image("resources/cover.png")?;
    let left_keys = reader.image_directory("resources/left-keys")?;
    let right_keys = reader.image_directory("resources/right-keys")?;
    let mode_hint = if right_keys.iter().any(|resource| resource.id == "East") {
        ModelModeHint::Gamepad
    } else if !right_keys.is_empty() {
        ModelModeHint::Keyboard
    } else if !left_keys.is_empty() {
        ModelModeHint::Standard
    } else {
        ModelModeHint::Unclassified
    };
    let companion_resources = CompanionResources {
        mode_hint,
        background,
        cover,
        left_keys,
        right_keys,
    };
    let inventory = reader.inventory()?;

    Ok(ModelPackageIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        model_version: model.version,
        entry: entry_name,
        moc,
        textures,
        display_info,
        expressions,
        motion_groups,
        physics,
        pose,
        user_data,
        groups,
        hit_areas,
        companion_resources,
        package_file_count: inventory.file_count,
        package_total_bytes: inventory.total_bytes,
        unreferenced_files: inventory.unreferenced_files,
    })
}

fn discover_entry(root: &Path) -> Result<PathBuf, ModelPackageError> {
    let entries = fs::read_dir(root).map_err(|error| {
        ModelPackageError::new(
            DiagnosticCode::ModelIoError,
            None,
            format!("package root cannot be listed: {error}"),
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                None,
                format!("package entry cannot be read: {error}"),
            )
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.ends_with(".model3.json") {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    match candidates.len() {
        0 => Err(ModelPackageError::new(
            DiagnosticCode::ModelEntryMissing,
            None,
            "package root has no .model3.json entry",
        )),
        1 => Ok(candidates.remove(0)),
        _ => Err(ModelPackageError::new(
            DiagnosticCode::ModelEntryAmbiguous,
            None,
            format!("package root has {} .model3.json entries", candidates.len()),
        )),
    }
}

fn normalize_reference(reference: &str) -> Result<String, ModelPackageError> {
    let normalized = reference.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains('\0')
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelReferenceEscapesRoot,
            None,
            "resource path is absolute, empty, or traverses outside the package",
        ));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part.contains(':') {
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelReferenceEscapesRoot,
                None,
                "resource path contains a platform path prefix",
            ));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelReferenceInvalid,
            Some(reference),
            "resource path does not name a file",
        ));
    }
    Ok(parts.join("/"))
}

fn path_from_normalized(reference: &str) -> PathBuf {
    reference.split('/').collect()
}

fn missing_code(kind: ResourceKind) -> DiagnosticCode {
    match kind {
        ResourceKind::Moc => DiagnosticCode::ModelMocMissing,
        ResourceKind::Texture => DiagnosticCode::ModelTextureMissing,
        ResourceKind::Other => DiagnosticCode::ModelResourceMissing,
    }
}

fn read_typed_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    invalid_code: DiagnosticCode,
) -> Result<T, ModelPackageError> {
    let bytes = read_bounded(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelJsonTooLarge,
    )?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ModelPackageError::new(
            invalid_code,
            Some(reference),
            format!("invalid JSON: {error}"),
        )
    })
}

fn read_json_object(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    invalid_code: DiagnosticCode,
) -> Result<(), ModelPackageError> {
    let value: serde_json::Value = read_typed_json(path, reference, maximum_bytes, invalid_code)?;
    if !value.is_object() {
        return Err(ModelPackageError::new(
            invalid_code,
            Some(reference),
            "resource JSON top level must be an object",
        ));
    }
    Ok(())
}

fn inspect_display_info_file(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<DisplayInfoInspection, ModelPackageError> {
    let display_info: DisplayInfoDefinition = read_typed_json(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelDisplayInfoInvalid,
    )?;
    if display_info.version != 3 {
        return resource_error(
            DiagnosticCode::ModelDisplayInfoInvalid,
            reference,
            format!("cdi3 version {} is not supported", display_info.version),
        );
    }

    let parameter_group_ids = collect_display_info_ids(
        display_info.parameter_groups.iter().map(|group| &group.id),
        reference,
        "parameter group",
    )?;
    for group in &display_info.parameter_groups {
        validate_display_info_group(
            &group.group_id,
            &group.id,
            &parameter_group_ids,
            reference,
            "parameter group",
        )?;
    }
    validate_display_info_group_cycles(&display_info.parameter_groups, reference)?;

    let parameter_ids = collect_display_info_ids(
        display_info
            .parameters
            .iter()
            .map(|parameter| &parameter.id),
        reference,
        "parameter",
    )?;
    for parameter in &display_info.parameters {
        validate_display_info_group(
            &parameter.group_id,
            &parameter.id,
            &parameter_group_ids,
            reference,
            "parameter",
        )?;
    }

    let part_ids = collect_display_info_ids(
        display_info.parts.iter().map(|part| &part.id),
        reference,
        "part",
    )?;

    Ok(DisplayInfoInspection {
        resource: DisplayInfoResource {
            file: reference.to_owned(),
            parameter_count: parameter_ids.len(),
            parameter_group_count: parameter_group_ids.len(),
            part_count: part_ids.len(),
        },
    })
}

fn collect_display_info_ids<'a>(
    items: impl IntoIterator<Item = &'a String>,
    reference: &str,
    label: &str,
) -> Result<BTreeSet<String>, ModelPackageError> {
    let mut ids = BTreeSet::new();
    for id in items {
        require_resource_identifier(
            id,
            DiagnosticCode::ModelDisplayInfoInvalid,
            reference,
            &format!("{label} Id"),
        )?;
        if !ids.insert(id.clone()) {
            return resource_error(
                DiagnosticCode::ModelDisplayInfoInvalid,
                reference,
                format!("{label} Ids must be unique"),
            );
        }
    }
    Ok(ids)
}

fn validate_display_info_group(
    group_id: &str,
    item_id: &str,
    known_groups: &BTreeSet<String>,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if group_id.is_empty() {
        return Ok(());
    }
    if group_id == item_id || !known_groups.contains(group_id) {
        return resource_error(
            DiagnosticCode::ModelDisplayInfoInvalid,
            reference,
            format!("{label} GroupId must reference a different parameter group"),
        );
    }
    Ok(())
}

fn validate_display_info_group_cycles(
    groups: &[DisplayInfoItem],
    reference: &str,
) -> Result<(), ModelPackageError> {
    let parents = groups
        .iter()
        .map(|group| (group.id.as_str(), group.group_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    for group in groups {
        let mut visited = BTreeSet::new();
        let mut current = group.id.as_str();
        while !current.is_empty() {
            if !visited.insert(current) {
                return resource_error(
                    DiagnosticCode::ModelDisplayInfoInvalid,
                    reference,
                    "parameter group hierarchy must not contain a cycle",
                );
            }
            current = parents.get(current).copied().unwrap_or("");
        }
    }
    Ok(())
}

pub fn inspect_physics_resource(
    path: impl AsRef<Path>,
    maximum_bytes: u64,
) -> Result<PhysicsResourceSummary, ModelPackageError> {
    inspect_physics_file(path.as_ref(), "physics3.json", maximum_bytes)
}

pub fn inspect_pose_resource(
    path: impl AsRef<Path>,
    maximum_bytes: u64,
) -> Result<PoseResourceSummary, ModelPackageError> {
    inspect_pose_file(path.as_ref(), "pose3.json", maximum_bytes)
}

fn inspect_pose_file(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<PoseResourceSummary, ModelPackageError> {
    let pose: PoseDefinition = read_typed_json(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelPoseInvalid,
    )?;
    if pose.pose_type != "Live2D Pose" {
        return resource_error(
            DiagnosticCode::ModelPoseInvalid,
            reference,
            "Type must be Live2D Pose",
        );
    }
    validate_resource_fade(
        pose.fade_in_seconds,
        DiagnosticCode::ModelPoseInvalid,
        reference,
        "FadeInTime",
    )?;
    if pose.groups.is_empty() {
        return resource_error(
            DiagnosticCode::ModelPoseInvalid,
            reference,
            "Groups must contain at least one pose group",
        );
    }

    let mut part_ids = BTreeSet::new();
    let mut part_count = 0usize;
    let mut link_count = 0usize;
    for group in &pose.groups {
        if group.is_empty() {
            return resource_error(
                DiagnosticCode::ModelPoseInvalid,
                reference,
                "pose groups must not be empty",
            );
        }
        part_count = part_count.checked_add(group.len()).ok_or_else(|| {
            ModelPackageError::new(
                DiagnosticCode::ModelPoseInvalid,
                Some(reference),
                "pose part count overflowed",
            )
        })?;
        for part in group {
            require_resource_identifier(
                &part.id,
                DiagnosticCode::ModelPoseInvalid,
                reference,
                "pose part Id",
            )?;
            if !part_ids.insert(part.id.as_str()) {
                return resource_error(
                    DiagnosticCode::ModelPoseInvalid,
                    reference,
                    "pose part Ids must be unique across groups",
                );
            }

            let mut links = BTreeSet::new();
            for link in &part.links {
                require_resource_identifier(
                    link,
                    DiagnosticCode::ModelPoseInvalid,
                    reference,
                    "pose link Id",
                )?;
                if link == &part.id {
                    return resource_error(
                        DiagnosticCode::ModelPoseInvalid,
                        reference,
                        "a pose part must not link to itself",
                    );
                }
                if !links.insert(link.as_str()) {
                    return resource_error(
                        DiagnosticCode::ModelPoseInvalid,
                        reference,
                        "pose link Ids must be unique within a part",
                    );
                }
                link_count = link_count.checked_add(1).ok_or_else(|| {
                    ModelPackageError::new(
                        DiagnosticCode::ModelPoseInvalid,
                        Some(reference),
                        "pose link count overflowed",
                    )
                })?;
            }
        }
    }

    Ok(PoseResourceSummary {
        fade_in_seconds: pose.fade_in_seconds,
        group_count: pose.groups.len(),
        part_count,
        link_count,
    })
}

fn inspect_physics_file(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<PhysicsResourceSummary, ModelPackageError> {
    let physics: PhysicsDefinition = read_typed_json(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelPhysicsInvalid,
    )?;
    if physics.version != 3 {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            format!("physics3 version {} is not supported", physics.version),
        );
    }
    if !physics.meta.fps.is_finite() || physics.meta.fps <= 0.0 {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            "Meta.Fps must be a finite positive number",
        );
    }
    validate_physics_vector(
        physics.meta.effective_forces.gravity,
        reference,
        "Meta.EffectiveForces.Gravity",
    )?;
    validate_physics_vector(
        physics.meta.effective_forces.wind,
        reference,
        "Meta.EffectiveForces.Wind",
    )?;

    let mut setting_ids = BTreeSet::new();
    let mut input_count = 0usize;
    let mut output_count = 0usize;
    let mut vertex_count = 0usize;
    for setting in &physics.settings {
        require_resource_identifier(
            &setting.id,
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            "physics setting Id",
        )?;
        if !setting_ids.insert(setting.id.clone()) {
            return resource_error(
                DiagnosticCode::ModelPhysicsInvalid,
                reference,
                "physics setting Ids must be unique",
            );
        }
        if setting.inputs.is_empty() || setting.outputs.is_empty() || setting.vertices.len() < 2 {
            return resource_error(
                DiagnosticCode::ModelPhysicsInvalid,
                reference,
                "each physics setting requires input, output, and at least two vertices",
            );
        }
        validate_physics_range(setting.normalization.position, reference, "Position")?;
        validate_physics_range(setting.normalization.angle, reference, "Angle")?;

        for input in &setting.inputs {
            require_resource_identifier(
                &input.source.id,
                DiagnosticCode::ModelPhysicsInvalid,
                reference,
                "input source Id",
            )?;
            validate_physics_weight(input.weight, reference, "input Weight")?;
        }
        for output in &setting.outputs {
            require_resource_identifier(
                &output.destination.id,
                DiagnosticCode::ModelPhysicsInvalid,
                reference,
                "output destination Id",
            )?;
            validate_physics_weight(output.weight, reference, "output Weight")?;
            if !output.scale.is_finite() {
                return resource_error(
                    DiagnosticCode::ModelPhysicsInvalid,
                    reference,
                    "output Scale must be finite",
                );
            }
            if output.vertex_index >= setting.vertices.len() {
                return resource_error(
                    DiagnosticCode::ModelPhysicsInvalid,
                    reference,
                    "output VertexIndex must reference a setting vertex",
                );
            }
        }
        for vertex in &setting.vertices {
            validate_physics_vector(vertex.position, reference, "vertex Position")?;
            if [
                vertex.mobility,
                vertex.delay,
                vertex.acceleration,
                vertex.radius,
            ]
            .iter()
            .any(|value| !value.is_finite())
            {
                return resource_error(
                    DiagnosticCode::ModelPhysicsInvalid,
                    reference,
                    "vertex coefficients must be finite",
                );
            }
        }
        input_count = input_count
            .checked_add(setting.inputs.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
        output_count = output_count
            .checked_add(setting.outputs.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
        vertex_count = vertex_count
            .checked_add(setting.vertices.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
    }

    let mut dictionary_ids = BTreeSet::new();
    for entry in &physics.meta.dictionary {
        require_resource_identifier(
            &entry.id,
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            "physics dictionary Id",
        )?;
        if !dictionary_ids.insert(entry.id.clone()) {
            return resource_error(
                DiagnosticCode::ModelPhysicsInvalid,
                reference,
                "physics dictionary Ids must be unique",
            );
        }
    }
    if dictionary_ids != setting_ids {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            "physics dictionary Ids must match setting Ids",
        );
    }
    if physics.meta.setting_count != physics.settings.len()
        || physics.meta.input_count != input_count
        || physics.meta.output_count != output_count
        || physics.meta.vertex_count != vertex_count
    {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            format!(
                "Meta counts are settings={} inputs={} outputs={} vertices={}; parsed settings={} inputs={} outputs={} vertices={}",
                physics.meta.setting_count,
                physics.meta.input_count,
                physics.meta.output_count,
                physics.meta.vertex_count,
                physics.settings.len(),
                input_count,
                output_count,
                vertex_count
            ),
        );
    }

    Ok(PhysicsResourceSummary {
        version: physics.version,
        fps: physics.meta.fps,
        setting_count: physics.settings.len(),
        input_count,
        output_count,
        vertex_count,
    })
}

fn validate_physics_vector(
    vector: PhysicsVector,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if !vector.x.is_finite() || !vector.y.is_finite() {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            format!("{label} must contain finite coordinates"),
        );
    }
    Ok(())
}

fn validate_physics_range(
    range: PhysicsRange,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if !range.minimum.is_finite()
        || !range.default.is_finite()
        || !range.maximum.is_finite()
        || range.minimum > range.default
        || range.default > range.maximum
    {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            format!("Normalization.{label} must satisfy finite minimum <= default <= maximum"),
        );
    }
    Ok(())
}

fn validate_physics_weight(
    weight: f64,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if !weight.is_finite() || !(0.0..=100.0).contains(&weight) {
        return resource_error(
            DiagnosticCode::ModelPhysicsInvalid,
            reference,
            format!("{label} must be finite and within 0..=100"),
        );
    }
    Ok(())
}

fn physics_count_overflow(reference: &str) -> ModelPackageError {
    ModelPackageError::new(
        DiagnosticCode::ModelPhysicsInvalid,
        Some(reference),
        "physics count overflowed",
    )
}

fn inspect_motion_file(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<MotionSummary, ModelPackageError> {
    let motion: MotionDefinition = read_typed_json(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelMotionInvalid,
    )?;
    if motion.version != 3 {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!("motion3 version {} is not supported", motion.version),
        );
    }
    if !motion.meta.duration.is_finite() || motion.meta.duration < 0.0 {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "Meta.Duration must be a finite non-negative number",
        );
    }
    if !motion.meta.fps.is_finite() || motion.meta.fps <= 0.0 {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "Meta.Fps must be a finite positive number",
        );
    }
    if motion.meta.curve_count != motion.curves.len() {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!(
                "Meta.CurveCount is {}; parsed {} curves",
                motion.meta.curve_count,
                motion.curves.len()
            ),
        );
    }

    let mut summary = MotionSummary {
        curve_count: motion.curves.len(),
        user_data_count: motion.user_data.len(),
        ..MotionSummary::default()
    };
    for curve in motion.curves {
        require_resource_identifier(
            &curve.id,
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "curve Id",
        )?;
        validate_resource_fade(
            curve.fade_in_seconds,
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "curve FadeInTime",
        )?;
        validate_resource_fade(
            curve.fade_out_seconds,
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "curve FadeOutTime",
        )?;
        let _target = curve.target;
        let curve_summary =
            inspect_motion_segments(&curve.segments, motion.meta.duration, reference)?;
        summary.segment_count = summary
            .segment_count
            .checked_add(curve_summary.segment_count)
            .ok_or_else(|| {
                ModelPackageError::new(
                    DiagnosticCode::ModelMotionInvalid,
                    Some(reference),
                    "motion segment count overflowed",
                )
            })?;
        summary.point_count = summary
            .point_count
            .checked_add(curve_summary.point_count)
            .ok_or_else(|| {
                ModelPackageError::new(
                    DiagnosticCode::ModelMotionInvalid,
                    Some(reference),
                    "motion point count overflowed",
                )
            })?;
    }

    if motion.meta.user_data_count != summary.user_data_count {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!(
                "Meta.UserDataCount is {}; parsed {} entries",
                motion.meta.user_data_count, summary.user_data_count
            ),
        );
    }
    let mut user_data_bytes = 0usize;
    for user_data in motion.user_data {
        if !user_data.time.is_finite()
            || user_data.time < 0.0
            || user_data.time > motion.meta.duration
        {
            return resource_error(
                DiagnosticCode::ModelMotionInvalid,
                reference,
                "UserData.Time must be finite and within the motion duration",
            );
        }
        user_data_bytes = user_data_bytes
            .checked_add(user_data.value.len())
            .ok_or_else(|| {
                ModelPackageError::new(
                    DiagnosticCode::ModelMotionInvalid,
                    Some(reference),
                    "user data byte count overflowed",
                )
            })?;
    }
    if motion.meta._total_user_data_size != user_data_bytes {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!(
                "Meta.TotalUserDataSize is {}; parsed {user_data_bytes} UTF-8 bytes",
                motion.meta._total_user_data_size
            ),
        );
    }
    if motion.meta.total_segment_count != summary.segment_count
        || motion.meta.total_point_count != summary.point_count
    {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!(
                "Meta totals are segments={} points={}; parsed segments={} points={}",
                motion.meta.total_segment_count,
                motion.meta.total_point_count,
                summary.segment_count,
                summary.point_count
            ),
        );
    }
    Ok(summary)
}

fn inspect_motion_segments(
    segments: &[f64],
    duration: f64,
    reference: &str,
) -> Result<MotionSummary, ModelPackageError> {
    if segments.len() < 2 {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "curve Segments must begin with an initial time/value point",
        );
    }
    if segments.iter().any(|value| !value.is_finite()) {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            "curve Segments must contain only finite numbers",
        );
    }
    let mut previous_time = segments[0];
    validate_motion_time(previous_time, 0.0, duration, reference, "initial point")?;
    let mut index = 2usize;
    let mut segment_count = 0usize;
    let mut point_count = 1usize;
    while index < segments.len() {
        let segment_code = segments[index];
        if segment_code.fract() != 0.0 {
            return resource_error(
                DiagnosticCode::ModelMotionInvalid,
                reference,
                format!("segment code at index {index} must be an integer"),
            );
        }
        let (width, added_points, end_offset) = match segment_code as i32 {
            0 | 2 | 3 => (3usize, 1usize, 1usize),
            1 => (7usize, 3usize, 5usize),
            code => {
                return resource_error(
                    DiagnosticCode::ModelMotionInvalid,
                    reference,
                    format!("segment code {code} at index {index} is unsupported"),
                );
            }
        };
        if segments.len().saturating_sub(index) < width {
            return resource_error(
                DiagnosticCode::ModelMotionInvalid,
                reference,
                format!("segment at index {index} is truncated"),
            );
        }
        let end_time = segments[index + end_offset];
        validate_motion_time(end_time, previous_time, duration, reference, "segment end")?;
        if width == 7 {
            validate_motion_time(
                segments[index + 1],
                previous_time,
                end_time,
                reference,
                "Bezier control point 1",
            )?;
            validate_motion_time(
                segments[index + 3],
                previous_time,
                end_time,
                reference,
                "Bezier control point 2",
            )?;
        }
        segment_count += 1;
        point_count += added_points;
        previous_time = end_time;
        index += width;
    }
    Ok(MotionSummary {
        curve_count: 1,
        segment_count,
        point_count,
        user_data_count: 0,
    })
}

fn validate_motion_time(
    time: f64,
    minimum: f64,
    duration: f64,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    const TIME_TOLERANCE: f64 = 0.000_001;
    if !time.is_finite() || time + TIME_TOLERANCE < minimum || time > duration + TIME_TOLERANCE {
        return resource_error(
            DiagnosticCode::ModelMotionInvalid,
            reference,
            format!("{label} time {time} is outside [{minimum}, {duration}]"),
        );
    }
    Ok(())
}

fn inspect_expression_file(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<ExpressionSummary, ModelPackageError> {
    let expression: ExpressionDefinition = read_typed_json(
        path,
        reference,
        maximum_bytes,
        DiagnosticCode::ModelExpressionInvalid,
    )?;
    if expression.expression_type != "Live2D Expression" {
        return resource_error(
            DiagnosticCode::ModelExpressionInvalid,
            reference,
            "Type must be Live2D Expression",
        );
    }
    validate_resource_fade(
        expression.fade_in_seconds,
        DiagnosticCode::ModelExpressionInvalid,
        reference,
        "FadeInTime",
    )?;
    validate_resource_fade(
        expression.fade_out_seconds,
        DiagnosticCode::ModelExpressionInvalid,
        reference,
        "FadeOutTime",
    )?;

    let mut summary = ExpressionSummary {
        parameter_count: expression.parameters.len(),
        ..ExpressionSummary::default()
    };
    for parameter in expression.parameters {
        require_resource_identifier(
            &parameter.id,
            DiagnosticCode::ModelExpressionInvalid,
            reference,
            "parameter Id",
        )?;
        if !parameter.value.is_finite() {
            return resource_error(
                DiagnosticCode::ModelExpressionInvalid,
                reference,
                "parameter Value must be finite",
            );
        }
        *summary.blend_counts.entry(parameter.blend).or_insert(0) += 1;
    }
    Ok(summary)
}

fn require_resource_identifier(
    value: &str,
    code: DiagnosticCode,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if value.trim().is_empty() {
        return resource_error(code, reference, format!("{label} must not be blank"));
    }
    Ok(())
}

fn validate_resource_fade(
    value: Option<f64>,
    code: DiagnosticCode,
    reference: &str,
    label: &str,
) -> Result<(), ModelPackageError> {
    if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return resource_error(
            code,
            reference,
            format!("{label} must be a finite non-negative number"),
        );
    }
    Ok(())
}

fn resource_error<T>(
    code: DiagnosticCode,
    reference: &str,
    detail: impl Into<String>,
) -> Result<T, ModelPackageError> {
    Err(ModelPackageError::new(code, Some(reference), detail))
}

fn read_bounded(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    too_large_code: DiagnosticCode,
) -> Result<Vec<u8>, ModelPackageError> {
    let metadata = path.metadata().map_err(|error| {
        ModelPackageError::new(
            DiagnosticCode::ModelIoError,
            Some(reference),
            format!("resource metadata cannot be read: {error}"),
        )
    })?;
    if metadata.len() > maximum_bytes {
        return Err(ModelPackageError::new(
            too_large_code,
            Some(reference),
            format!(
                "resource is {} bytes; limit is {maximum_bytes} bytes",
                metadata.len()
            ),
        ));
    }
    fs::read(path).map_err(|error| {
        ModelPackageError::new(
            DiagnosticCode::ModelIoError,
            Some(reference),
            format!("resource cannot be read: {error}"),
        )
    })
}

fn read_png_dimensions(path: &Path, reference: &str) -> Result<(u32, u32), ModelPackageError> {
    let mut header = [0_u8; 24];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelTextureInvalidPng,
                Some(reference),
                format!("PNG header cannot be read: {error}"),
            )
        })?;
    if header[..8] != *b"\x89PNG\r\n\x1a\n" || header[12..16] != *b"IHDR" {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelTextureInvalidPng,
            Some(reference),
            "texture does not have a PNG IHDR header",
        ));
    }
    let width = u32::from_be_bytes(header[16..20].try_into().expect("fixed PNG width slice"));
    let height = u32::from_be_bytes(header[20..24].try_into().expect("fixed PNG height slice"));
    if width == 0 || height == 0 {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelTextureInvalidPng,
            Some(reference),
            "texture dimensions must be non-zero",
        ));
    }
    Ok((width, height))
}

fn require_identifier(value: &str, label: &str) -> Result<(), ModelPackageError> {
    if value.trim().is_empty() {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelJsonInvalid,
            None,
            format!("{label} must not be blank"),
        ));
    }
    Ok(())
}

fn validate_fade(value: Option<f64>, field: &str) -> Result<(), ModelPackageError> {
    if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelJsonInvalid,
            None,
            format!("{field} must be a finite non-negative number"),
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
) -> Result<(), ModelPackageError> {
    if depth > limits.maximum_directory_depth {
        return Err(ModelPackageError::new(
            DiagnosticCode::ModelPackageDepthExceeded,
            None,
            format!(
                "package directory depth exceeds {}",
                limits.maximum_directory_depth
            ),
        ));
    }
    for entry in fs::read_dir(directory).map_err(|error| {
        ModelPackageError::new(
            DiagnosticCode::ModelIoError,
            None,
            format!("package directory cannot be listed: {error}"),
        )
    })? {
        let entry = entry.map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                None,
                format!("package directory entry cannot be read: {error}"),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            ModelPackageError::new(
                DiagnosticCode::ModelIoError,
                None,
                format!("package entry type cannot be read: {error}"),
            )
        })?;
        if file_type.is_symlink() {
            let canonical = path.canonicalize().map_err(|error| {
                ModelPackageError::new(
                    DiagnosticCode::ModelIoError,
                    None,
                    format!("package symlink cannot be resolved: {error}"),
                )
            })?;
            let reference = relative_reference(root, &path)?;
            if !canonical.starts_with(root) {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelReferenceSymlinkEscape,
                    Some(&reference),
                    "package symlink resolves outside the package root",
                ));
            }
            if canonical.is_dir() {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelSymlinkDirectoryUnsupported,
                    Some(&reference),
                    "symlinked directories are rejected to prevent recursive traversal",
                ));
            }
        }
        if path.is_dir() {
            collect_package_files(root, &path, depth + 1, limits, files)?;
        } else if path.is_file() {
            let reference = relative_reference(root, &path)?;
            let size = path
                .metadata()
                .map_err(|error| {
                    ModelPackageError::new(
                        DiagnosticCode::ModelIoError,
                        Some(&reference),
                        format!("package file metadata cannot be read: {error}"),
                    )
                })?
                .len();
            if size > limits.maximum_file_bytes {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelFileTooLarge,
                    Some(&reference),
                    format!(
                        "resource is {size} bytes; limit is {} bytes",
                        limits.maximum_file_bytes
                    ),
                ));
            }
            files.push((reference, size));
            if files.len() > limits.maximum_file_count {
                return Err(ModelPackageError::new(
                    DiagnosticCode::ModelFileCountExceeded,
                    None,
                    format!("package has more than {} files", limits.maximum_file_count),
                ));
            }
        } else {
            let reference = relative_reference(root, &path)?;
            return Err(ModelPackageError::new(
                DiagnosticCode::ModelResourceNotFile,
                Some(&reference),
                "package entry is not a regular file or directory",
            ));
        }
    }
    Ok(())
}

fn relative_reference(root: &Path, path: &Path) -> Result<String, ModelPackageError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        ModelPackageError::new(
            DiagnosticCode::ModelReferenceSymlinkEscape,
            None,
            "package entry is outside the package root",
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let part = component.as_os_str().to_str().ok_or_else(|| {
            ModelPackageError::new(
                DiagnosticCode::ModelReferenceInvalid,
                None,
                "package path is not valid UTF-8",
            )
        })?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    const MINIMAL_PHYSICS_JSON: &str = r#"{
        "Version":3,
        "Meta":{
            "PhysicsSettingCount":1,
            "TotalInputCount":1,
            "TotalOutputCount":1,
            "VertexCount":2,
            "Fps":60.0,
            "EffectiveForces":{
                "Gravity":{"X":0.0,"Y":-1.0},
                "Wind":{"X":0.0,"Y":0.0}
            },
            "PhysicsDictionary":[{"Id":"Physics1","Name":""}]
        },
        "PhysicsSettings":[{
            "Id":"Physics1",
            "Input":[{
                "Source":{"Target":"Parameter","Id":"ParamInput"},
                "Weight":100.0,
                "Type":"X",
                "Reflect":false
            }],
            "Output":[{
                "Destination":{"Target":"Parameter","Id":"ParamOutput"},
                "VertexIndex":1,
                "Scale":1.0,
                "Weight":100.0,
                "Type":"Angle",
                "Reflect":false
            }],
            "Vertices":[
                {"Position":{"X":0.0,"Y":0.0},"Mobility":0.8,"Delay":0.8,"Acceleration":1.0,"Radius":0.0},
                {"Position":{"X":0.0,"Y":10.0},"Mobility":0.8,"Delay":0.8,"Acceleration":1.0,"Radius":10.0}
            ],
            "Normalization":{
                "Position":{"Minimum":-10.0,"Default":0.0,"Maximum":10.0},
                "Angle":{"Minimum":-10.0,"Default":0.0,"Maximum":10.0}
            }
        }]
    }"#;

    const MINIMAL_POSE_JSON: &str = r#"{
        "Type":"Live2D Pose",
        "FadeInTime":0.5,
        "Groups":[[
            {"Id":"PartArmA","Link":["PartArmALink"]},
            {"Id":"PartArmB","Link":[]}
        ]]
    }"#;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("spike is nested two levels below repository root")
            .to_owned()
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create fixture destination");
        for entry in fs::read_dir(source).expect("list fixture source") {
            let entry = entry.expect("read fixture entry");
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if source_path.is_dir() {
                copy_tree(&source_path, &destination_path);
            } else {
                fs::copy(source_path, destination_path).expect("copy fixture file");
            }
        }
    }

    fn materialize_case(case: &Value) -> TempDir {
        let root = repository_root();
        let source = root
            .join("shared/fixtures/model-fixtures/cases")
            .join(case["directory"].as_str().expect("case directory"));
        let temporary = tempfile::tempdir().expect("create fixture temp directory");
        copy_tree(&source, temporary.path());

        if let (Some(source), Some(target)) = (
            case.get("entrySource").and_then(Value::as_str),
            case.get("materializedEntry").and_then(Value::as_str),
        ) {
            fs::copy(temporary.path().join(source), temporary.path().join(target))
                .expect("materialize model entry");
        }
        if let Some(materializations) = case.get("materialize").and_then(Value::as_array) {
            for materialization in materializations {
                let source = temporary.path().join(
                    materialization["source"]
                        .as_str()
                        .expect("materialization source"),
                );
                let target = temporary.path().join(
                    materialization["target"]
                        .as_str()
                        .expect("materialization target"),
                );
                fs::create_dir_all(target.parent().expect("materialization parent"))
                    .expect("create materialization parent");
                let hex = fs::read_to_string(source).expect("read fixture hex");
                let (pairs, remainder) = hex.trim().as_bytes().as_chunks::<2>();
                assert!(
                    remainder.is_empty(),
                    "fixture hex must contain complete bytes"
                );
                let bytes = pairs
                    .iter()
                    .map(|pair| {
                        u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                            .expect("valid fixture hex")
                    })
                    .collect::<Vec<_>>();
                fs::write(target, bytes).expect("write materialized file");
            }
        }
        temporary
    }

    #[test]
    fn synthetic_fixture_diagnostics_match_shared_contract() {
        let manifest_path = repository_root().join("shared/fixtures/model-fixtures/cases.json");
        let manifest: Value =
            serde_json::from_slice(&fs::read(manifest_path).expect("read cases manifest"))
                .expect("parse cases manifest");
        for case in manifest["cases"].as_array().expect("cases array") {
            let package = materialize_case(case);
            let result = inspect_model_package(package.path(), ModelPackageLimits::default());
            let expected = case["expectedDiagnostics"]
                .as_array()
                .expect("expected diagnostics")
                .iter()
                .map(|value| value.as_str().expect("diagnostic string"))
                .collect::<Vec<_>>();
            match result {
                Ok(_) => assert!(
                    expected.is_empty(),
                    "case {} unexpectedly accepted",
                    case["id"]
                ),
                Err(error) => assert_eq!(
                    vec![error.code.as_str()],
                    expected,
                    "case {} diagnostic mismatch",
                    case["id"]
                ),
            }
        }
    }

    #[test]
    fn preset_model_indices_match_frozen_snapshot() {
        let root = repository_root();
        let snapshot_path = root.join("shared/fixtures/model-fixtures/preset-model3-index.json");
        let expected: BTreeMap<String, ModelPackageIndex> =
            serde_json::from_slice(&fs::read(snapshot_path).expect("read preset snapshot"))
                .expect("parse preset snapshot");
        let actual = ["standard", "keyboard", "gamepad"]
            .into_iter()
            .map(|mode| {
                let package = root.join("src-tauri/assets/models").join(mode);
                let index = inspect_model_package(package, ModelPackageLimits::default())
                    .unwrap_or_else(|error| panic!("inspect {mode}: {error}"));
                (mode.to_owned(), index)
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected);
    }

    #[cfg(unix)]
    #[test]
    fn referenced_symlink_cannot_escape_package_root() {
        use std::os::unix::fs::symlink;

        let package = tempfile::tempdir().expect("create package");
        let outside = tempfile::NamedTempFile::new().expect("create outside resource");
        symlink(outside.path(), package.path().join("model.moc3")).expect("create symlink");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("write model entry");

        let error = inspect_model_package(package.path(), ModelPackageLimits::default())
            .expect_err("escaping symlink must fail");
        assert_eq!(error.code, DiagnosticCode::ModelReferenceSymlinkEscape);
    }

    #[test]
    fn package_inventory_rejects_directory_depth_before_recursing_further() {
        let package = tempfile::tempdir().expect("create package");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("write model entry");
        fs::write(package.path().join("model.moc3"), b"placeholder").expect("write moc");
        fs::create_dir_all(package.path().join("one/two")).expect("create nested package");
        fs::write(package.path().join("one/two/file.bin"), b"value").expect("write nested file");

        let limits = ModelPackageLimits {
            maximum_directory_depth: 1,
            ..ModelPackageLimits::default()
        };
        let error = inspect_model_package(package.path(), limits).expect_err("depth must fail");
        assert_eq!(error.code, DiagnosticCode::ModelPackageDepthExceeded);
    }

    #[test]
    fn optional_model3_resources_are_validated_and_indexed() {
        let package = tempfile::tempdir().expect("create package");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
                "Version": 3,
                "FileReferences": {
                    "Moc": "model.moc3",
                    "Textures": ["texture.png"],
                    "DisplayInfo": "display.cdi3.json",
                    "Expressions": [{"Name":"smile","File":"smile.exp3.json"}],
                    "Motions": {"Tap":[{"File":"tap.motion3.json","Sound":"tap.flac"}]},
                    "Physics": "model.physics3.json",
                    "Pose": "model.pose3.json",
                    "UserData": "model.userdata3.json"
                },
                "Groups": [{"Target":"Parameter","Name":"EyeBlink","Ids":["Eye"]}],
                "HitAreas": [{"Id":"Head","Name":"head"}]
            }"#,
        )
        .expect("write model entry");
        fs::write(package.path().join("model.moc3"), b"placeholder").expect("write moc");
        fs::write(package.path().join("model.pose3.json"), MINIMAL_POSE_JSON)
            .expect("write pose resource");
        fs::write(package.path().join("model.userdata3.json"), b"{}")
            .expect("write user data resource");
        fs::write(
            package.path().join("model.physics3.json"),
            MINIMAL_PHYSICS_JSON,
        )
        .expect("write physics resource");
        fs::write(
            package.path().join("display.cdi3.json"),
            br#"{
                "Version":3,
                "Parameters":[{"Id":"Eye","GroupId":"","Name":""}],
                "ParameterGroups":[],
                "Parts":[]
            }"#,
        )
        .expect("write display info resource");
        fs::write(
            package.path().join("smile.exp3.json"),
            br#"{"Type":"Live2D Expression","Parameters":[]}"#,
        )
        .expect("write expression resource");
        fs::write(
            package.path().join("tap.motion3.json"),
            br#"{
                "Version":3,
                "Meta":{
                    "Duration":0.1,
                    "Fps":30.0,
                    "Loop":false,
                    "AreBeziersRestricted":true,
                    "CurveCount":0,
                    "TotalSegmentCount":0,
                    "TotalPointCount":0,
                    "UserDataCount":0,
                    "TotalUserDataSize":0
                },
                "Curves":[]
            }"#,
        )
        .expect("write motion resource");
        fs::write(package.path().join("tap.flac"), b"placeholder").expect("write audio");
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&1_u32.to_be_bytes());
        png[20..24].copy_from_slice(&2_u32.to_be_bytes());
        fs::write(package.path().join("texture.png"), png).expect("write texture");

        let index = inspect_model_package(package.path(), ModelPackageLimits::default())
            .expect("inspect complete model3 package");
        assert_eq!(index.textures[0].width, 1);
        assert_eq!(index.textures[0].height, 2);
        assert_eq!(index.display_info.as_ref().unwrap().parameter_count, 1);
        assert_eq!(index.physics.as_deref(), Some("model.physics3.json"));
        assert_eq!(index.pose.as_deref(), Some("model.pose3.json"));
        assert_eq!(index.user_data.as_deref(), Some("model.userdata3.json"));
        assert_eq!(
            index.motion_groups[0].motions[0].sound.as_deref(),
            Some("tap.flac")
        );
        assert_eq!(index.groups[0].ids, ["Eye"]);
        assert_eq!(index.hit_areas[0].id, "Head");
        assert!(index.unreferenced_files.is_empty());
    }

    #[test]
    fn physics_counts_dictionary_ranges_weights_and_vertices_are_validated() {
        let directory = tempfile::tempdir().expect("create physics directory");
        let path = directory.path().join("model.physics3.json");
        for (pointer, replacement, expected_detail) in [
            ("/Meta/TotalOutputCount", Value::from(2), "Meta counts"),
            (
                "/Meta/PhysicsDictionary/0/Id",
                Value::from("Other"),
                "dictionary Ids",
            ),
            (
                "/PhysicsSettings/0/Input/0/Weight",
                Value::from(101),
                "input Weight",
            ),
            (
                "/PhysicsSettings/0/Normalization/Position/Minimum",
                Value::from(1),
                "Normalization.Position",
            ),
            (
                "/PhysicsSettings/0/Output/0/VertexIndex",
                Value::from(2),
                "VertexIndex",
            ),
        ] {
            let mut physics: Value =
                serde_json::from_str(MINIMAL_PHYSICS_JSON).expect("parse minimal physics fixture");
            *physics
                .pointer_mut(pointer)
                .expect("fixture pointer exists") = replacement;
            fs::write(
                &path,
                serde_json::to_vec(&physics).expect("serialize invalid physics"),
            )
            .expect("write invalid physics");

            let error = inspect_physics_resource(&path, 16 * 1024)
                .expect_err("invalid physics resource must fail");
            assert_eq!(error.code, DiagnosticCode::ModelPhysicsInvalid);
            assert!(error.detail.contains(expected_detail), "{}", error.detail);
            assert_eq!(error.resource.as_deref(), Some("physics3.json"));
        }
    }

    #[test]
    fn pose_type_groups_ids_links_and_fade_are_validated() {
        let directory = tempfile::tempdir().expect("create pose directory");
        let path = directory.path().join("model.pose3.json");
        fs::write(&path, MINIMAL_POSE_JSON).expect("write valid pose");
        let summary = inspect_pose_resource(&path, 16 * 1024).expect("inspect valid pose resource");
        assert_eq!(summary.fade_in_seconds, Some(0.5));
        assert_eq!(summary.group_count, 1);
        assert_eq!(summary.part_count, 2);
        assert_eq!(summary.link_count, 1);

        for (pointer, replacement, expected_detail) in [
            ("/Type", Value::from("Other"), "Type"),
            ("/FadeInTime", Value::from(-1), "FadeInTime"),
            ("/Groups/0", Value::Array(Vec::new()), "must not be empty"),
            ("/Groups/0/1/Id", Value::from("PartArmA"), "must be unique"),
            ("/Groups/0/0/Link/0", Value::from(""), "must not be blank"),
            (
                "/Groups/0/0/Link/0",
                Value::from("PartArmA"),
                "must not link to itself",
            ),
        ] {
            let mut pose: Value =
                serde_json::from_str(MINIMAL_POSE_JSON).expect("parse minimal pose fixture");
            *pose.pointer_mut(pointer).expect("fixture pointer exists") = replacement;
            fs::write(
                &path,
                serde_json::to_vec(&pose).expect("serialize invalid pose"),
            )
            .expect("write invalid pose");

            let error = inspect_pose_resource(&path, 16 * 1024)
                .expect_err("invalid pose resource must fail");
            assert_eq!(error.code, DiagnosticCode::ModelPoseInvalid);
            assert!(error.detail.contains(expected_detail), "{}", error.detail);
            assert_eq!(error.resource.as_deref(), Some("pose3.json"));
        }
    }

    #[test]
    fn display_info_rejects_duplicate_ids_and_group_cycles() {
        let directory = tempfile::tempdir().expect("create display info directory");
        let path = directory.path().join("display.cdi3.json");
        fs::write(
            &path,
            br#"{
                "Version":3,
                "Parameters":[
                    {"Id":"Param","GroupId":"","Name":"first"},
                    {"Id":"Param","GroupId":"","Name":"second"}
                ],
                "ParameterGroups":[],
                "Parts":[]
            }"#,
        )
        .expect("write duplicate display info");

        let duplicate = inspect_display_info_file(&path, "display.cdi3.json", 1024)
            .expect_err("duplicate display info Id must fail");
        assert_eq!(duplicate.code, DiagnosticCode::ModelDisplayInfoInvalid);
        assert!(duplicate.detail.contains("unique"));

        fs::write(
            &path,
            br#"{
                "Version":3,
                "Parameters":[{"Id":"Param","GroupId":"Missing","Name":"Param"}],
                "ParameterGroups":[],
                "Parts":[]
            }"#,
        )
        .expect("write dangling display info group");

        let dangling = inspect_display_info_file(&path, "display.cdi3.json", 1024)
            .expect_err("dangling display info GroupId must fail");
        assert_eq!(dangling.code, DiagnosticCode::ModelDisplayInfoInvalid);
        assert!(dangling.detail.contains("GroupId"));

        fs::write(
            &path,
            br#"{
                "Version":3,
                "Parameters":[],
                "ParameterGroups":[
                    {"Id":"A","GroupId":"B","Name":"A"},
                    {"Id":"B","GroupId":"A","Name":"B"}
                ],
                "Parts":[]
            }"#,
        )
        .expect("write cyclic display info");

        let cycle = inspect_display_info_file(&path, "display.cdi3.json", 1024)
            .expect_err("cyclic display info groups must fail");
        assert_eq!(cycle.code, DiagnosticCode::ModelDisplayInfoInvalid);
        assert!(cycle.detail.contains("cycle"));
    }

    #[test]
    fn malformed_associated_json_is_rejected_before_runtime_loading() {
        let package = tempfile::tempdir().expect("create package");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
                "Version": 3,
                "FileReferences": {
                    "Moc": "model.moc3",
                    "Textures": [],
                    "Motions": {"Tap":[{"File":"tap.motion3.json"}]}
                }
            }"#,
        )
        .expect("write model entry");
        fs::write(package.path().join("model.moc3"), b"placeholder").expect("write moc");
        fs::write(package.path().join("tap.motion3.json"), b"{").expect("write malformed motion");

        let error = inspect_model_package(package.path(), ModelPackageLimits::default())
            .expect_err("malformed associated JSON must fail");
        assert_eq!(error.code, DiagnosticCode::ModelMotionInvalid);
        assert_eq!(error.resource.as_deref(), Some("tap.motion3.json"));
    }

    #[test]
    fn all_preset_motion_and_expression_files_are_strongly_parsed() {
        let root = repository_root().join("src-tauri/assets/models");
        let mut motion_files = 0usize;
        let mut motion_summary = MotionSummary::default();
        let mut expression_files = 0usize;
        let mut expression_summary = ExpressionSummary::default();
        for mode in ["standard", "keyboard", "gamepad"] {
            for entry in fs::read_dir(root.join(mode)).expect("list preset model") {
                let path = entry.expect("read preset entry").path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if name.ends_with(".motion3.json") {
                    let summary = inspect_motion_file(&path, name, 16 * 1024 * 1024)
                        .unwrap_or_else(|error| panic!("inspect {mode}/{name}: {error}"));
                    motion_files += 1;
                    motion_summary.curve_count += summary.curve_count;
                    motion_summary.segment_count += summary.segment_count;
                    motion_summary.point_count += summary.point_count;
                    motion_summary.user_data_count += summary.user_data_count;
                } else if name.ends_with(".exp3.json") {
                    let summary = inspect_expression_file(&path, name, 16 * 1024 * 1024)
                        .unwrap_or_else(|error| panic!("inspect {mode}/{name}: {error}"));
                    expression_files += 1;
                    expression_summary.parameter_count += summary.parameter_count;
                    for (blend, count) in summary.blend_counts {
                        *expression_summary.blend_counts.entry(blend).or_insert(0) += count;
                    }
                }
            }
        }
        assert_eq!(motion_files, 6);
        assert_eq!(motion_summary.curve_count, 12);
        assert_eq!(motion_summary.segment_count, 45);
        assert_eq!(motion_summary.point_count, 123);
        assert_eq!(motion_summary.user_data_count, 0);
        assert_eq!(expression_files, 15);
        assert_eq!(expression_summary.parameter_count, 15);
        assert_eq!(
            expression_summary.blend_counts,
            BTreeMap::from([(ExpressionBlend::Add, 9), (ExpressionBlend::Multiply, 6),])
        );
    }

    #[test]
    fn motion_meta_and_segment_encoding_must_match() {
        let package = tempfile::tempdir().expect("create package");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
                "Version":3,
                "FileReferences":{
                    "Moc":"model.moc3",
                    "Textures":[],
                    "Motions":{"Tap":[{"File":"tap.motion3.json"}]}
                }
            }"#,
        )
        .expect("write model entry");
        fs::write(package.path().join("model.moc3"), b"placeholder").expect("write moc");
        fs::write(
            package.path().join("tap.motion3.json"),
            r#"{
                "Version":3,
                "Meta":{
                    "Duration":1.0,
                    "Fps":30.0,
                    "Loop":false,
                    "AreBeziersRestricted":true,
                    "CurveCount":1,
                    "TotalSegmentCount":1,
                    "TotalPointCount":2,
                    "UserDataCount":0,
                    "TotalUserDataSize":0
                },
                "Curves":[{"Target":"Parameter","Id":"Param","Segments":[0,0,1,0.5]}]
            }"#,
        )
        .expect("write truncated motion");

        let error = inspect_model_package(package.path(), ModelPackageLimits::default())
            .expect_err("truncated segment must fail");
        assert_eq!(error.code, DiagnosticCode::ModelMotionInvalid);
        assert_eq!(error.resource.as_deref(), Some("tap.motion3.json"));
        assert!(error.detail.contains("truncated"));
    }

    #[test]
    fn bezier_control_times_must_not_pass_the_segment_end() {
        let error = inspect_motion_segments(
            &[0.0, 0.0, 1.0, 0.25, 0.0, 1.25, 0.0, 0.5, 0.0],
            2.0,
            "tap.motion3.json",
        )
        .expect_err("Bezier control time after the segment end must fail");

        assert_eq!(error.code, DiagnosticCode::ModelMotionInvalid);
        assert_eq!(error.resource.as_deref(), Some("tap.motion3.json"));
        assert!(error.detail.contains("Bezier control point 2"));
    }

    #[test]
    fn expression_type_and_blend_are_strict() {
        let package = tempfile::tempdir().expect("create package");
        fs::write(
            package.path().join("cat.model3.json"),
            r#"{
                "Version":3,
                "FileReferences":{
                    "Moc":"model.moc3",
                    "Textures":[],
                    "Expressions":[{"Name":"bad","File":"bad.exp3.json"}]
                }
            }"#,
        )
        .expect("write model entry");
        fs::write(package.path().join("model.moc3"), b"placeholder").expect("write moc");
        fs::write(
            package.path().join("bad.exp3.json"),
            r#"{
                "Type":"Live2D Expression",
                "Parameters":[{"Id":"Param","Value":1.0,"Blend":"Unknown"}]
            }"#,
        )
        .expect("write invalid expression");

        let error = inspect_model_package(package.path(), ModelPackageLimits::default())
            .expect_err("unknown blend must fail");
        assert_eq!(error.code, DiagnosticCode::ModelExpressionInvalid);
        assert_eq!(error.resource.as_deref(), Some("bad.exp3.json"));
    }

    #[test]
    fn unsafe_paths_are_rejected_without_echoing_them() {
        for reference in [
            "/private/user/model.moc3",
            r"C:\Users\person\model.moc3",
            r"\\server\share\model.moc3",
            r"textures\..\outside.png",
        ] {
            let error = normalize_reference(reference).expect_err("unsafe path must fail");
            assert_eq!(error.code, DiagnosticCode::ModelReferenceEscapesRoot);
            assert_eq!(error.resource, None);
            assert!(!error.to_string().contains(reference));
        }
        assert_eq!(
            normalize_reference(r"textures\model.png").expect("normalize backslashes"),
            "textures/model.png"
        );
    }
}
