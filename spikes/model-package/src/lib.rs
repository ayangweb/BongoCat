#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const INDEX_SCHEMA_VERSION: u32 = 1;

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
    pub display_info: Option<String>,
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
    let display_info = model
        .file_references
        .display_info
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let expressions = model
        .file_references
        .expressions
        .into_iter()
        .map(|resource| {
            require_identifier(&resource.name, "expression name")?;
            Ok(NamedResource {
                name: resource.name,
                file: reader.resolve_json(&resource.file)?,
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
                    Ok(MotionResource {
                        file: reader.resolve_json(&motion.file)?,
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
        .map(|reference| reader.resolve_json(reference))
        .transpose()?;
    let pose = model
        .file_references
        .pose
        .as_deref()
        .map(|reference| reader.resolve_json(reference))
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
                let bytes = hex
                    .trim()
                    .as_bytes()
                    .chunks_exact(2)
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
        for resource in [
            "display.cdi3.json",
            "smile.exp3.json",
            "tap.motion3.json",
            "model.physics3.json",
            "model.pose3.json",
            "model.userdata3.json",
        ] {
            fs::write(package.path().join(resource), b"{}").expect("write JSON resource");
        }
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
        assert_eq!(error.code, DiagnosticCode::ModelResourceJsonInvalid);
        assert_eq!(error.resource.as_deref(), Some("tap.motion3.json"));
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
