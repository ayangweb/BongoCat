use crate::{UpdateChannel, update_channel};
use atomic_write_file::AtomicWriteFile;
use bongocat_config::StorageLayout;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::{self, OpenOptions, TryLockError},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

pub const UPDATE_SEQUENCE_STATE_SCHEMA_VERSION: u32 = 1;
const UPDATE_SEQUENCE_STATE_FILE: &str = "update-sequence.json";
const UPDATE_SEQUENCE_LOCK_FILE: &str = "update-sequence.lock";
const MAX_UPDATE_SEQUENCE_STATE_BYTES: u64 = 1024;

/// Stable, path-free error codes for the environment-local update sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateSequenceStoreErrorCode {
    DirectoryCreateFailed,
    DirectoryInvalid,
    DirectoryPermissionFailed,
    SequenceInvalid,
    SequenceRollbackDetected,
    StateChannelMismatch,
    StateInvalid,
    StateLockFailed,
    StateLockUnavailable,
    StateReadFailed,
    StateSchemaUnsupported,
    StateTooLarge,
    StateVerificationFailed,
    StateWriteFailed,
}

impl UpdateSequenceStoreErrorCode {
    pub const ALL: [Self; 14] = [
        Self::DirectoryCreateFailed,
        Self::DirectoryInvalid,
        Self::DirectoryPermissionFailed,
        Self::SequenceInvalid,
        Self::SequenceRollbackDetected,
        Self::StateChannelMismatch,
        Self::StateInvalid,
        Self::StateLockFailed,
        Self::StateLockUnavailable,
        Self::StateReadFailed,
        Self::StateSchemaUnsupported,
        Self::StateTooLarge,
        Self::StateVerificationFailed,
        Self::StateWriteFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectoryCreateFailed => "update_sequence_directory_create_failed",
            Self::DirectoryInvalid => "update_sequence_directory_invalid",
            Self::DirectoryPermissionFailed => "update_sequence_directory_permission_failed",
            Self::SequenceInvalid => "update_sequence_invalid",
            Self::SequenceRollbackDetected => "update_sequence_rollback_detected",
            Self::StateChannelMismatch => "update_sequence_channel_mismatch",
            Self::StateInvalid => "update_sequence_state_invalid",
            Self::StateLockFailed => "update_sequence_lock_failed",
            Self::StateLockUnavailable => "update_sequence_lock_unavailable",
            Self::StateReadFailed => "update_sequence_state_read_failed",
            Self::StateSchemaUnsupported => "update_sequence_schema_unsupported",
            Self::StateTooLarge => "update_sequence_state_too_large",
            Self::StateVerificationFailed => "update_sequence_state_verification_failed",
            Self::StateWriteFailed => "update_sequence_state_write_failed",
        }
    }
}

impl fmt::Display for UpdateSequenceStoreErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateSequenceStoreError {
    code: UpdateSequenceStoreErrorCode,
}

impl UpdateSequenceStoreError {
    const fn new(code: UpdateSequenceStoreErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> UpdateSequenceStoreErrorCode {
        self.code
    }
}

impl fmt::Display for UpdateSequenceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.code.fmt(formatter)
    }
}

impl std::error::Error for UpdateSequenceStoreError {}

#[derive(Clone, Debug)]
pub struct UpdateSequenceStore {
    channel: UpdateChannel,
    state_path: PathBuf,
    lock_path: PathBuf,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateSequenceState {
    schema_version: u32,
    channel: UpdateChannel,
    highest_verified_sequence: u64,
}

impl UpdateSequenceStore {
    /// Open the update store for the immutable environment represented by the
    /// product storage layout.
    pub fn open_for_layout(layout: &StorageLayout) -> Result<Self, UpdateSequenceStoreError> {
        Self::open(&layout.updates, update_channel(layout.environment))
    }

    fn open(
        directory: impl AsRef<Path>,
        channel: UpdateChannel,
    ) -> Result<Self, UpdateSequenceStoreError> {
        let directory = directory.as_ref();
        ensure_directory(directory)?;
        Ok(Self {
            channel,
            state_path: directory.join(UPDATE_SEQUENCE_STATE_FILE),
            lock_path: directory.join(UPDATE_SEQUENCE_LOCK_FILE),
        })
    }

    /// Return the highest manifest sequence successfully verified in this
    /// environment. A missing state is the initial sequence zero.
    pub fn highest_verified_sequence(&self) -> Result<u64, UpdateSequenceStoreError> {
        self.read_state()
            .map(|state| state.map_or(0, |state| state.highest_verified_sequence))
    }

