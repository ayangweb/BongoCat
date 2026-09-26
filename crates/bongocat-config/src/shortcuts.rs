//! The shortcut vocabulary: chords, keys, modifiers, bindings and the compiled
//! table the platform registers.
//!
//! A chord is stored as text so it round-trips through `config.json` unchanged,
//! and it is parsed rather than trusted: an unknown token, an ambiguous part or a
//! chord two bindings claim is rejected before anything is written.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShortcutConfig {
    /// Whether the application command bindings may reach the platform table.
    ///
    /// Defaults to `true`: the shortcuts page renders this gate as "disable
    /// window shortcuts", so a fresh v1 configuration keeps every command
    /// shortcut live. Turning it off excludes [`Self::command_bindings`] from
    /// [`Self::active_bindings`] and never rewrites or clears the recorded
    /// bindings, so turning it back on restores them without re-recording.
    /// Model behaviours have their own, independent gate.
    pub commands_enabled: bool,
    pub model_behaviors_enabled: bool,
    pub command_bindings: Vec<ShortcutBinding>,
    pub model_behavior_bindings: Vec<ModelBehaviorBinding>,
}

impl Default for ShortcutConfig {
    /// A derived `Default` would leave `commands_enabled` at `false` and a
    /// fresh configuration without any live command shortcut, which is the
    /// opposite of what the gate means.
    fn default() -> Self {
        Self {
            commands_enabled: true,
            model_behaviors_enabled: false,
            command_bindings: Vec::new(),
            model_behavior_bindings: Vec::new(),
        }
    }
}

