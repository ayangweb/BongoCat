//! Shortcut compilation, the command gate and the model behavior defaults.

use super::*;

/// The legacy implementation auto-assigned a chord to every motion and
/// expression the moment a model loaded, which is what makes its shortcuts
/// page show a default in every row instead of an empty field. The Native
/// rewrite does the same on activation, and the assignment rides on the
/// same commit that selects the model.
#[test]
fn activating_a_model_fills_in_the_legacy_default_behavior_shortcuts() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    assert!(
        application
            .config()
            .shortcuts
            .model_behavior_bindings
            .is_empty(),
        "a fresh configuration binds no model behaviour"
    );

    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare standard model");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let frame = wait_for_model_commit_frame(&consumer, token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: frame.model_commit.expect("commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit standard model");
    application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("standard model activation");

    // `standard` declares four motions in two groups and three
    // expressions. The legacy ordering walks motions before expressions,
    // so the seven behaviours land on the first seven digit slots.
    let bindings = application
        .config()
        .shortcuts
        .model_behavior_bindings
        .clone();
    assert_eq!(bindings.len(), 7);
    assert!(
        bindings
            .iter()
            .all(|binding| binding.model.id == "standard")
    );
    let primary = behavior_shortcut_primary_name();
    for (behavior_id, slot) in [
        ("motion:CAT_motion:0", 1),
        ("motion:CAT_motion:1", 2),
        ("motion:CAT_motion_lock:0", 3),
        ("motion:CAT_motion_lock:1", 4),
        ("expression:live2d_expression0.exp3.json", 5),
        ("expression:live2d_expression1.exp3.json", 6),
        ("expression:live2d_expression2.exp3.json", 7),
    ] {
        let binding = bindings
            .iter()
            .find(|binding| binding.behavior_id == behavior_id)
            .unwrap_or_else(|| panic!("{behavior_id} has no default binding"));
        assert_eq!(
            binding.shortcut,
            format!("{primary}+{slot}"),
            "{behavior_id}"
        );
    }

    // The chords are persisted, but the switch still gates whether the
    // platform adapters see them: a fresh v1 configuration leaves model
    // behaviour shortcuts off until the user opts in.
    assert!(!application.config().shortcuts.model_behaviors_enabled);
    let modifiers = behavior_shortcut_primary_modifiers();
    assert!(
        application
            .shortcut_table()
            .load()
            .resolve(modifiers, "1")
            .is_none()
    );
    application
        .set_behavior_shortcuts_enabled(true)
        .expect("enable behaviour shortcuts");
    assert!(
        application
            .shortcut_table()
            .load()
            .resolve(modifiers, "1")
            .is_some()
    );
    application.shutdown().expect("clean shutdown");
}

/// The command gate is a projection too: switching it off stops the
/// recorded command chords from reaching the platform table without
/// rewriting the configuration, so switching it back on restores them.
/// The model behaviour gate next to it stays untouched.
#[test]
fn the_command_gate_keeps_the_recorded_bindings_and_only_leaves_the_table() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    // Control+Alt+0 sits outside the primary tier, so the expectation below
    // reads the same on macOS and Windows.
    application
        .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
            commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+Alt+0".to_owned(),
            }],
            ..bongocat_ui_protocol::SettingsShortcuts::default()
        })
        .expect("persist user shortcuts");
    let modifiers = bongocat_config::ShortcutModifiers::from_bits(
        bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::ALT,
    )
    .expect("valid modifiers");
    let resolves = |application: &Application| {
        application
            .shortcut_table()
            .load()
            .resolve(modifiers, "0")
            .is_some()
    };
    assert!(resolves(&application), "a recorded command is registered");
    let behavior_gate = application.config().shortcuts.model_behaviors_enabled;

    application
        .set_command_shortcuts_enabled(false)
        .expect("disable command shortcuts");
    assert!(
        !resolves(&application),
        "the gate must empty the platform table"
    );
    assert_eq!(application.config().shortcuts.command_bindings.len(), 1);
    assert!(!application.config().shortcuts.commands_enabled);
    assert_eq!(
        application.config().shortcuts.model_behaviors_enabled,
        behavior_gate,
        "the model behaviour gate is a separate switch"
    );
    assert!(
        std::fs::read_to_string(&layout.config)
            .expect("persisted config")
            .contains("\"commands_enabled\": false")
    );

    application
        .set_command_shortcuts_enabled(true)
        .expect("enable command shortcuts");
    assert!(
        resolves(&application),
        "re-enabling must not need the chord recorded again"
    );
    application.shutdown().expect("clean shutdown");
}

