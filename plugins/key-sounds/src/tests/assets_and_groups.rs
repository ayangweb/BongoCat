use super::*;

#[test]
fn resetting_settings_keeps_the_application_language() {
    let mut plugin = KeySounds::default();
    plugin
        .handle(HostMessage::Locale {
            locale: "zh-CN".into(),
        })
        .unwrap();
    plugin
        .configure(&[("volume".into(), ConfigValue::Integer(80))].into())
        .unwrap();
    let output = plugin.handle(HostMessage::Reset).unwrap();
    assert!(output.validate());
    assert_eq!(plugin.volume, 100);
    assert!(output.settings.nodes.iter().any(|node| {
        matches!(node, UiNode::Integer { label, .. } if label == SIMPLIFIED_CHINESE.volume)
    }));
}

/// Every embedded set has to be playable, because nothing else can repair it
/// after the fact: a set that ships a file this build cannot decode would
/// fail at the user's first keystroke rather than here.
#[test]
fn every_embedded_set_decodes_and_declares_its_release_honestly() {
    let mut ids = BTreeSet::new();
    for pack in PACKS {
        assert!(ids.insert(pack.id), "{} is embedded twice", pack.id);
        assert!(!pack.down.is_empty(), "{} carries no press sound", pack.id);
        assert_eq!(
            pack.supports_key_up,
            !pack.up.is_empty(),
            "{} disagrees with itself about its release sound",
            pack.id
        );
        let decoded = decode_pack(pack).unwrap_or_else(|_| panic!("{} decodes", pack.id));
        assert_eq!(decoded.down.len(), pack.down.len());
        assert_eq!(decoded.up.len(), pack.up.len());
        assert_eq!(decoded.key_groups.len(), pack.key_groups.len());
        for sound in decoded
            .down
            .iter()
            .chain(&decoded.up)
            .chain(decoded.key_groups.values().flat_map(|group| &group.down))
            .chain(decoded.key_groups.values().flat_map(|group| &group.up))
        {
            assert!(
                sound.clone().count() > 0,
                "{} carries an empty sound",
                pack.id
            );
        }
    }
    for category in [PackCategory::Keyboard, PackCategory::Mouse] {
        assert!(
            category.packs().count() > 0,
            "{category:?} has no set to fall back to"
        );
    }
}

/// Thock resolves a named key group first and falls back to `default` only
/// when the pack has no such group. A group that exists but has no recording
/// for this direction is silent instead of falling back again.
#[test]
fn keyboard_edges_use_thocks_key_group_fallback() {
    let with_space = PACKS
        .iter()
        .find(|pack| pack.key_groups.iter().any(|group| group.name == "space"))
        .expect("the registry carries a space group");
    let decoded = decode_pack(with_space).unwrap();
    let space = decoded.key_groups.get("space").unwrap();
    assert_eq!(
        decoded
            .keyboard_sounds(Action::KeyboardDown, KeyGroup::Space)
            .len(),
        space.down.len()
    );
    assert_eq!(
        decoded
            .keyboard_sounds(Action::KeyboardUp, KeyGroup::Space)
            .len(),
        space.up.len()
    );
    assert_eq!(
        decoded
            .keyboard_sounds(Action::KeyboardDown, KeyGroup::Default)
            .len(),
        decoded.down.len()
    );

    let no_space_up = PACKS
        .iter()
        .find(|pack| {
            pack.category == PackCategory::Keyboard
                && pack
                    .key_groups
                    .iter()
                    .any(|group| group.name == "space" && group.up.is_empty())
        })
        .expect("the registry carries a space group without release recordings");
    let decoded = decode_pack(no_space_up).unwrap();
    assert!(
        decoded
            .keyboard_sounds(Action::KeyboardUp, KeyGroup::Space)
            .is_empty(),
        "an existing group with no release recording must stay silent"
    );

    let no_space = PACKS
        .iter()
        .find(|pack| {
            pack.category == PackCategory::Keyboard
                && pack.key_groups.iter().all(|group| group.name != "space")
        })
        .expect("the registry carries a keyboard set without a space group");
    let decoded = decode_pack(no_space).unwrap();
    assert_eq!(
        decoded
            .keyboard_sounds(Action::KeyboardDown, KeyGroup::Space)
            .len(),
        decoded.down.len()
    );
}