impl ShortcutConfig {
    /// Return the persisted representation with every accepted binding in its
    /// stable spelling. Validation remains a separate step so callers can
    /// choose whether to normalize before an atomic commit.
    pub fn canonicalized(&self) -> Result<Self, ConfigError> {
        let command_bindings = self
            .command_bindings
            .iter()
            .map(|binding| {
                let command = ShortcutCommand::parse(&binding.command)
                    .map_err(|_| ConfigError::InvalidValue("shortcuts.command"))?
                    .as_str()
                    .to_owned();
                let shortcut = ShortcutChord::parse(&binding.shortcut)
                    .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?
                    .canonical();
                Ok(ShortcutBinding { command, shortcut })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        let model_behavior_bindings = self
            .model_behavior_bindings
            .iter()
            .map(|binding| {
                let behavior_id = binding
                    .parse_action()
                    .map_err(|_| ConfigError::InvalidValue("shortcuts.behavior"))?
                    .behavior_id();
                let shortcut = ShortcutChord::parse(&binding.shortcut)
                    .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?
                    .canonical();
                Ok(ModelBehaviorBinding {
                    model: binding.model.clone(),
                    behavior_id,
                    shortcut,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        Ok(Self {
            commands_enabled: self.commands_enabled,
            model_behaviors_enabled: self.model_behaviors_enabled,
            command_bindings,
            model_behavior_bindings,
        })
    }

    /// The bindings that are live at the same moment: every application
    /// command [`Self::commands_enabled`] admits, plus the behaviors of at most
    /// one model.
    ///
    /// Command bindings are gated here rather than dropped from the
    /// configuration, so switching them off and back on is lossless. Model
    /// behaviour chords are only unique *inside* their own model. The
    /// configuration keeps a binding for every model the user has activated,
    /// and exactly one of them is active at a time — the shortcuts page shows
    /// that model's behaviors, and the dispatcher drops a target whose complete
    /// `{ id, source }` identity is not the active one. Two models may therefore carry the
    /// same chord, which is what lets each model count its own defaults from
    /// the primary modifier's first digit.
    ///
    /// That also means compiling the whole configuration is ambiguous. This
    /// projection is the input [`Self::compile`] expects; it is likewise what
    /// keeps the chords of every other model out of the platform's global
    /// hotkey registrations, where they would occupy chords they can never
    /// fire from.
    pub fn active_bindings(&self, active_model: Option<&ModelIdentity>) -> Self {
        Self {
            commands_enabled: self.commands_enabled,
            model_behaviors_enabled: self.model_behaviors_enabled,
            command_bindings: if self.commands_enabled {
                self.command_bindings.clone()
            } else {
                Vec::new()
            },
            model_behavior_bindings: if self.model_behaviors_enabled {
                self.model_behavior_bindings
                    .iter()
                    .filter(|binding| active_model.is_some_and(|active| active == &binding.model))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    /// Compile persisted bindings once at the configuration/platform
    /// boundary. The resulting table contains only closed, typed targets;
    /// platform adapters can match a mapped key token without reparsing
    /// user-controlled strings on an input callback.
    ///
    /// A compiled table must be unambiguous, so a configuration that carries
    /// several models is projected onto the live one with [`Self::active_bindings`]
    /// before it gets here.
    pub fn compile(&self) -> Result<CompiledShortcuts, ConfigError> {
        CompiledShortcuts::compile(self)
    }
}

/// The digits, in the order the legacy auto-assignment walked them.
pub(crate) const BEHAVIOR_SHORTCUT_DIGITS: &str = "1234567890";
/// The letters, in the same order the legacy auto-assignment walked them. It is
/// a keyboard layout order rather than alphabetical, and it is kept as-is so a
/// model's Nth behavior lands on the same chord it did before.
pub(crate) const BEHAVIOR_SHORTCUT_LETTERS: &str = "QWERTYUIOPASDFGHJKLZXCVBNM";
/// The four modifier tiers layered over each alphabet, all on top of the
/// platform's command modifier.
pub(crate) const BEHAVIOR_SHORTCUT_MODIFIER_TIERS: [u8; 4] = [
    0,
    ShortcutModifiers::SHIFT,
    ShortcutModifiers::ALT,
    ShortcutModifiers::SHIFT | ShortcutModifiers::ALT,
];

/// How many chords the legacy auto-assignment can hand out: four modifier tiers
/// over the ten digits, then the same four over the twenty-six letters.
pub const BEHAVIOR_SHORTCUT_CAPACITY: usize = BEHAVIOR_SHORTCUT_MODIFIER_TIERS.len()
    * (BEHAVIOR_SHORTCUT_DIGITS.len() + BEHAVIOR_SHORTCUT_LETTERS.len());

/// The chord the legacy implementation handed out at `position`, counting from
/// zero over the digit tiers first and the letter tiers second.
///
/// `primary` is the platform's command modifier — [`ShortcutModifiers::META`]
/// on macOS, [`ShortcutModifiers::CONTROL`] everywhere else. It is a parameter
/// rather than a platform check so this crate stays platform-free. Returns
/// `None` past the last slot; the legacy implementation returned an empty
/// string there and left the remaining behaviors unbound.
pub fn default_behavior_shortcut(position: usize, primary: u8) -> Option<ShortcutChord> {
    if position >= BEHAVIOR_SHORTCUT_CAPACITY {
        return None;
    }
    let digit_capacity = BEHAVIOR_SHORTCUT_MODIFIER_TIERS.len() * BEHAVIOR_SHORTCUT_DIGITS.len();
    let (alphabet, offset) = if position < digit_capacity {
        (BEHAVIOR_SHORTCUT_DIGITS, position)
    } else {
        (BEHAVIOR_SHORTCUT_LETTERS, position - digit_capacity)
    };
    let tier = offset / alphabet.len();
    let key = alphabet.as_bytes()[offset % alphabet.len()] as char;
    let modifiers = ShortcutModifiers::from_bits(primary | BEHAVIOR_SHORTCUT_MODIFIER_TIERS[tier])?;
    Some(ShortcutChord {
        modifiers,
        key: ShortcutKey::parse(&key.to_string()).ok()?,
    })
}

pub(crate) fn canonical_chord(value: &str) -> Option<String> {
    ShortcutChord::parse(value)
        .map(|chord| chord.canonical())
        .ok()
}

/// Fill in the legacy default chord for every behavior of one model that has no
/// binding yet, and return how many bindings were added.
///
/// The legacy implementation ran this on every model load: it walked the
/// model's motions in declaration order, then its expressions, and bound each
/// unbound one to the next chord of the tiering above. Deliberate differences
/// from that implementation:
///
/// - A chord any binding already uses **in the same scope** is skipped instead
///   of reused. The legacy implementation indexed by position, so once a user
///   edited one binding the next behavior could be handed a chord that was
///   already taken; the configuration rejects duplicate chords within a
///   scope outright, which would make the whole configuration invalid rather
///   than merely ambiguous. The scope is the model being assigned plus the
///   application command bindings: another model's chords do not count,
///   because only one model's behaviors are live at a time and every model is
///   meant to count its own defaults from the primary modifier's first digit.
/// - Application command bindings count as taken. The legacy implementation
///   kept window and behavior shortcuts in separate stores even though both
///   were registered globally, so the two could collide. Commands are live
///   regardless of which model is active, so they stay in scope here.
///
/// An existing binding is never rewritten, which is what makes this safe to
/// repeat on every activation: only behaviors the user has not touched are
/// filled in. `position` counts from zero for every call, so activating a
/// second model restarts at the first chord rather than continuing where the
/// previous model stopped.
pub fn assign_default_behavior_shortcuts(
    shortcuts: &mut ShortcutConfig,
    model: &ModelIdentity,
    behavior_ids: &[String],
    primary: u8,
) -> usize {
    let mut taken: std::collections::BTreeSet<String> = shortcuts
        .command_bindings
        .iter()
        .filter_map(|binding| canonical_chord(&binding.shortcut))
        .chain(
            shortcuts
                .model_behavior_bindings
                .iter()
                .filter(|binding| binding.model == *model)
                .filter_map(|binding| canonical_chord(&binding.shortcut)),
        )
        .collect();

    let mut position = 0_usize;
    let mut added = 0_usize;
    for behavior_id in behavior_ids {
        if shortcuts
            .model_behavior_bindings
            .iter()
            .any(|binding| binding.model == *model && binding.behavior_id == *behavior_id)
        {
            continue;
        }
        let mut assigned = None;
        while let Some(chord) = default_behavior_shortcut(position, primary) {
            position += 1;
            let candidate = chord.canonical();
            if taken.insert(candidate.clone()) {
                assigned = Some(candidate);
                break;
            }
        }
        let Some(shortcut) = assigned else {
            break;
        };
        shortcuts
            .model_behavior_bindings
            .push(ModelBehaviorBinding {
                model: model.clone(),
                behavior_id: behavior_id.clone(),
                shortcut,
            });
        added += 1;
    }
    added
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutTarget {
    Application(ShortcutCommand),
    ModelBehavior {
        model: ModelIdentity,
        action: ModelBehaviorAction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledShortcut {
    chord: ShortcutChord,
    target: ShortcutTarget,
}

impl CompiledShortcut {
    pub fn chord(&self) -> &ShortcutChord {
        &self.chord
    }

    pub fn target(&self) -> &ShortcutTarget {
        &self.target
    }

    pub fn matches(&self, modifiers: ShortcutModifiers, key: &str) -> bool {
        self.chord.matches(modifiers, key)
    }

    pub fn matches_hid_usage(&self, modifiers: ShortcutModifiers, hid_usage: u16) -> bool {
        self.chord.matches_hid_usage(modifiers, hid_usage)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompiledShortcuts {
    bindings: Vec<CompiledShortcut>,
}

/// Shared, atomically replaceable shortcut table used by the input owner.
/// Configuration commits publish a complete compiled table; readers never
/// observe partially parsed bindings or hold a config writer lock.
#[derive(Clone, Default)]
pub struct ShortcutTable {
    value: Arc<RwLock<CompiledShortcuts>>,
}

impl ShortcutTable {
    pub fn new(value: CompiledShortcuts) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
        }
    }

    pub fn load(&self) -> CompiledShortcuts {
        self.value
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn replace(&self, value: CompiledShortcuts) {
        *self
            .value
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    }
}

impl CompiledShortcuts {
    pub fn compile(config: &ShortcutConfig) -> Result<Self, ConfigError> {
        let mut bindings = Vec::with_capacity(
            config
                .command_bindings
                .len()
                .saturating_add(config.model_behavior_bindings.len()),
        );
        let mut seen = std::collections::BTreeSet::new();

        for binding in &config.command_bindings {
            let chord = ShortcutChord::parse(&binding.shortcut)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?;
            if !seen.insert(chord.canonical()) {
                return Err(ConfigError::InvalidValue("shortcuts.conflict"));
            }
            let command = ShortcutCommand::parse(&binding.command)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.command"))?;
            bindings.push(CompiledShortcut {
                chord,
                target: ShortcutTarget::Application(command),
            });
        }

        for binding in &config.model_behavior_bindings {
            if binding.model.id.trim().is_empty() {
                return Err(ConfigError::InvalidValue(
                    "shortcuts.model_behavior_bindings",
                ));
            }
            let chord = ShortcutChord::parse(&binding.shortcut)
                .map_err(|_| ConfigError::InvalidValue("shortcuts.binding"))?;
            if !seen.insert(chord.canonical()) {
                return Err(ConfigError::InvalidValue("shortcuts.conflict"));
            }
            let action = binding
                .parse_action()
                .map_err(|_| ConfigError::InvalidValue("shortcuts.behavior"))?;
            bindings.push(CompiledShortcut {
                chord,
                target: ShortcutTarget::ModelBehavior {
                    model: ModelIdentity {
                        id: binding.model.id.trim().to_owned(),
                        source: binding.model.source,
                    },
                    action,
                },
            });
        }

        Ok(Self { bindings })
    }

    pub fn iter(&self) -> impl Iterator<Item = &CompiledShortcut> {
        self.bindings.iter()
    }

    pub fn resolve(&self, modifiers: ShortcutModifiers, key: &str) -> Option<&CompiledShortcut> {
        self.bindings
            .iter()
            .find(|binding| binding.matches(modifiers, key))
    }

    pub fn resolve_hid_usage(
        &self,
        modifiers: ShortcutModifiers,
        hid_usage: u16,
    ) -> Option<&CompiledShortcut> {
        self.bindings
            .iter()
            .find(|binding| binding.matches_hid_usage(modifiers, hid_usage))
    }
}

/// Application-level commands that may be persisted as global shortcuts.
/// Keeping this list closed prevents an unvalidated string from becoming a
/// platform registration or runtime command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutCommand {
    ToggleOverlay,
    OpenSettings,
    ToggleMirror,
    ToggleIgnoreMouseInput,
    ToggleIgnoreKeyboardInput,
    ToggleIgnoreGamepadInput,
    ToggleClickThrough,
    ToggleAlwaysOnTop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ShortcutCommandParseError {
    #[error("shortcut command must not be blank")]
    Empty,
    #[error("shortcut command is not supported")]
    Unknown,
}

impl ShortcutCommand {
    pub fn parse(value: &str) -> Result<Self, ShortcutCommandParseError> {
        match value.trim() {
            "toggle_overlay" => Ok(Self::ToggleOverlay),
            "open_settings" => Ok(Self::OpenSettings),
            "toggle_mirror" => Ok(Self::ToggleMirror),
            "toggle_ignore_mouse_input" => Ok(Self::ToggleIgnoreMouseInput),
            "toggle_ignore_keyboard_input" => Ok(Self::ToggleIgnoreKeyboardInput),
            "toggle_ignore_gamepad_input" => Ok(Self::ToggleIgnoreGamepadInput),
            "toggle_click_through" => Ok(Self::ToggleClickThrough),
            "toggle_always_on_top" => Ok(Self::ToggleAlwaysOnTop),
            "" => Err(ShortcutCommandParseError::Empty),
            _ => Err(ShortcutCommandParseError::Unknown),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToggleOverlay => "toggle_overlay",
            Self::OpenSettings => "open_settings",
            Self::ToggleMirror => "toggle_mirror",
            Self::ToggleIgnoreMouseInput => "toggle_ignore_mouse_input",
            Self::ToggleIgnoreKeyboardInput => "toggle_ignore_keyboard_input",
            Self::ToggleIgnoreGamepadInput => "toggle_ignore_gamepad_input",
            Self::ToggleClickThrough => "toggle_click_through",
            Self::ToggleAlwaysOnTop => "toggle_always_on_top",
        }
    }
}

/// A platform-neutral keyboard chord used to validate persisted shortcut
/// bindings before a platform adapter attempts to capture or register them.
/// The key is kept as a canonical token because mapping it to a physical key
/// is platform-specific and belongs outside the configuration crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutChord {
    modifiers: ShortcutModifiers,
    key: ShortcutKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutKey {
    canonical: String,
    hid_usage: u16,
}

impl ShortcutKey {
    pub fn parse(value: &str) -> Result<Self, ShortcutParseError> {
        let value = value.trim();
        if value.is_empty()
            || !value.is_ascii()
            || value.chars().any(char::is_whitespace)
            || value.chars().any(|character| !character.is_ascii_graphic())
        {
            return Err(ShortcutParseError::InvalidKey);
        }

        if value.len() == 1 {
            let byte = value.as_bytes()[0];
            if byte.is_ascii_alphabetic() {
                let upper = byte.to_ascii_uppercase();
                return Ok(Self {
                    canonical: char::from(upper).to_string(),
                    hid_usage: 0x04 + u16::from(upper - b'A'),
                });
            }
            if byte.is_ascii_digit() {
                let hid_usage = if byte == b'0' {
                    0x27
                } else {
                    0x1e + u16::from(byte - b'1')
                };
                return Ok(Self {
                    canonical: char::from(byte).to_string(),
                    hid_usage,
                });
            }
        }

        if value.len() == 4 && value[..3].eq_ignore_ascii_case("key") {
            return Self::parse(&value[3..]);
        }
        if value.len() == 6 && value[..5].eq_ignore_ascii_case("digit") {
            return Self::parse(&value[5..]);
        }

        let (canonical, hid_usage) = NAMED_SHORTCUT_KEYS
            .iter()
            .find_map(|(alias, canonical, usage)| {
                value
                    .eq_ignore_ascii_case(alias)
                    .then_some((*canonical, *usage))
            })
            .ok_or(ShortcutParseError::InvalidKey)?;
        Ok(Self {
            canonical: canonical.to_owned(),
            hid_usage,
        })
    }

    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    pub const fn hid_usage(&self) -> u16 {
        self.hid_usage
    }
}

pub(crate) const NAMED_SHORTCUT_KEYS: &[(&str, &str, u16)] = &[
    ("-", "-", 0x2d),
    ("Minus", "-", 0x2d),
    ("=", "=", 0x2e),
    ("Equal", "=", 0x2e),
    ("Enter", "Enter", 0x28),
    ("Escape", "Escape", 0x29),
    ("Esc", "Escape", 0x29),
    ("Backspace", "Backspace", 0x2a),
    ("Tab", "Tab", 0x2b),
    ("Space", "Space", 0x2c),
    ("BracketLeft", "BracketLeft", 0x2f),
    ("BracketRight", "BracketRight", 0x30),
    ("Backslash", "Backslash", 0x31),
    ("Semicolon", "Semicolon", 0x33),
    ("Quote", "Quote", 0x34),
    ("Backquote", "Backquote", 0x35),
    ("Comma", "Comma", 0x36),
    ("Period", "Period", 0x37),
    ("Slash", "Slash", 0x38),
    ("CapsLock", "CapsLock", 0x39),
    ("F1", "F1", 0x3a),
    ("F2", "F2", 0x3b),
    ("F3", "F3", 0x3c),
    ("F4", "F4", 0x3d),
    ("F5", "F5", 0x3e),
    ("F6", "F6", 0x3f),
    ("F7", "F7", 0x40),
    ("F8", "F8", 0x41),
    ("F9", "F9", 0x42),
    ("F10", "F10", 0x43),
    ("F11", "F11", 0x44),
    ("F12", "F12", 0x45),
    ("PrintScreen", "PrintScreen", 0x46),
    ("ScrollLock", "ScrollLock", 0x47),
    ("Pause", "Pause", 0x48),
    ("Insert", "Insert", 0x49),
    ("Home", "Home", 0x4a),
    ("PageUp", "PageUp", 0x4b),
    ("Delete", "Delete", 0x4c),
    ("End", "End", 0x4d),
    ("PageDown", "PageDown", 0x4e),
    ("ArrowRight", "ArrowRight", 0x4f),
    ("ArrowLeft", "ArrowLeft", 0x50),
    ("ArrowDown", "ArrowDown", 0x51),
    ("ArrowUp", "ArrowUp", 0x52),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShortcutModifiers(u8);

impl ShortcutModifiers {
    pub const CONTROL: u8 = 1 << 0;
    pub const ALT: u8 = 1 << 1;
    pub const SHIFT: u8 = 1 << 2;
    pub const META: u8 = 1 << 3;

    pub(crate) const VALID_BITS: u8 = Self::CONTROL | Self::ALT | Self::SHIFT | Self::META;

    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::VALID_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ShortcutParseError {
    #[error("shortcut must contain a key")]
    MissingKey,
    #[error("shortcut contains an empty part")]
    EmptyPart,
    #[error("shortcut contains a duplicate modifier")]
    DuplicateModifier,
    #[error("shortcut must contain exactly one key")]
    MultipleKeys,
    #[error("shortcut key is not a supported physical key token")]
    InvalidKey,
}

impl ShortcutChord {
    pub fn parse(value: &str) -> Result<Self, ShortcutParseError> {
        let mut modifiers = 0_u8;
        let mut key = None;
        for part in value.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err(ShortcutParseError::EmptyPart);
            }
            let modifier = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => Some(ShortcutModifiers::CONTROL),
                "alt" | "option" => Some(ShortcutModifiers::ALT),
                "shift" => Some(ShortcutModifiers::SHIFT),
                "meta" | "command" | "cmd" | "win" | "windows" => Some(ShortcutModifiers::META),
                _ => None,
            };
            if let Some(modifier) = modifier {
                if modifiers & modifier != 0 {
                    return Err(ShortcutParseError::DuplicateModifier);
                }
                modifiers |= modifier;
                continue;
            }
            if key.is_some() {
                return Err(ShortcutParseError::MultipleKeys);
            }
            key = Some(ShortcutKey::parse(part)?);
        }
        let key = key.ok_or(ShortcutParseError::MissingKey)?;
        Ok(Self {
            modifiers: ShortcutModifiers(modifiers),
            key,
        })
    }

    pub const fn modifiers(&self) -> ShortcutModifiers {
        self.modifiers
    }

    pub fn key(&self) -> &str {
        self.key.canonical()
    }

    pub const fn key_hid_usage(&self) -> u16 {
        self.key.hid_usage()
    }

    pub fn matches(&self, modifiers: ShortcutModifiers, key: &str) -> bool {
        self.modifiers == modifiers
            && ShortcutKey::parse(key).is_ok_and(|key| key.hid_usage() == self.key.hid_usage())
    }

    pub fn matches_hid_usage(&self, modifiers: ShortcutModifiers, hid_usage: u16) -> bool {
        self.modifiers == modifiers && self.key.hid_usage() == hid_usage
    }

    /// Return a stable representation used for conflict detection and later
    /// platform registration. Modifier aliases and input ordering collapse to
    /// this one form.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        for (bit, name) in [
            (ShortcutModifiers::CONTROL, "Control"),
            (ShortcutModifiers::ALT, "Alt"),
            (ShortcutModifiers::SHIFT, "Shift"),
            (ShortcutModifiers::META, "Meta"),
        ] {
            if self.modifiers.0 & bit != 0 {
                parts.push(name);
            }
        }
        parts.push(self.key.canonical());
        parts.join("+")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShortcutBinding {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub command: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub shortcut: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelBehaviorBinding {
    /// The complete model identity, including source. An id alone is not
    /// unique because built-in and imported catalogs may contain the same id.
    pub model: ModelIdentity,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub behavior_id: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub shortcut: String,
}

/// A model action encoded by the Native shortcut contract. The model identity
/// is kept on the binding so the application can scope the action to one model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelBehaviorAction {
    Motion { group: String, index: usize },
    Expression { name: String },
}

impl ModelBehaviorAction {
    /// The canonical `behavior_id` spelling this action persists as. Both the
    /// canonicalizing path and the default-assignment path go through here, so
    /// a generated binding and a user-recorded one can never disagree on how
    /// the same motion or expression is named.
    pub fn behavior_id(&self) -> String {
        match self {
            Self::Motion { group, index } => format!("motion:{group}:{index}"),
            Self::Expression { name } => format!("expression:{name}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelBehaviorParseError {
    #[error("model behavior must not be blank")]
    Empty,
    #[error("model motion behavior must be motion:<group>:<index>")]
    InvalidMotion,
    #[error("model expression behavior must be expression:<name>")]
    InvalidExpression,
    #[error("model behavior kind is not supported")]
    UnknownKind,
}

impl ModelBehaviorBinding {
    pub fn parse_action(&self) -> Result<ModelBehaviorAction, ModelBehaviorParseError> {
        let value = self.behavior_id.trim();
        if value.is_empty() {
            return Err(ModelBehaviorParseError::Empty);
        }
        let mut parts = value.split(':');
        match parts.next() {
            Some("motion") => {
                let group = parts.next().unwrap_or_default().trim();
                let Some(index) = parts.next() else {
                    return Err(ModelBehaviorParseError::InvalidMotion);
                };
                if group.is_empty() || parts.next().is_some() {
                    return Err(ModelBehaviorParseError::InvalidMotion);
                }
                let index = index
                    .parse::<usize>()
                    .map_err(|_| ModelBehaviorParseError::InvalidMotion)?;
                Ok(ModelBehaviorAction::Motion {
                    group: group.to_owned(),
                    index,
                })
            }
            Some("expression") => {
                let name = parts.collect::<Vec<_>>().join(":");
                if name.trim().is_empty() {
                    return Err(ModelBehaviorParseError::InvalidExpression);
                }
                Ok(ModelBehaviorAction::Expression {
                    name: name.trim().to_owned(),
                })
            }
            Some(_) => Err(ModelBehaviorParseError::UnknownKind),
            None => Err(ModelBehaviorParseError::Empty),
        }
    }
}
