//! The model catalog: what a model is, where it came from, and how a new
//! installation is checked before it is written.
//!
//! A model's identity is its id *and* its source, so two catalog entries that
//! happen to share a name are still two models. The id is validated rather than
//! trusted: a portable id may not contain a path separator, and a Windows id may
//! not be one the operating system reserves, because either would let a document
//! name a file outside the store.

use super::*;

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
    /// What the model plays on its own, or that it plays nothing on its own.
    pub mode: RandomBehaviorMode,
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
            mode: RandomBehaviorMode::default(),
            interval_seconds: DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        }
    }
}

/// What the idle scheduler is allowed to pick on its own.
///
/// A model declares motions and expressions as two different things a user can
/// also bind by hand, so "play something at random" was never one decision: some
/// users want the model to look alive without it walking around or interrupting
/// the pose they bound, and some want the opposite.
///
/// Turning it off is a fourth choice rather than a separate `enabled` flag. A
/// switch next to a mode dropdown has to be read as two questions before it means
/// anything, and it admits a state no user asked for: on, with nothing to play.
/// One enumeration has exactly one answer per configuration, and it is the answer
/// the settings page shows as a single dropdown.
///
/// `Off` is the default because a fresh configuration has never been told to
/// animate itself, and that is the behavior the v1 document already described.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RandomBehaviorMode {
    #[default]
    Off,
    Expressions,
    Motions,
    MotionsAndExpressions,
}

impl RandomBehaviorMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Expressions,
        Self::Motions,
        Self::MotionsAndExpressions,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Expressions => "expressions",
            Self::Motions => "motions",
            Self::MotionsAndExpressions => "motions_and_expressions",
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
