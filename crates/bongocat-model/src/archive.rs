//! Model packages handed out as `.zip` archives.
//!
//! A user-supplied model reaches the store either as the directory they picked
//! or as the archive a model site handed out. An archive is deliberately *not* a
//! second package parser: it is only another way to materialize the same bytes.
//! Extraction therefore writes into the store's own staging directory and the
//! ordinary directory inspection then runs unchanged, so an archive cannot
//! bypass anything the directory path enforces.
//!
//! Two properties follow from that and drive the whole module:
//!
//! * Nothing is decompressed before the archive's own central directory has been
//!   checked against [`ModelPackageLimits`]. Entry names, kinds, counts, depth
//!   and declared sizes are all validated from metadata first, because a
//!   decompressor is exactly where a compression bomb would spend the memory.
//! * Every byte that is written is written *inside* the staging directory the
//!   store owns, so a rejected archive leaves no trace and a committed one is
//!   still the result of a single atomic rename.

use crate::{
    ModelPackageLimits, normalize_reference, path_from_reference,
    store::{
        COPY_BUFFER_BYTES, CopyStatistics, ImportObservation, ModelImportProgress,
        ModelImportStage, ModelStoreDiagnostic, ModelStoreError, file_count_for_progress,
        is_platform_metadata_name,
    },
};
use bongocat_storage::{set_private_directory, set_private_file};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive};

/// The AppleDouble sidecar tree macOS adds when it archives a folder. It is
/// archive tooling state, never model content.
const MACOSX_DIRECTORY: &str = "__MACOSX";

/// The four byte signatures that start a zip archive: a local file header, an
/// empty archive, and the marker a spanned archive starts with.
const ARCHIVE_SIGNATURES: [[u8; 4]; 3] = [*b"PK\x03\x04", *b"PK\x05\x06", *b"PK\x07\x08"];

/// How a user-supplied source is turned into package content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelSourceKind {
    Directory,
    ZipArchive,
}

/// Recognize the source without trusting its file extension: a directory is read
/// in place, a regular file is a model archive only when it really starts with a
/// zip signature. Detecting by content is what makes renaming an archive (or
/// dropping the extension) harmless, and it is also how a bare `.rar` or a
/// corrupted download fails with one stable code instead of a parse error.
pub(crate) fn detect_source_kind(
    canonical_source: &Path,
    limits: ModelPackageLimits,
) -> Result<ModelSourceKind, ModelStoreError> {
    let metadata = fs::metadata(canonical_source).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("model source cannot be inspected: {error}"),
        )
    })?;
    if metadata.is_dir() {
        return Ok(ModelSourceKind::Directory);
    }
    if !metadata.is_file() {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceEntryUnsupported,
            None,
            "model source is neither a directory nor a regular file",
        ));
    }

    if metadata.len() > limits.maximum_archive_bytes {
        // Bound the input before the archive reader parses (and therefore
        // allocates) its central directory.
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceArchiveUnsupported,
            None,
            "model archive is larger than the archive byte limit",
        ));
    }

    let mut signature = [0_u8; 4];
    let mut file = File::open(canonical_source).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("model source cannot be opened: {error}"),
        )
    })?;
    let read = file.read(&mut signature).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("model source cannot be read: {error}"),
        )
    })?;
    if read == signature.len() && ARCHIVE_SIGNATURES.contains(&signature) {
        return Ok(ModelSourceKind::ZipArchive);
    }
    Err(ModelStoreError::new(
        ModelStoreDiagnostic::SourceArchiveUnsupported,
        None,
        "selected file is not a zip archive",
    ))
}

/// One archive entry that survived the metadata and safety checks and still
/// carries both its archive identity and its package reference.
#[derive(Debug)]
struct CollectedEntry {
    /// Index in the archive, used to read the entry back during extraction.
    index: usize,
    /// The archive's own name, kept so extraction can prove it is still reading
    /// the entry that was planned.
    original_name: String,
    is_directory: bool,
    /// Bytes the central directory declares for this entry.
    declared_bytes: u64,
    /// Package-relative reference after normalization and wrapper removal.
    reference: String,
}

