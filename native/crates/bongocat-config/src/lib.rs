#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const BUNDLE_ID: &str = "com.ayangweb.bongo-cat";
pub const SCHEMA_VERSION: u32 = 2;
const PREVIOUS_SCHEMA_VERSION: u32 = 1;
const BACKUP_FORMAT_VERSION: u32 = 1;
const MAX_CONFIG_BACKUPS: usize = 8;
const MAX_CONFIG_BACKUP_BYTES: u64 = 8 * 1024 * 1024;

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

struct WriterLock {
    _file: File,
}

pub struct ConfigStore {
    layout: StorageLayout,
}

impl ConfigStore {
    pub fn new(layout: StorageLayout) -> Result<Self, ConfigError> {
        layout.create_directories()?;
        Ok(Self { layout })
    }

    pub const fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn load_or_default(&self) -> Result<(NativeConfig, ConfigRevision), ConfigError> {
        let _lock = self.acquire_writer_lock()?;
        match fs::read(&self.layout.config) {
            Ok(bytes) => {
                let (config, revision, migrated) = parse_config(&bytes)?;
                if migrated {
                    let revision = self.commit_unlocked(&config)?;
                    Ok((config, revision))
                } else {
                    Ok((config, revision))
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let config = NativeConfig::default();
                self.commit_unlocked(&config)?;
                let bytes = serde_json::to_vec_pretty(&config)?;
                Ok((config, revision_for_bytes(&bytes)))
            }
            Err(error) => Err(error.into()),
        }
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

    fn read_revision(&self) -> Result<ConfigRevision, ConfigError> {
        let bytes = fs::read(&self.layout.config)?;
        let (_, revision, _) = parse_config(&bytes)?;
        Ok(revision)
    }

    fn commit_unlocked(&self, config: &NativeConfig) -> Result<ConfigRevision, ConfigError> {
        config.validate()?;
        let bytes = serde_json::to_vec_pretty(config)?;
        if let Ok(current) = fs::read(&self.layout.config) {
            self.backup_current_unlocked(&current)?;
        }
        write_atomic(&self.layout.config, &bytes)?;
        Ok(revision_for_bytes(&bytes))
    }

    fn backup_current_unlocked(&self, current: &[u8]) -> Result<(), ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(current)?;
        let source_schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(ConfigError::InvalidValue("schema_version"))?;
        let (_, source_revision, _) = parse_config(current)?;
        let created_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))?
            .as_millis()
            .try_into()
            .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))?;
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.commit()?;
    Ok(())
}

fn parse_config(bytes: &[u8]) -> Result<(NativeConfig, ConfigRevision, bool), ConfigError> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let migrated = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(u64::from(PREVIOUS_SCHEMA_VERSION));
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
    use tempfile::tempdir;

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
        let (mut config, initial_revision) = store.load_or_default().expect("default config");
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
        let (config, _) = store.load_or_default().expect("default config");
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

        let (migrated, revision) = store.load_or_default().expect("migrate v1 config");
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
            let (reloaded, reloaded_revision) = store.load_or_default().expect("reload v2 config");
            assert_eq!(reloaded, migrated);
            assert_eq!(reloaded_revision, revision);
        }
        assert_eq!(config_backup_paths(store.layout()).len(), 1);
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
    fn config_backups_are_bounded_and_do_not_remove_unowned_files() {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let (mut config, _) = store.load_or_default().expect("default config");
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
}
