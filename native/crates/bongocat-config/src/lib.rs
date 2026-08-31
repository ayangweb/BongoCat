#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod state;
pub use state::{
    ApplicationState, STATE_SCHEMA_VERSION, StateError, StateLoadOutcome, StateLoadStatus,
    StateStore, WindowPlacement,
};

pub const BUNDLE_ID: &str = "com.ayangweb.bongo-cat";
pub const SCHEMA_VERSION: u32 = 2;
const PREVIOUS_SCHEMA_VERSION: u32 = 1;
const BACKUP_FORMAT_VERSION: u32 = 1;
const MAX_CONFIG_BACKUPS: usize = 8;
const MAX_CONFIG_BACKUP_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONFIG_QUARANTINES: usize = 4;
const MAX_CONFIG_QUARANTINE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INTERRUPTED_ARCHIVES: usize = 4;
const MAX_INTERRUPTED_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;
const RECOVERY_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const RECOVERY_LOCK_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigBackup {
    backup_format_version: u32,
    created_at_unix_ms: u64,
    source_schema_version: u32,
    source_revision: String,
    config: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildEnvironment {
    Development,
    Production,
}

impl BuildEnvironment {
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageLayout {
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub models: PathBuf,
    pub backups: PathBuf,
    pub logs: PathBuf,
    pub locks: PathBuf,
}

impl StorageLayout {
    pub fn under_application_root(
        application_root: impl AsRef<Path>,
        environment: BuildEnvironment,
    ) -> Self {
        let root = application_root.as_ref().join(environment.directory_name());
        Self {
            config: root.join("config.json"),
            state: root.join("state.json"),
            models: root.join("models"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            locks: root.join("locks"),
            root,
        }
    }

    pub fn under(base: impl AsRef<Path>, environment: BuildEnvironment) -> Self {
        Self::under_application_root(base.as_ref().join(BUNDLE_ID), environment)
    }

    fn create_directories(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        for directory in [&self.models, &self.backups, &self.logs, &self.locks] {
            fs::create_dir_all(directory)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum PlatformStorageError {
    DataDirectoryUnavailable,
    UnsupportedPlatform,
}

impl fmt::Display for PlatformStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirectoryUnavailable => {
                formatter.write_str("platform data directory is unavailable")
            }
            Self::UnsupportedPlatform => formatter.write_str("platform is not supported"),
        }
    }
}

impl std::error::Error for PlatformStorageError {}

#[cfg(target_os = "macos")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    let root = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join(BUNDLE_ID);
    Ok(StorageLayout::under_application_root(root, environment))
}

#[cfg(target_os = "windows")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    let root = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join("BongoCat");
    Ok(StorageLayout::under_application_root(root, environment))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn platform_layout(
    _environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    Err(PlatformStorageError::UnsupportedPlatform)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NativeConfig {
    pub schema_version: u32,
    pub application: ApplicationConfig,
    pub appearance: AppearanceConfig,
    pub overlay: OverlayConfig,
    pub model: ModelConfig,
    pub shortcuts: ShortcutConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApplicationConfig {
    pub launch_at_login: bool,
    pub show_taskbar_icon: bool,
    pub show_status_icon: bool,
    pub check_for_updates_automatically: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppearanceConfig {
    pub theme: Theme,
    pub language: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    pub visible: bool,
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    pub corner_radius_percent: u8,
    pub hide_on_pointer_hover: bool,
    pub hide_on_pointer_hover_delay_ms: u32,
    pub keep_inside_work_area: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub selected_model_id: Option<String>,
    pub selected_model_origin: Option<SelectedModelOrigin>,
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    pub play_motion_audio: bool,
    pub enable_behavior_shortcuts: bool,
    pub maximum_fps: u16,
    pub ignore_pointer: bool,
    pub release_fallback_timeout_ms: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectedModelOrigin {
    Preset,
    Installed,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShortcutConfig {
    pub commands: Vec<ShortcutBinding>,
    pub model_behaviors: Vec<ModelBehaviorBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShortcutBinding {
    pub command: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelBehaviorBinding {
    pub model_id: String,
    pub behavior_id: String,
    pub shortcut: String,
}

impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            application: ApplicationConfig {
                launch_at_login: false,
                show_taskbar_icon: true,
                show_status_icon: true,
                check_for_updates_automatically: true,
            },
            appearance: AppearanceConfig {
                theme: Theme::System,
                language: "en-US".into(),
            },
            overlay: OverlayConfig {
                visible: true,
                click_through: true,
                always_on_top: true,
                scale_percent: 100,
                opacity_percent: 100,
                corner_radius_percent: 0,
                hide_on_pointer_hover: false,
                hide_on_pointer_hover_delay_ms: 250,
                keep_inside_work_area: true,
            },
            model: ModelConfig {
                selected_model_id: None,
                selected_model_origin: None,
                mirror: false,
                mirror_pointer_tracking: false,
                play_motion_audio: true,
                enable_behavior_shortcuts: true,
                maximum_fps: 60,
                ignore_pointer: false,
                release_fallback_timeout_ms: 500,
            },
            shortcuts: ShortcutConfig::default(),
        }
    }
}

impl NativeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        if !(25..=400).contains(&self.overlay.scale_percent) {
            return Err(ConfigError::InvalidValue("overlay.scale_percent"));
        }
        if !(1..=100).contains(&self.overlay.opacity_percent) {
            return Err(ConfigError::InvalidValue("overlay.opacity_percent"));
        }
        if self.overlay.corner_radius_percent > 100 {
            return Err(ConfigError::InvalidValue("overlay.corner_radius_percent"));
        }
        if !(15..=240).contains(&self.model.maximum_fps) {
            return Err(ConfigError::InvalidValue("model.maximum_fps"));
        }
        if self.model.release_fallback_timeout_ms > 60_000 {
            return Err(ConfigError::InvalidValue(
                "model.release_fallback_timeout_ms",
            ));
        }
        if self.appearance.language.trim().is_empty() {
            return Err(ConfigError::InvalidValue("appearance.language"));
        }
        if self
            .model
            .selected_model_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("model.selected_model_id"));
        }
        if self.model.selected_model_id.is_some() != self.model.selected_model_origin.is_some() {
            return Err(ConfigError::InvalidValue("model.selected_model_selection"));
        }
        if self
            .shortcuts
            .commands
            .iter()
            .any(|binding| binding.command.trim().is_empty() || binding.shortcut.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("shortcuts.commands"));
        }
        if self.shortcuts.model_behaviors.iter().any(|binding| {
            binding.model_id.trim().is_empty()
                || binding.behavior_id.trim().is_empty()
                || binding.shortcut.trim().is_empty()
        }) {
            return Err(ConfigError::InvalidValue("shortcuts.model_behaviors"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigRevision(u64);

impl ConfigRevision {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigRecovery {
    source_schema_version: u32,
    skipped_newer_backups: u32,
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

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Json(serde_json::Error),
    LockUnavailable,
    RevisionConflict {
        expected: ConfigRevision,
        actual: ConfigRevision,
    },
    UnsupportedSchema(u32),
    InvalidValue(&'static str),
    BackupTooLarge,
    RecoveryArchiveTooLarge,
    InterruptedArchiveTooLarge,
    WriteTargetOccupied,
    RecoveryNotRequired,
    NoValidRecoveryBackup {
        candidates: usize,
    },
    RecoveryVerificationFailed,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "config I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "config JSON failed: {error}"),
            Self::LockUnavailable => formatter.write_str("config writer lock is unavailable"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "config revision conflict: expected {}, found {}",
                expected.value(),
                actual.value()
            ),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported schema_version {version}")
            }
            Self::InvalidValue(field) => write!(formatter, "invalid config value: {field}"),
            Self::BackupTooLarge => formatter.write_str("config backup exceeds retention budget"),
            Self::RecoveryArchiveTooLarge => {
                formatter.write_str("invalid config exceeds recovery archive budget")
            }
            Self::InterruptedArchiveTooLarge => {
                formatter.write_str("interrupted config exceeds archive budget")
            }
            Self::WriteTargetOccupied => {
                formatter.write_str("configuration write target is occupied")
            }
            Self::RecoveryNotRequired => {
                formatter.write_str("configuration recovery is not required")
            }
            Self::NoValidRecoveryBackup { candidates } => write!(
                formatter,
                "invalid config has no valid recovery backup among {candidates} candidates"
            ),
            Self::RecoveryVerificationFailed => {
                formatter.write_str("restored configuration failed verification")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

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

struct WriterLock {
    _file: File,
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigFileStatus {
    Missing,
    Valid,
    Invalid,
    UnsupportedSchema(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterruptedArchiveKind {
    Stale,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigWriteStage {
    BeforeTempCreate,
    AfterTempCreate,
    AfterReplace,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InjectedConfigWriteFailure {
    PermissionDenied,
    StorageFull,
    VerificationCorruption,
}

pub struct ConfigStore {
    layout: StorageLayout,
    #[cfg(test)]
    injected_write_failure: Option<InjectedConfigWriteFailure>,
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
    fn inject_write_failure(&mut self, failure: InjectedConfigWriteFailure) {
        self.injected_write_failure = Some(failure);
    }

    pub fn load_or_default(&self) -> Result<ConfigLoadOutcome, ConfigError> {
        let _lock = self.acquire_recovery_lock(RECOVERY_LOCK_TIMEOUT)?;
        let interrupted_recovery = self.recover_interrupted_commit_unlocked()?;
        let mut outcome = match fs::read(&self.layout.config) {
            Ok(bytes) => match parse_config(&bytes) {
                Ok((config, revision, migrated)) => {
                    let revision = if migrated {
                        self.commit_unlocked(&config)?
                    } else {
                        revision
                    };
                    Ok(ConfigLoadOutcome {
                        config,
                        revision,
                        recovery: None,
                        interrupted_recovery: None,
                    })
                }
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

    pub fn restore_default_after_failed_recovery(&self) -> Result<ConfigLoadOutcome, ConfigError> {
        let _lock = self.acquire_recovery_lock(RECOVERY_LOCK_TIMEOUT)?;
        let interrupted_recovery = self.recover_interrupted_commit_unlocked()?;
        let invalid_current = match inspect_config_file(&self.layout.config)? {
            ConfigFileStatus::Invalid => fs::read(&self.layout.config)?,
            ConfigFileStatus::UnsupportedSchema(version) => {
                return Err(ConfigError::UnsupportedSchema(version));
            }
            ConfigFileStatus::Missing | ConfigFileStatus::Valid => {
                return Err(ConfigError::RecoveryNotRequired);
            }
        };
        self.archive_invalid_config_unlocked(&invalid_current)?;
        let config = NativeConfig::default();
        let bytes = serde_json::to_vec_pretty(&config)?;
        self.write_config_atomic(&self.layout.config, &bytes)?;
        let verified = fs::read(&self.layout.config)?;
        let Ok((verified_config, revision, false)) = parse_config(&verified) else {
            restore_config_bytes(&self.layout.config, Some(&invalid_current))?;
            return Err(ConfigError::RecoveryVerificationFailed);
        };
        if verified_config != config {
            restore_config_bytes(&self.layout.config, Some(&invalid_current))?;
            return Err(ConfigError::RecoveryVerificationFailed);
        }
        Ok(ConfigLoadOutcome {
            config,
            revision,
            recovery: None,
            interrupted_recovery,
        })
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

    fn acquire_writer_lock(&self) -> Result<WriterLock, ConfigError> {
        let path = self.layout.locks.join("config.writer.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(WriterLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ConfigError::LockUnavailable),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    fn acquire_recovery_lock(&self, timeout: Duration) -> Result<WriterLock, ConfigError> {
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

    fn read_revision(&self) -> Result<ConfigRevision, ConfigError> {
        let bytes = fs::read(&self.layout.config)?;
        let (_, revision, _) = parse_config(&bytes)?;
        Ok(revision)
    }

    fn commit_unlocked(&self, config: &NativeConfig) -> Result<ConfigRevision, ConfigError> {
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
        let Ok((verified_config, revision, false)) = verification else {
            restore_config_bytes(&self.layout.config, previous.as_deref())?;
            return Err(ConfigError::RecoveryVerificationFailed);
        };
        if verified_config != *config {
            restore_config_bytes(&self.layout.config, previous.as_deref())?;
            return Err(ConfigError::RecoveryVerificationFailed);
        }
        Ok(revision)
    }

    fn write_config_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
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

    fn recover_interrupted_commit_unlocked(
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

    fn promote_interrupted_temp_unlocked(
        &self,
        temp_path: &Path,
        replaced_current: Option<&[u8]>,
    ) -> Result<(), ConfigError> {
        let candidate = fs::read(temp_path)?;
        let (candidate_config, candidate_revision, _) = parse_config(&candidate)?;
        write_atomic(&self.layout.config, &candidate)?;

        let verification = fs::read(&self.layout.config)
            .map_err(ConfigError::from)
            .and_then(|verified| parse_config(&verified));
        if verification.as_ref().is_ok_and(|(config, revision, _)| {
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

    fn archive_interrupted_temp_unlocked(
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

    fn backup_current_unlocked(&self, current: &[u8]) -> Result<(), ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(current)?;
        let source_schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(ConfigError::InvalidValue("schema_version"))?;
        let (_, source_revision, _) = parse_config(current)?;
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

    fn recover_from_backup_unlocked(
        &self,
        invalid_current: &[u8],
    ) -> Result<ConfigLoadOutcome, ConfigError> {
        let mut candidates = owned_config_backup_paths(&self.layout.backups)?;
        candidates.sort_by(|left, right| right.cmp(left));
        let candidate_count = candidates.len();
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
            let Ok((verified_config, verified_revision, false)) = parse_config(&verified) else {
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

        Err(ConfigError::NoValidRecoveryBackup {
            candidates: candidate_count,
        })
    }

    fn archive_invalid_config_unlocked(&self, invalid_current: &[u8]) -> Result<(), ConfigError> {
        if u64::try_from(invalid_current.len()).unwrap_or(u64::MAX) > MAX_CONFIG_QUARANTINE_BYTES {
            return Err(ConfigError::RecoveryArchiveTooLarge);
        }
        let created_at_unix_ms = unix_time_millis()?;
        let path = next_quarantine_path(&self.layout.backups, created_at_unix_ms)?;
        write_atomic(&path, invalid_current)?;
        prune_config_quarantines(&self.layout.backups)
    }
}

fn unix_time_millis() -> Result<u64, ConfigError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))?
        .as_millis()
        .try_into()
        .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))
}

fn validate_config_backup(
    bytes: &[u8],
) -> Result<(NativeConfig, ConfigRevision, u32), ConfigError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BACKUP_BYTES {
        return Err(ConfigError::BackupTooLarge);
    }
    let backup: ConfigBackup = serde_json::from_slice(bytes)?;
    if backup.backup_format_version != BACKUP_FORMAT_VERSION || backup.created_at_unix_ms == 0 {
        return Err(ConfigError::InvalidValue("backup.format"));
    }
    let actual_schema_version = backup
        .config
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or(ConfigError::InvalidValue("backup.source_schema_version"))?;
    if actual_schema_version != backup.source_schema_version {
        return Err(ConfigError::InvalidValue("backup.source_schema_version"));
    }
    let config_bytes = serde_json::to_vec(&backup.config)?;
    let (config, revision, _) = parse_config(&config_bytes)?;
    if backup.source_revision != format!("{:016x}", revision.value()) {
        return Err(ConfigError::InvalidValue("backup.source_revision"));
    }
    Ok((config, revision, backup.source_schema_version))
}

fn owned_config_backup_paths(backups: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.file_name().to_str().is_some_and(is_owned_backup_name)
        {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

fn next_backup_path(backups: &Path, created_at_unix_ms: u64) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_backup_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!("config-{order_millis:020}-{sequence:05}.json"));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "config backup filename space exhausted",
    )))
}

fn next_quarantine_path(backups: &Path, created_at_unix_ms: u64) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_quarantine_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!(
            "config-corrupt-{order_millis:020}-{sequence:05}.bin"
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "config quarantine filename space exhausted",
    )))
}

fn next_interrupted_archive_path(
    backups: &Path,
    created_at_unix_ms: u64,
    kind: InterruptedArchiveKind,
) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_interrupted_archive_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    let kind = match kind {
        InterruptedArchiveKind::Stale => "stale",
        InterruptedArchiveKind::Invalid => "invalid",
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!(
            "config-interrupted-{kind}-{order_millis:020}-{sequence:05}.bin"
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "interrupted config archive filename space exhausted",
    )))
}

fn prune_config_backups(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_owned_backup_name(&name) {
            entries.push((name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total_bytes = entries.iter().map(|entry| entry.2).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_CONFIG_BACKUPS);
    let mut removed = 0_usize;
    for (_, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_CONFIG_BACKUP_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

fn prune_config_quarantines(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_owned_quarantine_name(&name) {
            entries.push((name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total_bytes = entries.iter().map(|entry| entry.2).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_CONFIG_QUARANTINES);
    let mut removed = 0_usize;
    for (_, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_CONFIG_QUARANTINE_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

fn prune_interrupted_archives(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_interrupted_archive_name(&name) {
            entries.push((order, name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut total_bytes = entries.iter().map(|entry| entry.3).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_INTERRUPTED_ARCHIVES);
    let mut removed = 0_usize;
    for (_, _, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_INTERRUPTED_ARCHIVE_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

fn is_owned_backup_name(name: &str) -> bool {
    parse_owned_backup_name(name).is_some()
}

fn parse_owned_backup_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-")
        .and_then(|name| name.strip_suffix(".json"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

fn is_owned_quarantine_name(name: &str) -> bool {
    parse_owned_quarantine_name(name).is_some()
}

fn parse_owned_quarantine_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-corrupt-")
        .and_then(|name| name.strip_suffix(".bin"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

fn parse_owned_interrupted_archive_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-interrupted-stale-")
        .or_else(|| name.strip_prefix("config-interrupted-invalid-"))
        .and_then(|name| name.strip_suffix(".bin"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

fn config_temp_path(config: &Path) -> PathBuf {
    config.with_extension("json.tmp")
}

fn inspect_config_file(path: &Path) -> Result<ConfigFileStatus, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ConfigFileStatus::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    match parse_config(&bytes) {
        Ok(_) => Ok(ConfigFileStatus::Valid),
        Err(ConfigError::UnsupportedSchema(version)) => {
            Ok(ConfigFileStatus::UnsupportedSchema(version))
        }
        Err(_) => Ok(ConfigFileStatus::Invalid),
    }
}

fn write_config_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    write_config_atomic_with_hook(path, bytes, |_| Ok(()))
}

fn write_config_atomic_with_hook(
    path: &Path,
    bytes: &[u8],
    mut hook: impl FnMut(ConfigWriteStage) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let previous = match fs::read(path) {
        Ok(previous) => Some(previous),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let temp_path = config_temp_path(path);
    let mut replaced = false;
    let mut temp_created = false;
    let result = (|| -> Result<(), ConfigError> {
        match fs::symlink_metadata(&temp_path) {
            Ok(_) => return Err(ConfigError::WriteTargetOccupied),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        hook(ConfigWriteStage::BeforeTempCreate)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp_created = true;
        hook(ConfigWriteStage::AfterTempCreate)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        write_atomic(path, bytes)?;
        replaced = true;
        hook(ConfigWriteStage::AfterReplace)?;
        fs::remove_file(&temp_path)?;
        Ok(())
    })();
    if let Err(error) = result {
        if replaced {
            restore_config_bytes(path, previous.as_deref())?;
        } else if temp_created {
            match fs::remove_file(&temp_path) {
                Ok(()) => {}
                Err(remove_error) if remove_error.kind() == ErrorKind::NotFound => {}
                Err(remove_error) => return Err(remove_error.into()),
            }
        }
        return Err(error);
    }
    Ok(())
}

fn restore_config_bytes(path: &Path, previous: Option<&[u8]>) -> Result<(), ConfigError> {
    match previous {
        Some(bytes) => write_atomic(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    write_atomic_io(path, bytes).map_err(ConfigError::from)
}

fn write_atomic_io(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.commit()?;
    Ok(())
}

fn parse_config(bytes: &[u8]) -> Result<(NativeConfig, ConfigRevision, bool), ConfigError> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or(ConfigError::InvalidValue("schema_version"))?;
    if schema_version != PREVIOUS_SCHEMA_VERSION && schema_version != SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema(schema_version));
    }
    let migrated = schema_version == PREVIOUS_SCHEMA_VERSION;
    if migrated {
        migrate_v1_to_v2(&mut value)?;
    }
    let config: NativeConfig = serde_json::from_value(value)?;
    config.validate()?;
    let normalized = serde_json::to_vec_pretty(&config)?;
    let revision = revision_for_bytes(&normalized);
    Ok((config, revision, migrated))
}

fn migrate_v1_to_v2(value: &mut serde_json::Value) -> Result<(), ConfigError> {
    let model = value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(ConfigError::InvalidValue("model"))?;
    if model.contains_key("selected_model_origin") {
        return Err(ConfigError::InvalidValue("model.selected_model_origin"));
    }
    let origin = if model
        .get("selected_model_id")
        .is_some_and(|id| !id.is_null())
    {
        serde_json::Value::String("preset".to_owned())
    } else {
        serde_json::Value::Null
    };
    model.insert("selected_model_origin".to_owned(), origin);
    value["schema_version"] = serde_json::Value::from(SCHEMA_VERSION);
    Ok(())
}

fn revision_for_bytes(bytes: &[u8]) -> ConfigRevision {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ConfigRevision(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use tempfile::tempdir;

    const CRASH_PROBE_BASE: &str = "BONGOCAT_CONFIG_CRASH_PROBE_BASE";
    const CRASH_PROBE_READY: &str = "BONGOCAT_CONFIG_CRASH_PROBE_READY";

    #[test]
    fn environments_have_identical_shape_and_disjoint_roots() {
        let base = tempdir().expect("temp directory");
        let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
        assert_ne!(development.root, production.root);
        assert_eq!(
            development.config.file_name(),
            production.config.file_name()
        );
        assert!(!development.root.starts_with(&production.root));
        assert!(!production.root.starts_with(&development.root));
    }

    #[test]
    fn load_creates_valid_default_and_commit_is_revision_checked() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let loaded = store.load_or_default().expect("default config");
        let mut config = loaded.config;
        let initial_revision = loaded.revision;
        assert_eq!(config, NativeConfig::default());

        config.overlay.visible = false;
        let next_revision = store
            .commit_if_revision(&config, initial_revision)
            .expect("revision checked commit");
        assert_ne!(next_revision, initial_revision);
        let stale = store.commit_if_revision(&config, initial_revision);
        assert!(matches!(stale, Err(ConfigError::RevisionConflict { .. })));
        assert_eq!(config_backup_paths(store.layout()).len(), 1);
    }

    #[test]
    fn injected_permission_and_storage_failures_preserve_current_and_clean_temp() {
        for (failure, expected_reason) in [
            (
                InjectedConfigWriteFailure::PermissionDenied,
                ConfigWriteFailureReason::PermissionDenied,
            ),
            (
                InjectedConfigWriteFailure::StorageFull,
                ConfigWriteFailureReason::StorageFull,
            ),
        ] {
            let base = tempdir().expect("temp directory");
            let mut store = ConfigStore::new(StorageLayout::under(
                base.path(),
                BuildEnvironment::Development,
            ))
            .expect("config store");
            let loaded = store.load_or_default().expect("default config");
            let original = fs::read(&store.layout().config).expect("original config");
            let mut next = loaded.config;
            next.overlay.visible = false;
            store.inject_write_failure(failure);

            let error = store
                .commit_if_revision(&next, loaded.revision)
                .expect_err("injected write failure");
            assert_eq!(error.write_failure_reason(), Some(expected_reason));
            assert_eq!(
                fs::read(&store.layout().config).expect("preserved current"),
                original
            );
            assert!(!config_temp_path(&store.layout().config).exists());
        }
    }

    #[test]
    fn occupied_temp_file_or_directory_is_retained_and_never_replaces_current() {
        for directory in [false, true] {
            let base = tempdir().expect("temp directory");
            let store = ConfigStore::new(StorageLayout::under(
                base.path(),
                BuildEnvironment::Production,
            ))
            .expect("config store");
            let loaded = store.load_or_default().expect("default config");
            let original = fs::read(&store.layout().config).expect("original config");
            let occupied = config_temp_path(&store.layout().config);
            if directory {
                fs::create_dir(&occupied).expect("occupied temp directory");
            } else {
                fs::write(&occupied, b"unowned occupied target").expect("occupied temp file");
            }
            let mut next = loaded.config;
            next.overlay.visible = false;

            let error = store
                .commit_if_revision(&next, loaded.revision)
                .expect_err("occupied target failure");
            assert!(matches!(error, ConfigError::WriteTargetOccupied));
            assert_eq!(
                error.write_failure_reason(),
                Some(ConfigWriteFailureReason::TargetOccupied)
            );
            assert_eq!(
                fs::read(&store.layout().config).expect("preserved current"),
                original
            );
            if directory {
                assert!(occupied.is_dir());
            } else {
                assert_eq!(
                    fs::read(&occupied).expect("preserved occupied file"),
                    b"unowned occupied target"
                );
            }
        }
    }

    #[test]
    fn interrupted_commit_preserves_valid_current_and_archives_stale_temp() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let current = store.load_or_default().expect("default config").config;
        let mut candidate = current.clone();
        candidate.appearance.language = "zh-CN".to_owned();
        let candidate_bytes = write_interrupted_temp(&store, &candidate);

        let recovered = store.load_or_default().expect("recover stale temp");
        assert_eq!(recovered.config, current);
        assert_eq!(
            recovered.interrupted_recovery,
            Some(InterruptedConfigRecovery::ArchivedStaleTemp)
        );
        assert_eq!(
            fs::read(
                interrupted_archive_paths(store.layout())
                    .first()
                    .expect("interrupted archive"),
            )
            .expect("interrupted archive bytes"),
            candidate_bytes
        );
        assert!(!config_temp_path(&store.layout().config).exists());

        let reloaded = store.load_or_default().expect("idempotent reload");
        assert_eq!(reloaded.interrupted_recovery, None);
        assert_eq!(interrupted_archive_paths(store.layout()).len(), 1);
    }

    #[test]
    fn interrupted_commit_promotes_valid_temp_when_current_is_missing_or_invalid() {
        for invalid_current in [None, Some(b"invalid-current".as_slice())] {
            let base = tempdir().expect("temp directory");
            let store = ConfigStore::new(StorageLayout::under(
                base.path(),
                BuildEnvironment::Production,
            ))
            .expect("config store");
            if let Some(bytes) = invalid_current {
                fs::write(&store.layout().config, bytes).expect("invalid current config");
            }
            let mut candidate = NativeConfig::default();
            candidate.overlay.scale_percent = if invalid_current.is_some() { 125 } else { 150 };
            write_interrupted_temp(&store, &candidate);

            let recovered = store.load_or_default().expect("promote interrupted temp");
            assert_eq!(recovered.config, candidate);
            assert_eq!(
                recovered.interrupted_recovery,
                Some(InterruptedConfigRecovery::PromotedTemp {
                    replaced_invalid_current: invalid_current.is_some(),
                })
            );
            assert!(!config_temp_path(&store.layout().config).exists());
            assert!(interrupted_archive_paths(store.layout()).is_empty());
            let quarantines = config_quarantine_paths(store.layout());
            assert_eq!(quarantines.len(), usize::from(invalid_current.is_some()));
            if let Some(bytes) = invalid_current {
                assert_eq!(fs::read(&quarantines[0]).expect("quarantine bytes"), bytes);
            }
        }
    }

    #[test]
    fn interrupted_commit_archives_invalid_temp_without_replacing_current_or_defaulting_it() {
        for create_current in [false, true] {
            let base = tempdir().expect("temp directory");
            let store = ConfigStore::new(StorageLayout::under(
                base.path(),
                BuildEnvironment::Development,
            ))
            .expect("config store");
            let expected = if create_current {
                store.load_or_default().expect("default config").config
            } else {
                NativeConfig::default()
            };
            let invalid_temp = b"{interrupted";
            fs::write(config_temp_path(&store.layout().config), invalid_temp)
                .expect("invalid interrupted temp");

            let recovered = store.load_or_default().expect("archive invalid temp");
            assert_eq!(recovered.config, expected);
            assert_eq!(
                recovered.interrupted_recovery,
                Some(InterruptedConfigRecovery::ArchivedInvalidTemp)
            );
            let archives = interrupted_archive_paths(store.layout());
            assert_eq!(archives.len(), 1);
            assert_eq!(fs::read(&archives[0]).expect("archive bytes"), invalid_temp);
        }
    }

    #[test]
    fn future_interrupted_schema_is_preserved_without_touching_current() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let current = store.load_or_default().expect("default config").config;
        let mut future = serde_json::to_value(NativeConfig::default()).expect("future value");
        future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
        future["future_section"] = serde_json::json!({ "new_field": true });
        let future_bytes = serde_json::to_vec_pretty(&future).expect("future bytes");
        let temp_path = config_temp_path(&store.layout().config);
        fs::write(&temp_path, &future_bytes).expect("future interrupted temp");

        assert!(matches!(
            store.load_or_default(),
            Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
        ));
        assert_eq!(
            fs::read(&temp_path).expect("preserved future temp"),
            future_bytes
        );
        assert_eq!(
            store.load_or_default().err().map(|error| error.to_string()),
            Some(format!("unsupported schema_version {}", SCHEMA_VERSION + 1))
        );
        assert_eq!(
            parse_config(&fs::read(&store.layout().config).expect("current bytes"))
                .expect("current config")
                .0,
            current
        );
        assert!(interrupted_archive_paths(store.layout()).is_empty());
    }

    #[test]
    fn interrupted_archives_are_bounded_environment_local_and_ignore_unowned_files() {
        let base = tempdir().expect("temp directory");
        let development = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("development store");
        let production = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("production store");
        let mut current = development
            .load_or_default()
            .expect("development config")
            .config;
        production.load_or_default().expect("production config");
        let unowned = development
            .layout()
            .backups
            .join("config-interrupted-note.bin");
        fs::write(&unowned, b"keep").expect("unowned marker");

        for index in 0..6 {
            current.overlay.scale_percent = 100 + index;
            write_interrupted_temp(&development, &current);
            development.load_or_default().expect("archive stale temp");
        }

        let archives = interrupted_archive_paths(development.layout());
        assert_eq!(archives.len(), MAX_INTERRUPTED_ARCHIVES);
        assert!(
            archives
                .iter()
                .map(|path| fs::metadata(path).expect("archive metadata").len())
                .sum::<u64>()
                <= MAX_INTERRUPTED_ARCHIVE_BYTES
        );
        assert_eq!(fs::read(unowned).expect("unowned marker"), b"keep");
        assert!(interrupted_archive_paths(production.layout()).is_empty());
    }

    #[test]
    fn forced_process_exit_releases_writer_lock_and_recovers_synced_temp() {
        let base = tempdir().expect("temp directory");
        let ready = base.path().join("crash-probe.ready");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let current = store.load_or_default().expect("default config").config;
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--ignored")
            .arg("--exact")
            .arg("tests::interrupted_commit_crash_probe_child")
            .arg("--nocapture")
            .env(CRASH_PROBE_BASE, base.path())
            .env(CRASH_PROBE_READY, &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn crash probe");

        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if let Some(status) = child.try_wait().expect("poll crash probe") {
                panic!("crash probe exited before ready: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "crash probe did not become ready"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let mut rejected = current.clone();
        rejected.overlay.scale_percent = 175;
        assert!(matches!(
            store.commit(&rejected),
            Err(ConfigError::LockUnavailable)
        ));

        child.kill().expect("terminate crash probe");
        child.wait().expect("wait for crash probe");
        let recovered = store.load_or_default().expect("recover after process exit");
        assert_eq!(recovered.config, current);
        assert_eq!(
            recovered.interrupted_recovery,
            Some(InterruptedConfigRecovery::ArchivedStaleTemp)
        );
        assert_eq!(interrupted_archive_paths(store.layout()).len(), 1);
        assert!(!config_temp_path(&store.layout().config).exists());
    }

    #[test]
    #[ignore = "spawned by forced_process_exit_releases_writer_lock_and_recovers_synced_temp"]
    fn interrupted_commit_crash_probe_child() {
        let Some(base) = std::env::var_os(CRASH_PROBE_BASE) else {
            return;
        };
        let ready =
            PathBuf::from(std::env::var_os(CRASH_PROBE_READY).expect("crash probe ready path"));
        let store = ConfigStore::new(StorageLayout::under(base, BuildEnvironment::Development))
            .expect("crash probe store");
        let mut candidate = store.load_or_default().expect("crash probe config").config;
        candidate.overlay.scale_percent = 150;
        let _lock = store
            .acquire_writer_lock()
            .expect("crash probe writer lock");
        write_interrupted_temp(&store, &candidate);
        fs::write(ready, b"ready").expect("crash probe ready marker");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn unknown_and_legacy_fields_are_rejected() {
        let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
        value["general"] = serde_json::json!({ "old_pinia_field": true });
        let error = serde_json::from_value::<NativeConfig>(value).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn default_matches_the_shared_configuration_fixture() {
        let fixture = include_str!("../../../../shared/config/fixtures/default.json");
        let expected: NativeConfig = serde_json::from_str(fixture).expect("shared fixture");
        assert_eq!(NativeConfig::default(), expected);
    }

    #[test]
    fn invalid_values_never_replace_last_valid_config() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let config = store.load_or_default().expect("default config").config;
        let original = fs::read(&store.layout().config).expect("original bytes");

        let mut invalid = config;
        invalid.overlay.opacity_percent = 0;
        assert!(matches!(
            store.commit(&invalid),
            Err(ConfigError::InvalidValue("overlay.opacity_percent"))
        ));
        assert_eq!(
            fs::read(&store.layout().config).expect("config bytes"),
            original
        );
    }

    #[test]
    fn selected_model_id_and_origin_are_required_as_a_pair() {
        let mut config = NativeConfig::default();
        config.model.selected_model_id = Some("standard".to_owned());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("model.selected_model_selection"))
        ));

        config.model.selected_model_origin = Some(SelectedModelOrigin::Preset);
        assert!(config.validate().is_ok());
        config.model.selected_model_id = None;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("model.selected_model_selection"))
        ));
    }

    #[test]
    fn native_v1_selection_migrates_once_and_preserves_original_backup() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let mut v1 = serde_json::to_value(NativeConfig::default()).expect("serialize config");
        v1["schema_version"] = serde_json::Value::from(PREVIOUS_SCHEMA_VERSION);
        v1["model"]["selected_model_id"] = serde_json::Value::String("standard".to_owned());
        v1["model"]
            .as_object_mut()
            .expect("model object")
            .remove("selected_model_origin");
        let original = serde_json::to_vec_pretty(&v1).expect("v1 bytes");
        fs::write(&store.layout().config, &original).expect("write v1 config");

        let loaded = store.load_or_default().expect("migrate v1 config");
        let migrated = loaded.config;
        let revision = loaded.revision;
        assert_eq!(loaded.recovery, None);
        assert_eq!(migrated.schema_version, SCHEMA_VERSION);
        assert_eq!(
            migrated.model.selected_model_origin,
            Some(SelectedModelOrigin::Preset)
        );
        let backup_paths = config_backup_paths(store.layout());
        assert_eq!(backup_paths.len(), 1);
        let backup: ConfigBackup =
            serde_json::from_slice(&fs::read(&backup_paths[0]).expect("migration backup"))
                .expect("backup envelope");
        assert_eq!(backup.backup_format_version, BACKUP_FORMAT_VERSION);
        assert_eq!(backup.source_schema_version, PREVIOUS_SCHEMA_VERSION);
        assert_eq!(backup.config, v1);
        assert!(backup.created_at_unix_ms > 0);
        assert_eq!(backup.source_revision.len(), 16);
        for _ in 0..10 {
            let reloaded = store.load_or_default().expect("reload v2 config");
            assert_eq!(reloaded.config, migrated);
            assert_eq!(reloaded.revision, revision);
            assert_eq!(reloaded.recovery, None);
        }
        assert_eq!(config_backup_paths(store.layout()).len(), 1);
    }

    #[test]
    fn post_replace_verification_failure_restores_v1_bytes_and_allows_retry() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let mut v1 = serde_json::to_value(NativeConfig::default()).expect("serialize config");
        v1["schema_version"] = serde_json::Value::from(PREVIOUS_SCHEMA_VERSION);
        v1["model"]
            .as_object_mut()
            .expect("model object")
            .remove("selected_model_origin");
        let original = serde_json::to_vec_pretty(&v1).expect("v1 bytes");

        let mut faulting_store = ConfigStore::new(layout.clone()).expect("config store");
        fs::write(&faulting_store.layout().config, &original).expect("write v1 config");
        faulting_store.inject_write_failure(InjectedConfigWriteFailure::VerificationCorruption);

        assert!(matches!(
            faulting_store.load_or_default(),
            Err(ConfigError::RecoveryVerificationFailed)
        ));
        assert_eq!(
            fs::read(&faulting_store.layout().config).expect("restored v1 config"),
            original
        );
        assert!(!config_temp_path(&faulting_store.layout().config).exists());
        drop(faulting_store);

        let retry_store = ConfigStore::new(layout).expect("retry config store");
        let retried = retry_store.load_or_default().expect("retry v1 migration");
        assert_eq!(retried.config.schema_version, SCHEMA_VERSION);
        assert_eq!(retried.config.model.selected_model_id, None);
        assert_eq!(retried.config.model.selected_model_origin, None);
        assert_eq!(
            retry_store.read_revision().expect("verified revision"),
            retried.revision
        );
    }

    #[test]
    fn invalid_existing_configs_are_reported_without_replacement_or_backup() {
        let mut wrong_type = serde_json::to_value(NativeConfig::default()).expect("config value");
        wrong_type["overlay"]["visible"] = serde_json::Value::String("yes".to_owned());
        let mut out_of_range = serde_json::to_value(NativeConfig::default()).expect("config value");
        out_of_range["overlay"]["opacity_percent"] = serde_json::Value::from(0);
        let mut unknown = serde_json::to_value(NativeConfig::default()).expect("config value");
        unknown["application"]["legacy_alias"] = serde_json::Value::Bool(true);
        let cases = [
            b"not-json".to_vec(),
            br#"{"schema_version":2,"application":{"#.to_vec(),
            serde_json::to_vec_pretty(&wrong_type).expect("wrong type bytes"),
            serde_json::to_vec_pretty(&out_of_range).expect("out of range bytes"),
            serde_json::to_vec_pretty(&unknown).expect("unknown field bytes"),
        ];

        for (index, bytes) in cases.into_iter().enumerate() {
            let base = tempdir().expect("temp directory");
            let store = ConfigStore::new(StorageLayout::under(
                base.path(),
                BuildEnvironment::Development,
            ))
            .expect("config store");
            fs::write(&store.layout().config, &bytes).expect("invalid config");

            assert!(
                store.load_or_default().is_err(),
                "invalid config case {index} was accepted"
            );
            assert_eq!(
                fs::read(&store.layout().config).expect("preserved config"),
                bytes
            );
            assert!(config_backup_paths(store.layout()).is_empty());
        }
    }

    #[test]
    fn corrupt_current_config_recovers_newest_valid_backup_and_is_idempotent() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.scale_percent = 110;
        store.commit(&config).expect("first config commit");
        config.overlay.scale_percent = 120;
        store.commit(&config).expect("second config commit");

        let invalid = br#"{"schema_version":2,"overlay":{"#;
        fs::write(&store.layout().config, invalid).expect("corrupt current config");
        let recovered = store.load_or_default().expect("recover valid backup");
        assert_eq!(recovered.config.overlay.scale_percent, 110);
        let recovery = recovered.recovery.expect("recovery diagnostic");
        assert_eq!(recovery.source_schema_version(), SCHEMA_VERSION);
        assert_eq!(recovery.skipped_newer_backups(), 0);
        let quarantines = config_quarantine_paths(store.layout());
        assert_eq!(quarantines.len(), 1);
        assert_eq!(
            fs::read(&quarantines[0]).expect("quarantine bytes"),
            invalid
        );

        let reloaded = store.load_or_default().expect("reload recovered config");
        assert_eq!(reloaded.config, recovered.config);
        assert_eq!(reloaded.revision, recovered.revision);
        assert_eq!(reloaded.recovery, None);
        assert_eq!(config_quarantine_paths(store.layout()).len(), 1);
    }

    #[test]
    fn recovery_skips_future_format_schema_and_revision_mismatch() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        for scale_percent in [105, 110, 120, 130] {
            config.overlay.scale_percent = scale_percent;
            store.commit(&config).expect("config commit");
        }

        let backups = config_backup_paths(store.layout());
        let mut future_schema: ConfigBackup =
            serde_json::from_slice(&fs::read(&backups[1]).expect("backup bytes"))
                .expect("backup envelope");
        future_schema.source_schema_version = SCHEMA_VERSION + 1;
        future_schema.config["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
        fs::write(
            &backups[1],
            serde_json::to_vec_pretty(&future_schema).expect("future schema backup"),
        )
        .expect("replace backup with future schema");
        let mut revision_mismatch: ConfigBackup =
            serde_json::from_slice(&fs::read(&backups[2]).expect("backup bytes"))
                .expect("backup envelope");
        revision_mismatch.source_revision = "0000000000000000".to_owned();
        fs::write(
            &backups[2],
            serde_json::to_vec_pretty(&revision_mismatch).expect("revision mismatch backup"),
        )
        .expect("replace backup with revision mismatch");
        let mut future_format: ConfigBackup =
            serde_json::from_slice(&fs::read(&backups[3]).expect("backup bytes"))
                .expect("backup envelope");
        future_format.backup_format_version = BACKUP_FORMAT_VERSION + 1;
        fs::write(
            &backups[3],
            serde_json::to_vec_pretty(&future_format).expect("future format backup"),
        )
        .expect("replace backup with future format");

        fs::write(&store.layout().config, b"invalid-current").expect("corrupt current config");
        let recovered = store.load_or_default().expect("recover older backup");
        assert_eq!(recovered.config, NativeConfig::default());
        assert_eq!(
            recovered
                .recovery
                .expect("recovery diagnostic")
                .skipped_newer_backups(),
            3
        );
    }

    #[test]
    fn recovery_failure_preserves_current_config_when_all_backups_are_invalid() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.scale_percent = 110;
        store.commit(&config).expect("config commit");
        let backup = config_backup_paths(store.layout())
            .pop()
            .expect("config backup");
        fs::write(backup, b"invalid-backup").expect("corrupt backup");
        let invalid_current = b"invalid-current";
        fs::write(&store.layout().config, invalid_current).expect("corrupt current config");

        assert!(matches!(
            store.load_or_default(),
            Err(ConfigError::NoValidRecoveryBackup { candidates: 1 })
        ));
        assert_eq!(
            fs::read(&store.layout().config).expect("preserved current config"),
            invalid_current
        );
        assert!(config_quarantine_paths(store.layout()).is_empty());
    }

    #[test]
    fn explicit_default_recovery_archives_invalid_current_and_is_restart_idempotent() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let invalid_current = b"invalid-current-without-backup";
        fs::write(&store.layout().config, invalid_current).expect("invalid current config");

        assert!(matches!(
            store.load_or_default(),
            Err(ConfigError::NoValidRecoveryBackup { candidates: 0 })
        ));
        let recovered = store
            .restore_default_after_failed_recovery()
            .expect("explicit default recovery");
        assert_eq!(recovered.config, NativeConfig::default());
        assert_eq!(recovered.recovery, None);
        let quarantines = config_quarantine_paths(store.layout());
        assert_eq!(quarantines.len(), 1);
        assert_eq!(
            fs::read(&quarantines[0]).expect("quarantine bytes"),
            invalid_current
        );

        let restarted = store.load_or_default().expect("restart with defaults");
        assert_eq!(restarted.config, recovered.config);
        assert_eq!(restarted.revision, recovered.revision);
        assert!(matches!(
            store.restore_default_after_failed_recovery(),
            Err(ConfigError::RecoveryNotRequired)
        ));
        assert_eq!(config_quarantine_paths(store.layout()).len(), 1);
    }

    #[test]
    fn explicit_default_recovery_never_downgrades_a_future_schema() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let mut future = serde_json::to_value(NativeConfig::default()).expect("future config");
        future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
        let bytes = serde_json::to_vec_pretty(&future).expect("future bytes");
        fs::write(&store.layout().config, &bytes).expect("future current config");

        assert!(matches!(
            store.restore_default_after_failed_recovery(),
            Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
        ));
        assert_eq!(
            fs::read(&store.layout().config).expect("preserved future config"),
            bytes
        );
        assert!(config_quarantine_paths(store.layout()).is_empty());
    }

    #[test]
    fn future_current_schema_is_not_rolled_back_to_an_older_backup() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.scale_percent = 110;
        store.commit(&config).expect("config commit");
        let mut future = serde_json::to_value(&config).expect("config value");
        future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
        future["future_section"] = serde_json::json!({ "new_field": true });
        let future_bytes = serde_json::to_vec_pretty(&future).expect("future config bytes");
        fs::write(&store.layout().config, &future_bytes).expect("future current config");

        assert!(matches!(
            store.load_or_default(),
            Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
        ));
        assert_eq!(
            fs::read(&store.layout().config).expect("preserved future config"),
            future_bytes
        );
        assert!(config_quarantine_paths(store.layout()).is_empty());
    }

    #[test]
    fn recovery_quarantines_are_bounded_and_environment_local() {
        let base = tempdir().expect("temp directory");
        let development = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("development config store");
        let production = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("production config store");
        let mut development_config = development
            .load_or_default()
            .expect("development default")
            .config;
        development_config.overlay.scale_percent = 110;
        development
            .commit(&development_config)
            .expect("development commit");
        let mut production_config = production
            .load_or_default()
            .expect("production default")
            .config;
        production_config.overlay.scale_percent = 130;
        production
            .commit(&production_config)
            .expect("production commit");

        for index in 0..6 {
            let invalid = format!("invalid-development-config-{index}");
            fs::write(&development.layout().config, invalid).expect("corrupt development config");
            development
                .load_or_default()
                .expect("recover development config");
        }

        let quarantines = config_quarantine_paths(development.layout());
        assert_eq!(quarantines.len(), MAX_CONFIG_QUARANTINES);
        assert!(
            quarantines
                .iter()
                .map(|path| fs::metadata(path).expect("quarantine metadata").len())
                .sum::<u64>()
                <= MAX_CONFIG_QUARANTINE_BYTES
        );
        assert!(config_quarantine_paths(production.layout()).is_empty());
        assert_eq!(
            production
                .load_or_default()
                .expect("production reload")
                .config
                .overlay
                .scale_percent,
            130
        );
    }

    #[test]
    fn config_backups_are_bounded_and_do_not_remove_unowned_files() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        let unowned = store.layout().backups.join("manual-note.json");
        fs::write(&unowned, b"keep").expect("unowned backup marker");

        for scale_percent in 101..=112 {
            config.overlay.scale_percent = scale_percent;
            store.commit(&config).expect("bounded backup commit");
        }

        let backup_paths = config_backup_paths(store.layout());
        assert_eq!(backup_paths.len(), MAX_CONFIG_BACKUPS);
        let total_bytes = backup_paths
            .iter()
            .map(|path| fs::metadata(path).expect("backup metadata").len())
            .sum::<u64>();
        assert!(total_bytes <= MAX_CONFIG_BACKUP_BYTES);
        assert_eq!(fs::read(unowned).expect("unowned marker"), b"keep");

        let retained_scales = backup_paths
            .iter()
            .map(|path| {
                let backup: ConfigBackup =
                    serde_json::from_slice(&fs::read(path).expect("backup bytes"))
                        .expect("backup envelope");
                assert_eq!(backup.source_schema_version, SCHEMA_VERSION);
                backup.config["overlay"]["scale_percent"]
                    .as_u64()
                    .expect("backup scale")
            })
            .collect::<Vec<_>>();
        assert_eq!(retained_scales, (104..=111).collect::<Vec<_>>());
    }

    #[test]
    fn backup_order_does_not_regress_when_the_wall_clock_moves_backwards() {
        let base = tempdir().expect("temp directory");
        let backups = base.path();
        let newest = backups.join("config-00000000000000000100-00004.json");
        fs::write(&newest, b"existing").expect("existing backup");

        assert_eq!(
            next_backup_path(backups, 50).expect("next backup path"),
            backups.join("config-00000000000000000100-00005.json")
        );
        assert!(!is_owned_backup_name("config-100-5.json"));
        assert!(!is_owned_backup_name("manual-note.json"));
    }

    fn config_backup_paths(layout: &StorageLayout) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(&layout.backups)
            .expect("backup directory")
            .map(|entry| entry.expect("backup entry"))
            .filter(|entry| entry.file_name().to_str().is_some_and(is_owned_backup_name))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn config_quarantine_paths(layout: &StorageLayout) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(&layout.backups)
            .expect("backup directory")
            .map(|entry| entry.expect("backup entry"))
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(is_owned_quarantine_name)
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn interrupted_archive_paths(layout: &StorageLayout) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(&layout.backups)
            .expect("backup directory")
            .map(|entry| entry.expect("backup entry"))
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| parse_owned_interrupted_archive_name(name).is_some())
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn write_interrupted_temp(store: &ConfigStore, config: &NativeConfig) -> Vec<u8> {
        let bytes = serde_json::to_vec_pretty(config).expect("interrupted config bytes");
        let path = config_temp_path(&store.layout().config);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("interrupted config temp");
        file.write_all(&bytes).expect("write interrupted config");
        file.sync_all().expect("sync interrupted config");
        bytes
    }
}