/// One entry the archive contributes to the package.
#[derive(Debug)]
struct PlannedEntry {
    /// Index in the archive, used to read the entry back during extraction.
    index: usize,
    /// The archive's own name, kept so extraction can prove it is still reading
    /// the entry that was planned.
    original_name: String,
    /// Package-relative reference after normalization and wrapper removal.
    reference: String,
    /// Bytes the central directory declares for this entry.
    bytes: u64,
}

/// Everything the archive declares, checked against the package limits but not
/// yet decompressed.
pub(crate) struct ArchivePlan {
    directories: Vec<PathBuf>,
    files: Vec<PlannedEntry>,
}

impl ArchivePlan {
    /// Whether the plan carries a file with this package reference.
    pub(crate) fn contains_file(&self, reference: &str) -> bool {
        self.files.iter().any(|file| file.reference == reference)
    }

    /// Every file reference the plan carries.
    pub(crate) fn file_references(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|file| file.reference.as_str())
    }

    /// The bytes the central directory declares for one planned entry.
    pub(crate) fn file_size(&self, reference: &str) -> Option<u64> {
        self.files
            .iter()
            .find(|file| file.reference == reference)
            .map(|file| file.bytes)
    }

    /// Read one planned entry without extracting the archive.
    ///
    /// The re-validation the full extraction performs applies here too: the
    /// entry must still be the planned one, must still declare the same size,
    /// and must deliver exactly that many bytes. A reader that takes a subset of
    /// the archive therefore cannot be fooled by an archive that changed
    /// underneath it either. Opening the archive again per entry costs one
    /// central-directory scan, which is what the two-pass extraction already
    /// pays once.
    pub(crate) fn read_file(
        &self,
        archive_path: &Path,
        reference: &str,
    ) -> Result<Vec<u8>, ModelStoreError> {
        let planned = self
            .files
            .iter()
            .find(|file| file.reference == reference)
            .ok_or_else(|| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::SourceChanged,
                    Some(reference.to_owned()),
                    "planned archive entry is missing",
                )
            })?;
        let file = File::open(archive_path).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(reference.to_owned()),
                format!("model archive cannot be reopened: {error}"),
            )
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.to_owned()),
                format!("model archive cannot be read again: {error}"),
            )
        })?;
        let mut input = archive.by_index(planned.index).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.to_owned()),
                format!("model archive entry cannot be read: {error}"),
            )
        })?;
        if input.name() != planned.original_name
            || input.size() != planned.bytes
            || !input.is_file()
        {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.to_owned()),
                "model archive entry changed after it was validated",
            ));
        }

        // The declared size is the allocation budget, capped at one buffer so an
        // inconsistent central directory cannot reserve an unreasonable amount
        // before any byte has been verified.
        let capacity = usize::try_from(planned.bytes).unwrap_or(usize::MAX);
        let mut bytes = Vec::with_capacity(capacity.min(COPY_BUFFER_BYTES));
        let mut buffer = [0_u8; COPY_BUFFER_BYTES];
        let mut read_total = 0_u64;
        loop {
            let read = input.read(&mut buffer).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::SourceArchiveUnsupported,
                    Some(reference.to_owned()),
                    format!("model archive entry cannot be decompressed: {error}"),
                )
            })?;
            if read == 0 {
                break;
            }
            read_total = read_total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
            if read_total > planned.bytes {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceChanged,
                    Some(reference.to_owned()),
                    "model archive entry delivered more bytes than it declared",
                ));
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        if read_total != planned.bytes {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.to_owned()),
                format!(
                    "model archive entry declared {} bytes but delivered {read_total}",
                    planned.bytes
                ),
            ));
        }
        Ok(bytes)
    }
}

