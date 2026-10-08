//! Bounded, read-only discovery of model roots inside a selected folder.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSourceCandidate {
    pub source_root: PathBuf,
    pub relative_path: PathBuf,
    pub content: ModelSourceContent,
}

impl ModelStore {
    /// Find independent model roots, stopping at packages and Mver sources so
    /// their own assets and conversion modes never become extra candidates.
    pub fn discover_sources(
        &self,
        source_root: impl AsRef<Path>,
    ) -> Result<Vec<ModelSourceCandidate>, ModelStoreError> {
        let root = source_root.as_ref().canonicalize().map_err(discovery_io)?;
        if self.canonical_root.starts_with(&root) {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceContainsStore,
                None,
                "model source cannot contain the destination store",
            ));
        }
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        // Mver detection reads resource subtrees. Bound and validate the whole
        // selection first, including assets below roots discovery will prune.
        self.validate_discovery_tree(&root)?;
        let mut candidates = Vec::new();
        let mut remaining = self.limits.maximum_file_count;
        self.scan_sources(&root, &root, 0, &mut remaining, &mut candidates)?;
        candidates.sort_by(|left, right| left.source_root.cmp(&right.source_root));
        Ok(candidates)
    }

    fn validate_discovery_tree(&self, root: &Path) -> Result<(), ModelStoreError> {
        let mut remaining = self.limits.maximum_file_count;
        for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
            let entry = entry.map_err(|error| {
                ModelStoreError::new(ModelStoreDiagnostic::IoError, None, error.to_string())
            })?;
            remaining = remaining.checked_sub(1).ok_or_else(discovery_limit)?;
            if entry.depth() > self.limits.maximum_directory_depth {
                return Err(discovery_limit());
            }
            let kind = entry.file_type();
            if kind.is_symlink() {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceSymlinkUnsupported,
                    None,
                    "model discovery does not follow symbolic links",
                ));
            }
            if !kind.is_file() && !kind.is_dir() {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceEntryUnsupported,
                    None,
                    "model discovery accepts only regular files and directories",
                ));
            }
            if !entry
                .path()
                .canonicalize()
                .map_err(discovery_io)?
                .starts_with(root)
            {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceChanged,
                    None,
                    "model discovery resolved outside the selected folder",
                ));
            }
        }
        Ok(())
    }

    fn scan_sources(
        &self,
        selected: &Path,
        directory: &Path,
        depth: usize,
        remaining: &mut usize,
        candidates: &mut Vec<ModelSourceCandidate>,
    ) -> Result<(), ModelStoreError> {
        if depth > self.limits.maximum_directory_depth {
            return Err(discovery_limit());
        }
        let canonical = directory.canonicalize().map_err(discovery_io)?;
        if !canonical.starts_with(selected) {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                None,
                "model discovery resolved outside the selected folder",
            ));
        }
        let content = self.inspect_source(&canonical)?;
        if matches!(content, ModelSourceContent::Mver { .. }) {
            candidates.push(ModelSourceCandidate {
                relative_path: canonical
                    .strip_prefix(selected)
                    .expect("validated root")
                    .to_owned(),
                source_root: canonical,
                content,
            });
            return Ok(());
        }
        let mut children = Vec::new();
        let mut has_entry = false;
        for entry in fs::read_dir(&canonical).map_err(discovery_io)? {
            let entry = entry.map_err(discovery_io)?;
            *remaining = remaining.checked_sub(1).ok_or_else(discovery_limit)?;
            let kind = entry.file_type().map_err(discovery_io)?;
            if kind.is_symlink() {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceSymlinkUnsupported,
                    None,
                    "model discovery does not follow symbolic links",
                ));
            }
            if kind.is_dir() {
                children.push(entry.path());
            } else if kind.is_file() {
                has_entry |= entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".model3.json"));
            } else {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceEntryUnsupported,
                    None,
                    "model discovery accepts only regular files and directories",
                ));
            }
        }
        if has_entry {
            // A directly selected package keeps its existing diagnostic path.
            // Nested candidates must pass the same package and mode contracts
            // used by import; malformed packages are not selectable models.
            let valid = depth == 0
                || (PreparedModel::prepare(
                    ModelId::parse("discovery").expect("portable id"),
                    &canonical,
                    self.limits,
                )
                .is_ok()
                    && classify_input_mode(&canonical).is_ok());
            if valid {
                candidates.push(ModelSourceCandidate {
                    relative_path: canonical
                        .strip_prefix(selected)
                        .expect("validated root")
                        .to_owned(),
                    source_root: canonical,
                    content,
                });
                return Ok(());
            }
        }
        children.sort();
        for child in children {
            self.scan_sources(selected, &child, depth + 1, remaining, candidates)?;
        }
        Ok(())
    }
}

fn discovery_io(error: io::Error) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::IoError,
        None,
        format!("model folder cannot be read: {error}"),
    )
}

fn discovery_limit() -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::InvalidPackage,
        None,
        "model discovery exceeded the directory depth or entry budget",
    )
}
