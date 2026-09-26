//! Reading a legacy source without following anything out of it.
//!
//! The source is a folder the user picked, so every read resolves inside it and
//! refuses a symbolic link rather than following one, and every file is read
//! under a bound. The root is canonicalized once, because a temporary directory
//! on macOS is reached through a link and would otherwise reject its own files.

use super::*;

/// Where a legacy source's bytes are read from.
///
/// The folder a user exported is read in place: nothing is copied before the
/// conversion decides whether the source is a legacy one, and a source that
/// turns out to be a package is then imported by the ordinary path.
pub(crate) struct MverSource {
    pub(crate) root: PathBuf,
}

impl MverSource {
    /// Read a directory in place.
    ///
    /// The root is resolved once, here, because every read walks it: a source
    /// reached through a symbolic link (a temporary directory on macOS is the
    /// everyday case) would otherwise compare unresolved reads against an
    /// unresolved root and reject its own files.
    pub(crate) fn directory(root: impl AsRef<Path>) -> Result<Self, ModelStoreError> {
        let canonical = root.as_ref().canonicalize().map_err(|error| {
            conversion_error(
                None,
                format!("legacy source directory cannot be opened: {error}"),
            )
        })?;
        Ok(Self { root: canonical })
    }

    /// Whether `reference` names a regular file of the source.
    ///
    /// A symbolic link is reported as an unsupported source rather than
    /// followed or ignored: an overlay that silently disappears because the
    /// model reached outside itself is worse than a stable diagnostic.
    pub(crate) fn is_file(&self, reference: &str) -> Result<bool, ModelStoreError> {
        let Ok(metadata) = fs::symlink_metadata(self.root.join(path_from_reference(reference)))
        else {
            return Ok(false);
        };
        if metadata.file_type().is_symlink() {
            return Err(symlink_unsupported(reference));
        }
        Ok(metadata.is_file())
    }

    /// Whether `reference` names a directory that holds at least one entry.
    pub(crate) fn is_directory(&self, reference: &str) -> Result<bool, ModelStoreError> {
        let Ok(metadata) = fs::symlink_metadata(self.root.join(path_from_reference(reference)))
        else {
            return Ok(false);
        };
        if metadata.file_type().is_symlink() {
            return Err(symlink_unsupported(reference));
        }
        Ok(metadata.is_dir())
    }

    /// Every regular file below `prefix`, as package-relative references.
    ///
    /// A source that does not contain `prefix` is empty rather than an error:
    /// the caller is probing for folders the legacy application makes optional.
    pub(crate) fn files_below(
        &self,
        prefix: &str,
        limits: ModelPackageLimits,
    ) -> Result<Vec<String>, ModelStoreError> {
        let directory = self.root.join(path_from_reference(prefix));
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            return Ok(Vec::new());
        };
        if metadata.file_type().is_symlink() {
            return Err(symlink_unsupported(prefix));
        }
        if !metadata.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        collect_source_files(&directory, prefix, limits, &mut files)?;
        files.sort();
        Ok(files)
    }

    /// Read one source resource into memory.
    pub(crate) fn read(&self, reference: &str) -> Result<Vec<u8>, ModelStoreError> {
        read_source_file(&self.root, reference)
    }

    /// Read the marker file that decides whether this source is a legacy one.
    ///
    /// Detection is speculative, so "the file is not there" and "the file could
    /// not be read" are the same answer here: not a legacy source. Nothing is
    /// lost by treating them alike, because the package import path is what
    /// runs next and it reports the real diagnostic.
    pub(crate) fn read_legacy_config(&self) -> Option<Vec<u8>> {
        match self.is_file(LEGACY_CONFIG_FILE) {
            Ok(true) => self.read(LEGACY_CONFIG_FILE).ok(),
            _ => None,
        }
    }
}

pub(crate) fn symlink_unsupported(reference: &str) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::SourceSymlinkUnsupported,
        Some(reference.to_owned()),
        "legacy models are converted without following symbolic links",
    )
}

pub(crate) fn read_source_file(root: &Path, reference: &str) -> Result<Vec<u8>, ModelStoreError> {
    let candidate = root.join(path_from_reference(reference));
    let canonical = candidate.canonicalize().map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be opened: {error}"),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(symlink_unsupported(reference));
    }
    let metadata = canonical.metadata().map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be inspected: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(conversion_error(
            Some(reference),
            "legacy resource is not a regular file",
        ));
    }
    read_bounded(&canonical, reference, metadata.len())
}

/// Read a file whose size has already been checked against the legacy bound.
pub(crate) fn read_bounded(
    path: &Path,
    reference: &str,
    size: u64,
) -> Result<Vec<u8>, ModelStoreError> {
    if size > LEGACY_RESOURCE_MAXIMUM_BYTES {
        return Err(conversion_error(
            Some(reference),
            format!("legacy resource is {size} bytes"),
        ));
    }
    fs::read(path).map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be read: {error}"),
        )
    })
}

/// Collect the regular files below `directory`, as package-relative references.
///
/// Symbolic links are rejected rather than followed, for the same reason the
/// ordinary package import rejects them: a source that reaches outside itself
/// is not a model.
pub(crate) fn collect_source_files(
    directory: &Path,
    prefix: &str,
    limits: ModelPackageLimits,
    files: &mut Vec<String>,
) -> Result<(), ModelStoreError> {
    for entry in WalkDir::new(directory)
        .follow_links(false)
        .min_depth(1)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|error| source_walk_error(directory, prefix, error))?;
        let reference = source_reference(prefix, directory, entry.path())?;
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            return Err(symlink_unsupported(&reference));
        }
        if file_type.is_dir() {
            if entry.depth() > limits.maximum_directory_depth {
                return Err(conversion_error(
                    Some(&reference),
                    "legacy source is nested deeper than the package limit allows",
                ));
            }
        } else if file_type.is_file() {
            files.push(reference);
        }
    }
    Ok(())
}

pub(crate) fn source_walk_error(
    directory: &Path,
    prefix: &str,
    error: walkdir::Error,
) -> ModelStoreError {
    let resource = error
        .path()
        .and_then(|path| source_reference(prefix, directory, path).ok())
        .unwrap_or_else(|| prefix.to_owned());
    let detail = error
        .io_error()
        .map(ToString::to_string)
        .unwrap_or_else(|| "directory traversal failed".to_owned());
    let action = if error
        .path()
        .is_some_and(|path| path == directory || path.is_dir())
    {
        "legacy source directory cannot be listed"
    } else {
        "legacy source entry cannot be read"
    };
    conversion_error(Some(&resource), format!("{action}: {detail}"))
}

pub(crate) fn source_reference(
    prefix: &str,
    directory: &Path,
    path: &Path,
) -> Result<String, ModelStoreError> {
    let relative = path.strip_prefix(directory).map_err(|_| {
        conversion_error(
            Some(prefix),
            "legacy source directory traversal returned an entry outside its root",
        )
    })?;
    let mut reference = prefix.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(conversion_error(
                Some(prefix),
                "legacy source traversal returned a non-normal path component",
            ));
        };
        let name = name.to_str().ok_or_else(|| {
            conversion_error(None, "legacy source contains a non-UTF-8 entry name")
        })?;
        reference = join_reference(&reference, name);
    }
    Ok(reference)
}
