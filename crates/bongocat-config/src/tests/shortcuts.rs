//! Chords, bindings, the legacy default tiers and the compiled table.

use super::*;

#[test]
fn shortcut_chords_normalize_aliases_and_modifier_order() {
    let chord = ShortcutChord::parse(" shift + ctrl + b ").expect("valid chord");
    assert_eq!(
        chord.modifiers().bits(),
        ShortcutModifiers::CONTROL | ShortcutModifiers::SHIFT
    );
    assert_eq!(chord.key(), "B");
    assert_eq!(chord.key_hid_usage(), 0x05);
    assert_eq!(chord.canonical(), "Control+Shift+B");

    let meta = ShortcutChord::parse("CMD+option+P").expect("valid mac chord");
    assert_eq!(meta.canonical(), "Alt+Meta+P");
    assert_eq!(
        ShortcutChord::parse("Control+KeyB")
            .expect("DOM key alias")
            .canonical(),
        "Control+B"
    );
    assert_eq!(
        ShortcutChord::parse("Shift+Digit1")
            .expect("DOM digit alias")
            .canonical(),
        "Shift+1"
    );
}

#[test]
fn shortcut_chords_reject_ambiguous_parts() {
    for (value, expected) in [
        ("Control+", ShortcutParseError::EmptyPart),
        ("Control+Control+A", ShortcutParseError::DuplicateModifier),
        ("A+B", ShortcutParseError::MultipleKeys),
        ("Control", ShortcutParseError::MissingKey),
        ("Control+bad key", ShortcutParseError::InvalidKey),
        ("Control+Mouse1", ShortcutParseError::InvalidKey),
    ] {
        assert_eq!(ShortcutChord::parse(value), Err(expected), "{value}");
    }
}

#[test]
fn shortcut_key_tokens_map_to_usb_hid_usages() {
    for (token, canonical, usage) in [
        ("a", "A", 0x04),
        ("Z", "Z", 0x1d),
        ("1", "1", 0x1e),
        ("0", "0", 0x27),
        ("Escape", "Escape", 0x29),
        ("F12", "F12", 0x45),
        ("ArrowLeft", "ArrowLeft", 0x50),
        ("Delete", "Delete", 0x4c),
        ("Minus", "-", 0x2d),
        ("Equal", "=", 0x2e),
    ] {
        let key = ShortcutKey::parse(token).expect("supported shortcut key");
        assert_eq!(key.canonical(), canonical, "{token}");
        assert_eq!(key.hid_usage(), usage, "{token}");
    }
}

#[test]
fn shortcut_config_canonicalized_stabilizes_commands_behaviors_and_chords() {
    let config = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: " toggle_overlay ".to_owned(),
            shortcut: " shift + ctrl + b ".to_owned(),
        }],
        model_behavior_bindings: vec![ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: " expression: happy ".to_owned(),
            shortcut: "cmd+option+p".to_owned(),
        }],
    };
    assert_eq!(
        config.canonicalized().expect("canonical shortcuts"),
        ShortcutConfig {
            commands_enabled: true,
            model_behaviors_enabled: true,
            command_bindings: vec![ShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+Shift+B".to_owned(),
            }],
            model_behavior_bindings: vec![ModelBehaviorBinding {
                model: ModelIdentity {
                    id: "standard".to_owned(),
                    source: ModelSource::BuiltIn
                },
                behavior_id: "expression:happy".to_owned(),
                shortcut: "Alt+Meta+P".to_owned(),
            }],
        }
    );
}

#[test]
fn config_rejects_shortcut_parse_errors_and_cross_domain_conflicts() {
    let mut config = NativeConfig::default();
    config.shortcuts.command_bindings.push(ShortcutBinding {
        command: "open_settings".to_owned(),
        shortcut: "Control+Alt+B".to_owned(),
    });
    config
        .shortcuts
        .model_behavior_bindings
        .push(ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "motion:tap:0".to_owned(),
            shortcut: "alt + ctrl + b".to_owned(),
        });
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.conflict"))
    ));

    config.shortcuts.model_behavior_bindings[0].shortcut = "Control+".to_owned();
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.binding"))
    ));
}

