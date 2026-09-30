//! The configuration document and its validation.
//!
//! `schema_version: 1` is the only accepted version. There is no migration and no
//! older-shape fallback: a document that is not this schema is rejected by the
//! version gate before any field is read, and a document that is this schema but
//! carries an unusable value fails validation and never replaces the last valid
//! configuration.

use super::*;

mod appearance;
mod input;
mod logging;
mod model;
mod multiplayer;
mod overlay;
mod system;
mod updates;

// The document reaches a domain's crate-private half through this one prelude:
// a field of `NativeConfig` is named in the document's own `Default` and
// `validate`, and a private import is the only way to reach it without claiming
// it is public.
use model::*;

// The document's public half leaves the crate through the crate root's
// `pub use config_schema::*`, so it is named here rather than left to a glob: a
// glob would also carry the crate-private helpers, and a `pub` glob cannot
// widen them.
pub use appearance::{AppearanceConfig, Language, Theme};
pub use input::{GamepadInputConfig, InputConfig};
pub use logging::{LoggingConfig, LoggingLevel};
pub use model::{
    BuiltInModelMetadata, GamepadAutoSwitchConfig, ImportedModelMetadata,
    MAXIMUM_MODEL_EXPRESSION_MEMORIES, MODEL_EXPRESSION_MEMORY_MAXIMUM_NAME_BYTES,
    MODEL_METADATA_MAXIMUM_ID_BYTES, MODEL_METADATA_MAXIMUM_TITLE_CHARS, ModelConfig,
    ModelExpressionMemory, ModelIdentity, ModelInputMode, ModelSource, RandomBehaviorConfig,
    RandomBehaviorMode,
};
pub use multiplayer::{
    MAXIMUM_MULTIPLAYER_NICKNAME_CHARS, MAXIMUM_MULTIPLAYER_SERVER_URL_BYTES, MultiplayerConfig,
};
pub use overlay::{MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS, OverlayConfig};
pub use system::SystemConfig;
pub use updates::UpdateConfig;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeConfig {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 1))
    )]
    pub schema_version: u32,
    pub appearance: AppearanceConfig,
    pub overlay: OverlayConfig,
    pub input: InputConfig,
    pub logging: LoggingConfig,
    pub model: ModelConfig,
    pub shortcuts: ShortcutConfig,
    pub system: SystemConfig,
    pub updates: UpdateConfig,
    /// Written before this section existed, configurations keep parsing
    /// without it and land on the unconfigured default. New fields inside the
    /// section must in turn carry their own defaults.
    #[serde(default)]
    pub multiplayer: MultiplayerConfig,
}

impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            appearance: AppearanceConfig {
                theme: Theme::System,
                language: Language::default(),
            },
            overlay: OverlayConfig {
                click_through: false,
                always_on_top: true,
                scale_percent: 100,
                opacity_percent: 100,
                maximum_fps: 60,
                corner_radius_percent: 0,
                hide_on_pointer_hover: false,
                hide_on_pointer_hover_delay_seconds: 0,
                keep_inside_screen: true,
            },
            input: InputConfig {
                gamepad: GamepadInputConfig {
                    stick_dead_zone: 0.15,
                    trigger_dead_zone: 0.0,
                },
            },
            logging: LoggingConfig::default(),
            model: ModelConfig {
                selected_model: None,
                imported_models: Vec::new(),
                built_in_models: Vec::new(),
                mirror: false,
                mirror_pointer_tracking_horizontal: false,
                mirror_pointer_tracking_vertical: false,
                play_motion_audio: false,
                ignore_keyboard: false,
                ignore_gamepad: false,
                ignore_pointer: false,
                random_behavior: RandomBehaviorConfig::default(),
                gamepad_auto_switch: GamepadAutoSwitchConfig::default(),
                remember_last_expression: false,
                last_expressions: Vec::new(),
            },
            shortcuts: ShortcutConfig::default(),
            system: SystemConfig {
                show_taskbar_icon: false,
                show_dock_icon: false,
                show_status_icon: true,
            },
            updates: UpdateConfig {
                check_automatically: false,
                check_interval_hours: DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS,
            },
            multiplayer: MultiplayerConfig::default(),
        }
    }
}