/// Read the archive's central directory and validate it against the package
/// limits. No entry data is decompressed here, so a compression bomb is rejected
/// while it is still only a number in a header.
pub(crate) fn plan_archive(
    archive_path: &Path,
    limits: ModelPackageLimits,
) -> Result<ArchivePlan, ModelStoreError> {
    let file = File::open(archive_path).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("model archive cannot be opened: {error}"),
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::SourceArchiveUnsupported,
            None,
            format!("model archive cannot be read: {error}"),
        )
    })?;

    let declared = archive.len();
    if declared == 0 {
        return Err(archive_unsupported("model archive has no entries"));
    }
    // Structural ceiling applied before iterating: directory and file-manager
    // metadata entries do not count towards the package limit but do occupy
    // archive entries, so the ceiling has headroom over the usable file count.
    if declared > limits.maximum_file_count.saturating_mul(4) {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            None,
            "model archive declares more entries than the package limit allows",
        ));
    }

    let mut entries = Vec::with_capacity(declared);
    for index in 0..declared {
        // `by_index_raw` exposes the entry metadata without setting up a
        // decompressor, which is what keeps this pass cheap and bounded.
        let entry = archive.by_index_raw(index).map_err(|error| {
            archive_unsupported(format!("model archive entry is invalid: {error}"))
        })?;
        let name = entry.name();
        if name.is_empty() {
            return Err(archive_unsupported("model archive entry has a blank name"));
        }
        if entry.encrypted() {
            return Err(archive_unsupported(
                "encrypted model archives are not supported",
            ));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(archive_unsupported(
                "model archive uses an unsupported compression method",
            ));
        }
        if entry.is_symlink() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                Some(name.to_owned()),
                "model archives are imported without following symbolic links",
            ));
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceEntryUnsupported,
                    Some(name.to_owned()),
                    "model archive entry is not a regular file or directory",
                ));
            }
        }

        let is_directory = entry.is_dir();
        let declared_bytes = entry.size();
        let reference = normalize_archive_entry_name(name, is_directory)?;
        // Archive tooling state that no model can reference is dropped here for
        // the same reason the store ignores it inside an installed directory: it
        // belongs to the file manager, not to the package.
        if is_archive_metadata(&reference) {
            continue;
        }

        let depth = reference.split('/').count().saturating_sub(1);
        if depth > limits.maximum_directory_depth {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.clone()),
                "model archive entry is nested deeper than the package limit allows",
            ));
        }
        if !is_directory && declared_bytes > limits.maximum_file_bytes {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(reference.clone()),
                format!("model archive entry declares {declared_bytes} bytes"),
            ));
        }

        entries.push(CollectedEntry {
            index,
            original_name: name.to_owned(),
            is_directory,
            declared_bytes,
            reference,
        });
    }

    strip_wrapper_directories(&mut entries, limits.maximum_directory_depth);

    let mut plan = ArchivePlan {
        directories: Vec::new(),
        files: Vec::new(),
    };
    let mut declared_files = BTreeSet::new();
    let mut declared_directories = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for entry in entries {
        let CollectedEntry {
            index,
            original_name,
            is_directory,
            declared_bytes,
            reference: stripped,
        } = entry;
        if stripped.is_empty() {
            // The archive's own wrapper directory, which is already the package
            // root once the prefix is gone.
            continue;
        }
        if is_directory {
            // Repeated directory declarations are idempotent and harmless, so
            // only a path that is already a file is a conflict.
            if declared_files.contains(&stripped) {
                return Err(entry_conflict(&stripped));
            }
            if declared_directories.insert(stripped.clone()) {
                plan.directories.push(path_from_reference(&stripped));
            }
            continue;
        }
        if !declared_files.insert(stripped.clone()) {
            return Err(entry_conflict(&stripped));
        }
        if declared_directories.contains(&stripped) {
            return Err(entry_conflict(&stripped));
        }
        // A path that is also an ancestor of another path cannot be a file: the
        // two entries would overwrite or shadow each other.
        if has_file_ancestor(&stripped, &declared_files) {
            return Err(entry_conflict(&stripped));
        }
        total_bytes = total_bytes.checked_add(declared_bytes).ok_or_else(|| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(stripped.clone()),
                "model archive byte count overflowed",
            )
        })?;
        plan.files.push(PlannedEntry {
            index,
            original_name,
            reference: stripped,
            bytes: declared_bytes,
        });
    }

    if plan.files.is_empty() {
        return Err(archive_unsupported("model archive has no files"));
    }
    if plan.files.len() > limits.maximum_file_count {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            None,
            "model archive contains more files than the package limit allows",
        ));
    }
    if total_bytes > limits.maximum_package_bytes {
        return Err(ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            None,
            "model archive declares more bytes than the package limit allows",
        ));
    }
    Ok(plan)
}