#[test]
fn default_behavior_shortcuts_follow_the_legacy_tiering() {
    let control = ShortcutModifiers::CONTROL;
    assert_eq!(BEHAVIOR_SHORTCUT_CAPACITY, 144);
    for (position, canonical) in [
        (0_usize, "Control+1"),
        (9, "Control+0"),
        (10, "Control+Shift+1"),
        (19, "Control+Shift+0"),
        (20, "Control+Alt+1"),
        (29, "Control+Alt+0"),
        (30, "Control+Alt+Shift+1"),
        (39, "Control+Alt+Shift+0"),
        (40, "Control+Q"),
        (65, "Control+M"),
        (66, "Control+Shift+Q"),
        (92, "Control+Alt+Q"),
        (118, "Control+Alt+Shift+Q"),
        (143, "Control+Alt+Shift+M"),
    ] {
        assert_eq!(
            default_behavior_shortcut(position, control)
                .expect("legacy slot")
                .canonical(),
            canonical,
            "position {position}"
        );
    }
    assert!(default_behavior_shortcut(BEHAVIOR_SHORTCUT_CAPACITY, control).is_none());

    // macOS uses Command as the primary modifier instead of Control.
    let meta = ShortcutModifiers::META;
    for (position, canonical) in [
        (0_usize, "Meta+1"),
        (10, "Shift+Meta+1"),
        (20, "Alt+Meta+1"),
        (30, "Alt+Shift+Meta+1"),
        (40, "Meta+Q"),
    ] {
        assert_eq!(
            default_behavior_shortcut(position, meta)
                .expect("legacy slot")
                .canonical(),
            canonical,
            "position {position}"
        );
    }
}

#[test]
fn default_behavior_assignment_skips_taken_chords_and_keeps_existing_bindings() {
    let control = ShortcutModifiers::CONTROL;
    let existing = ModelBehaviorBinding {
        model: ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "motion:CAT_motion:1".to_owned(),
        shortcut: "Control+3".to_owned(),
    };
    let mut shortcuts = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "ctrl+1".to_owned(),
        }],
        model_behavior_bindings: vec![existing.clone()],
    };
    let behaviors = [
        "motion:CAT_motion:0".to_owned(),
        "motion:CAT_motion:1".to_owned(),
        "motion:CAT_motion_lock:0".to_owned(),
    ];
    let added = assign_default_behavior_shortcuts(
        &mut shortcuts,
        &test_model_identity("standard"),
        &behaviors,
        control,
    );
    assert_eq!(added, 2);
    assert_eq!(
        shortcuts.model_behavior_bindings,
        vec![
            existing,
            ModelBehaviorBinding {
                model: ModelIdentity {
                    id: "standard".to_owned(),
                    source: ModelSource::BuiltIn
                },
                behavior_id: "motion:CAT_motion:0".to_owned(),
                shortcut: "Control+2".to_owned(),
            },
            ModelBehaviorBinding {
                model: ModelIdentity {
                    id: "standard".to_owned(),
                    source: ModelSource::BuiltIn
                },
                behavior_id: "motion:CAT_motion_lock:0".to_owned(),
                shortcut: "Control+4".to_owned(),
            },
        ]
    );
    shortcuts
        .compile()
        .expect("auto-assigned chords do not conflict");

    // Running it again fills nothing in and rewrites nothing.
    let unchanged = shortcuts.clone();
    assert_eq!(
        assign_default_behavior_shortcuts(
            &mut shortcuts,
            &test_model_identity("standard"),
            &behaviors,
            control
        ),
        0
    );
    assert_eq!(shortcuts, unchanged);

    // A different model counts from the first slot of its own scope. The
    // first model's chords do not push it forward — only one model's
    // behaviors are live at a time — but the application command binding
    // still counts, because commands are live whatever the active model is.
    let other = ["expression:happy".to_owned()];
    assert_eq!(
        assign_default_behavior_shortcuts(
            &mut shortcuts,
            &test_model_identity("keyboard"),
            &other,
            control
        ),
        1
    );
    assert_eq!(
        shortcuts
            .model_behavior_bindings
            .last()
            .expect("assigned binding")
            .shortcut,
        "Control+2"
    );

    // Both models now hold `Control+2`, so the whole configuration is no
    // longer an unambiguous table: only the projection onto one live model
    // is compilable.
    assert!(matches!(
        shortcuts.compile(),
        Err(ConfigError::InvalidValue("shortcuts.conflict"))
    ));
    for model_id in ["standard", "keyboard"] {
        let identity = test_model_identity(model_id);
        shortcuts
            .active_bindings(Some(&identity))
            .compile()
            .unwrap_or_else(|error| panic!("{model_id} compiles on its own: {error:?}"));
    }
    shortcuts
        .active_bindings(None)
        .compile()
        .expect("commands compile on their own");
}

