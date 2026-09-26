//! The configuration document and its validation.
//!
//! `schema_version: 1` is the only accepted version. There is no migration and no
//! older-shape fallback: a document that is not this schema is rejected by the
//! version gate before any field is read, and a document that is this schema but
//! carries an unusable value fails validation and never replaces the last valid
//! configuration.

use super::*;

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
}

/// Desktop integration preferences. These are system-owned surfaces rather
/// than model or overlay state, so they have their own namespace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    pub show_taskbar_icon: bool,
    pub show_status_icon: bool,
}

/// Automatic update policy. The interval remains persisted when the switch is
/// off, just like the other preference pairs in the configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UpdateConfig {
    pub check_automatically: bool,
    /// Whole hours to wait after an automatic update check before checking again.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 8760))
    )]
    pub check_interval_hours: u16,
}

/// User-controlled filtering and retention for the human-readable application
/// and Cubism Core logs. Daily rollover and the per-file size guard are fixed
/// safety policy and therefore intentionally do not appear in configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: LoggingLevel,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 30))
    )]
    pub retention_days: u8,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LoggingLevel::default(),
            retention_days: DEFAULT_LOG_RETENTION_DAYS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoggingLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LoggingLevel {
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AppearanceConfig {
    pub theme: Theme,
    pub language: Language,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    #[serde(rename = "zh-CN")]
    ChineseSimplified,
    #[serde(rename = "en-US")]
    EnglishUnitedStates,
}

impl Language {
    pub const ALL: [Self; 3] = [
        Self::System,
        Self::ChineseSimplified,
        Self::EnglishUnitedStates,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ChineseSimplified => "zh-CN",
            Self::EnglishUnitedStates => "en-US",
        }
    }

    pub fn from_system_locale(locale: &str) -> Self {
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        let subtags = locale.split('-').collect::<Vec<_>>();
        if subtags.first() == Some(&"zh")
            && !subtags
                .iter()
                .any(|subtag| matches!(*subtag, "hant" | "tw" | "hk" | "mo"))
        {
            Self::ChineseSimplified
        } else {
            Self::EnglishUnitedStates
        }
    }

    pub const fn resolve(self, system_language: Self) -> Self {
        match self {
            Self::System => match system_language {
                Self::ChineseSimplified => Self::ChineseSimplified,
                Self::System | Self::EnglishUnitedStates => Self::EnglishUnitedStates,
            },
            Self::ChineseSimplified => Self::ChineseSimplified,
            Self::EnglishUnitedStates => Self::EnglishUnitedStates,
        }
    }
}

/// Overlay window configuration. Every field is a property of the single
/// product overlay window; the settings window and every other product window
/// keep their own platform chrome and are not affected.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    pub click_through: bool,
    pub always_on_top: bool,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 25, max = 400))
    )]
    pub scale_percent: u16,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 100))
    )]
    pub opacity_percent: u8,
    /// Maximum frame rate for the product overlay and its runtime scheduler.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 15, max = 240))
    )]
    pub maximum_fps: u16,
    /// Corner radius of the overlay window box, as a percentage of the window
    /// width and height. The value keeps the legacy `border-radius: N%`
    /// semantics: each corner arc is an ellipse with a horizontal semi-axis of
    /// `N%` of the window width and a vertical semi-axis of `N%` of the window
    /// height, so the same number rounds a wide window more horizontally than
    /// vertically. `0` leaves square corners. At `50` the four arcs meet and the
    /// window content is clipped to the full inscribed ellipse; the legacy
    /// implementation scaled every radius above that point back down to the same
    /// ellipse, so `50` is the effective upper bound of the legacy behavior.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 50))
    )]
    pub corner_radius_percent: u8,
    /// Hide the overlay while the pointer rests on it, keeping the model out of
    /// the way of whatever the pointer is reaching for underneath.
    ///
    /// This mirrors the legacy `window.hideOnHover` switch. The overlay stays a
    /// normal window and keeps presenting frames; only its rendered alpha drops
    /// to zero, and pointer events pass through until the pointer leaves the
    /// window box again.
    pub hide_on_pointer_hover: bool,
    /// How long the pointer must stay inside the overlay box before the hover
    /// hide starts, in whole seconds. `0` hides as soon as the pointer enters.
    ///
    /// The legacy input also took whole seconds with a lower bound of `0`, so
    /// this field keeps the unit the settings page edits and needs no
    /// conversion between the stored value and the visible one. The cap at `60`
    /// seconds is a first-version contract decision rather than a legacy
    /// ceiling (the legacy input had no upper bound): a hover delay longer than
    /// a minute is indistinguishable from leaving the feature off. The overlay
    /// frame loop still counts in milliseconds and converts once at its own
    /// boundary, because the hover state machine compares against the
    /// monotonic millisecond clock.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 60))
    )]
    pub hide_on_pointer_hover_delay_seconds: u32,
    /// Keep the overlay window fully on a display. `next` keeps the window on
    /// the union of the connected displays, so it may cover a taskbar, Dock or
    /// menu bar, and a window dragged off the desktop is moved back only after
    /// the drag has stopped. This replaces the earlier work-area constraint,
    /// which forbade the desktop chrome strip entirely.
    pub keep_inside_screen: bool,
}

