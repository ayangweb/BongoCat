use crate::{StorageLayout, UpdateErrorCode, VerifiedArtifact};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_STAGING_FILE_CREATE_ATTEMPTS: usize = 32;
static NEXT_STAGING_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStagingErrorCode {
    Cancelled,
    DirectoryCreateFailed,
    DirectoryInvalid,
    DirectoryPermissionFailed,
    FileCreateFailed,
    FileSyncFailed,
    FileWriteFailed,
    Integrity(UpdateErrorCode),
    RemoveFailed,
}

impl UpdateStagingErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "update_staging_cancelled",
            Self::DirectoryCreateFailed => "update_staging_directory_create_failed",
            Self::DirectoryInvalid => "update_staging_directory_invalid",
            Self::DirectoryPermissionFailed => "update_staging_directory_permission_failed",
            Self::FileCreateFailed => "update_staging_file_create_failed",
            Self::FileSyncFailed => "update_staging_file_sync_failed",
            Self::FileWriteFailed => "update_staging_file_write_failed",
            Self::Integrity(code) => code.as_str(),
            Self::RemoveFailed => "update_staging_remove_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateStagingError {
    pub code: UpdateStagingErrorCode,
}

impl fmt::Display for UpdateStagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateStagingError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedUpdateArtifact {
    path: PathBuf,
    byte_length: u64,
}

impl StagedUpdateArtifact {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }
}

impl VerifiedArtifact {
    pub fn stage_reader(
        &self,
        layout: &StorageLayout,
        mut reader: impl Read,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<StagedUpdateArtifact, UpdateStagingError> {
        ensure_staging_directory(&layout.update_staging)?;
        let (path, file) = create_staging_file(&layout.update_staging)?;
        let result = stage_into_file(self, file, &mut reader, &mut cancelled);
        match result {
            Ok(byte_length) => Ok(StagedUpdateArtifact { path, byte_length }),
            Err(error) => {
                if fs::remove_file(&path)
                    .is_err_and(|remove| remove.kind() != io::ErrorKind::NotFound)
                {
                    return Err(UpdateStagingError {
                        code: UpdateStagingErrorCode::RemoveFailed,
                    });
                }
                Err(error)
            }
        }
    }
}

fn next_staging_path(directory: &Path) -> PathBuf {
    let sequence = NEXT_STAGING_FILE.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(
        "artifact-{}-{sequence}.verified",
        std::process::id()
    ))
}

fn ensure_staging_directory(directory: &Path) -> Result<(), UpdateStagingError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(UpdateStagingError {
                code: UpdateStagingErrorCode::DirectoryInvalid,
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(directory).map_err(|_| UpdateStagingError {
                code: UpdateStagingErrorCode::DirectoryCreateFailed,
            })?;
            let metadata = fs::symlink_metadata(directory).map_err(|_| UpdateStagingError {
                code: UpdateStagingErrorCode::DirectoryInvalid,
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(UpdateStagingError {
                    code: UpdateStagingErrorCode::DirectoryInvalid,
                });
            }
        }
        Err(_) => {
            return Err(UpdateStagingError {
                code: UpdateStagingErrorCode::DirectoryInvalid,
            });
        }
    }
    set_private_directory(directory)
}

fn set_private_directory(directory: &Path) -> Result<(), UpdateStagingError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|_| {
            UpdateStagingError {
                code: UpdateStagingErrorCode::DirectoryPermissionFailed,
            }
        })?;
    }
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

