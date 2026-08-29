#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};

pub const BUNDLE_ID: &str = "com.ayangweb.bongo-cat";
pub const SCHEMA_VERSION: u32 = 1;

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
    pub fn under_application_root(base: impl AsRef<Path>, environment: BuildEnvironment) -> Self {
        let root = base.as_ref().join(environment.directory_name());
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

    /// Test helper that supplies a neutral base and keeps the bundle namespace visible.
    pub fn under(base: impl AsRef<Path>, environment: BuildEnvironment) -> Self {
        Self::under_application_root(base.as_ref().join(BUNDLE_ID), environment)
    }

    pub fn create_directories(&self) -> io::Result<()> {
        for directory in [&self.models, &self.backups, &self.logs, &self.locks] {
            fs::create_dir_all(directory)?;
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

/// Stable equality token for optimistic config writes, not an integrity hash.
fn revision_for_bytes(bytes: &[u8]) -> ConfigRevision {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ConfigRevision(hash)
}

pub struct WriterLock {
    path: PathBuf,
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
pub enum PlatformStorageError {
    DataDirectoryUnavailable,
}

impl std::fmt::Display for PlatformStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataDirectoryUnavailable => write!(f, "platform data directory unavailable"),
        }
    }
}

impl std::error::Error for PlatformStorageError {}

#[cfg(target_os = "macos")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    let base = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join(BUNDLE_ID);
    Ok(StorageLayout::under_application_root(base, environment))
}

#[cfg(target_os = "windows")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    let base = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join("BongoCat");
    Ok(StorageLayout::under_application_root(base, environment))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn platform_layout(
    _environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    Err(PlatformStorageError::DataDirectoryUnavailable)
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    pub play_motion_audio: bool,
    pub enable_behavior_shortcuts: bool,
    pub maximum_fps: u16,
    pub ignore_pointer: bool,
    pub release_fallback_timeout_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ShortcutConfig {
    pub commands: Vec<ShortcutBinding>,
    pub model_behaviors: Vec<ModelBehaviorBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShortcutBinding {
    pub command: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
        if self.overlay.opacity_percent == 0 || self.overlay.opacity_percent > 100 {
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
            .is_some_and(|model_id| model_id.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("model.selected_model_id"));
        }
        if self
            .shortcuts
            .commands
            .iter()
            .any(|binding| binding.command.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("shortcuts.commands.command"));
        }
        if self
            .shortcuts
            .commands
            .iter()
            .any(|binding| binding.shortcut.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("shortcuts.commands.shortcut"));
        }
        if self
            .shortcuts
            .model_behaviors
            .iter()
            .any(|binding| binding.model_id.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue(
                "shortcuts.model_behaviors.model_id",
            ));
        }
        if self
            .shortcuts
            .model_behaviors
            .iter()
            .any(|binding| binding.behavior_id.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue(
                "shortcuts.model_behaviors.behavior_id",
            ));
        }
        if self
            .shortcuts
            .model_behaviors
            .iter()
            .any(|binding| binding.shortcut.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue(
                "shortcuts.model_behaviors.shortcut",
            ));
        }
        Ok(())
    }
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
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "config I/O failed: {error}"),
            Self::Json(error) => write!(f, "config JSON failed: {error}"),
            Self::LockUnavailable => write!(f, "config writer lock unavailable"),
            Self::RevisionConflict { expected, actual } => write!(
                f,
                "config revision conflict: expected {}, found {}",
                expected.value(),
                actual.value()
            ),
            Self::UnsupportedSchema(version) => write!(f, "unsupported schema_version {version}"),
            Self::InvalidValue(field) => write!(f, "invalid config value: {field}"),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    NothingToRecover,
    ArchivedStaleTemp,
    ArchivedInvalidTemp,
    PromotedTemp,
}

#[derive(Clone, Debug)]
enum ConfigFileStatus {
    Missing,
    Valid,
    Invalid,
}

pub struct ConfigStore {
    layout: StorageLayout,
}