#[test]
fn default_behavior_assignment_numbers_each_model_from_the_first_slot() {
    let control = ShortcutModifiers::CONTROL;
    let mut shortcuts = ShortcutConfig::default();
    let first = (0..7)
        .map(|index| format!("motion:first:{index}"))
        .collect::<Vec<_>>();
    let second = (0..7)
        .map(|index| format!("motion:second:{index}"))
        .collect::<Vec<_>>();

    assert_eq!(
        assign_default_behavior_shortcuts(
            &mut shortcuts,
            &test_model_identity("standard"),
            &first,
            control
        ),
        7
    );
    assert_eq!(
        assign_default_behavior_shortcuts(
            &mut shortcuts,
            &test_model_identity("keyboard"),
            &second,
            control
        ),
        7
    );

    for model_id in ["standard", "keyboard"] {
        let chords = shortcuts
            .model_behavior_bindings
            .iter()
            .filter(|binding| binding.model.id == *model_id)
            .map(|binding| binding.shortcut.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            chords,
            (1..=7)
                .map(|slot| format!("Control+{slot}"))
                .collect::<Vec<_>>(),
            "{model_id} numbers its own defaults from the first digit"
        );
    }
}

#[test]
fn config_allows_cross_model_chords_and_rejects_scope_conflicts() {
    let binding = |model_id: &str, behavior_id: &str, shortcut: &str| ModelBehaviorBinding {
        model: ModelIdentity {
            id: model_id.to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: behavior_id.to_owned(),
        shortcut: shortcut.to_owned(),
    };

    // The same chord in two models is what per-model numbering produces.
    let mut config = NativeConfig::default();
    config.shortcuts.model_behavior_bindings = vec![
        binding("standard", "motion:a:0", "Control+1"),
        binding("keyboard", "motion:b:0", "Control+1"),
    ];
    config
        .validate()
        .expect("two models may share a chord, only one is live");

    // The same id from different sources is still two distinct models.
    config.shortcuts.model_behavior_bindings = vec![
        ModelBehaviorBinding {
            model: ModelIdentity {
                id: "duplicate".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "motion:a:0".to_owned(),
            shortcut: "Control+1".to_owned(),
        },
        ModelBehaviorBinding {
            model: ModelIdentity {
                id: "duplicate".to_owned(),
                source: ModelSource::Imported,
            },
            behavior_id: "motion:a:0".to_owned(),
            shortcut: "Control+1".to_owned(),
        },
    ];
    config
        .validate()
        .expect("the same id in two sources may share a chord");
    config.shortcuts.model_behaviors_enabled = true;
    assert_eq!(
        config
            .shortcuts
            .active_bindings(Some(&ModelIdentity {
                id: "duplicate".to_owned(),
                source: ModelSource::Imported,
            }))
            .model_behavior_bindings
            .len(),
        1
    );

    // Twice inside one model is still a conflict.
    config.shortcuts.model_behavior_bindings = vec![
        binding("standard", "motion:a:0", "Control+1"),
        binding("standard", "motion:a:1", "Control+1"),
    ];
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.conflict"))
    ));

    // A command is live whatever the active model is, so a model behavior
    // may not shadow one.
    config.shortcuts.command_bindings = vec![ShortcutBinding {
        command: "toggle_overlay".to_owned(),
        shortcut: "Control+1".to_owned(),
    }];
    config.shortcuts.model_behavior_bindings = vec![binding("standard", "motion:a:0", "Control+1")];
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.conflict"))
    ));
}

