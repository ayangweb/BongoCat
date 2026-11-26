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
    /// Whether the model's pointer tracking runs backwards along the X axis.
    ///
    /// This flips the pointer's X and Z parameters together: Z is the roll that
    /// follows the same horizontal sweep, so reversing only X would leave the
    /// head turned the wrong way.
    pub mirror_pointer_tracking_horizontal: bool,
    /// Whether the model's pointer tracking runs backwards along the Y axis.
    ///
    /// Independent of [`Self::mirror_pointer_tracking_horizontal`] because the
    /// two axes are separate corrections: a model that follows the cursor
    /// correctly side to side can still look up when the cursor goes down, and
    /// only the second switch fixes that.
    pub mirror_pointer_tracking_vertical: bool,
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
    /// Whether every held key with artwork draws its own key image instead of
    /// one image per hand.
    ///
    /// Off, the hand that a model binds several keys to draws only the most
    /// recently pressed of them, so a chord collapses into a single picture.
    /// On, each held key keeps its own picture and they are stacked in press
    /// order, newest on top, which is what makes a fast chord legible instead
    /// of indistinguishable from a single tap. The paw parameters are unchanged
    /// either way: a hand is down while any key bound to it is held.
    ///
    /// Defaults to `false`, and the field carries `#[serde(default)]` so a
    /// configuration written before it existed still loads as the compatibility
    /// mode rather than failing the strict v1 parse.
    #[serde(default)]
    pub show_all_pressed_keys: bool,
    pub ignore_pointer: bool,
    pub random_behavior: RandomBehaviorConfig,
    /// Switching the selected model when gamepads connect or disconnect.
    pub gamepad_auto_switch: GamepadAutoSwitchConfig,
    /// Whether a model returns to the expression the user last chose for it.
    ///
    /// Defaults to `false`: a fresh configuration has never been told to restore
    /// anything, and an expression a user only previewed once is not a standing
    /// preference. The recorded expressions themselves are kept either way, so
    /// turning this off stops the restore without discarding what is remembered.
    pub remember_last_expression: bool,
    /// Whether triggering the expression that is already showing turns it off.
    ///
    /// An expression holds until something replaces it, so a repeat request for
    /// the same one has always been a no-op. The legacy build made it a toggle —
    /// asking for the face you are already wearing takes it off and the model
    /// returns to its own default — and a product that cannot get back to the
    /// default face without switching models cannot be corrected from the UI.
    ///
    /// Defaults to `false` because that is what this build has always done and a
    /// user who relies on it must keep it; the field carries `#[serde(default)]`
    /// so a configuration written before it existed still loads as the
    /// non-toggling behavior rather than failing the strict v1 parse.
    #[serde(default)]
    pub toggle_repeated_expression: bool,
    /// Whether distinct motions play concurrently in accepted start order.
    #[serde(default)]
    pub allow_motion_overlap: bool,
    /// The expression each model was last showing, one record per model.
    ///
    /// Expressions are per-model assets, so a name recorded for one model is
    /// meaningless to another; recording them separately is what lets switching
    /// back to a model return to the face it was left wearing.
    pub last_expressions: Vec<ModelExpressionMemory>,
    /// What each motion and expression is called in the settings window.
    ///
    /// The rows and the checkboxes both name a behavior by position — "Motion 3" —
    /// because the resource names inside a package are internal numbering the user
    /// cannot see. A row's own name is what lets the user find the one they meant
    /// without counting positions, so a name is per model and per behavior.
    ///
    /// Defaults to empty, which is what every configuration written before the field
    /// existed means: every row keeps showing its numbered label. The field carries
    /// `#[serde(default)]` so such a document still loads instead of failing the
    /// strict v1 parse.
    ///
    /// An empty `name` is not stored: it means "go back to the numbered label", which
    /// is a removal rather than a name. `model.random_behavior.included` shares this
    /// model-plus-behavior identity for the same reason — one model, one row, one
    /// spelling for both.
    ///
    /// `uniqueItems` covers the case the schema can express — two byte-identical rows —
    /// which is what a hand-edited document duplicated by copy-paste looks like. Two
    /// rows that agree on the model and the behavior but differ in the name are a
    /// narrower case that only the Rust validator can see; it is covered by
    /// `a_behavior_name_is_parsed_bounded_and_held_once_per_behavior` rather than by a
    /// fixture, because a fixture the schema accepts would make the two layers disagree.
    #[serde(default)]
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(extend("uniqueItems" = true))
    )]
    pub behavior_names: Vec<ModelBehaviorName>,
}