fn create_staging_file(directory: &Path) -> Result<(PathBuf, File), UpdateStagingError> {
    for _ in 0..MAX_STAGING_FILE_CREATE_ATTEMPTS {
        let path = next_staging_path(directory);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
    Err(UpdateStagingError {
        code: UpdateStagingErrorCode::FileCreateFailed,
    })
}

fn stage_into_file(
    artifact: &VerifiedArtifact,
    mut file: File,
    reader: &mut impl Read,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<u64, UpdateStagingError> {
    let mut hasher = Sha256::new();
    let mut byte_length = 0_u64;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        if cancelled() {
            return Err(UpdateStagingError {
                code: UpdateStagingErrorCode::Cancelled,
            });
        }
        let read = reader.read(&mut buffer).map_err(|_| UpdateStagingError {
            code: UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactReadFailed),
        })?;
        if read == 0 {
            break;
        }
        byte_length = byte_length
            .checked_add(read as u64)
            .ok_or(UpdateStagingError {
                code: UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactLengthMismatch),
            })?;
        if byte_length > artifact.byte_length() {
            return Err(UpdateStagingError {
                code: UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactLengthMismatch),
            });
        }
        file.write_all(&buffer[..read])
            .map_err(|_| UpdateStagingError {
                code: UpdateStagingErrorCode::FileWriteFailed,
            })?;
        hasher.update(&buffer[..read]);
    }
    if cancelled() {
        return Err(UpdateStagingError {
            code: UpdateStagingErrorCode::Cancelled,
        });
    }
    if byte_length != artifact.byte_length() {
        return Err(UpdateStagingError {
            code: UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactLengthMismatch),
        });
    }
    if hasher.finalize().as_slice() != artifact.sha256 {
        return Err(UpdateStagingError {
            code: UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactHashMismatch),
        });
    }
    file.sync_all().map_err(|_| UpdateStagingError {
        code: UpdateStagingErrorCode::FileSyncFailed,
    })?;
    Ok(byte_length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::BuildEnvironment;
    use std::io::{self, Cursor};
    use tempfile::tempdir;

    fn artifact(bytes: &[u8]) -> VerifiedArtifact {
        VerifiedArtifact::from_test_bytes(
            crate::UpdateTarget::new(crate::TargetTriple::Aarch64AppleDarwin),
            "https://updates.example.invalid/bongocat.pkg",
            bytes,
        )
    }

    fn staging_files(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .expect("read staging directory")
            .map(|entry| entry.expect("staging entry").path())
            .collect()
    }

    #[test]
    fn stages_a_verified_artifact_in_the_environment_staging_directory() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let bytes = b"verified update artifact";

        let first = artifact(bytes)
            .stage_reader(&layout, Cursor::new(bytes), || false)
            .expect("stage first artifact");
        let second = artifact(bytes)
            .stage_reader(&layout, Cursor::new(bytes), || false)
            .expect("stage second artifact");

        assert_eq!(first.byte_length(), bytes.len() as u64);
        assert_eq!(fs::read(first.path()).expect("first artifact bytes"), bytes);
        assert_eq!(
            fs::read(second.path()).expect("second artifact bytes"),
            bytes
        );
        assert_ne!(first.path(), second.path());
        assert_eq!(first.path().parent(), Some(layout.update_staging.as_path()));
        assert_eq!(staging_files(&layout.update_staging).len(), 2);
    }

    #[test]
    fn cancellation_removes_the_partial_staging_file() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let mut checks = 0;

        let error = artifact(b"cancelled artifact")
            .stage_reader(&layout, Cursor::new(b"cancelled artifact"), || {
                checks += 1;
                checks > 1
            })
            .expect_err("cancelled staging");

        assert_eq!(error.code, UpdateStagingErrorCode::Cancelled);
        assert!(staging_files(&layout.update_staging).is_empty());
    }

    #[test]
    fn integrity_and_reader_failures_remove_partial_staging_files() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);

        let length_error = artifact(b"expected artifact")
            .stage_reader(&layout, Cursor::new(b"short"), || false)
            .expect_err("length mismatch");
        assert_eq!(
            length_error.code,
            UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactLengthMismatch)
        );
        assert!(staging_files(&layout.update_staging).is_empty());

        let hash_error = artifact(b"expected")
            .stage_reader(&layout, Cursor::new(b"wrong---"), || false)
            .expect_err("hash mismatch");
        assert_eq!(
            hash_error.code,
            UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactHashMismatch)
        );
        assert!(staging_files(&layout.update_staging).is_empty());

        let reader_error = artifact(b"expected")
            .stage_reader(&layout, FailingReader, || false)
            .expect_err("reader failure");
        assert_eq!(
            reader_error.code,
            UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactReadFailed)
        );
        assert!(staging_files(&layout.update_staging).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn repairs_the_private_staging_directory_and_creates_private_artifacts() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        fs::create_dir_all(&layout.update_staging).expect("create staging directory");
        fs::set_permissions(&layout.update_staging, fs::Permissions::from_mode(0o755))
            .expect("loosen staging permissions");

        let staged = artifact(b"private artifact")
            .stage_reader(&layout, Cursor::new(b"private artifact"), || false)
            .expect("stage artifact");

        assert_eq!(
            fs::metadata(&layout.update_staging)
                .expect("staging directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(staged.path())
                .expect("staged artifact metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic read failure"))
        }
    }
}
