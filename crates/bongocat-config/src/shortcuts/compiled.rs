//! The form the operating system registers.
//!
//! A `CompiledShortcut` is a chord that has already been checked, and a
//! `ShortcutTable` is the one current set of them. The table hands out a
//! revision with every value so a reader can tell whether what it holds is still
//! current — that is what lets a thread re-read the table after a sleep without
//! registering the same chord twice or missing a change.

use super::*;

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
    pub(crate) chord: ShortcutChord,
    pub(crate) target: ShortcutTarget,
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
    pub(crate) bindings: Vec<CompiledShortcut>,
}

/// Shared, atomically replaceable shortcut table used by the input owner.
/// Configuration commits publish a complete compiled table; readers never
/// observe partially parsed bindings or hold a config writer lock.
#[derive(Clone, Default)]
pub struct ShortcutTable {
    pub(crate) value: Arc<RwLock<CompiledShortcuts>>,
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