/// One behavior's user-facing name.
///
/// The behavior is a complete [`ModelIdentity`] plus a `behavior_id` rather than a
/// bare id, for the same reason [`ModelExpressionMemory`] is: two catalog entries may
/// share an id, and a behavior name only means anything inside the package it came
/// from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelBehaviorName {
    pub model: ModelIdentity,
    /// The behavior this names, spelled the way the whole configuration spells it:
    /// `motion:<group>:<index>` or `expression:<name>`.
    ///
    /// The schema carries that shape rather than a "not blank" pattern, because the
    /// generated JSON Schema is itself checked against the fixture corpus: a loose
    /// pattern would let the schema accept a document the Rust validator refuses, and
    /// the two layers would then disagree about what a valid `config.json` is.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(
            length(min = 1, max = 255),
            regex(pattern = "(motion:[^:]+:[0-9]+|expression:.+)")
        )
    )]
    pub behavior_id: String,
    /// What the row shows instead of its numbered label.
    ///
    /// Bounded and printable: it is the row's own text, so a long or unprintable one
    /// would be the page's problem rather than the model's.
    ///
    /// The pattern spells out "no control characters and not only whitespace" rather
    /// than leaning on `\S`, because the generated JSON Schema is checked against the
    /// same fixture corpus as the Rust validator and the two have to agree on what a
    /// valid `config.json` is. `\x7f-\x9f` is included because `char::is_control` covers
    /// the C1 range too, not just the C0 one.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(
            length(min = 1, max = 64),
            regex(
                pattern = "^[^\\x00-\\x1f\\x7f-\\x9f]*[^\\s\\x00-\\x1f\\x7f-\\x9f][^\\x00-\\x1f\\x7f-\\x9f]*$"
            )
        )
    )]
    pub name: String,
}

/// One model's remembered expression.
///
/// The model is a complete [`ModelIdentity`] rather than a bare id because two
/// catalog entries may share an id, and an expression name only means anything
/// inside the package it came from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelExpressionMemory {
    pub model: ModelIdentity,
    /// The expression file name, exactly as the model declares it.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1, max = 255), regex(pattern = ".*\\S.*"))
    )]
    pub expression: String,
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

/// One remembered expression is recorded per model, so the list is bounded by how
/// many models a catalog can hold rather than by how often the user plays an
/// expression. Imported models can be added and removed freely, so the ceiling is
/// generous enough that no real installation reaches it and small enough that a
/// hand-edited document cannot turn the list into an unbounded array.
pub const MAXIMUM_MODEL_EXPRESSION_MEMORIES: usize = 256;

/// An expression name is a package-relative file name such as
/// `live2d_expression0.exp3.json`. The longest plausible asset name is far below
/// this; the bound exists so a stored name can never be an arbitrary blob.
pub const MODEL_EXPRESSION_MEMORY_MAXIMUM_NAME_BYTES: usize = 255;

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

/// Validate the remembered-expression list.
///
/// A record is only useful if it names exactly one model and one of that model's
/// own expressions, so the list holds no duplicate model: two records for one
/// model would leave the restored face up to which of them the parser saw last.
/// The name itself is bounded but otherwise left alone, because whether an
/// expression still exists is a property of the model package rather than of the
/// configuration document — a model that no longer ships it simply restores
/// nothing.
pub(crate) fn validate_model_expression_memories(
    memories: &[ModelExpressionMemory],
) -> Result<(), ConfigError> {
    if memories.len() > MAXIMUM_MODEL_EXPRESSION_MEMORIES {
        return Err(ConfigError::InvalidValue("model.last_expressions"));
    }
    let mut models = std::collections::BTreeSet::new();
    for memory in memories {
        if !is_portable_model_id(&memory.model.id) || !models.insert(&memory.model) {
            return Err(ConfigError::InvalidValue("model.last_expressions.model.id"));
        }
        if memory.expression.trim().is_empty()
            || memory.expression.len() > MODEL_EXPRESSION_MEMORY_MAXIMUM_NAME_BYTES
            || memory.expression.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidValue(
                "model.last_expressions.expression",
            ));
        }
    }
    Ok(())
}

/// The longest user-facing name one behavior row may carry.
///
/// A row is a single line in a settings list, so this is generous enough for
/// "the sleepy one" and small enough that a hand-edited document cannot turn the
/// label into an unbounded blob.
pub const MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS: usize = 64;

/// How many behavior names one installation can hold.
///
/// A user names the behaviors they care about, and the list is bounded by how many
/// models exist rather than by how often they rename: an imported model can be added
/// and removed freely, so this is generous enough that no real installation reaches
/// it.
pub const MAXIMUM_MODEL_BEHAVIOR_NAMES: usize = 1024;

/// Validate the behavior-name list.
///
/// One row per behavior of a model: two rows for the same behavior would leave the
/// label up to which of them the parser saw last. The behavior is parsed rather than
/// trusted — it is the same `behavior_id` vocabulary the shortcut bindings use — and
/// the name is bounded and printable because it is the row's own text.
pub(crate) fn validate_model_behavior_names(
    names: &[ModelBehaviorName],
) -> Result<(), ConfigError> {
    if names.len() > MAXIMUM_MODEL_BEHAVIOR_NAMES {
        return Err(ConfigError::InvalidValue("model.behavior_names"));
    }
    let mut rows = std::collections::BTreeSet::new();
    for row in names {
        if !is_portable_model_id(&row.model.id)
            || !rows.insert((row.model.clone(), row.behavior_id.clone()))
        {
            return Err(ConfigError::InvalidValue("model.behavior_names.model.id"));
        }
        let binding = ModelBehaviorBinding {
            model: row.model.clone(),
            behavior_id: row.behavior_id.clone(),
            shortcut: String::new(),
        };
        let name = row.name.trim();
        if binding.parse_action().is_err() {
            return Err(ConfigError::InvalidValue(
                "model.behavior_names.behavior_id",
            ));
        }
        if name.is_empty()
            || name.chars().count() > MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS
            || row.name.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidValue("model.behavior_names.name"));
        }
    }
    Ok(())
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