/// Materialize a planned archive into `destination`, the store's own staging
/// directory.
///
/// The archive is re-read rather than kept open so that planning can reject a
/// bad archive before the store creates anything. Each entry is therefore
/// checked against the plan again before it is written: an archive that changed
/// between the two passes (or a reader that disagrees with the central
/// directory) fails with a stable diagnostic instead of producing a package that
/// only *partly* matches what was validated.
pub(crate) fn extract_archive<Observe, IsCancelled>(
    archive_path: &Path,
    plan: &ArchivePlan,
    destination: &Path,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    let file = File::open(archive_path).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            None,
            format!("model archive cannot be reopened: {error}"),
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::SourceChanged,
            None,
            format!("model archive cannot be read again: {error}"),
        )
    })?;

    let mut created = BTreeSet::new();
    for directory in &plan.directories {
        create_package_directory(destination, directory, &mut created)?;
    }

    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    for planned in &plan.files {
        observation.check_cancelled()?;
        let parent = planned
            .reference
            .rsplit_once('/')
            .map(|(parent, _)| PathBuf::from(parent));
        if let Some(parent) = parent {
            create_package_directory(destination, &parent, &mut created)?;
        }
        let target = destination.join(path_from_reference(&planned.reference));

        let mut input = archive.by_index(planned.index).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(planned.reference.clone()),
                format!("model archive entry cannot be read: {error}"),
            )
        })?;
        if input.name() != planned.original_name
            || input.size() != planned.bytes
            || !input.is_file()
        {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(planned.reference.clone()),
                "model archive entry changed after it was validated",
            ));
        }

        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(planned.reference.clone()),
                    format!("staging file cannot be created: {error}"),
                )
            })?;
        set_private_file(&output).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(planned.reference.clone()),
                format!("staging file permissions cannot be set: {error}"),
            )
        })?;

        let mut written = 0_u64;
        loop {
            observation.check_cancelled()?;
            let read = input.read(&mut buffer).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::SourceArchiveUnsupported,
                    Some(planned.reference.clone()),
                    format!("model archive entry cannot be decompressed: {error}"),
                )
            })?;
            if read == 0 {
                break;
            }
            written = written.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
            if written > planned.bytes || written > limits.maximum_file_bytes {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::SourceChanged,
                    Some(planned.reference.clone()),
                    "model archive entry delivered more bytes than it declared",
                ));
            }
            output.write_all(&buffer[..read]).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(planned.reference.clone()),
                    format!("staging file cannot be written: {error}"),
                )
            })?;
            observation.report(ModelImportProgress {
                stage: ModelImportStage::Copying,
                files_copied: file_count_for_progress(statistics.file_count),
                bytes_copied: statistics.total_bytes.saturating_add(written),
            });
        }
        if written != planned.bytes {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(planned.reference.clone()),
                format!(
                    "model archive entry declared {} bytes but delivered {written}",
                    planned.bytes
                ),
            ));
        }
        output
            .flush()
            .and_then(|()| output.sync_all())
            .map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(planned.reference.clone()),
                    format!("staging file cannot be flushed: {error}"),
                )
            })?;

        let next_total_bytes = statistics
            .total_bytes
            .checked_add(planned.bytes)
            .ok_or_else(|| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::SourceChanged,
                    Some(planned.reference.clone()),
                    "source package byte count overflowed",
                )
            })?;
        statistics.file_count += 1;
        statistics.total_bytes = next_total_bytes;
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Copying,
            files_copied: file_count_for_progress(statistics.file_count),
            bytes_copied: statistics.total_bytes,
        });
    }
    Ok(())
}

/// Create one package directory below `destination`, reusing whatever the
/// previous entries already created.
///
/// Parents are created lazily instead of relying on the archive declaring every
/// directory before the files inside it, which the zip format does not require.
pub(crate) fn create_package_directory(
    destination: &Path,
    relative: &Path,
    created: &mut BTreeSet<PathBuf>,
) -> Result<(), ModelStoreError> {
    let mut current = PathBuf::new();
    for component in relative.components() {
        current.push(component);
        let path = destination.join(&current);
        if created.contains(&current) {
            continue;
        }
        match fs::create_dir(&path) {
            Ok(()) => {
                set_private_directory(&path).map_err(|error| {
                    ModelStoreError::new(
                        ModelStoreDiagnostic::IoError,
                        None,
                        format!("staging directory permissions cannot be set: {error}"),
                    )
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Either a previous entry created it, or the archive declares a
                // file where this entry needs a directory. The plan already
                // rejects the latter, so an existing non-directory here means
                // the staged tree disagrees with the validated plan.
                if !path.is_dir() {
                    return Err(ModelStoreError::new(
                        ModelStoreDiagnostic::SourceChanged,
                        None,
                        "staging path is not a directory",
                    ));
                }
            }
            Err(error) => {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("staging directory cannot be created: {error}"),
                ));
            }
        }
        created.insert(current.clone());
    }
    Ok(())
}

