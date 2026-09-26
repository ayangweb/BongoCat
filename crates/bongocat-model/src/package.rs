//! Reading a package off disk, and refusing the ones that are not safe.
//!
//! A package is a directory tree the product did not create, so every path is
//! resolved inside it and every file is read under a byte limit. The inventory
//! is built from what is actually on disk rather than from what the index
//! declares, because the index is the part of a package a user can edit.

use super::*;

pub(crate) struct PackageReader {
    pub(crate) canonical_root: PathBuf,
    pub(crate) limits: ModelPackageLimits,
    pub(crate) referenced_files: BTreeSet<String>,
}

impl PackageReader {
    pub(crate) fn new(root: &Path, limits: ModelPackageLimits) -> Result<Self, ModelError> {
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

    pub(crate) fn resolve_file(
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

    pub(crate) fn resolve_display_info(&mut self, reference: &str) -> Result<String, ModelError> {
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

    pub(crate) fn resolve_expression(&mut self, reference: &str) -> Result<String, ModelError> {
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

    pub(crate) fn resolve_motion(&mut self, reference: &str) -> Result<String, ModelError> {
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

    pub(crate) fn resolve_pose(&mut self, reference: &str) -> Result<String, ModelError> {
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

    pub(crate) fn resolve_physics(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_physics_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    pub(crate) fn resolve_user_data(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_model_user_data_resource(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )?;
        Ok(normalized)
    }

    pub(crate) fn resolve_audio(&mut self, reference: &str) -> Result<String, ModelError> {
        let (normalized, path) =
            self.resolve_file(reference, ModelDiagnostic::ModelResourceMissing)?;
        validate_flac_resource(&path, &normalized)?;
        Ok(normalized)
    }

    pub(crate) fn resolve_image(&mut self, reference: &str) -> Result<ImageResource, ModelError> {
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

    pub(crate) fn inventory(&self) -> Result<PackageInventory, ModelError> {
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

pub(crate) struct PackageInventory {
    pub(crate) file_count: usize,
    pub(crate) total_bytes: u64,
    pub(crate) unreferenced_files: Vec<String>,
}

pub(crate) fn inspect_model_package(
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
        .map(|reference| reader.resolve_physics(reference))
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
        .map(|reference| reader.resolve_user_data(reference))
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

pub(crate) fn discover_entry(root: &Path) -> Result<PathBuf, ModelError> {
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

pub(crate) fn read_bounded(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, ModelError> {
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

pub(crate) fn collect_package_files(
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

pub(crate) fn relative_reference(root: &Path, path: &Path) -> Result<String, ModelError> {
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
