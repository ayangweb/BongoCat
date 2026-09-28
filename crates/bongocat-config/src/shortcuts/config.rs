//! What the document says: the user's own keys, the ones a model owns, and the
//! defaults handed to a model that has none.
//!
//! A chord is canonicalised on the way in rather than on the way out, so two
//! spellings of the same chord cannot both be in the table and both be live. A
//! chord that is already canonical is left alone, and one that cannot be is
//! refused — a configuration that silently kept a key it could not register
//! would show the user a shortcut that does nothing.

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