/// Normalize an archive entry name into a package-relative reference, rejecting
/// everything a package reference may never contain: absolute paths, parent
/// traversal, platform prefixes and blank names. Reusing the same normalizer the
/// model3 resource references go through keeps one definition of a safe package
/// path inside the crate.
fn normalize_archive_entry_name(name: &str, is_directory: bool) -> Result<String, ModelStoreError> {
    let trimmed = if is_directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    normalize_reference(trimmed).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::SourceEntryUnsupported,
            Some(name.to_owned()),
            error.detail,
        )
    })
}

/// Archive tooling state that is never model content: the AppleDouble tree and
/// the sidecars, which the store already ignores inside an installed directory.
fn is_archive_metadata(reference: &str) -> bool {
    if reference.split('/').next() == Some(MACOSX_DIRECTORY) {
        return true;
    }
    let name = reference.rsplit('/').next().unwrap_or(reference);
    is_platform_metadata_name(name)
}

/// The single directory every entry sits inside, if there is one.
///
/// Archiving a folder prefixes every entry with that folder's name, so a model
/// archive normally holds the package one level down. That level is archive
/// tooling, not package content, and leaving it in place would make entry
/// discovery fail; stripping it is what makes `X/cat.model3.json` behave exactly
/// like a directory holding `cat.model3.json`.
///
/// A *file* directly at the archive root means the root already is the package,
/// so there is nothing to strip. A directory entry at the archive root is not
/// content of its own: it is either the wrapper naming itself, which is exactly
/// what has to go away, or a root directory that disagrees with the others and
/// then simply makes the archive ambiguous.
fn wrapper_prefix(entries: &[CollectedEntry]) -> Option<String> {
    let mut prefix: Option<&str> = None;
    for entry in entries {
        let reference = entry.reference.as_str();
        let head = reference.split('/').next()?;
        if !entry.is_directory && reference.len() == head.len() {
            return None;
        }
        match prefix {
            None => prefix = Some(head),
            Some(current) if current == head => {}
            Some(_) => return None,
        }
    }
    prefix.map(str::to_owned)
}

/// Remove wrapper directories repeatedly, because nested folders are archived
/// the same way one folder is. The loop stops on its own as soon as the
/// remaining entries no longer share a single head, and it is additionally
/// bounded by the package depth limit so a hostile archive cannot make it spin.
///
/// Every reference handed in has already been normalized by
/// [`normalize_archive_entry_name`], so removing a leading component cannot
/// leave a non-canonical spelling behind.
fn strip_wrapper_directories(entries: &mut Vec<CollectedEntry>, maximum_depth: usize) {
    for _ in 0..=maximum_depth {
        let Some(prefix) = wrapper_prefix(entries) else {
            break;
        };
        for entry in entries.iter_mut() {
            // A reference that is exactly the wrapper directory keeps nothing
            // once the prefix is gone and is dropped below.
            entry.reference = entry
                .reference
                .strip_prefix(&prefix)
                .and_then(|remainder| remainder.strip_prefix('/'))
                .unwrap_or_default()
                .to_owned();
        }
        entries.retain(|entry| !entry.reference.is_empty());
        if entries.is_empty() {
            break;
        }
    }
}

/// Whether any proper ancestor of `reference` was declared as a file.
fn has_file_ancestor(reference: &str, files: &BTreeSet<String>) -> bool {
    let mut end = 0;
    while let Some(offset) = reference[end..].find('/') {
        end += offset;
        if files.contains(&reference[..end]) {
            return true;
        }
        end += 1;
    }
    false
}

fn entry_conflict(reference: &str) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::SourceEntryUnsupported,
        Some(reference.to_owned()),
        "model archive declares the same path as both a file and a directory",
    )
}

