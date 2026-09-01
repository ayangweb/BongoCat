use bongocat_config::{CompiledShortcuts, ShortcutModifiers, ShortcutTarget};
use bongocat_runtime::{InputEdge, PhysicalKey};
use std::collections::BTreeSet;

/// Platform-neutral shortcut edge matcher used after native key codes have
/// already been mapped to USB HID usages. It owns only transient pressed state
/// and returns closed targets; registration and command execution stay with
/// their platform/application owners.
#[derive(Clone, Debug)]
pub struct ShortcutMatcher {
    shortcuts: CompiledShortcuts,
    pressed: BTreeSet<PhysicalKey>,
}

impl ShortcutMatcher {
    pub fn new(shortcuts: CompiledShortcuts) -> Self {
        Self {
            shortcuts,
            pressed: BTreeSet::new(),
        }
    }

    pub fn replace(&mut self, shortcuts: CompiledShortcuts) {
        self.shortcuts = shortcuts;
    }

    pub fn apply(&mut self, key: PhysicalKey, edge: InputEdge) -> Option<ShortcutTarget> {
        match edge {
            InputEdge::Down => {
                if !self.pressed.insert(key) || modifier_bit(key).is_some() {
                    return None;
                }
                self.shortcuts
                    .resolve_hid_usage(self.modifiers(), key.hid_usage())
                    .map(|binding| binding.target().clone())
            }
            InputEdge::Up => {
                self.pressed.remove(&key);
                None
            }
        }
    }

    pub fn reconcile(&mut self, pressed: impl IntoIterator<Item = PhysicalKey>) {
        self.pressed = pressed.into_iter().collect();
    }

    pub fn reset(&mut self) {
        self.pressed.clear();
    }

    pub fn pressed_count(&self) -> usize {
        self.pressed.len()
    }

    fn modifiers(&self) -> ShortcutModifiers {
        let bits = self
            .pressed
            .iter()
            .filter_map(|key| modifier_bit(*key))
            .fold(0, |bits, bit| bits | bit);
        ShortcutModifiers::from_bits(bits).expect("modifier mapping uses only declared bits")
    }
}

fn modifier_bit(key: PhysicalKey) -> Option<u8> {
    match key.hid_usage() {
        0xe0 | 0xe4 => Some(ShortcutModifiers::CONTROL),
        0xe1 | 0xe5 => Some(ShortcutModifiers::SHIFT),
        0xe2 | 0xe6 => Some(ShortcutModifiers::ALT),
        0xe3 | 0xe7 => Some(ShortcutModifiers::META),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::{
        ModelBehaviorAction, ModelBehaviorBinding, ShortcutBinding, ShortcutCommand, ShortcutConfig,
    };

    fn matcher() -> ShortcutMatcher {
        ShortcutMatcher::new(
            ShortcutConfig {
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Shift+B".to_owned(),
                }],
                model_behaviors: vec![ModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "expression:happy".to_owned(),
                    shortcut: "Alt+M".to_owned(),
                }],
            }
            .compile()
            .expect("compiled shortcuts"),
        )
    }

    #[test]
    fn matches_hid_edges_once_and_aggregates_left_and_right_modifiers() {
        let mut matcher = matcher();
        let left_control = PhysicalKey::from_hid_usage(0xe0);
        let right_control = PhysicalKey::from_hid_usage(0xe4);
        let left_shift = PhysicalKey::from_hid_usage(0xe1);
        let b = PhysicalKey::from_hid_usage(0x05);

        assert_eq!(matcher.apply(left_control, InputEdge::Down), None);
        assert_eq!(matcher.apply(right_control, InputEdge::Down), None);
        assert_eq!(matcher.apply(left_control, InputEdge::Up), None);
        assert_eq!(matcher.apply(left_shift, InputEdge::Down), None);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
        assert_eq!(matcher.apply(b, InputEdge::Down), None);
        assert_eq!(matcher.apply(b, InputEdge::Up), None);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
    }

    #[test]
    fn extra_modifiers_do_not_match_and_reset_clears_repeat_state() {
        let mut matcher = matcher();
        let control = PhysicalKey::from_hid_usage(0xe0);
        let shift = PhysicalKey::from_hid_usage(0xe1);
        let alt = PhysicalKey::from_hid_usage(0xe2);
        let b = PhysicalKey::from_hid_usage(0x05);
        matcher.apply(control, InputEdge::Down);
        matcher.apply(shift, InputEdge::Down);
        matcher.apply(alt, InputEdge::Down);
        assert_eq!(matcher.apply(b, InputEdge::Down), None);
        assert_eq!(matcher.pressed_count(), 4);

        matcher.reset();
        matcher.apply(control, InputEdge::Down);
        matcher.apply(shift, InputEdge::Down);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
    }

    #[test]
    fn reconcile_and_replace_preserve_repeat_suppression_until_release() {
        let mut matcher = matcher();
        let alt = PhysicalKey::from_hid_usage(0xe2);
        let m = PhysicalKey::from_hid_usage(0x10);
        matcher.reconcile([alt, m]);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
        matcher.apply(m, InputEdge::Up);
        assert_eq!(
            matcher.apply(m, InputEdge::Down),
            Some(ShortcutTarget::ModelBehavior {
                model_id: "standard".to_owned(),
                action: ModelBehaviorAction::Expression {
                    name: "happy".to_owned(),
                },
            })
        );

        matcher.replace(CompiledShortcuts::default());
        assert_eq!(matcher.pressed_count(), 2);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
        matcher.apply(m, InputEdge::Up);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
    }
}