/// A default is only a starting point: the assignment never rewrites a
/// binding the user recorded, and the behaviours they have not touched are
/// still filled in around it.
#[test]
fn selecting_a_model_keeps_the_behavior_bindings_the_user_recorded() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    // Both chords sit outside the primary tier, so the expectation below
    // reads the same on macOS and Windows.
    application
        .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
            commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+Alt+0".to_owned(),
            }],
            model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
                model: bongocat_ui_protocol::SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: bongocat_ui_protocol::SettingsModelOrigin::BuiltIn,
                },
                behavior_id: "motion:CAT_motion:0".to_owned(),
                shortcut: "Control+Alt+9".to_owned(),
            }],
        })
        .expect("persist user shortcuts");
    let revision_before = application.config_revision();

    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select standard model");

    let bindings = application
        .config()
        .shortcuts
        .model_behavior_bindings
        .clone();
    assert_eq!(bindings.len(), 7);
    let chord = |behavior_id: &str| {
        bindings
            .iter()
            .find(|binding| binding.behavior_id == behavior_id)
            .map(|binding| binding.shortcut.clone())
    };
    assert_eq!(
        chord("motion:CAT_motion:0"),
        Some("Control+Alt+9".to_owned()),
        "the recorded binding must survive the auto-assignment"
    );
    let primary = behavior_shortcut_primary_name();
    assert_eq!(
        chord("motion:CAT_motion:1"),
        Some(format!("{primary}+1")),
        "the first free slot is the primary tier's first digit"
    );
    assert_eq!(
        chord("motion:CAT_motion_lock:0"),
        Some(format!("{primary}+2"))
    );
    assert_eq!(
        chord("expression:live2d_expression2.exp3.json"),
        Some(format!("{primary}+6"))
    );
    assert_eq!(
        application.config().shortcuts.command_bindings.len(),
        1,
        "application commands are never rewritten"
    );
    // A config revision is a hash of the persisted document, not a counter,
    // so the only question it can answer is whether the document changed.
    assert!(
        application.config_revision() != revision_before,
        "the assignment commits with the selection"
    );
    application.shutdown().expect("clean shutdown");
}

/// "Clear all shortcuts" empties the configuration, but only the
/// application command half is durable: the model behaviour half comes back
/// on the next activation, because the auto-assignment fills every
/// behaviour that has no binding and runs on every activation.
///
/// This is the legacy behaviour — it re-assigned on every model load with
/// no way to opt out — and it is why the model behaviour switch, not
/// clearing, is how a user stops those chords from firing. The test exists
/// so a future change to either half is a deliberate decision rather than a
/// surprise.
#[test]
fn clearing_all_shortcuts_does_not_survive_the_next_activation() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select standard model");
    assert_eq!(
        application.config().shortcuts.model_behavior_bindings.len(),
        7
    );

    // What the page's "Clear all shortcuts" button sends.
    application
        .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts::default())
        .expect("clear all shortcuts");
    assert!(application.config().shortcuts.command_bindings.is_empty());
    assert!(
        application
            .config()
            .shortcuts
            .model_behavior_bindings
            .is_empty()
    );

    // Re-activating the model is what startup does on the next launch.
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("re-activate standard model");
    assert!(application.config().shortcuts.command_bindings.is_empty());
    assert_eq!(
        application.config().shortcuts.model_behavior_bindings.len(),
        7,
        "the model behaviour defaults are re-assigned by activation"
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_compiles_committed_shortcuts_for_platform_adapters() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    // A model behavior binding belongs to a model, and only the model that
    // is live reaches the platform table, so this test has to activate one
    // before its half of the table means anything.
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select standard model");
    // A fresh v1 configuration leaves model behaviour shortcuts off, so the
    // enabled half of this test has to opt in before recording the binding.
    application
        .set_behavior_shortcuts_enabled(true)
        .expect("enable behavior shortcuts");
    let shortcuts = bongocat_ui_protocol::SettingsShortcuts {
        commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "ctrl+shift+b".to_owned(),
        }],
        model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
            model: bongocat_ui_protocol::SettingsModelKey {
                id: "standard".to_owned(),
                origin: bongocat_ui_protocol::SettingsModelOrigin::BuiltIn,
            },
            behavior_id: "expression:happy".to_owned(),
            shortcut: "alt+m".to_owned(),
        }],
    };
    application
        .set_shortcuts(shortcuts)
        .expect("persist shortcuts");
    let compiled = application.shortcut_table().load();
    let modifiers = bongocat_config::ShortcutModifiers::from_bits(
        bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::SHIFT,
    )
    .expect("valid modifiers");
    assert!(compiled.resolve(modifiers, "B").is_some());
    assert!(compiled.resolve(modifiers, "C").is_none());
    let alt =
        bongocat_config::ShortcutModifiers::from_bits(bongocat_config::ShortcutModifiers::ALT)
            .expect("valid modifiers");
    assert!(compiled.resolve(alt, "M").is_some());

    application
        .set_behavior_shortcuts_enabled(false)
        .expect("disable behavior shortcuts");
    let disabled = application.shortcut_table().load();
    assert!(disabled.resolve(modifiers, "B").is_some());
    assert!(disabled.resolve(alt, "M").is_none());
    assert!(!application.config().shortcuts.model_behaviors_enabled);
    application.shutdown().expect("clean shutdown");

    let mut restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(
        restarted
            .shortcut_table()
            .load()
            .resolve(alt, "M")
            .is_none()
    );
    restarted
        .select_model(ModelOrigin::Preset, "standard")
        .expect("re-select standard model");
    restarted
        .set_behavior_shortcuts_enabled(true)
        .expect("re-enable behavior shortcuts");
    let reenabled = restarted.shortcut_table().load();
    assert!(reenabled.resolve(modifiers, "B").is_some());
    assert!(reenabled.resolve(alt, "M").is_some());
    restarted.shutdown().expect("clean restarted shutdown");
}