impl ConfigStore {
    pub fn new(layout: StorageLayout) -> Result<Self, ConfigError> {
        layout.create_directories()?;
        Ok(Self { layout })
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn acquire_writer_lock(&self) -> Result<WriterLock, ConfigError> {
        let path = self.layout.locks.join("config.writer.lock");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(WriterLock { path }),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                Err(ConfigError::LockUnavailable)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn load_or_default(&self) -> Result<NativeConfig, ConfigError> {
        self.recover_interrupted_commit()?;
        match fs::read(&self.layout.config) {
            Ok(bytes) => {
                let config: NativeConfig = serde_json::from_slice(&bytes)?;
                config.validate()?;
                Ok(config)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let config = NativeConfig::default();
                self.commit(&config)?;
                Ok(config)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn recover_interrupted_commit(&self) -> Result<RecoveryAction, ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        self.recover_interrupted_commit_unlocked()
    }

    pub fn revision(&self) -> Result<ConfigRevision, ConfigError> {
        let bytes = fs::read(&self.layout.config)?;
        let config: NativeConfig = serde_json::from_slice(&bytes)?;
        config.validate()?;
        Ok(revision_for_bytes(&serde_json::to_vec(&config)?))
    }

    pub fn commit(&self, config: &NativeConfig) -> Result<(), ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        self.commit_unlocked(config)
    }

    pub fn commit_if_revision(
        &self,
        config: &NativeConfig,
        expected: ConfigRevision,
    ) -> Result<(), ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        let actual = self.revision()?;
        if actual != expected {
            return Err(ConfigError::RevisionConflict { expected, actual });
        }
        self.commit_unlocked(config)
    }

    fn commit_unlocked(&self, config: &NativeConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let bytes = serde_json::to_vec_pretty(config)?;
        let temp_path = self.layout.config.with_extension("json.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        self.back_up_current_config()?;
        fs::rename(&temp_path, &self.layout.config)?;
        File::open(&self.layout.config)?.sync_all()?;
        Ok(())
    }

    fn recover_interrupted_commit_unlocked(&self) -> Result<RecoveryAction, ConfigError> {
        let temp_path = self.layout.config.with_extension("json.tmp");
        let temp_status = inspect_config_file(&temp_path)?;
        if matches!(temp_status, ConfigFileStatus::Missing) {
            return Ok(RecoveryAction::NothingToRecover);
        }
        let current_status = inspect_config_file(&self.layout.config)?;
        match temp_status {
            ConfigFileStatus::Valid => match current_status {
                ConfigFileStatus::Valid => {
                    archive_file(&temp_path, &self.layout.backups, "config.interrupted")?;
                    Ok(RecoveryAction::ArchivedStaleTemp)
                }
                ConfigFileStatus::Missing => {
                    fs::rename(&temp_path, &self.layout.config)?;
                    File::open(&self.layout.config)?.sync_all()?;
                    Ok(RecoveryAction::PromotedTemp)
                }
                ConfigFileStatus::Invalid => {
                    archive_file(&self.layout.config, &self.layout.backups, "config.corrupt")?;
                    fs::rename(&temp_path, &self.layout.config)?;
                    File::open(&self.layout.config)?.sync_all()?;
                    Ok(RecoveryAction::PromotedTemp)
                }
            },
            ConfigFileStatus::Invalid => {
                archive_file(
                    &temp_path,
                    &self.layout.backups,
                    "config.interrupted.invalid",
                )?;
                Ok(RecoveryAction::ArchivedInvalidTemp)
            }
            ConfigFileStatus::Missing => unreachable!("missing temp was returned above"),
        }
    }

    fn back_up_current_config(&self) -> Result<(), ConfigError> {
        if !self.layout.config.is_file() {
            return Ok(());
        }
        let backup_path = self.layout.backups.join("config.previous.json");
        let backup_temp_path = self.layout.backups.join("config.previous.json.tmp");
        fs::copy(&self.layout.config, &backup_temp_path)?;
        File::open(&backup_temp_path)?.sync_all()?;
        fs::rename(backup_temp_path, backup_path)?;
        Ok(())
    }
}

fn inspect_config_file(path: &Path) -> Result<ConfigFileStatus, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ConfigFileStatus::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    let config = match serde_json::from_slice::<NativeConfig>(&bytes) {
        Ok(config) => config,
        Err(_) => return Ok(ConfigFileStatus::Invalid),
    };
    if config.validate().is_err() {
        return Ok(ConfigFileStatus::Invalid);
    }
    Ok(ConfigFileStatus::Valid)
}

fn archive_file(source: &Path, backups: &Path, stem: &str) -> Result<PathBuf, ConfigError> {
    for suffix in 0..1000u16 {
        let filename = if suffix == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}.{suffix}.json")
        };
        let destination = backups.join(filename);
        let mut destination_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let mut source_file = File::open(source)?;
        if let Err(error) = io::copy(&mut source_file, &mut destination_file) {
            drop(destination_file);
            let _ = fs::remove_file(&destination);
            return Err(error.into());
        }
        destination_file.sync_all()?;
        drop(destination_file);
        drop(source_file);
        fs::remove_file(source)?;
        return Ok(destination);
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "too many config recovery archives",
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn environments_have_identical_relative_layouts_but_different_roots() {
        let base = tempdir().unwrap();
        let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
        assert_ne!(development.root, production.root);
        let development_paths = [
            &development.config,
            &development.state,
            &development.models,
            &development.backups,
            &development.logs,
            &development.locks,
        ];
        let production_paths = [
            &production.config,
            &production.state,
            &production.models,
            &production.backups,
            &production.logs,
            &production.locks,
        ];
        for (development_path, production_path) in
            development_paths.into_iter().zip(production_paths)
        {
            assert_eq!(
                development_path.strip_prefix(&development.root).unwrap(),
                production_path.strip_prefix(&production.root).unwrap()
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_platform_layout_uses_bundle_namespace_once() {
        let layout = platform_layout(BuildEnvironment::Development).unwrap();
        assert!(
            layout
                .root
                .ends_with(Path::new(BUNDLE_ID).join("development"))
        );
        assert_eq!(
            layout.root.file_name().and_then(|name| name.to_str()),
            Some("development")
        );
        let bundle_root = layout.root.parent().expect("environment parent");
        assert_eq!(
            bundle_root.file_name().and_then(|name| name.to_str()),
            Some(BUNDLE_ID)
        );
        assert!(!bundle_root.parent().unwrap().ends_with(BUNDLE_ID));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_platform_layout_uses_roaming_app_data_product_namespace() {
        let data_root = dirs::data_dir().expect("Windows roaming AppData");
        let development = platform_layout(BuildEnvironment::Development).unwrap();
        let production = platform_layout(BuildEnvironment::Production).unwrap();
        assert_eq!(
            development.root,
            data_root.join("BongoCat").join("development")
        );
        assert_eq!(
            production.root,
            data_root.join("BongoCat").join("production")
        );
        assert_ne!(development.root, production.root);
    }

    #[test]
    fn development_and_production_data_never_crosses() {
        let base = tempdir().unwrap();
        let development = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .unwrap();
        let production = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .unwrap();
        let mut dev_config = development.load_or_default().unwrap();
        dev_config.appearance.language = "zh-CN".into();
        development.commit(&dev_config).unwrap();
        assert_eq!(
            production.load_or_default().unwrap().appearance.language,
            "en-US"
        );
        assert_eq!(
            development.load_or_default().unwrap().appearance.language,
            "zh-CN"
        );
    }

    #[test]
    fn invalid_commit_preserves_previous_valid_file() {
        let base = tempdir().unwrap();
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .unwrap();
        let original = store.load_or_default().unwrap();
        let mut invalid = original.clone();
        invalid.overlay.opacity_percent = 0;
        assert!(matches!(
            store.commit(&invalid),
            Err(ConfigError::InvalidValue("overlay.opacity_percent"))
        ));
        assert_eq!(store.load_or_default().unwrap(), original);
    }

    #[test]
    fn successful_commit_keeps_previous_config_backup() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let store = ConfigStore::new(layout.clone()).unwrap();
        let original = store.load_or_default().unwrap();
        let mut updated = original.clone();
        updated.appearance.language = "zh-CN".into();
        store.commit(&updated).unwrap();
        let backup_path = layout.backups.join("config.previous.json");
        let backup: NativeConfig = serde_json::from_slice(&fs::read(backup_path).unwrap()).unwrap();
        assert_eq!(backup, original);
        assert_eq!(store.load_or_default().unwrap(), updated);
    }

    #[test]
    fn recovery_promotes_valid_temp_when_current_config_is_missing() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let store = ConfigStore::new(layout.clone()).unwrap();
        let mut candidate = NativeConfig::default();
        candidate.appearance.language = "zh-CN".into();
        let temp_path = layout.config.with_extension("json.tmp");
        fs::write(&temp_path, serde_json::to_vec_pretty(&candidate).unwrap()).unwrap();

        assert_eq!(
            store.recover_interrupted_commit().unwrap(),
            RecoveryAction::PromotedTemp
        );
        assert!(!temp_path.exists());
        assert_eq!(store.load_or_default().unwrap(), candidate);
    }

    #[test]
    fn recovery_preserves_valid_current_and_archives_stale_temp() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let store = ConfigStore::new(layout.clone()).unwrap();
        let current = store.load_or_default().unwrap();
        let mut stale = current.clone();
        stale.appearance.language = "zh-CN".into();
        let temp_path = layout.config.with_extension("json.tmp");
        fs::write(&temp_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();

        assert_eq!(
            store.recover_interrupted_commit().unwrap(),
            RecoveryAction::ArchivedStaleTemp
        );
        assert_eq!(store.load_or_default().unwrap(), current);
        let archived: NativeConfig = serde_json::from_slice(
            &fs::read(layout.backups.join("config.interrupted.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(archived, stale);
        assert_eq!(
            store.recover_interrupted_commit().unwrap(),
            RecoveryAction::NothingToRecover
        );
    }

    #[test]
    fn recovery_never_overwrites_an_existing_archive() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let store = ConfigStore::new(layout.clone()).unwrap();
        let current = store.load_or_default().unwrap();
        let temp_path = layout.config.with_extension("json.tmp");

        let mut first = current.clone();
        first.appearance.language = "zh-CN".into();
        fs::write(&temp_path, serde_json::to_vec_pretty(&first).unwrap()).unwrap();
        store.recover_interrupted_commit().unwrap();

        let mut second = current;
        second.appearance.language = "ja-JP".into();
        fs::write(&temp_path, serde_json::to_vec_pretty(&second).unwrap()).unwrap();
        store.recover_interrupted_commit().unwrap();

        let archived_first: NativeConfig = serde_json::from_slice(
            &fs::read(layout.backups.join("config.interrupted.json")).unwrap(),
        )
        .unwrap();
        let archived_second: NativeConfig = serde_json::from_slice(
            &fs::read(layout.backups.join("config.interrupted.1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(archived_first, first);
        assert_eq!(archived_second, second);
    }

    #[test]
    fn recovery_archives_invalid_temp_without_touching_valid_current() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let store = ConfigStore::new(layout.clone()).unwrap();
        let current = store.load_or_default().unwrap();
        let temp_path = layout.config.with_extension("json.tmp");
        fs::write(&temp_path, b"{interrupted").unwrap();

        assert_eq!(
            store.recover_interrupted_commit().unwrap(),
            RecoveryAction::ArchivedInvalidTemp
        );
        assert_eq!(store.load_or_default().unwrap(), current);
        assert_eq!(
            fs::read(layout.backups.join("config.interrupted.invalid.json")).unwrap(),
            b"{interrupted"
        );
    }

    #[test]
    fn recovery_archives_corrupt_current_before_promoting_valid_temp() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let store = ConfigStore::new(layout.clone()).unwrap();
        fs::write(&layout.config, b"{corrupt").unwrap();
        let mut candidate = NativeConfig::default();
        candidate.appearance.language = "ja-JP".into();
        let temp_path = layout.config.with_extension("json.tmp");
        fs::write(&temp_path, serde_json::to_vec_pretty(&candidate).unwrap()).unwrap();

        assert_eq!(
            store.recover_interrupted_commit().unwrap(),
            RecoveryAction::PromotedTemp
        );
        assert_eq!(store.load_or_default().unwrap(), candidate);
        assert_eq!(
            fs::read(layout.backups.join("config.corrupt.json")).unwrap(),
            b"{corrupt"
        );
    }

    #[test]
    fn stale_revision_is_rejected_without_overwriting_newer_config() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let first = ConfigStore::new(layout.clone()).unwrap();
        let second = ConfigStore::new(layout).unwrap();
        let original = first.load_or_default().unwrap();
        let revision = first.revision().unwrap();
        let mut newer = original.clone();
        newer.appearance.language = "zh-CN".into();
        second.commit_if_revision(&newer, revision).unwrap();
        let mut stale = original;
        stale.appearance.language = "pt-BR".into();
        let error = first.commit_if_revision(&stale, revision).unwrap_err();
        assert!(matches!(error, ConfigError::RevisionConflict { .. }));
        assert_eq!(
            second.load_or_default().unwrap().appearance.language,
            "zh-CN"
        );
    }

    #[test]
    fn writer_lock_rejects_concurrent_commit_and_releases_on_drop() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let first = ConfigStore::new(layout.clone()).unwrap();
        let second = ConfigStore::new(layout).unwrap();
        let lock = first.acquire_writer_lock().unwrap();
        assert!(matches!(
            second.commit(&NativeConfig::default()),
            Err(ConfigError::LockUnavailable)
        ));
        drop(lock);
        second.commit(&NativeConfig::default()).unwrap();
    }

    #[test]
    fn malformed_file_is_rejected_without_silent_default_overwrite() {
        let base = tempdir().unwrap();
        let layout = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let store = ConfigStore::new(layout.clone()).unwrap();
        store.load_or_default().unwrap();
        fs::write(&layout.config, b"{not-json").unwrap();
        assert!(matches!(store.load_or_default(), Err(ConfigError::Json(_))));
        assert_eq!(fs::read(&layout.config).unwrap(), b"{not-json");
    }

    #[test]
    fn serialized_keys_follow_native_snake_case_contract() {
        let value = serde_json::to_value(NativeConfig::default()).unwrap();
        assert!(value.get("schema_version").is_some());
        assert!(value["application"].get("launch_at_login").is_some());
        assert!(
            value["appearance"]
                .get("check_for_updates_automatically")
                .is_none()
        );
        assert!(
            value["overlay"]
                .get("hide_on_pointer_hover_delay_ms")
                .is_some()
        );
        assert!(value["model"].get("release_fallback_timeout_ms").is_some());
    }

    #[test]
    fn default_serialization_matches_shared_config_fixture() {
        let expected = include_str!("../../../shared/config/fixtures/default.json");
        let expected: serde_json::Value = serde_json::from_str(expected).unwrap();
        assert_eq!(
            serde_json::to_value(NativeConfig::default()).unwrap(),
            expected
        );
    }

    #[test]
    fn unknown_fields_are_rejected_like_the_json_schema() {
        let mut value = serde_json::to_value(NativeConfig::default()).unwrap();
        value["unexpected_field"] = serde_json::json!(true);
        let error = serde_json::from_value::<NativeConfig>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn blank_binding_is_rejected_by_typed_validation() {
        let mut config = NativeConfig::default();
        config.shortcuts.commands.push(ShortcutBinding {
            command: "  ".into(),
            shortcut: "Control+Alt+B".into(),
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("shortcuts.commands.command"))
        ));
    }
}