fn archive_unsupported(detail: impl Into<String>) -> ModelStoreError {
    ModelStoreError::new(ModelStoreDiagnostic::SourceArchiveUnsupported, None, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(reference: &str, is_directory: bool) -> CollectedEntry {
        CollectedEntry {
            index: 0,
            original_name: reference.to_owned(),
            is_directory,
            declared_bytes: 0,
            reference: reference.to_owned(),
        }
    }

    fn references(entries: &[CollectedEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.reference.as_str())
            .collect()
    }

    #[test]
    fn wrapper_prefix_requires_one_directory_shared_by_every_entry() {
        assert_eq!(
            wrapper_prefix(&[
                entry("猫/cat.model3.json", false),
                entry("猫/moc.moc3", false)
            ])
            .as_deref(),
            Some("猫")
        );
        // The wrapper declaring itself as a directory is not package content and
        // must not stop the strip: this is exactly how "compress a folder" looks.
        assert_eq!(
            wrapper_prefix(&[entry("猫", true), entry("猫/cat.model3.json", false)]).as_deref(),
            Some("猫")
        );
        // A file at the archive root means the root already is the package.
        assert_eq!(
            wrapper_prefix(&[entry("cat.model3.json", false), entry("猫/moc.moc3", false)]),
            None
        );
        // Two root directories are ambiguous, not a wrapper.
        assert_eq!(
            wrapper_prefix(&[
                entry("a/cat.model3.json", false),
                entry("b/moc.moc3", false)
            ]),
            None
        );
        assert_eq!(wrapper_prefix(&[entry("猫", true)]).as_deref(), Some("猫"));
        assert_eq!(wrapper_prefix(&[]), None);
    }

    #[test]
    fn wrapper_stripping_is_repeated_but_stops_at_the_package_root() {
        let mut entries = vec![
            entry("models/猫/cat.model3.json", false),
            entry("models/猫/moc.moc3", false),
            entry("models/猫", true),
        ];
        strip_wrapper_directories(&mut entries, 32);
        assert_eq!(references(&entries), ["cat.model3.json", "moc.moc3"]);

        let mut entries = vec![entry("cat.model3.json", false), entry("textures", true)];
        strip_wrapper_directories(&mut entries, 32);
        assert_eq!(references(&entries), ["cat.model3.json", "textures"]);

        // An archive whose only entry is the wrapper directory keeps nothing.
        let mut entries = vec![entry("猫", true)];
        strip_wrapper_directories(&mut entries, 32);
        assert!(entries.is_empty());
    }

    #[test]
    fn file_ancestor_detection_covers_deep_conflicts() {
        let files = BTreeSet::from(["a".to_owned(), "b/c".to_owned()]);
        assert!(has_file_ancestor("a/b", &files));
        assert!(has_file_ancestor("b/c/d.png", &files));
        assert!(!has_file_ancestor("b/d.png", &files));
        assert!(!has_file_ancestor("c/d.png", &files));
    }

    #[test]
    fn archive_entry_names_are_normalized_or_rejected() {
        assert_eq!(
            normalize_archive_entry_name("猫\\textures/tex.png", false).expect("normalized"),
            "猫/textures/tex.png"
        );
        assert_eq!(
            normalize_archive_entry_name("textures/", true).expect("directory"),
            "textures"
        );
        // Two names that differ only in path spelling collapse onto one package
        // reference, which is what makes the duplicate check meaningful.
        assert_eq!(
            normalize_archive_entry_name("猫//model.moc3", false).expect("aliased"),
            normalize_archive_entry_name("猫/model.moc3", false).expect("plain")
        );
        for name in ["/etc/passwd", "../escape.moc3", r"C:\models\moc.moc3", ".."] {
            assert_eq!(
                normalize_archive_entry_name(name, false)
                    .expect_err("unsafe archive entry")
                    .code,
                ModelStoreDiagnostic::SourceEntryUnsupported,
                "entry {name}"
            );
        }
    }

    #[test]
    fn archive_metadata_is_recognized_at_any_depth() {
        assert!(is_archive_metadata("__MACOSX/猫/._cat.model3.json"));
        assert!(is_archive_metadata("猫/._cat.model3.json"));
        assert!(is_archive_metadata(".DS_Store"));
        assert!(is_archive_metadata("猫/.DS_Store"));
        assert!(!is_archive_metadata("猫/cat.model3.json"));
        assert!(!is_archive_metadata("__MACOSXish/cat.model3.json"));
    }
}