impl NativeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        if !(1..=MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS)
            .contains(&self.updates.check_interval_hours)
        {
            return Err(ConfigError::InvalidValue("updates.check_interval_hours"));
        }
        // An empty server URL is the unconfigured steady state. Anything else
        // must name a scheme and a host and nothing beyond them, because the
        // room service is reached at its root and a stray path would only
        // produce a connection the user cannot explain.
        if !self.multiplayer.server_url.trim().is_empty() {
            let url = self.multiplayer.server_url.trim();
            if url.len() > MAXIMUM_MULTIPLAYER_SERVER_URL_BYTES {
                return Err(ConfigError::InvalidValue("multiplayer.server_url"));
            }
            match url::Url::parse(url) {
                Ok(parsed)
                    if matches!(parsed.scheme(), "http" | "https")
                        && matches!(parsed.path(), "/" | "")
                        && parsed.host_str().is_some_and(|host| !host.is_empty())
                        && parsed.query().is_none()
                        && parsed.fragment().is_none() => {}
                _ => return Err(ConfigError::InvalidValue("multiplayer.server_url")),
            }
        }
        // An empty nickname is the unconfigured steady state; the multiplayer
        // page requires one before joining. A stored nickname may not carry
        // control characters, and its trimmed length is what other members see.
        if self.multiplayer.nickname.chars().any(char::is_control)
            || self.multiplayer.nickname.trim().chars().count() > MAXIMUM_MULTIPLAYER_NICKNAME_CHARS
        {
            return Err(ConfigError::InvalidValue("multiplayer.nickname"));
        }
        if !(25..=400).contains(&self.overlay.scale_percent) {
            return Err(ConfigError::InvalidValue("overlay.scale_percent"));
        }
        if !(1..=100).contains(&self.overlay.opacity_percent) {
            return Err(ConfigError::InvalidValue("overlay.opacity_percent"));
        }
        if !(15..=240).contains(&self.overlay.maximum_fps) {
            return Err(ConfigError::InvalidValue("overlay.maximum_fps"));
        }
        if !(0..=50).contains(&self.overlay.corner_radius_percent) {
            return Err(ConfigError::InvalidValue("overlay.corner_radius_percent"));
        }
        if self.overlay.hide_on_pointer_hover_delay_seconds
            > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS
        {
            return Err(ConfigError::InvalidValue(
                "overlay.hide_on_pointer_hover_delay_seconds",
            ));
        }
        if !(0.0..1.0).contains(&self.input.gamepad.stick_dead_zone)
            || !self.input.gamepad.stick_dead_zone.is_finite()
        {
            return Err(ConfigError::InvalidValue("input.gamepad.stick_dead_zone"));
        }
        if !(0.0..1.0).contains(&self.input.gamepad.trigger_dead_zone)
            || !self.input.gamepad.trigger_dead_zone.is_finite()
        {
            return Err(ConfigError::InvalidValue("input.gamepad.trigger_dead_zone"));
        }
        if !(1..=MAXIMUM_LOG_RETENTION_DAYS).contains(&self.logging.retention_days) {
            return Err(ConfigError::InvalidValue("logging.retention_days"));
        }
        if !(MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS..=MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS)
            .contains(&self.model.random_behavior.interval_seconds)
        {
            return Err(ConfigError::InvalidValue(
                "model.random_behavior.interval_seconds",
            ));
        }
        if self
            .model
            .selected_model
            .as_ref()
            .is_some_and(|selected| !is_portable_model_id(&selected.id))
        {
            return Err(ConfigError::InvalidValue("model.selected_model.id"));
        }
        for (field, target) in [
            (
                "model.gamepad_auto_switch.connected_model.id",
                self.model.gamepad_auto_switch.connected_model.as_ref(),
            ),
            (
                "model.gamepad_auto_switch.disconnected_model.id",
                self.model.gamepad_auto_switch.disconnected_model.as_ref(),
            ),
        ] {
            if target.is_some_and(|target| !is_portable_model_id(&target.id)) {
                return Err(ConfigError::InvalidValue(field));
            }
        }
        validate_model_metadata(
            "model.imported_models.id",
            "model.imported_models.title",
            self.model
                .imported_models
                .iter()
                .map(|record| (record.id.as_str(), record.title.as_str())),
        )?;
        validate_model_metadata(
            "model.built_in_models.id",
            "model.built_in_models.title",
            self.model
                .built_in_models
                .iter()
                .map(|record| (record.id.as_str(), record.title.as_str())),
        )?;
        validate_model_expression_memories(&self.model.last_expressions)?;
        if self
            .shortcuts
            .command_bindings
            .iter()
            .any(|binding| binding.command.trim().is_empty() || binding.shortcut.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("shortcuts.command_bindings"));
        }
        if self
            .shortcuts
            .model_behavior_bindings
            .iter()
            .any(|binding| {
                !is_portable_model_id(&binding.model.id)
                    || binding.behavior_id.trim().is_empty()
                    || binding.shortcut.trim().is_empty()
            })
        {
            return Err(ConfigError::InvalidValue(
                "shortcuts.model_behavior_bindings",
            ));
        }
        for binding in &self.shortcuts.command_bindings {
            ShortcutCommand::parse(&binding.command)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.command"))?;
        }
        for binding in &self.shortcuts.model_behavior_bindings {
            binding
                .parse_action()
                .map_err(|_| ConfigError::InvalidValue("shortcuts.behavior"))?;
        }
        // A chord may be reused across models — only one model's behaviors are
        // live at a time — but never twice inside one scope. The scopes are the
        // application command list and each model's own behavior list. A model
        // behavior may not shadow a command either, because commands are
        // registered whatever the active model is.
        let mut command_chords = std::collections::BTreeSet::new();
        for shortcut in self
            .shortcuts
            .command_bindings
            .iter()
            .map(|binding| binding.shortcut.as_str())
        {
            let chord = ShortcutChord::parse(shortcut)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?;
            if !command_chords.insert(chord.canonical()) {
                return Err(ConfigError::InvalidValue("shortcuts.conflict"));
            }
        }
        let mut model_chords: std::collections::BTreeMap<
            &ModelIdentity,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        for binding in &self.shortcuts.model_behavior_bindings {
            let chord = ShortcutChord::parse(&binding.shortcut)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?;
            let canonical = chord.canonical();
            if command_chords.contains(&canonical)
                || !model_chords
                    .entry(&binding.model)
                    .or_default()
                    .insert(canonical)
            {
                return Err(ConfigError::InvalidValue("shortcuts.conflict"));
            }
        }
        Ok(())
    }
}