    /// Persist a successful manifest verification. The stored sequence is
    /// monotonic: lower values are rejected and an equal value is idempotent.
    pub fn record_verified_sequence(&self, sequence: u64) -> Result<u64, UpdateSequenceStoreError> {
        if sequence == 0 {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::SequenceInvalid,
            ));
        }
        let lock = self.acquire_lock()?;
        let existing = self
            .read_state()?
            .map_or(0, |state| state.highest_verified_sequence);
        if sequence < existing {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::SequenceRollbackDetected,
            ));
        }
        if sequence == existing {
            return Ok(existing);
        }

        let state = UpdateSequenceState {
            schema_version: UPDATE_SEQUENCE_STATE_SCHEMA_VERSION,
            channel: self.channel,
            highest_verified_sequence: sequence,
        };
        let bytes = serde_json::to_vec(&state).map_err(|_| {
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateWriteFailed)
        })?;
        write_state_atomic(&self.state_path, &bytes)?;
        let verified = self.read_state()?.ok_or_else(|| {
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateVerificationFailed)
        })?;
        drop(lock);
        if verified != state {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateVerificationFailed,
            ));
        }
        Ok(sequence)
    }

    fn acquire_lock(&self) -> Result<std::fs::File, UpdateSequenceStoreError> {
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(|_| {
                UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateLockFailed)
            })?;
        match lock.try_lock() {
            Ok(()) => Ok(lock),
            Err(TryLockError::WouldBlock) => Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateLockUnavailable,
            )),
            Err(TryLockError::Error(_)) => Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateLockFailed,
            )),
        }
    }

    fn read_state(&self) -> Result<Option<UpdateSequenceState>, UpdateSequenceStoreError> {
        let metadata = match fs::symlink_metadata(&self.state_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(UpdateSequenceStoreError::new(
                    UpdateSequenceStoreErrorCode::StateReadFailed,
                ));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateInvalid,
            ));
        }
        if metadata.len() > MAX_UPDATE_SEQUENCE_STATE_BYTES {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateTooLarge,
            ));
        }
        let bytes = fs::read(&self.state_path).map_err(|_| {
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateReadFailed)
        })?;
        let state: UpdateSequenceState = serde_json::from_slice(&bytes).map_err(|_| {
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateInvalid)
        })?;
        if state.schema_version != UPDATE_SEQUENCE_STATE_SCHEMA_VERSION {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateSchemaUnsupported,
            ));
        }
        if state.channel != self.channel {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::StateChannelMismatch,
            ));
        }
        if state.highest_verified_sequence == 0 {
            return Err(UpdateSequenceStoreError::new(
                UpdateSequenceStoreErrorCode::SequenceInvalid,
            ));
        }
        Ok(Some(state))
    }
}

fn ensure_directory(directory: &Path) -> Result<(), UpdateSequenceStoreError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::DirectoryInvalid),
        ),
        Ok(_) => set_private_directory(directory),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(directory).map_err(|_| {
                UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::DirectoryCreateFailed)
            })?;
            set_private_directory(directory)
        }
        Err(_) => Err(UpdateSequenceStoreError::new(
            UpdateSequenceStoreErrorCode::DirectoryInvalid,
        )),
    }
}

fn set_private_directory(directory: &Path) -> Result<(), UpdateSequenceStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|_| {
            UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::DirectoryPermissionFailed)
        })?;
    }
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