/// The behavior half of the platform table belongs to one model at a time:
/// the model being switched to answers its own shortcuts immediately, and
/// the model being left stops answering its chords. Every model also counts
/// its own defaults from the first digit, so the two models legitimately
/// hold the same chords — which is only sound while the other half is not
/// registered.
#[test]
fn switching_models_swaps_the_behavior_half_of_the_shortcut_table() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    application
        .set_behavior_shortcuts_enabled(true)
        .expect("enable behavior shortcuts");
    // Recorded before any model is active, so `standard` owns a chord no
    // other model will be handed: outside the primary tier, so the
    // expectation below reads the same on macOS and Windows.
    application
        .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
            commands: Vec::new(),
            model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
                model: bongocat_ui_protocol::SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: bongocat_ui_protocol::SettingsModelOrigin::BuiltIn,
                },
                behavior_id: "motion:CAT_motion:0".to_owned(),
                shortcut: "Control+Alt+9".to_owned(),
            }],
        })
        .expect("persist a recorded shortcut");

    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select standard model");
    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("select keyboard model");

    let primary = behavior_shortcut_primary_name();
    for model_id in ["standard", "keyboard"] {
        let chords = application
            .config()
            .shortcuts
            .model_behavior_bindings
            .iter()
            .filter(|binding| binding.model.id == model_id)
            .map(|binding| binding.shortcut.as_str())
            .collect::<Vec<_>>();
        assert!(
            chords.contains(&format!("{primary}+1").as_str()),
            "{model_id} counts its defaults from the first digit: {chords:?}"
        );
    }

    let compiled = application.shortcut_table().load();
    let behavior_targets = compiled
        .iter()
        .filter_map(|shortcut| match shortcut.target() {
            bongocat_config::ShortcutTarget::ModelBehavior { model, .. } => {
                Some((model.id.as_str(), model.source))
            }
            bongocat_config::ShortcutTarget::Application(_) => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !behavior_targets.is_empty(),
        "the live model's behaviors are registered"
    );
    assert!(
        behavior_targets.iter().all(|(model_id, source)| {
            *model_id == "keyboard" && *source == ModelSource::BuiltIn
        }),
        "only the live model is registered, not the one being left: {behavior_targets:?}"
    );

    // The chord the model being left owned no longer reaches anything.
    let recorded = bongocat_config::ShortcutModifiers::from_bits(
        bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::ALT,
    )
    .expect("valid modifiers");
    assert!(
        compiled.resolve(recorded, "9").is_none(),
        "the previous model's chords must stop working on the switch"
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn shortcut_capture_suspends_a_binding_without_persisting_and_restores_it_on_cancel() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    let shortcuts = bongocat_ui_protocol::SettingsShortcuts {
        commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "Meta+L".to_owned(),
        }],
        model_behaviors: Vec::new(),
    };
    application
        .set_shortcuts(shortcuts.clone())
        .expect("persist shortcut");
    let persisted_before_capture = std::fs::read(&layout.config).expect("read persisted config");
    let meta =
        bongocat_config::ShortcutModifiers::from_bits(bongocat_config::ShortcutModifiers::META)
            .expect("valid modifier");
    assert!(
        application
            .shortcut_table()
            .load()
            .resolve(meta, "L")
            .is_some()
    );

    application
        .suspend_shortcut_capture(bongocat_ui_protocol::SettingsShortcuts::default())
        .expect("suspend shortcut");
    assert!(
        application
            .shortcut_table()
            .load()
            .resolve(meta, "L")
            .is_none()
    );
    assert_eq!(
        std::fs::read(&layout.config).expect("config remains unchanged"),
        persisted_before_capture
    );

    application
        .resume_shortcut_capture()
        .expect("restore shortcut");
    assert!(
        application
            .shortcut_table()
            .load()
            .resolve(meta, "L")
            .is_some()
    );
    application.shutdown().expect("clean shutdown");
}
