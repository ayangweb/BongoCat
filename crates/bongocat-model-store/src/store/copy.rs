//! Copying a staged package into place.
//!
//! The copy is the only part of an import that can take long enough for the user
//! to cancel, so it is the only part that reports bytes as it goes. It copies
//! rather than renames so a source on another volume still works, and it counts
//! what it wrote so a cancelled import can say how far it got.

use super::*;

#[derive(Default)]
pub(crate) struct CopyStatistics {
    pub(crate) file_count: usize,
    pub(crate) total_bytes: u64,
}

pub(crate) const COPY_BUFFER_BYTES: usize = 64 * 1024;

pub(crate) fn copy_package<Observe, IsCancelled>(
    source_root: &Path,
    source_directory: &Path,
    destination_directory: &Path,
    depth: usize,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
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
        observation.check_cancelled()?;
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
            set_private_directory(&destination).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(resource.clone()),
                    format!("staging directory permissions cannot be set: {error}"),
                )
            })?;
            copy_package(
                source_root,
                &source,
                &destination,
                depth + 1,
                limits,
                statistics,
                observation,
            )?;
        } else if file_type.is_file() {
            copy_file(
                source_root,
                &source,
                &destination,
                &resource,
                limits,
                statistics,
                observation,
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

pub(crate) fn copy_file<Observe, IsCancelled>(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    resource: &str,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
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
    set_private_file(&output).map_err(|error| {
        ModelStoreError::new(
            ModelStoreDiagnostic::IoError,
            Some(resource.to_owned()),
            format!("staging file permissions cannot be set: {error}"),
        )
    })?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        observation.check_cancelled()?;
        let read = input.read(&mut buffer).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file cannot be read: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(resource.to_owned()),
                format!("source file cannot be copied: {error}"),
            )
        })?;
        copied = copied.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if copied > size {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::SourceChanged,
                Some(resource.to_owned()),
                "source file grew while copying",
            ));
        }
        observation.report(ModelImportProgress {
            stage: ModelImportStage::Copying,
            files_copied: file_count_for_progress(statistics.file_count),
            bytes_copied: statistics.total_bytes.saturating_add(copied),
        });
    }
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
    observation.report(ModelImportProgress {
        stage: ModelImportStage::Copying,
        files_copied: file_count_for_progress(statistics.file_count),
        bytes_copied: statistics.total_bytes,
    });
    Ok(())
}