#[test]
fn default_behavior_assignment_stops_at_the_legacy_capacity() {
    let control = ShortcutModifiers::CONTROL;
    let mut shortcuts = ShortcutConfig::default();
    let behaviors = (0..BEHAVIOR_SHORTCUT_CAPACITY + 5)
        .map(|index| format!("motion:group:{index}"))
        .collect::<Vec<_>>();
    assert_eq!(
        assign_default_behavior_shortcuts(
            &mut shortcuts,
            &test_model_identity("standard"),
            &behaviors,
            control
        ),
        BEHAVIOR_SHORTCUT_CAPACITY
    );
    assert_eq!(
        shortcuts.model_behavior_bindings.len(),
        BEHAVIOR_SHORTCUT_CAPACITY
    );
    shortcuts.compile().expect("no conflicts at capacity");
}

#[test]
fn shortcut_commands_and_model_behaviors_parse_as_closed_actions() {
    assert_eq!(
        ShortcutCommand::parse("toggle_overlay").expect("command"),
        ShortcutCommand::ToggleOverlay
    );
    assert_eq!(
        ShortcutCommand::parse("open_settings").expect("command"),
        ShortcutCommand::OpenSettings
    );
    for (value, expected) in [
        (
            "toggle_ignore_mouse_input",
            ShortcutCommand::ToggleIgnoreMouseInput,
        ),
        (
            "toggle_ignore_keyboard_input",
            ShortcutCommand::ToggleIgnoreKeyboardInput,
        ),
        (
            "toggle_ignore_gamepad_input",
            ShortcutCommand::ToggleIgnoreGamepadInput,
        ),
    ] {
        assert_eq!(ShortcutCommand::parse(value).expect("command"), expected);
        assert_eq!(expected.as_str(), value);
    }
    assert_eq!(
        ShortcutCommand::ToggleAlwaysOnTop.as_str(),
        "toggle_always_on_top"
    );
    assert_eq!(
        ShortcutCommand::parse("unknown"),
        Err(ShortcutCommandParseError::Unknown)
    );

    let motion = ModelBehaviorBinding {
        model: ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "motion:CAT_motion:12".to_owned(),
        shortcut: "Control+1".to_owned(),
    };
    assert_eq!(
        motion.parse_action().expect("motion action"),
        ModelBehaviorAction::Motion {
            group: "CAT_motion".to_owned(),
            index: 12,
        }
    );
    let expression = ModelBehaviorBinding {
        model: ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "expression:happy:variant".to_owned(),
        shortcut: "Control+2".to_owned(),
    };
    assert_eq!(
        expression.parse_action().expect("expression action"),
        ModelBehaviorAction::Expression {
            name: "happy:variant".to_owned(),
        }
    );
    for behavior_id in ["motion:group", "motion:group:nope", "unknown:name"] {
        let binding = ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: behavior_id.to_owned(),
            shortcut: "Control+3".to_owned(),
        };
        assert!(binding.parse_action().is_err(), "{behavior_id}");
    }
}