/// Upper bound of the hover hide delay, in whole seconds.
///
/// See [`OverlayConfig::hide_on_pointer_hover_delay_seconds`] for why the legacy
/// implementation's unbounded second-valued input is capped here.
pub const MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS: u32 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad: GamepadInputConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KeyboardInputConfig {
    /// Final fallback for a captured keyboard key whose normal release,
    /// reconciliation and reset paths all failed to clear it.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 60_000))
    )]
    pub release_fallback_timeout_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GamepadInputConfig {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0.0, max = 1.0))
    )]
    pub stick_dead_zone: f64,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0.0, max = 1.0))
    )]
    pub trigger_dead_zone: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// The selected model is one nullable identity object. Keeping `id` and
    /// `source` together makes an incomplete selection unrepresentable.
    pub selected_model: Option<ModelIdentity>,
    /// User-imported model metadata, including the input mode resolved once at
    /// import time.
    pub imported_models: Vec<ImportedModelMetadata>,
    /// Editable metadata for the models shipped by the build.
    pub built_in_models: Vec<BuiltInModelMetadata>,
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    /// Whether a motion that ships an audio clip is allowed to play it.
    ///
    /// Defaults to `false`: the Model behavior page renders this as the "play
    /// motion audio" opt-in, so a fresh v1 configuration stays silent until the
    /// user turns it on. This is a deliberate divergence from the legacy
    /// implementation, which recorded an enabled default; the reasoning lives
    /// in `shared/config/contract.md`.
    pub play_motion_audio: bool,
    /// Whether keyboard input is excluded from the model's input projection.
    pub ignore_keyboard: bool,
    /// Whether gamepad input is excluded from the model's input projection.
    pub ignore_gamepad: bool,
    pub ignore_pointer: bool,
    pub random_behavior: RandomBehaviorConfig,
    /// Switching the selected model when gamepads connect or disconnect.
    pub gamepad_auto_switch: GamepadAutoSwitchConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RandomBehaviorConfig {
    pub enabled: bool,
    /// Delay between automatic behavior selections, in whole seconds.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 3600))
    )]
    pub interval_seconds: u32,
}

impl Default for RandomBehaviorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        }
    }
}

/// Choosing the shown model from gamepad connection state.
///
/// `enabled` is the only gate and defaults to `false`: a fresh v1 configuration
/// keeps the user's own selection until they ask for the switch. Each target is
/// a complete [`ModelIdentity`] or `null`, and `null` is the default meaning —
/// "the last model the user activated for this input family". The product
/// remembers that per family, so the switch follows the user's own habits
/// instead of a second pair of settings to keep in step.
///
/// The two fields describe *when* a model is used, not *which* models qualify.
/// The settings page offers a model whose recorded input mode matches the
/// direction, but a target is honoured exactly as configured: the stored
/// identity is the only thing this schema constrains, so a hand-edited value
/// never silently becomes a different model.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GamepadAutoSwitchConfig {
    pub enabled: bool,
    /// The model to show while at least one gamepad is connected. `None` means
    /// the last activated gamepad-mode model, and there is nothing to switch to
    /// until the user has activated one.
    pub connected_model: Option<ModelIdentity>,
    /// The model to show while no gamepad is connected. `None` means the last
    /// activated model of any other mode, and there is nothing to switch to
    /// until the user has activated one.
    pub disconnected_model: Option<ModelIdentity>,
}

/// The stable identity of a model as seen by the user-facing configuration.
/// `source` distinguishes two catalog entries that happen to share an id.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(
            length(min = 1, max = 64),
            regex(pattern = "^[A-Za-z0-9_-](?:[A-Za-z0-9._-]{0,62}[A-Za-z0-9_-])?$")
        )
    )]
    pub id: String,
    pub source: ModelSource,
}

