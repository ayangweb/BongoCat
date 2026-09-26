//! Capturing a new chord without the keystroke reaching the app.
//!
//! Recording a shortcut means pressing keys, and those keys would otherwise
//! reach the product and fire its commands. Capture therefore suspends the
//! shortcut dispatcher for as long as it is armed, and the target says which
//! scope is being recorded so the page can show it.

use super::*;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ShortcutCaptureTarget {
    Command(String),
    ModelBehavior {
        model: SettingsModelKey,
        behavior_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShortcutCapture {
    pub(crate) target: ShortcutCaptureTarget,
    pub(crate) modifiers: Modifiers,
    pub(crate) keys: BTreeSet<String>,
}

impl ShortcutCapture {
    pub(crate) fn new(target: ShortcutCaptureTarget) -> Self {
        Self {
            target,
            modifiers: Modifiers::default(),
            keys: BTreeSet::new(),
        }
    }

    pub(crate) fn clear_temporary_input(&mut self) {
        self.modifiers = Modifiers::default();
        self.keys.clear();
    }
}
