//! A chord maps to a keycode, or it is refused.

use super::*;

#[test]
fn modifier_bits_and_letter_digit_keys_map_onto_global_hotkeys() {
    let control_shift_b = hotkey("Control+Shift+B");
    assert_eq!(control_shift_b.key, Code::KeyB);
    assert!(control_shift_b.mods.contains(Modifiers::CONTROL));
    assert!(control_shift_b.mods.contains(Modifiers::SHIFT));
    assert!(!control_shift_b.mods.contains(Modifiers::ALT));

    assert_eq!(hotkey("Alt+M").key, Code::KeyM);
    assert_eq!(hotkey("Shift+7").key, Code::Digit7);
    assert_eq!(hotkey("0").key, Code::Digit0);
    let meta_o = hotkey("Meta+O");
    // `HotKey::new` normalizes META onto SUPER; both map to the
    // platform Cmd/Win modifier inside global-hotkey.
    assert!(meta_o.mods.contains(Modifiers::SUPER));
    assert!(!meta_o.mods.contains(Modifiers::CONTROL));
}

#[test]
fn named_key_tokens_map_onto_the_closed_code_vocabulary() {
    assert_eq!(hotkey("Control+-").key, Code::Minus);
    assert_eq!(hotkey("Control+=").key, Code::Equal);
    assert_eq!(hotkey("Shift+Enter").key, Code::Enter);
    assert_eq!(hotkey("Escape").key, Code::Escape);
    assert_eq!(hotkey("F12").key, Code::F12);
    assert_eq!(hotkey("ArrowUp").key, Code::ArrowUp);
    assert_eq!(hotkey("BracketLeft").key, Code::BracketLeft);
    assert_eq!(hotkey("PrintScreen").key, Code::PrintScreen);
}

#[test]
fn equal_chords_produce_equal_hotkey_ids_for_event_routing() {
    assert_eq!(hotkey("Control+Shift+B"), hotkey("shift+control+KeyB"));
    assert_ne!(hotkey("Control+B"), hotkey("Control+Shift+B"));
}

#[test]
fn desired_registrations_preserve_targets_and_unique_ids() {
    let compiled = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "Control+Shift+B".to_owned(),
        }],
        model_behavior_bindings: vec![ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "expression:happy".to_owned(),
            shortcut: "Alt+M".to_owned(),
        }],
    }
    .compile()
    .expect("compiled shortcuts");
    let (registrations, unsupported) = desired_registrations(&compiled);
    assert!(unsupported.is_empty());
    assert_eq!(registrations.len(), 2);
    let ids: BTreeSet<u32> = registrations.iter().map(|r| r.hotkey.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(registrations.iter().any(|registration| matches!(
        registration.target,
        ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)
    )));
    assert!(
        registrations.iter().any(|registration| matches!(
            registration.target,
            ShortcutTarget::ModelBehavior { .. }
        ))
    );
}