fn write_state_atomic(path: &Path, bytes: &[u8]) -> Result<(), UpdateSequenceStoreError> {
    let mut file = AtomicWriteFile::open(path).map_err(|_| {
        UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateWriteFailed)
    })?;
    file.write_all(bytes).map_err(|_| {
        UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateWriteFailed)
    })?;
    file.commit()
        .map_err(|_| UpdateSequenceStoreError::new(UpdateSequenceStoreErrorCode::StateWriteFailed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::BuildEnvironment;
    use tempfile::tempdir;

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes = UpdateSequenceStoreErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .all(|code| code.starts_with("update_sequence_"))
        );
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UpdateSequenceStoreErrorCode::ALL.len());
    }

    #[test]
    fn persisted_sequence_survives_reopen_and_is_environment_bound() {
        let directory = tempdir().expect("temporary directory");
        let development = StorageLayout::under(directory.path(), BuildEnvironment::Development);
        let production_layout =
            StorageLayout::under(directory.path(), BuildEnvironment::Production);
        let store = UpdateSequenceStore::open_for_layout(&development).expect("development store");
        assert_eq!(
            store.highest_verified_sequence().expect("initial sequence"),
            0
        );
        assert_eq!(
            store.record_verified_sequence(7).expect("record sequence"),
            7
        );

        let reopened =
            UpdateSequenceStore::open_for_layout(&development).expect("reopen development store");
        assert_eq!(
            reopened
                .highest_verified_sequence()
                .expect("persisted sequence"),
            7
        );
        let production =
            UpdateSequenceStore::open_for_layout(&production_layout).expect("production store");
        assert_eq!(
            production
                .highest_verified_sequence()
                .expect("independent production state"),
            0
        );
        assert_ne!(development.updates, production_layout.updates);

        let mismatched = UpdateSequenceStore::open(&development.updates, UpdateChannel::Production)
            .expect("test-only mismatched store");
        assert_eq!(
            mismatched
                .highest_verified_sequence()
                .expect_err("cross-environment state")
                .code(),
            UpdateSequenceStoreErrorCode::StateChannelMismatch
        );
    }

    #[test]
    fn sequence_only_advances_and_equal_writes_are_idempotent() {
        let directory = tempdir().expect("temporary directory");
        let store = UpdateSequenceStore::open(directory.path(), UpdateChannel::Development)
            .expect("update store");
        assert_eq!(
            store.record_verified_sequence(5).expect("first sequence"),
            5
        );
        let before = fs::read(&store.state_path).expect("first state bytes");
        assert_eq!(
            store.record_verified_sequence(5).expect("equal sequence"),
            5
        );
        assert_eq!(
            fs::read(&store.state_path).expect("same state bytes"),
            before
        );
        assert_eq!(
            store
                .record_verified_sequence(4)
                .expect_err("rollback sequence")
                .code(),
            UpdateSequenceStoreErrorCode::SequenceRollbackDetected
        );
        assert_eq!(
            store.highest_verified_sequence().expect("stored sequence"),
            5
        );
    }

    #[test]
    fn malformed_or_future_state_is_rejected_without_resetting_it() {
        let directory = tempdir().expect("temporary directory");
        let store = UpdateSequenceStore::open(directory.path(), UpdateChannel::Development)
            .expect("update store");
        let malformed = b"not-json";
        fs::write(&store.state_path, malformed).expect("write malformed state");
        assert_eq!(
            store
                .record_verified_sequence(1)
                .expect_err("malformed state")
                .code(),
            UpdateSequenceStoreErrorCode::StateInvalid
        );
        assert_eq!(
            fs::read(&store.state_path).expect("unchanged state"),
            malformed
        );

        let future =
            br#"{"schema_version":2,"channel":"development","highest_verified_sequence":9}"#;
        fs::write(&store.state_path, future).expect("write future state");
        assert_eq!(
            store
                .highest_verified_sequence()
                .expect_err("future schema")
                .code(),
            UpdateSequenceStoreErrorCode::StateSchemaUnsupported
        );
        assert_eq!(
            fs::read(&store.state_path).expect("future state preserved"),
            future
        );
    }

    #[test]
    fn state_lock_serializes_writers() {
        let directory = tempdir().expect("temporary directory");
        let store = UpdateSequenceStore::open(directory.path(), UpdateChannel::Development)
            .expect("update store");
        let lock = store.acquire_lock().expect("acquire first lock");
        assert_eq!(
            store
                .record_verified_sequence(1)
                .expect_err("second writer")
                .code(),
            UpdateSequenceStoreErrorCode::StateLockUnavailable
        );
        drop(lock);
        assert_eq!(
            store.record_verified_sequence(1).expect("released writer"),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn store_rejects_symlinked_state() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let target = directory.path().join("target.json");
        fs::write(&target, b"{}").expect("write target");
        let store = UpdateSequenceStore::open(directory.path(), UpdateChannel::Development)
            .expect("update store");
        symlink(&target, &store.state_path).expect("symlink state");
        assert_eq!(
            store
                .highest_verified_sequence()
                .expect_err("symlink state")
                .code(),
            UpdateSequenceStoreErrorCode::StateInvalid
        );
    }

    #[cfg(unix)]
    #[test]
    fn store_creates_and_repairs_owner_only_update_directories() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(directory.path(), BuildEnvironment::Development);
        UpdateSequenceStore::open_for_layout(&layout).expect("update store");
        assert_eq!(
            fs::metadata(&layout.updates)
                .expect("update directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        fs::set_permissions(&layout.updates, fs::Permissions::from_mode(0o755))
            .expect("loosen permissions");
        let reopened = UpdateSequenceStore::open_for_layout(&layout).expect("reopen update store");
        assert_eq!(
            reopened
                .highest_verified_sequence()
                .expect("initial sequence"),
            0
        );
        assert_eq!(
            fs::metadata(&layout.updates)
                .expect("repaired directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
