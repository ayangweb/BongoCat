//! Gamepad connectivity, the automatic model switch and the axis dead zones.

use super::*;

#[test]
fn the_gamepad_auto_switch_follows_the_last_used_model_of_each_family() {
    /// Wait until the runtime has applied an input event, so the connected
    /// count the reconcile reads is the one the test published.
    fn wait_for_input(application: &Application, sequence: u64) {
        application
            .runtime_client()
            .wait_for_input_sequence(sequence, RUNTIME_TIMEOUT)
            .expect("runtime applied the gamepad connection event");
    }

    fn identity(id: &str, source: ModelSource) -> ModelIdentity {
        ModelIdentity {
            id: id.to_owned(),
            source,
        }
    }

    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout.clone()).expect("start app");
    let axis = application.gamepad_axis_producer();
    let input = application.input_producer();

    // A fresh configuration is off and remembers nothing: the switch must
    // not touch the model while it is off.
    assert!(!application.config().model.gamepad_auto_switch.enabled);
    assert_eq!(
        application.config().model.gamepad_auto_switch,
        GamepadAutoSwitchConfig::default()
    );
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select the standard model");
    let connection = axis.connect(0).expect("gamepad connection");
    let connected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection event");
    wait_for_input(&application, connected);
    assert_eq!(
        application.apply_gamepad_auto_switch().expect("reconcile"),
        None,
        "a switched-off auto switch must not change the model"
    );

    // Turning it on keeps both targets at "the last model used", so the
    // switch follows what the user actually activated.
    application
        .set_gamepad_auto_switch(bongocat_ui_protocol::SettingsGamepadAutoSwitch {
            enabled: true,
            ..Default::default()
        })
        .expect("enable the auto switch");
    assert_eq!(
        application
            .config()
            .model
            .gamepad_auto_switch
            .connected_model,
        None
    );
    application
        .select_model(ModelOrigin::Preset, "gamepad")
        .expect("select the gamepad model");
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("return to the standard model");

    let switched = application
        .apply_gamepad_auto_switch()
        .expect("reconcile after connecting");
    assert_eq!(switched, Some(identity("gamepad", ModelSource::BuiltIn)));
    assert_eq!(
        application.live_model_identity(),
        Some(identity("gamepad", ModelSource::BuiltIn))
    );
    // The model it replaced is still the remembered non-gamepad model, so a
    // second notice while the pad is attached changes nothing.
    assert_eq!(
        application
            .apply_gamepad_auto_switch()
            .expect("reconcile again"),
        None
    );

    let disconnected = input
        .publish(InputEvent::GamepadDisconnected {
            connection,
            at: MonotonicMillis::new(1),
        })
        .expect("disconnection event");
    wait_for_input(&application, disconnected);
    let switched = application
        .apply_gamepad_auto_switch()
        .expect("reconcile after disconnecting");
    assert_eq!(switched, Some(identity("standard", ModelSource::BuiltIn)));
    assert_eq!(
        application.live_model_identity(),
        Some(identity("standard", ModelSource::BuiltIn))
    );

    // An explicit target wins over the memory in its own direction.
    application
        .set_gamepad_auto_switch(bongocat_ui_protocol::SettingsGamepadAutoSwitch {
            enabled: true,
            connected_model: Some(bongocat_ui_protocol::SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: bongocat_ui_protocol::SettingsModelOrigin::BuiltIn,
            }),
            disconnected_model: None,
        })
        .expect("pin the connected target");
    let reconnected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(2),
        })
        .expect("reconnection event");
    wait_for_input(&application, reconnected);
    assert_eq!(
        application.apply_gamepad_auto_switch().expect("reconcile"),
        Some(identity("keyboard", ModelSource::BuiltIn))
    );

    // The chosen target is persisted like any other selection, so a restart
    // restores the model the user was last shown.
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(persisted.contains("\"gamepad_auto_switch\""));
    assert!(persisted.contains("\"id\": \"keyboard\""));
    assert_eq!(
        application.config().model.selected_model,
        Some(identity("keyboard", ModelSource::BuiltIn))
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn deleting_a_model_forgets_it_as_a_gamepad_auto_switch_target() {
    let removed = ModelId::parse("imported-pad").expect("portable model id");
    let kept = ModelId::parse("other-pad").expect("portable model id");
    let target = |id: &ModelId, source: ModelSource| ModelIdentity {
        id: id.as_str().to_owned(),
        source,
    };
    let switch = GamepadAutoSwitchConfig {
        enabled: true,
        connected_model: Some(target(&removed, ModelSource::Imported)),
        disconnected_model: Some(target(&removed, ModelSource::Imported)),
    };

    // Only an imported model can be deleted, so a build-shipped target with
    // the same id is a different model and stays.
    let (cleared, changed) = without_removed_model_targets(&switch, &removed);
    assert!(changed);
    assert_eq!(cleared.connected_model, None);
    assert_eq!(cleared.disconnected_model, None);

    let preset_switch = GamepadAutoSwitchConfig {
        enabled: true,
        connected_model: Some(target(&removed, ModelSource::BuiltIn)),
        disconnected_model: Some(target(&kept, ModelSource::Imported)),
    };
    let (kept_switch, changed) = without_removed_model_targets(&preset_switch, &removed);
    assert!(!changed, "a build-shipped model cannot be the removed one");
    assert_eq!(kept_switch, preset_switch);
}

#[test]
fn configured_gamepad_dead_zones_apply_at_start_and_persist_updates() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.input.gamepad.stick_dead_zone = 0.4;
    config.input.gamepad.trigger_dead_zone = 0.2;
    store.commit(&config).expect("custom input config");
    drop(store);

    let mut application = Application::start_with_layout(layout.clone()).expect("start app");
    let axis = application.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection");
    let input = application.input_producer();
    input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection event");
    for (axis_kind, value) in [
        (GamepadAxis::LeftStickX, 0.3),
        (GamepadAxis::LeftTrigger, 0.1),
    ] {
        axis.publish(GamepadAxisSample {
            key: GamepadAxisKey {
                connection,
                axis: axis_kind,
            },
            value,
            at: MonotonicMillis::new(1),
        })
        .expect("axis sample");
    }
    let edge = input
        .publish(InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection,
                button: GamepadButton::South,
            }),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(2),
        })
        .expect("button edge");
    let initial = application
        .runtime_client()
        .wait_for_input_sequence(edge, RUNTIME_TIMEOUT)
        .expect("initial axis projection");
    assert_eq!(initial.model_input.stick_left_x, 0.0);
    assert_eq!(initial.model_input.left_trigger, 0.0);

    let updated = application
        .set_gamepad_axis_settings(GamepadAxisSettings::new(0.1, 0.05).expect("valid settings"))
        .expect("update dead zones");
    assert!((updated.model_input.stick_left_x - (0.2 / 0.9)).abs() < 0.0001);
    assert!((updated.model_input.left_trigger - (0.05 / 0.95)).abs() < 0.0001);
    assert_eq!(application.config().input.gamepad.stick_dead_zone, 0.1);
    assert_eq!(application.config().input.gamepad.trigger_dead_zone, 0.05);
    application.shutdown().expect("clean shutdown");

    let restarted = Application::start_with_layout(layout).expect("restart app");
    assert_eq!(restarted.config().input.gamepad.stick_dead_zone, 0.1);
    assert_eq!(restarted.config().input.gamepad.trigger_dead_zone, 0.05);
    restarted.shutdown().expect("clean restart shutdown");
}
