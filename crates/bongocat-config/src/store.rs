//! Reading and writing `config.json` under a revision check.
//!
//! Every write is: take the writer lock, refuse a stale expected revision, write a
//! temporary file beside the target, flush it, replace atomically, then read the
//! bytes back and verify them. A failure at any step leaves the previous file
//! exactly as it was, and a failure after the replace restores the previous bytes
//! rather than trusting a directory entry that may not be durable.

use super::*;
use crate::atomic::*;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigBackup {
    pub(crate) backup_format_version: u32,
    pub(crate) created_at_unix_ms: u64,
    pub(crate) source_schema_version: u32,
    pub(crate) source_revision: String,
    pub(crate) config: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigRevision(u64);

impl ConfigRevision {
    /// The revision a document's own bytes hash to.
    pub(crate) const fn from_hash(hash: u64) -> Self {
        Self(hash)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigRecovery {
    pub(crate) source_schema_version: u32,
    pub(crate) skipped_newer_backups: u32,
}

impl ConfigRecovery {
    pub const fn source_schema_version(self) -> u32 {
        self.source_schema_version
    }

    pub const fn skipped_newer_backups(self) -> u32 {
        self.skipped_newer_backups
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptedConfigRecovery {
    ArchivedStaleTemp,
    ArchivedInvalidTemp,
    PromotedTemp { replaced_invalid_current: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigLoadOutcome {
    pub config: NativeConfig,
    pub revision: ConfigRevision,
    pub recovery: Option<ConfigRecovery>,
    pub interrupted_recovery: Option<InterruptedConfigRecovery>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config I/O failed: {0}")]
    Io(io::Error),
    #[error("config JSON failed: {0}")]
    Json(serde_json::Error),
    #[error("config writer lock is unavailable")]
    LockUnavailable,
    #[error(
        "config revision conflict: expected {}, found {}",
        .expected.value(),
        .actual.value()
    )]
    RevisionConflict {
        expected: ConfigRevision,
        actual: ConfigRevision,
    },
    #[error("unsupported schema_version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid config value: {0}")]
    InvalidValue(&'static str),
    #[error("config backup exceeds retention budget")]
    BackupTooLarge,
    #[error("invalid config exceeds recovery archive budget")]
    RecoveryArchiveTooLarge,
    #[error("interrupted config exceeds archive budget")]
    InterruptedArchiveTooLarge,
    #[error("configuration write target is occupied")]
    WriteTargetOccupied,
    #[error("restored configuration failed verification")]
    RecoveryVerificationFailed,
}

impl From<io::Error> for ConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigWriteFailureReason {
    PermissionDenied,
    StorageFull,
    TargetOccupied,
}

impl ConfigError {
    pub fn write_failure_reason(&self) -> Option<ConfigWriteFailureReason> {
        match self {
            Self::WriteTargetOccupied => Some(ConfigWriteFailureReason::TargetOccupied),
            Self::Io(error) => match error.kind() {
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
                    Some(ConfigWriteFailureReason::PermissionDenied)
                }
                ErrorKind::StorageFull | ErrorKind::QuotaExceeded => {
                    Some(ConfigWriteFailureReason::StorageFull)
                }
                ErrorKind::AlreadyExists
                | ErrorKind::IsADirectory
                | ErrorKind::NotADirectory
                | ErrorKind::DirectoryNotEmpty => Some(ConfigWriteFailureReason::TargetOccupied),
                _ => None,
            },
            _ => None,
        }
    }
}

pub(crate) struct WriterLock {
    pub(crate) _file: File,
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigFileStatus {
    Missing,
    Valid,
    Invalid,
    UnsupportedSchema(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterruptedArchiveKind {
    Stale,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigWriteStage {
    BeforeTempCreate,
    AfterTempCreate,
    AfterReplace,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InjectedConfigWriteFailure {
    PermissionDenied,
    StorageFull,
    VerificationCorruption,
}

pub struct ConfigStore {
    pub(crate) layout: StorageLayout,
    #[cfg(test)]
    pub(crate) injected_write_failure: Option<InjectedConfigWriteFailure>,
}

impl ConfigStore {
    pub fn new(layout: StorageLayout) -> Result<Self, ConfigError> {
        layout.create_directories()?;
        Ok(Self {
            layout,
            #[cfg(test)]
            injected_write_failure: None,
        })
    }

    pub const fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    #[cfg(test)]
    pub(crate) fn inject_write_failure(&mut self, failure: InjectedConfigWriteFailure) {
        self.injected_write_failure = Some(failure);
    }

    pub fn load_or_default(&self) -> Result<ConfigLoadOutcome, ConfigError> {
        let _lock = self.acquire_recovery_lock(RECOVERY_LOCK_TIMEOUT)?;
        let interrupted_recovery = self.recover_interrupted_commit_unlocked()?;
        let mut outcome = match fs::read(&self.layout.config) {
            Ok(bytes) => match parse_config(&bytes) {
                Ok((config, revision)) => Ok(ConfigLoadOutcome {
                    config,
                    revision,
                    recovery: None,
                    interrupted_recovery: None,
                }),
                Err(error @ ConfigError::UnsupportedSchema(_)) => Err(error),
                Err(_) => self.recover_from_backup_unlocked(&bytes),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let config = NativeConfig::default();
                let revision = self.commit_unlocked(&config)?;
                Ok(ConfigLoadOutcome {
                    config,
                    revision,
                    recovery: None,
                    interrupted_recovery: None,
                })
            }
            Err(error) => Err(error.into()),
        }?;
        outcome.interrupted_recovery = interrupted_recovery;
        Ok(outcome)
    }

    pub fn recover_interrupted_commit(
        &self,
    ) -> Result<Option<InterruptedConfigRecovery>, ConfigError> {
        let _lock = self.acquire_recovery_lock(RECOVERY_LOCK_TIMEOUT)?;
        self.recover_interrupted_commit_unlocked()
    }

    pub fn commit(&self, config: &NativeConfig) -> Result<ConfigRevision, ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        self.commit_unlocked(config)
    }

    pub fn commit_if_revision(
        &self,
        config: &NativeConfig,
        expected: ConfigRevision,
    ) -> Result<ConfigRevision, ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        let actual = self.read_revision()?;
        if actual != expected {
            return Err(ConfigError::RevisionConflict { expected, actual });
        }
        self.commit_unlocked(config)
    }

    pub(crate) fn acquire_writer_lock(&self) -> Result<WriterLock, ConfigError> {
        let path = self.layout.locks.join("config.writer.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        set_private_file(&file)?;
        match file.try_lock() {
            Ok(()) => Ok(WriterLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ConfigError::LockUnavailable),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    pub(crate) fn acquire_recovery_lock(
        &self,
        timeout: Duration,
    ) -> Result<WriterLock, ConfigError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        loop {
            match self.acquire_writer_lock() {
                Ok(lock) => return Ok(lock),
                Err(ConfigError::LockUnavailable) if Instant::now() < deadline => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    thread::sleep(RECOVERY_LOCK_RETRY_INTERVAL.min(remaining));
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn read_revision(&self) -> Result<ConfigRevision, ConfigError> {
        let bytes = fs::read(&self.layout.config)?;
        let (_, revision) = parse_config(&bytes)?;
        Ok(revision)
    }

    pub(crate) fn commit_unlocked(
        &self,
        config: &NativeConfig,
    ) -> Result<ConfigRevision, ConfigError> {
        config.validate()?;
        let bytes = serde_json::to_vec_pretty(config)?;
        let previous = match fs::read(&self.layout.config) {
            Ok(current) => {
                self.backup_current_unlocked(&current)?;
                Some(current)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        self.write_config_atomic(&self.layout.config, &bytes)?;
        let verification = fs::read(&self.layout.config)
            .map_err(ConfigError::from)
            .and_then(|verified| parse_config(&verified));
        let Ok((verified_config, revision)) = verification else {
            restore_config_bytes(&self.layout.config, previous.as_deref())?;
            return Err(ConfigError::RecoveryVerificationFailed);
        };
        if verified_config != *config {
            restore_config_bytes(&self.layout.config, previous.as_deref())?;
            return Err(ConfigError::RecoveryVerificationFailed);
        }
        Ok(revision)
    }

    pub(crate) fn write_config_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
        #[cfg(test)]
        if let Some(failure) = self.injected_write_failure {
            let verification_path = path.to_path_buf();
            return write_config_atomic_with_hook(path, bytes, move |stage| {
                match (failure, stage) {
                    (
                        InjectedConfigWriteFailure::PermissionDenied,
                        ConfigWriteStage::BeforeTempCreate,
                    ) => Err(io::Error::from(ErrorKind::PermissionDenied).into()),
                    (
                        InjectedConfigWriteFailure::StorageFull,
                        ConfigWriteStage::AfterTempCreate,
                    ) => Err(io::Error::from(ErrorKind::StorageFull).into()),
                    (
                        InjectedConfigWriteFailure::VerificationCorruption,
                        ConfigWriteStage::AfterReplace,
                    ) => {
                        fs::write(&verification_path, b"post-replace verification corruption")?;
                        Ok(())
                    }
                    _ => Ok(()),
                }
            });
        }
        write_config_atomic(path, bytes)
    }

    pub(crate) fn recover_interrupted_commit_unlocked(
        &self,
    ) -> Result<Option<InterruptedConfigRecovery>, ConfigError> {
        let temp_path = config_temp_path(&self.layout.config);
        let temp_status = inspect_config_file(&temp_path)?;
        match temp_status {
            ConfigFileStatus::Missing => return Ok(None),
            ConfigFileStatus::UnsupportedSchema(version) => {
                return Err(ConfigError::UnsupportedSchema(version));
            }
            ConfigFileStatus::Invalid => {
                let bytes = fs::read(&temp_path)?;
                self.archive_interrupted_temp_unlocked(
                    &temp_path,
                    &bytes,
                    InterruptedArchiveKind::Invalid,
                )?;
                return Ok(Some(InterruptedConfigRecovery::ArchivedInvalidTemp));
            }
            ConfigFileStatus::Valid => {}
        }

        let current_status = inspect_config_file(&self.layout.config)?;
        match current_status {
            ConfigFileStatus::Valid | ConfigFileStatus::UnsupportedSchema(_) => {
                let bytes = fs::read(&temp_path)?;
                self.archive_interrupted_temp_unlocked(
                    &temp_path,
                    &bytes,
                    InterruptedArchiveKind::Stale,
                )?;
                Ok(Some(InterruptedConfigRecovery::ArchivedStaleTemp))
            }
            ConfigFileStatus::Missing => {
                self.promote_interrupted_temp_unlocked(&temp_path, None)?;
                Ok(Some(InterruptedConfigRecovery::PromotedTemp {
                    replaced_invalid_current: false,
                }))
            }
            ConfigFileStatus::Invalid => {
                let invalid_current = fs::read(&self.layout.config)?;
                self.archive_invalid_config_unlocked(&invalid_current)?;
                self.promote_interrupted_temp_unlocked(&temp_path, Some(&invalid_current))?;
                Ok(Some(InterruptedConfigRecovery::PromotedTemp {
                    replaced_invalid_current: true,
                }))
            }
        }
    }

    pub(crate) fn promote_interrupted_temp_unlocked(
        &self,
        temp_path: &Path,
        replaced_current: Option<&[u8]>,
    ) -> Result<(), ConfigError> {
        let candidate = fs::read(temp_path)?;
        let (candidate_config, candidate_revision) = parse_config(&candidate)?;
        write_atomic(&self.layout.config, &candidate)?;

        let verification = fs::read(&self.layout.config)
            .map_err(ConfigError::from)
            .and_then(|verified| parse_config(&verified));
        if verification.as_ref().is_ok_and(|(config, revision)| {
            *config == candidate_config && *revision == candidate_revision
        }) {
            match fs::remove_file(temp_path) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    restore_config_bytes(&self.layout.config, replaced_current)?;
                    return Err(error.into());
                }
            }
        }

        restore_config_bytes(&self.layout.config, replaced_current)?;
        Err(ConfigError::RecoveryVerificationFailed)
    }

    pub(crate) fn archive_interrupted_temp_unlocked(
        &self,
        temp_path: &Path,
        bytes: &[u8],
        kind: InterruptedArchiveKind,
    ) -> Result<(), ConfigError> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INTERRUPTED_ARCHIVE_BYTES {
            return Err(ConfigError::InterruptedArchiveTooLarge);
        }
        let created_at_unix_ms = unix_time_millis()?;
        let path = next_interrupted_archive_path(&self.layout.backups, created_at_unix_ms, kind)?;
        write_atomic(&path, bytes)?;
        prune_interrupted_archives(&self.layout.backups)?;
        fs::remove_file(temp_path)?;
        Ok(())
    }

    pub(crate) fn backup_current_unlocked(&self, current: &[u8]) -> Result<(), ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(current)?;
        let source_schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(ConfigError::InvalidValue("schema_version"))?;
        let (_, source_revision) = parse_config(current)?;
        let created_at_unix_ms = unix_time_millis()?;
        let backup = ConfigBackup {
            backup_format_version: BACKUP_FORMAT_VERSION,
            created_at_unix_ms,
            source_schema_version,
            source_revision: format!("{:016x}", source_revision.value()),
            config: value,
        };
        let bytes = serde_json::to_vec_pretty(&backup)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BACKUP_BYTES {
            return Err(ConfigError::BackupTooLarge);
        }
        let path = next_backup_path(&self.layout.backups, created_at_unix_ms)?;
        write_atomic(&path, &bytes)?;
        prune_config_backups(&self.layout.backups)
    }

    pub(crate) fn recover_from_backup_unlocked(
        &self,
        invalid_current: &[u8],
    ) -> Result<ConfigLoadOutcome, ConfigError> {
        let mut candidates = owned_config_backup_paths(&self.layout.backups)?;
        candidates.sort_by(|left, right| right.cmp(left));
        let mut skipped_newer_backups = 0_u32;

        for path in candidates {
            let bytes = fs::read(path)?;
            let Ok((config, revision, source_schema_version)) = validate_config_backup(&bytes)
            else {
                skipped_newer_backups = skipped_newer_backups.saturating_add(1);
                continue;
            };
            let restored = serde_json::to_vec_pretty(&config)?;
            self.archive_invalid_config_unlocked(invalid_current)?;
            self.write_config_atomic(&self.layout.config, &restored)?;
            let verified = fs::read(&self.layout.config)?;
            let Ok((verified_config, verified_revision)) = parse_config(&verified) else {
                write_config_atomic(&self.layout.config, invalid_current)?;
                return Err(ConfigError::RecoveryVerificationFailed);
            };
            if verified_config != config || verified_revision != revision {
                write_config_atomic(&self.layout.config, invalid_current)?;
                return Err(ConfigError::RecoveryVerificationFailed);
            }
            return Ok(ConfigLoadOutcome {
                config,
                revision,
                recovery: Some(ConfigRecovery {
                    source_schema_version,
                    skipped_newer_backups,
                }),
                interrupted_recovery: None,
            });
        }

        self.restore_defaults_unlocked(invalid_current, None)
    }

    pub(crate) fn restore_defaults_unlocked(
        &self,
        invalid_current: &[u8],
        interrupted_recovery: Option<InterruptedConfigRecovery>,
    ) -> Result<ConfigLoadOutcome, ConfigError> {
        self.archive_invalid_config_unlocked(invalid_current)?;
        let config = NativeConfig::default();
        let bytes = serde_json::to_vec_pretty(&config)?;
        self.write_config_atomic(&self.layout.config, &bytes)?;
        let verified = fs::read(&self.layout.config)?;
        let Ok((verified_config, revision)) = parse_config(&verified) else {
            restore_config_bytes(&self.layout.config, Some(invalid_current))?;
            return Err(ConfigError::RecoveryVerificationFailed);
        };
        if verified_config != config {
            restore_config_bytes(&self.layout.config, Some(invalid_current))?;
            return Err(ConfigError::RecoveryVerificationFailed);
        }
        Ok(ConfigLoadOutcome {
            config,
            revision,
            recovery: None,
            interrupted_recovery,
        })
    }

    pub(crate) fn archive_invalid_config_unlocked(
        &self,
        invalid_current: &[u8],
    ) -> Result<(), ConfigError> {
        if u64::try_from(invalid_current.len()).unwrap_or(u64::MAX) > MAX_CONFIG_QUARANTINE_BYTES {
            return Err(ConfigError::RecoveryArchiveTooLarge);
        }
        let created_at_unix_ms = unix_time_millis()?;
        let path = next_quarantine_path(&self.layout.backups, created_at_unix_ms)?;
        write_atomic(&path, invalid_current)?;
        prune_config_quarantines(&self.layout.backups)
    }
}
