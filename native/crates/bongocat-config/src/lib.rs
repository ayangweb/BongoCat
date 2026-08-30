#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};

pub const BUNDLE_ID: &str = "com.ayangweb.bongo-cat";
pub const SCHEMA_VERSION: u32 = 2;
const PREVIOUS_SCHEMA_VERSION: u32 = 1;

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
            write_atomic(&self.layout.backups.join("config.previous.json"), &current)?;
        }
        write_atomic(&self.layout.config, &bytes)?;
        Ok(revision_for_bytes(&bytes))
    }
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
        assert!(
            store
                .layout()
                .backups
                .join("config.previous.json")
                .is_file()
        );
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
        assert_eq!(
            fs::read(store.layout().backups.join("config.previous.json"))
                .expect("migration backup"),
            original
        );
        let (reloaded, reloaded_revision) = store.load_or_default().expect("reload v2 config");
        assert_eq!(reloaded, migrated);
        assert_eq!(reloaded_revision, revision);
    }
}
