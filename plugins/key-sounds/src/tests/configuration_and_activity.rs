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

/// Cumulative totals already include input from before a plugin was started:
/// the first snapshot is a baseline, then down and up are independent edges.
#[test]
fn initial_activity_is_baselined_and_edges_play_separately() {
    let mut plugin = KeySounds::default();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 0,
            activity: InputActivity {
                keyboard_down: 40,
                keyboard_up: 39,
                ..InputActivity::default()
            },
        })
        .unwrap();
    assert!(plugin.playback.last_played.is_empty());

    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 1,
            activity: InputActivity {
                keyboard_down: 41,
                keyboard_up: 39,
                ..InputActivity::default()
            },
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![Action::KeyboardDown]
    );

    plugin.playback.last_played.clear();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 2,
            activity: InputActivity {
                keyboard_down: 41,
                keyboard_up: 40,
                ..InputActivity::default()
            },
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![Action::KeyboardUp]
    );
}

/// Every edge the host reports is one sound, and an edge nobody performed is
/// no sound at all — the counters are compared, not replayed.
#[test]
fn each_device_edge_plays_once_and_only_when_its_counter_moves() {
    let mut plugin = KeySounds::default();
    plugin
        .configure(&Configuration::from([(
            "mouse_enabled".into(),
            ConfigValue::Boolean(true),
        )]))
        .unwrap();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 0,
            activity: InputActivity::default(),
        })
        .unwrap();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 1,
            activity: InputActivity {
                keyboard_down: 1,
                keyboard_up: 1,
                mouse_down: 1,
                mouse_up: 1,
                ..InputActivity::default()
            },
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        Action::ALL
    );

    // A repeat of the same counters is not activity, and one counter moving
    // plays that action alone.
    plugin.playback.last_played.clear();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 100,
            activity: InputActivity {
                keyboard_down: 1,
                keyboard_up: 1,
                mouse_down: 1,
                mouse_up: 1,
                ..InputActivity::default()
            },
        })
        .unwrap();
    assert!(plugin.playback.last_played.is_empty());
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 200,
            activity: InputActivity {
                keyboard_up: 2,
                ..plugin.activity.unwrap_or_default()
            },
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![Action::KeyboardUp]
    );
}

/// Turning mouse sounds off silences both mouse edges
/// without touching the keyboard.
#[test]
fn the_mouse_switch_gates_both_mouse_edges() {
    let activity = InputActivity {
        keyboard_down: 1,
        keyboard_up: 1,
        mouse_down: 1,
        mouse_up: 1,
        ..InputActivity::default()
    };
    let mut plugin = KeySounds::default();
    plugin
        .configure(&Configuration::from([(
            "mouse_enabled".into(),
            ConfigValue::Boolean(false),
        )]))
        .unwrap();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 0,
            activity: InputActivity::default(),
        })
        .unwrap();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 1,
            activity,
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![Action::KeyboardDown, Action::KeyboardUp]
    );

    plugin.playback.last_played.clear();
    plugin
        .configure(&Configuration::from([(
            "mouse_enabled".into(),
            ConfigValue::Boolean(true),
        )]))
        .unwrap();
    plugin
        .handle(HostMessage::InputActivity {
            elapsed_ms: 100,
            activity: InputActivity {
                mouse_down: 2,
                mouse_up: 2,
                ..activity
            },
        })
        .unwrap();
    assert_eq!(
        plugin
            .playback
            .last_played
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![Action::MouseDown, Action::MouseUp]
    );
}

/// Pausing, resuming and a zero volume stop playback; the fixed Thock-derived
/// gap is per action, so a release is never swallowed by the press before it.
#[test]
fn cooldown_pause_and_mute_are_plugin_policy() {
    let mut plugin = KeySounds::default();
    assert!(plugin.admit(Action::KeyboardDown, 0));
    assert!(!plugin.admit(Action::KeyboardDown, MINIMUM_SOUND_GAP_MS - 1));
    assert!(plugin.admit(Action::KeyboardDown, MINIMUM_SOUND_GAP_MS));
    assert!(
        plugin.admit(Action::KeyboardUp, MINIMUM_SOUND_GAP_MS),
        "the release keeps its own clock"
    );

    plugin.handle(HostMessage::Pause).unwrap();
    assert!(!plugin.admit(Action::KeyboardDown, 10_000));
    plugin.handle(HostMessage::Resume).unwrap();
    assert!(plugin.admit(Action::KeyboardDown, 10_001));

    plugin
        .configure(&Configuration::from([(
            "volume".into(),
            ConfigValue::Integer(0),
        )]))
        .unwrap();
    assert!(!plugin.admit(Action::KeyboardDown, 20_000));
}