/// The command gate is a projection, not a rewrite.
///
/// Switched off it only keeps the commands out of the live table; the
/// recorded bindings stay in the configuration, so switching it back on
/// restores them without re-recording. The behaviour half of the same table
/// is untouched, because the two gates are independent.
#[test]
fn the_command_gate_empties_the_live_table_without_touching_the_bindings() {
    let mut config = ShortcutConfig::default();
    assert!(
        config.commands_enabled,
        "a fresh configuration keeps its command shortcuts live"
    );
    config.command_bindings.push(ShortcutBinding {
        command: "toggle_overlay".to_owned(),
        shortcut: "Control+Shift+B".to_owned(),
    });
    config.model_behavior_bindings.push(ModelBehaviorBinding {
        model: ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "expression:happy".to_owned(),
        shortcut: "Alt+M".to_owned(),
    });
    config.model_behaviors_enabled = true;
    assert_eq!(
        config
            .active_bindings(Some(&test_model_identity("standard")))
            .command_bindings
            .len(),
        1
    );

    config.commands_enabled = false;
    let gated = config.active_bindings(Some(&test_model_identity("standard")));
    assert!(gated.command_bindings.is_empty());
    assert_eq!(
        gated.model_behavior_bindings,
        config.model_behavior_bindings
    );
    assert_eq!(config.command_bindings.len(), 1);
    assert!(gated.compile().is_ok());

    config.commands_enabled = true;
    assert_eq!(
        config
            .active_bindings(Some(&test_model_identity("standard")))
            .command_bindings
            .len(),
        1
    );
}

#[test]
fn compiled_shortcuts_match_mapped_tokens_and_preserve_typed_targets() {
    let config = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "ctrl+shift+b".to_owned(),
        }],
        model_behavior_bindings: vec![ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "motion:CAT_motion:2".to_owned(),
            shortcut: "Alt+M".to_owned(),
        }],
    };
    let compiled = config.compile().expect("compile shortcuts");
    let modifiers =
        ShortcutModifiers::from_bits(ShortcutModifiers::CONTROL | ShortcutModifiers::SHIFT)
            .expect("valid modifiers");
    assert!(compiled.resolve(modifiers, "B").is_some());
    assert!(compiled.resolve(modifiers, " b ").is_some());
    assert!(compiled.resolve(modifiers, "KeyB").is_some());
    assert!(compiled.resolve_hid_usage(modifiers, 0x05).is_some());
    assert!(compiled.resolve(modifiers, "N").is_none());
    assert_eq!(compiled.iter().count(), 2);
    assert_eq!(
        compiled
            .resolve(modifiers, "B")
            .expect("command binding")
            .target(),
        &ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)
    );

    let alt = ShortcutModifiers::from_bits(ShortcutModifiers::ALT).expect("valid modifiers");
    assert_eq!(
        compiled
            .resolve(alt, "m")
            .expect("behavior binding")
            .target(),
        &ShortcutTarget::ModelBehavior {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            action: ModelBehaviorAction::Motion {
                group: "CAT_motion".to_owned(),
                index: 2,
            },
        }
    );
}

#[test]
fn compiled_shortcuts_reject_invalid_and_conflicting_bindings() {
    let invalid = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "unknown".to_owned(),
            shortcut: "Control+A".to_owned(),
        }],
        ..ShortcutConfig::default()
    };
    assert!(matches!(
        invalid.compile(),
        Err(ConfigError::InvalidValue("shortcuts.command"))
    ));

    let conflict = ShortcutConfig {
        commands_enabled: true,
        model_behaviors_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "open_settings".to_owned(),
            shortcut: "Control+A".to_owned(),
        }],
        model_behavior_bindings: vec![ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "expression:happy".to_owned(),
            shortcut: "ctrl+a".to_owned(),
        }],
    };
    assert!(matches!(
        conflict.compile(),
        Err(ConfigError::InvalidValue("shortcuts.conflict"))
    ));
}

#[test]
fn shortcut_modifier_bits_reject_unknown_flags() {
    assert!(ShortcutModifiers::from_bits(1 << 7).is_none());
}

#[test]
fn config_rejects_unknown_shortcut_actions_before_commit() {
    let mut config = NativeConfig::default();
    config.shortcuts.command_bindings.push(ShortcutBinding {
        command: "future_command".to_owned(),
        shortcut: "Control+1".to_owned(),
    });
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.command"))
    ));

    config.shortcuts.command_bindings.clear();
    config
        .shortcuts
        .model_behavior_bindings
        .push(ModelBehaviorBinding {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "motion:CAT_motion".to_owned(),
            shortcut: "Control+1".to_owned(),
        });
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InvalidValue("shortcuts.behavior"))
    ));
}
