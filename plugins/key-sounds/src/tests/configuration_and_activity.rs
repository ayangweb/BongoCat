use super::*;

/// The configuration vocabulary is closed, and a value outside a control's
/// range is refused rather than clamped: the host has to be told the edit did
/// not take, not shown a number it never stored.
#[test]
fn the_configuration_vocabulary_is_closed_and_bounded() {
    let mut plugin = KeySounds::default();
    let output = plugin
        .handle(HostMessage::Start {
            config: Configuration::new(),
            state: Vec::new(),
        })
        .unwrap();
    assert!(output.validate());
    let volume = output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Integer {
            key,
            value: value @ 100,
            ..
        } if key == "volume" => Some(*value),
        _ => None,
    });
    assert_eq!(volume, Some(100));
    assert!(plugin.mouse_enabled);

    for invalid in [
        ("volume".to_owned(), ConfigValue::Integer(101)),
        ("mouse_enabled".to_owned(), ConfigValue::Text("yes".into())),
        ("secret".to_owned(), ConfigValue::Boolean(true)),
        // A key this plugin does not declare is a typo, not a preference.
        ("cooldown_ms".to_owned(), ConfigValue::Integer(0)),
        (
            "keyboard_down_pack".to_owned(),
            ConfigValue::Text("alpaca".into()),
        ),
    ] {
        assert!(
            plugin
                .configure(&Configuration::from([invalid.clone()]))
                .is_err(),
            "{invalid:?} was accepted"
        );
    }
    assert_eq!(plugin.volume, 100);

    plugin
        .configure(&Configuration::from([
            ("volume".into(), ConfigValue::Integer(80)),
            ("mouse_enabled".into(), ConfigValue::Boolean(true)),
        ]))
        .unwrap();
    assert_eq!(plugin.volume, 80);
    assert!(plugin.mouse_enabled);

    plugin
        .configure(&Configuration::from([(
            "keyboard_pack".into(),
            ConfigValue::Text("skcm-blue".into()),
        )]))
        .unwrap();
    assert_eq!(plugin.selection.keyboard.id, "skcm-blue");
    assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, "skcm-blue");
}

#[test]
fn mouse_sounds_default_to_on_and_preserve_a_saved_opt_out() {
    let mut plugin = KeySounds::default();
    assert!(plugin.mouse_enabled);

    for enabled in [false, true] {
        let output = plugin
            .handle(HostMessage::Start {
                config: Configuration::from([(
                    "mouse_enabled".into(),
                    ConfigValue::Boolean(enabled),
                )]),
                state: Vec::new(),
            })
            .unwrap();
        assert_eq!(plugin.mouse_enabled, enabled);
        assert_eq!(
            output.config["mouse_enabled"],
            ConfigValue::Boolean(enabled)
        );
        assert!(output.settings.nodes.iter().any(|node| matches!(
            node,
            UiNode::Toggle { key, value, .. } if key == "mouse_enabled" && *value == enabled
        )));
    }

    plugin
        .configure(&Configuration::from([(
            "mouse_enabled".into(),
            ConfigValue::Boolean(false),
        )]))
        .unwrap();
    let output = plugin.handle(HostMessage::Reset).unwrap();
    assert_eq!(output.config["mouse_enabled"], ConfigValue::Boolean(true));

    plugin
        .handle(HostMessage::Start {
            config: Configuration::from([("volume".into(), ConfigValue::Integer(80))]),
            state: Vec::new(),
        })
        .unwrap();
    assert!(plugin.mouse_enabled);
}

/// A stored set id this build no longer carries resolves to the default
/// rather than failing the exchange.
#[test]
fn a_stored_set_id_this_build_lost_repairs_itself_to_the_default() {
    let mut plugin = KeySounds::default();
    plugin
        .configure(&Configuration::from([(
            "keyboard_pack".into(),
            ConfigValue::Text("gone".into()),
        )]))
        .unwrap();

    let keyboard = PackCategory::Keyboard.default_pack();
    assert_eq!(plugin.selection.keyboard.id, keyboard.id);
    // The published configuration is the resolved value, so the panel and the
    // stored session never drift from what is actually playing.
    let published = plugin.output().config;
    assert_eq!(
        published.get("keyboard_pack"),
        Some(&ConfigValue::Text(keyboard.id.into()))
    );
}

/// Both edges of a device use that device's selected set.
#[test]
fn one_device_set_serves_both_of_its_edges() {
    let pinned = PackCategory::Keyboard
        .packs()
        .nth(1)
        .expect("the keyboard ships more than one set");
    let mut plugin = KeySounds::default();
    plugin
        .configure(&Configuration::from([(
            "keyboard_pack".into(),
            ConfigValue::Text(pinned.id.into()),
        )]))
        .unwrap();

    assert_eq!(plugin.selection.pack(Action::KeyboardDown).id, pinned.id);
    assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, pinned.id);
}

