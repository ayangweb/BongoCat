//! The shortcut vocabulary: chords, keys, modifiers, bindings and the compiled
//! table the platform registers.
//!
//! A chord is stored as text so it round-trips through `config.json` unchanged,
//! and it is parsed rather than trusted: an unknown token, an ambiguous part or a
//! chord two bindings claim is rejected before anything is written.

use super::*;

mod chord;
mod command;
mod compiled;
mod config;

pub use chord::{ShortcutChord, ShortcutKey, ShortcutModifiers, ShortcutParseError};
pub use command::{
    ModelBehaviorAction, ModelBehaviorBinding, ModelBehaviorParseError, ShortcutBinding,
    ShortcutCommand, ShortcutCommandParseError,
};
pub use compiled::{
    CompiledShortcut, CompiledShortcuts, ShortcutTable, ShortcutTablePublication, ShortcutTarget,
};
pub use config::{
    BEHAVIOR_SHORTCUT_CAPACITY, ShortcutConfig, assign_default_behavior_shortcuts,
    default_behavior_shortcut,
};