/// A special-key group is selected only when its counter accounts for the
/// whole keyboard batch; a mixed batch still plays the default sound once,
/// rather than guessing which of several keys it was.
#[test]
fn special_key_edges_select_their_groups_without_double_playing() {
    let before = InputActivity::default();
    for (group, activity) in [
        (
            KeyGroup::Space,
            InputActivity {
                keyboard_down: 1,
                space_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Enter,
            InputActivity {
                keyboard_down: 1,
                enter_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Del,
            InputActivity {
                keyboard_down: 1,
                backspace_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Tab,
            InputActivity {
                keyboard_down: 1,
                tab_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::ShiftLeft,
            InputActivity {
                keyboard_down: 1,
                shift_left_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::ShiftRight,
            InputActivity {
                keyboard_down: 1,
                shift_right_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::CtrlLeft,
            InputActivity {
                keyboard_down: 1,
                ctrl_left_down: 1,
                ..InputActivity::default()
            },
        ),
    ] {
        assert_eq!(KeyGroup::for_edge(activity, before, false), group);
    }
    assert_eq!(
        KeyGroup::for_edge(
            InputActivity {
                keyboard_down: 2,
                enter_down: 2,
                ..InputActivity::default()
            },
            before,
            false,
        ),
        KeyGroup::Enter,
        "main Enter and keypad Enter share the same Thock group"
    );
    for (group, activity) in [
        (
            KeyGroup::Space,
            InputActivity {
                keyboard_up: 1,
                space_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Enter,
            InputActivity {
                keyboard_up: 1,
                enter_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Del,
            InputActivity {
                keyboard_up: 1,
                backspace_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::Tab,
            InputActivity {
                keyboard_up: 1,
                tab_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::ShiftLeft,
            InputActivity {
                keyboard_up: 1,
                shift_left_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::ShiftRight,
            InputActivity {
                keyboard_up: 1,
                shift_right_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            KeyGroup::CtrlLeft,
            InputActivity {
                keyboard_up: 1,
                ctrl_left_up: 1,
                ..InputActivity::default()
            },
        ),
    ] {
        assert_eq!(KeyGroup::for_edge(activity, before, true), group);
    }
    assert_eq!(
        KeyGroup::for_edge(
            InputActivity {
                keyboard_up: 2,
                enter_up: 2,
                ..InputActivity::default()
            },
            before,
            true,
        ),
        KeyGroup::Enter,
        "main Enter and keypad Enter share the same Thock group"
    );

    assert_eq!(
        KeyGroup::for_edge(
            InputActivity {
                keyboard_down: 2,
                space_down: 1,
                enter_down: 1,
                ..InputActivity::default()
            },
            before,
            false,
        ),
        KeyGroup::Default,
        "two special keys in one batch are not guessed"
    );
    assert_eq!(
        KeyGroup::for_edge(
            InputActivity {
                keyboard_down: 2,
                space_down: 1,
                ..InputActivity::default()
            },
            before,
            false,
        ),
        KeyGroup::Default,
        "a special key mixed with an ordinary key is not guessed"
    );

    let mut plugin = KeySounds {
        activity: Some(InputActivity::default()),
        ..Default::default()
    };
    plugin.play_edges(
        1,
        InputActivity {
            keyboard_down: 1,
            space_down: 1,
            ..InputActivity::default()
        },
    );
    assert_eq!(plugin.activity.unwrap().keyboard_down, 1);
    assert_eq!(plugin.playback.last_played.len(), 1);

    plugin.playback.last_played.clear();
    plugin.play_edges(
        100,
        InputActivity {
            keyboard_down: 3,
            space_down: 2,
            ..InputActivity::default()
        },
    );
    assert_eq!(
        plugin.playback.last_played.len(),
        1,
        "a mixed batch is still one keyboard edge batch"
    );
}