/// A set with no release recording is silent on release, and both device
/// selectors mark it so the choice is not mistaken for complete coverage.
#[test]
fn a_set_without_a_release_sound_is_silent_on_release() {
    let silent = PackCategory::Keyboard
        .packs()
        .find(|pack| !pack.supports_key_up)
        .expect("the registry carries sets with no release recording");
    assert!(silent.up.is_empty());

    let copy = &ENGLISH;
    let mut plugin = KeySounds::default();
    let output = plugin.output();
    let chosen = select_option(&output, "keyboard_pack", silent.id);
    assert!(chosen.label.ends_with(copy.no_release));

    plugin
        .configure(&Configuration::from([(
            "keyboard_pack".into(),
            ConfigValue::Text(silent.id.into()),
        )]))
        .unwrap();
    assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, silent.id);
    let decoded = decode_pack(silent).unwrap();
    assert!(decoded.sounds(Action::KeyboardUp).is_empty());
    assert!(!decoded.sounds(Action::KeyboardDown).is_empty());
}

fn edge(control: InputEventControl, pressed: bool, captured_ms: u64) -> PluginInputEvent {
    PluginInputEvent {
        control,
        pressed,
        captured_ms,
    }
}

#[test]
fn thock_default_keeps_every_rapid_edge_and_its_group_in_order() {
    let mut plugin = KeySounds {
        silent_output: true,
        ..Default::default()
    };
    plugin.configure(&Configuration::new()).unwrap();
    plugin.play_events(
        vec![
            edge(InputEventControl::Space, true, 0),
            edge(InputEventControl::Enter, true, 1),
            edge(InputEventControl::Space, false, 2),
            edge(InputEventControl::Keyboard, true, 2),
            edge(InputEventControl::Keyboard, true, 2),
            edge(InputEventControl::MouseLeft, true, 3),
            edge(InputEventControl::MouseRight, false, 4),
        ],
        0,
    );
    let actual: Vec<_> = plugin
        .resolved
        .iter()
        .map(|(action, group, _)| (*action, *group))
        .collect();
    assert_eq!(
        actual,
        vec![
            (Action::KeyboardDown, Some(KeyGroup::Space)),
            (Action::KeyboardDown, Some(KeyGroup::Enter)),
            (Action::KeyboardUp, Some(KeyGroup::Space)),
            (Action::KeyboardDown, Some(KeyGroup::Default)),
            (Action::KeyboardDown, Some(KeyGroup::Default)),
            (Action::MouseDown, Some(KeyGroup::MouseLeft)),
            (Action::MouseUp, Some(KeyGroup::MouseRight)),
        ]
    );
}

#[test]
fn configure_predecodes_selected_packs_and_mouse_groups_stay_distinct() {
    let mut plugin = KeySounds::default();
    plugin.configure(&Configuration::new()).unwrap();
    for id in plugin.selection.ids() {
        assert!(plugin.library.decoded.contains_key(id));
    }
    let decoded = decode_pack(pack_by_id("orochi-v2").unwrap()).unwrap();
    assert_eq!(
        decoded
            .keyboard_sounds(Action::MouseDown, KeyGroup::MouseLeft)
            .len(),
        1
    );
    assert_eq!(
        decoded
            .keyboard_sounds(Action::MouseDown, KeyGroup::MouseRight)
            .len(),
        1
    );
    assert_eq!(DEVICE_IDLE, Duration::from_secs(10));
}

#[test]
fn paused_muted_and_preview_events_are_consumed_without_replay() {
    let mut plugin = KeySounds {
        silent_output: true,
        ..Default::default()
    };
    plugin.handle(HostMessage::Pause).unwrap();
    plugin.play_events(vec![edge(InputEventControl::Space, true, 1)], 0);
    assert!(plugin.resolved.is_empty());
    plugin.handle(HostMessage::Resume).unwrap();
    super::views_and_preview::install_test_voice(&mut plugin, true);
    plugin.play_events(vec![edge(InputEventControl::Space, true, 2)], 0);
    assert!(plugin.resolved.is_empty());
    plugin.playback.clear_voices();
    plugin.play_events(vec![], 0);
    assert!(plugin.resolved.is_empty());
    plugin.play_events(vec![edge(InputEventControl::Space, false, 3)], 0);
    assert_eq!(plugin.resolved.len(), 1);
    plugin.volume = 0;
    plugin.play_events(vec![edge(InputEventControl::Space, true, 4)], 0);
    assert_eq!(plugin.resolved.len(), 1);
}

#[test]
fn overflow_is_observable_and_clears_old_voices() {
    let mut plugin = KeySounds {
        silent_output: true,
        ..Default::default()
    };
    super::views_and_preview::install_test_voice(&mut plugin, false);
    plugin.play_events(vec![edge(InputEventControl::Space, true, 1)], 1);
    assert_eq!(plugin.overflow, 1);
    assert!(plugin.playback.players.is_empty());
    assert_eq!(plugin.resolved.len(), 1);
}