/// The product-facing origin of a model entry. The model-store layer keeps its
/// technical `Installed`/`Preset` ownership types; those are not serialized in
/// `config.json` and describe storage mechanics rather than user-facing source.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Imported,
    BuiltIn,
}

/// User-facing metadata for one build-shipped model.
///
/// The `id` is the built-in directory name and `title` is an editable display
/// name that never participates in model identity. Which list holds the record
/// is what carries its lifecycle: an import creates imported metadata and a
/// delete removes it; nothing creates or removes a built-in record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BuiltInModelMetadata {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(
            length(min = 1, max = 64),
            regex(pattern = "^[A-Za-z0-9_-](?:[A-Za-z0-9._-]{0,62}[A-Za-z0-9_-])?$")
        )
    )]
    pub id: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1, max = 128), regex(pattern = ".*\\S.*"))
    )]
    pub title: String,
}

/// User-facing metadata for one imported model.
///
/// `input_mode` is resolved once when the source is imported and then persisted
/// with the title. The Models page reads this value rather than rescanning the
/// package on every render, so renaming a model, changing its artwork, or
/// restarting the application cannot silently change its displayed mode.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ImportedModelMetadata {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(
            length(min = 1, max = 64),
            regex(pattern = "^[A-Za-z0-9_-](?:[A-Za-z0-9._-]{0,62}[A-Za-z0-9_-])?$")
        )
    )]
    pub id: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1, max = 128), regex(pattern = ".*\\S.*"))
    )]
    pub title: String,
    pub input_mode: ModelInputMode,
}

/// The input family a model belongs to.
///
/// Mver conversion knows this from the selected source section, the build
/// derives it from the stable ids of its three built-in models, and an ordinary
/// package resolves it from the key artwork before import commits. A package
/// that cannot be classified is rejected by the model store rather than stored
/// with a fourth, non-mode value.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelInputMode {
    Standard,
    Keyboard,
    Gamepad,
}

impl ModelInputMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Keyboard => "keyboard",
            Self::Gamepad => "gamepad",
        }
    }
}

pub const MODEL_METADATA_MAXIMUM_ID_BYTES: usize = 64;
pub const MODEL_METADATA_MAXIMUM_TITLE_CHARS: usize = 128;

// Keep this platform-neutral validator aligned with `bongocat_model::ModelId`;
// config intentionally does not depend on the model crate just for this check.
pub(crate) fn is_portable_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MODEL_METADATA_MAXIMUM_ID_BYTES
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !is_windows_reserved_model_id(value)
}

pub(crate) fn is_windows_reserved_model_id(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && bytes.is_ascii()
        && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
        && matches!(bytes[3], b'1'..=b'9')
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
                keyboard: KeyboardInputConfig {
                    release_fallback_timeout_ms: 500,
                },
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
                mirror_pointer_tracking: false,
                play_motion_audio: false,
                ignore_keyboard: false,
                ignore_gamepad: false,
                ignore_pointer: false,
                random_behavior: RandomBehaviorConfig::default(),
                gamepad_auto_switch: GamepadAutoSwitchConfig::default(),
            },
            shortcuts: ShortcutConfig::default(),
            system: SystemConfig {
                show_taskbar_icon: true,
                show_status_icon: true,
            },
            updates: UpdateConfig {
                check_automatically: false,
                check_interval_hours: DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS,
            },
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
        if self.input.keyboard.release_fallback_timeout_ms > 60_000 {
            return Err(ConfigError::InvalidValue(
                "input.keyboard.release_fallback_timeout_ms",
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

/// Validate one list of user-facing model metadata.
///
/// Both metadata lists are checked the same way even though installed records
/// also carry an input mode: a stable id that is present and unique within its
/// list, and a display name that is present, printable and short enough. The
/// two error paths are passed in rather than derived, so each message keeps
/// naming the field a user would have to look at instead of collapsing into a
/// generic one.
pub(crate) fn validate_model_metadata<'a>(
    id_error: &'static str,
    title_error: &'static str,
    metadata: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), ConfigError> {
    let mut ids = std::collections::BTreeSet::new();
    for (raw_id, raw_title) in metadata {
        if !is_portable_model_id(raw_id) || !ids.insert(raw_id) {
            return Err(ConfigError::InvalidValue(id_error));
        }
        let title = raw_title.trim();
        if title.is_empty()
            || title.chars().count() > MODEL_METADATA_MAXIMUM_TITLE_CHARS
            || raw_title.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidValue(title_error));
        }
    }
    Ok(())
}
