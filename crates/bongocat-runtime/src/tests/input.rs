//! Reliable input, coalesced channels and the reconciled pressed set.

use super::*;

#[test]
fn platform_input_diagnostics_are_live_and_freeze_at_runtime_shutdown() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let producer = owner.platform_input_diagnostics_producer();
    let live = PlatformInputDiagnostics {
        service_status: PlatformInputServiceStatus::Running,
        service_start_attempts: 1,
        runtime_queue_overflows: 2,
        recovery_resets: 3,
        gamepad_connections: 4,
        gamepad_disconnections: 1,
        gamepad_axis_publish_rejections: 5,
        ..PlatformInputDiagnostics::default()
    };
    producer.publish(live).expect("live diagnostics accepted");
    assert_eq!(client.snapshot().platform_input, live);

    let final_diagnostics = PlatformInputDiagnostics {
        service_status: PlatformInputServiceStatus::Stopped,
        clean_shutdown: true,
        ..live
    };
    producer
        .publish(final_diagnostics)
        .expect("final diagnostics accepted");
    let stopped = owner.shutdown(TIMEOUT).expect("clean runtime stop");
    assert_eq!(stopped.platform_input, final_diagnostics);
    assert_eq!(
        producer.publish(PlatformInputDiagnostics::default()),
        Err(PlatformInputDiagnosticsPublishError::RuntimeStopped)
    );
    assert_eq!(client.snapshot().platform_input, final_diagnostics);
}

#[test]
fn input_reset_is_observable() {
    let owner = RuntimeOwner::start(true, 4);
    let client = owner.client();
    let ready = client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    client
        .send(RuntimeCommand::ResetInput(InputResetReason::ServiceRestart))
        .expect("reset accepted");
    let reset = client
        .wait_for_revision(ready.revision + 1, TIMEOUT)
        .expect("reset snapshot");
    assert_eq!(reset.input.diagnostics.reset_count, 1);
    assert_eq!(
        reset.input.last_reset_reason,
        Some(InputResetReason::ServiceRestart)
    );
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_applies_reliable_input_and_reconciled_release() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let down_sequence = client
        .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(0),
            },
        })))
        .expect("key down accepted");
    let pressed = client
        .wait_for_command(down_sequence, TIMEOUT)
        .expect("pressed snapshot");
    assert_eq!(pressed.input.pressed_key_count, 1);

    let release_sequence = client
        .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
            sequence: 1,
            event: InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Up,
                source: InputSource::Reconciliation,
                at: MonotonicMillis::new(500),
            },
        })))
        .expect("reconciled key up accepted");
    let released = client
        .wait_for_command(release_sequence, TIMEOUT)
        .expect("released snapshot");
    assert_eq!(released.input.pressed_key_count, 0);
    assert_eq!(released.input.diagnostics.reconciled_release, 1);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

/// No amount of runtime time releases a held key.
///
/// The pressed set is cleared by exactly three things: a captured release, a
/// reconciliation that stops seeing the control, and a reset. Elapsed wall time
/// is not one of them, because a key the user is still holding looks identical
/// to a lost release only if the elapsed time is the tie-breaker — and that is
/// the guess that turns a slow frame into a key the cat stops believing in.
#[test]
fn a_held_key_survives_any_amount_of_runtime_time() {
    let clock = Arc::new(ManualClock::default());
    let (owner, _consumer) = RuntimeOwner::start_with_rendering_and_clock(false, 8, clock.clone());
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let down = client
        .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(50_000),
            },
        })))
        .expect("key down accepted");
    let pressed = client
        .wait_for_command(down, TIMEOUT)
        .expect("key down published");
    assert_eq!(pressed.input.pressed_key_count, 1);

    clock.set(Duration::from_secs(3_600));
    for _ in 0..8 {
        let tick = client.send(RuntimeCommand::Tick).expect("tick accepted");
        let ticked = client
            .wait_for_command(tick, TIMEOUT)
            .expect("tick published");
        assert_eq!(
            ticked.input.pressed_key_count, 1,
            "an hour of runtime time is not a release"
        );
    }

    let release = client
        .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
            sequence: 1,
            event: InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Up,
                source: InputSource::Capture,
                at: MonotonicMillis::new(50_001),
            },
        })))
        .expect("key up accepted");
    let released = client
        .wait_for_command(release, TIMEOUT)
        .expect("key up published");
    assert_eq!(released.input.pressed_key_count, 0);
    assert_eq!(released.input.diagnostics.captured_up, 1);
    assert_eq!(released.input.diagnostics.reconciled_release, 0);
    assert_eq!(released.input.diagnostics.released_by_reset, 0);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_coalesces_cursor_flood_without_delaying_reliable_release() {
    let owner = RuntimeOwner::start(true, 4);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let input = owner.input_producer();
    let cursor = owner.cursor_producer();
    let down_sequence = input
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(0),
        })
        .expect("down accepted");
    client
        .wait_for_input_sequence(down_sequence, TIMEOUT)
        .expect("down consumed");

    for index in 1_u32..=10_000 {
        cursor
            .publish(cursor_sample(
                f64::from(index % 100),
                f64::from(index % 80),
                u64::from(index),
            ))
            .expect("cursor sample accepted");
    }
    let release_sequence = input
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: MonotonicMillis::new(10_001),
        })
        .expect("release accepted independently of cursor flood");
    let released = client
        .wait_for_input_sequence(release_sequence, TIMEOUT)
        .expect("release consumed");
    assert_eq!(released.input.pressed_key_count, 0);
    assert_eq!(released.input.transport.queue_full, 0);

    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.cursor.transport.published, 10_000);
    assert!(stopped.cursor.transport.coalesced > 0);
    assert_eq!(stopped.cursor.transport.pending, 0);
    assert_eq!(
        stopped.cursor.transport.published,
        stopped
            .cursor
            .transport
            .coalesced
            .saturating_add(stopped.cursor.transport.consumed)
    );
}

#[test]
fn runtime_coalesces_gamepad_axes_and_projects_dead_zone_without_blocking_edges() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    client
        .send(RuntimeCommand::SetGamepadAxisSettings(
            GamepadAxisSettings::new(0.2, 0.1).expect("settings"),
        ))
        .expect("axis settings accepted");
    let axis = owner.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection allocated");
    let input = owner.input_producer();
    input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection accepted");
    for index in 0..10_000 {
        axis.publish(GamepadAxisSample {
            key: GamepadAxisKey {
                connection,
                axis: GamepadAxis::LeftStickX,
            },
            value: index as f32 / 10_000.0,
            at: MonotonicMillis::new(index),
        })
        .expect("axis sample accepted");
    }
    let down = input
        .publish(InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection,
                button: GamepadButton::South,
            }),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(10_001),
        })
        .expect("button edge accepted");
    let snapshot = client
        .wait_for_input_sequence(down, TIMEOUT)
        .expect("button edge consumed");
    assert_eq!(snapshot.input.pressed_gamepad_button_count, 1);
    assert!((snapshot.model_input.stick_left_x - 0.999875).abs() < 0.0001);
    assert!(snapshot.gamepad_axis_transport.coalesced > 0);
    let reset_sequence = input
        .publish(InputEvent::Reset {
            reason: InputResetReason::DeviceRemoved,
            at: MonotonicMillis::new(10_002),
        })
        .expect("reset accepted");
    let reset = client
        .wait_for_input_sequence(reset_sequence, TIMEOUT)
        .expect("reset consumed");
    assert_eq!(reset.model_input.stick_left_x, 0.0);
    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.model_input.stick_left_x, 0.0);
    assert_eq!(stopped.gamepad_axis_transport.pending, 0);
}

/// The complete stick matrix through the real transport: a `GamepadAxisProducer`
/// sample for each of the six analog axes and an `InputProducer` edge for each
/// stick click, checked on the `ModelInputSnapshot` the runtime hands the
/// renderer.
///
/// This is the half of the chain the platform adapter feeds. Together with
/// `rendering::tests::model::every_stick_control_reaches_its_own_parameter_and_nothing_else`,
/// which reads the Live2D side, it covers a sample from a device to a model
/// parameter, so a control that stops reaching the snapshot cannot still reach the
/// model. Each row moves one control and checks all six fields, which is what
/// catches a mapping that shifts one stick's axis onto the other, or folds an
/// analog trigger into a stick.
#[test]
fn every_gamepad_axis_reaches_its_own_model_input_field() {
    /// Every axis the transport carries and the snapshot field it owns.
    const AXES: [(GamepadAxis, &str, f32); 6] = [
        (GamepadAxis::LeftStickX, "stick_left_x", 0.8),
        (GamepadAxis::LeftStickY, "stick_left_y", -0.6),
        (GamepadAxis::RightStickX, "stick_right_x", 0.8),
        (GamepadAxis::RightStickY, "stick_right_y", -0.6),
        (GamepadAxis::LeftTrigger, "left_trigger", 0.75),
        (GamepadAxis::RightTrigger, "right_trigger", 0.75),
    ];

    fn field(input: &ModelInputSnapshot, name: &str) -> f32 {
        match name {
            "stick_left_x" => input.stick_left_x,
            "stick_left_y" => input.stick_left_y,
            "stick_right_x" => input.stick_right_x,
            "stick_right_y" => input.stick_right_y,
            "left_trigger" => input.left_trigger,
            "right_trigger" => input.right_trigger,
            other => panic!("unknown model input field {other}"),
        }
    }

    let owner = RuntimeOwner::start(true, 32);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    // No dead zone, so the assertion is about which field a sample reaches and
    // not about how it was shaped; `GamepadAxisSettings` has its own tests.
    let settings = client
        .send(RuntimeCommand::SetGamepadAxisSettings(
            GamepadAxisSettings::new(0.0, 0.0).expect("settings"),
        ))
        .expect("axis settings accepted");
    client
        .wait_for_command(settings, TIMEOUT)
        .expect("axis settings applied");
    let axes = owner.gamepad_axis_producer();
    let connection = axes.connect(0).expect("gamepad connection allocated");
    let input = owner.input_producer();
    let connected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection accepted");
    let initial = client
        .wait_for_input_sequence(connected, TIMEOUT)
        .expect("connection consumed");
    for (_, name, _) in AXES {
        assert_eq!(field(&initial.model_input, name), 0.0, "{name} at connect");
    }
    let mut clock = 0u64;

    for (index, (axis, name, value)) in AXES.into_iter().enumerate() {
        clock += 1;
        // An axis is a latest-value slot, so the value the previous iteration
        // left behind is still there. Recording the six fields before the sample
        // is what turns "this axis carries its own value" into "this sample moved
        // this axis and left the other five alone", which is the property a
        // shifted or folded mapping breaks.
        let before = client.snapshot().model_input;
        axes.publish(GamepadAxisSample {
            key: GamepadAxisKey { connection, axis },
            value,
            at: MonotonicMillis::new(clock),
        })
        .expect("axis sample accepted");
        // A reliable edge is what makes the worker recompose its model input in
        // the same pass, so the sample is observed without waiting for a tick.
        let flush = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey {
                    connection,
                    button: GamepadButton::South,
                }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(clock),
            })
            .expect("flush edge accepted");
        let snapshot = client
            .wait_for_input_sequence(flush, TIMEOUT)
            .expect("input consumed");
        assert_eq!(
            field(&snapshot.model_input, name),
            value,
            "{name} must carry its own sample"
        );
        for (other, other_name, _) in AXES.into_iter().filter(|(other, _, _)| *other != axis) {
            assert_eq!(
                field(&snapshot.model_input, other_name),
                field(&before, other_name),
                "{name} must not reach {other:?}"
            );
        }
        assert!(
            !snapshot.model_input.stick_left_down && !snapshot.model_input.stick_right_down,
            "{name} is a continuous axis and must not claim a stick click"
        );
        if index == 0 {
            assert_eq!(
                field(&initial.model_input, name),
                0.0,
                "{name} starts at rest"
            );
        }
    }

    // Each stick click lands in its own field from a reliable edge rather than an
    // axis sample, so the two halves of the projection cannot be conflated.
    for (button, left, right) in [
        (GamepadButton::LeftStick, true, false),
        (GamepadButton::RightStick, false, true),
    ] {
        clock += 1;
        let pressed = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(clock),
            })
            .expect("stick click accepted");
        let snapshot = client
            .wait_for_input_sequence(pressed, TIMEOUT)
            .expect("stick click consumed");
        assert_eq!(
            snapshot.model_input.stick_left_down, left,
            "{button:?} left stick click"
        );
        assert_eq!(
            snapshot.model_input.stick_right_down, right,
            "{button:?} right stick click"
        );

        clock += 1;
        let released = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge: InputEdge::Up,
                source: InputSource::Capture,
                at: MonotonicMillis::new(clock),
            })
            .expect("stick click release accepted");
        let snapshot = client
            .wait_for_input_sequence(released, TIMEOUT)
            .expect("stick click release consumed");
        assert!(!snapshot.model_input.stick_left_down, "{button:?} released");
        assert!(
            !snapshot.model_input.stick_right_down,
            "{button:?} released"
        );
    }

    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn model_input_filters_preserve_raw_pressed_state_and_recompose_immediately() {
    let owner = RuntimeOwner::start(true, 16);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let bindings = Arc::new(InputBindings::with_gamepad_hands(
        BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]),
        BTreeMap::from([(GamepadButton::South, HandSide::Right)]),
    ));
    let configured = client
        .send(RuntimeCommand::SetInputBindings(bindings))
        .expect("bindings accepted");
    client
        .wait_for_command(configured, TIMEOUT)
        .expect("bindings published");

    let axis = owner.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection allocated");
    let input = owner.input_producer();
    let connected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection accepted");
    client
        .wait_for_input_sequence(connected, TIMEOUT)
        .expect("connection consumed");
    let key_down = input
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        })
        .expect("key down accepted");
    client
        .wait_for_input_sequence(key_down, TIMEOUT)
        .expect("key down consumed");
    let gamepad_down = input
        .publish(InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection,
                button: GamepadButton::South,
            }),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(2),
        })
        .expect("gamepad down accepted");
    let before_filter = client
        .wait_for_input_sequence(gamepad_down, TIMEOUT)
        .expect("gamepad down consumed");
    axis.publish(GamepadAxisSample {
        key: GamepadAxisKey {
            connection,
            axis: GamepadAxis::LeftStickX,
        },
        value: 1.0,
        at: MonotonicMillis::new(3),
    })
    .expect("axis accepted");
    let tick = client.send(RuntimeCommand::Tick).expect("tick accepted");
    let with_axis = client
        .wait_for_command(tick, TIMEOUT)
        .expect("axis consumed");
    assert!(before_filter.model_input.left_hand_down);
    assert!(before_filter.model_input.right_hand_down);
    assert!((with_axis.model_input.stick_left_x - 1.0).abs() < 0.0001);

    let keyboard_filter = client
        .send(RuntimeCommand::SetModelSettings(ModelSettings {
            mirror: false,
            mirror_pointer_tracking_horizontal: false,
            mirror_pointer_tracking_vertical: false,
            ignore_keyboard: true,
            ignore_gamepad: false,
            show_all_pressed_keys: false,
            toggle_repeated_expression: false,
            allow_motion_overlap: false,
            ignore_pointer: false,
        }))
        .expect("keyboard filter accepted");
    let keyboard_filtered = client
        .wait_for_command(keyboard_filter, TIMEOUT)
        .expect("keyboard filter published");
    assert_eq!(keyboard_filtered.input.pressed_key_count, 1);
    assert_eq!(keyboard_filtered.input.pressed_gamepad_button_count, 1);
    assert!(!keyboard_filtered.model_input.left_hand_down);
    assert!(keyboard_filtered.model_input.right_hand_down);
    assert_eq!(
        keyboard_filtered
            .model_input
            .key_presses
            .iter()
            .map(|press| press.key)
            .collect::<Vec<_>>(),
        vec![bongocat_render::KeyIdentity::Gamepad(GamepadButton::South)],
        "the ignored keyboard source takes its own overlay with it"
    );
    assert!((keyboard_filtered.model_input.stick_left_x - 1.0).abs() < 0.0001);

    let gamepad_filter = client
        .send(RuntimeCommand::SetModelSettings(ModelSettings {
            mirror: false,
            mirror_pointer_tracking_horizontal: false,
            mirror_pointer_tracking_vertical: false,
            ignore_keyboard: false,
            ignore_gamepad: true,
            show_all_pressed_keys: false,
            toggle_repeated_expression: false,
            allow_motion_overlap: false,
            ignore_pointer: false,
        }))
        .expect("gamepad filter accepted");
    let gamepad_filtered = client
        .wait_for_command(gamepad_filter, TIMEOUT)
        .expect("gamepad filter published");
    assert_eq!(gamepad_filtered.input.pressed_key_count, 1);
    assert_eq!(gamepad_filtered.input.pressed_gamepad_button_count, 1);
    assert!(gamepad_filtered.model_input.left_hand_down);
    assert!(!gamepad_filtered.model_input.right_hand_down);
    assert_eq!(
        gamepad_filtered
            .model_input
            .key_presses
            .iter()
            .map(|press| press.key)
            .collect::<Vec<_>>(),
        vec![bongocat_render::KeyIdentity::Keyboard(
            PhysicalKey::KEY_A.hid_usage()
        )],
        "the ignored gamepad source takes its own overlay with it"
    );
    assert_eq!(gamepad_filtered.model_input.stick_left_x, 0.0);

    let key_up = input
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: MonotonicMillis::new(4),
        })
        .expect("key up accepted");
    client
        .wait_for_input_sequence(key_up, TIMEOUT)
        .expect("key up consumed");
    let gamepad_up = input
        .publish(InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection,
                button: GamepadButton::South,
            }),
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: MonotonicMillis::new(5),
        })
        .expect("gamepad up accepted");
    let released = client
        .wait_for_input_sequence(gamepad_up, TIMEOUT)
        .expect("gamepad up consumed");
    assert_eq!(released.input.pressed_key_count, 0);
    assert_eq!(released.input.pressed_gamepad_button_count, 0);
    assert_eq!(released.model_input, ModelInputSnapshot::default());
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_discards_axis_samples_until_the_connection_is_active() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let axis = owner.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection allocated");
    let input = owner.input_producer();

    axis.publish(GamepadAxisSample {
        key: GamepadAxisKey {
            connection,
            axis: GamepadAxis::LeftStickX,
        },
        value: 0.9,
        at: MonotonicMillis::new(0),
    })
    .expect("axis sample accepted");
    let connected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(1),
        })
        .expect("connection accepted");
    let before_axis = client
        .wait_for_input_sequence(connected, TIMEOUT)
        .expect("connection consumed");
    assert_eq!(before_axis.model_input.stick_left_x, 0.0);

    axis.publish(GamepadAxisSample {
        key: GamepadAxisKey {
            connection,
            axis: GamepadAxis::LeftStickX,
        },
        value: 0.9,
        at: MonotonicMillis::new(2),
    })
    .expect("active axis sample accepted");
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
        .expect("button edge accepted");
    let active = client
        .wait_for_input_sequence(edge, TIMEOUT)
        .expect("active edge consumed");
    assert!(active.model_input.stick_left_x > 0.8);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn shutdown_flushes_a_pending_gamepad_axis_before_stopped_state() {
    let owner = RuntimeOwner::start(false, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let axis = owner.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection allocated");
    let input = owner.input_producer();
    let connected = input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection accepted");
    client
        .wait_for_input_sequence(connected, TIMEOUT)
        .expect("connection consumed");

    axis.publish(GamepadAxisSample {
        key: GamepadAxisKey {
            connection,
            axis: GamepadAxis::LeftStickX,
        },
        value: 0.75,
        at: MonotonicMillis::new(1),
    })
    .expect("pending axis accepted");

    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert_eq!(stopped.gamepad_axis_transport.pending, 0);
    assert_eq!(stopped.gamepad_axis_transport.consumed, 1);
    assert!(stopped.model_input.stick_left_x > 0.7);
}

#[test]
fn runtime_rejects_late_gamepad_generation_after_reconnect() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let input = owner.input_producer();
    let axes = owner.gamepad_axis_producer();
    let first = axes.connect(0).expect("first connection");
    let second = axes.connect(0).expect("reconnected generation");

    let connect_first = input
        .publish(InputEvent::GamepadConnected {
            connection: first,
            at: MonotonicMillis::new(0),
        })
        .expect("first connection accepted");
    client
        .wait_for_input_sequence(connect_first, TIMEOUT)
        .expect("first connection consumed");
    let disconnect = input
        .publish(InputEvent::GamepadDisconnected {
            connection: first,
            at: MonotonicMillis::new(1),
        })
        .expect("disconnect accepted");
    client
        .wait_for_input_sequence(disconnect, TIMEOUT)
        .expect("disconnect consumed");
    let connect_second = input
        .publish(InputEvent::GamepadConnected {
            connection: second,
            at: MonotonicMillis::new(2),
        })
        .expect("reconnection accepted");
    client
        .wait_for_input_sequence(connect_second, TIMEOUT)
        .expect("reconnection consumed");

    axes.publish(GamepadAxisSample {
        key: GamepadAxisKey {
            connection: second,
            axis: GamepadAxis::LeftStickX,
        },
        value: 0.8,
        at: MonotonicMillis::new(3),
    })
    .expect("current generation axis accepted");
    let stale_axis = axes
        .publish(GamepadAxisSample {
            key: GamepadAxisKey {
                connection: first,
                axis: GamepadAxis::LeftStickX,
            },
            value: 1.0,
            at: MonotonicMillis::new(4),
        })
        .expect_err("stale axis generation rejected");
    assert!(matches!(
        stale_axis,
        GamepadAxisPublishError::StaleGeneration(_)
    ));

    let stale_edge = input
        .publish(InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection: first,
                button: GamepadButton::South,
            }),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(4),
        })
        .expect("late edge accepted for runtime classification");
    let snapshot = client
        .wait_for_input_sequence(stale_edge, TIMEOUT)
        .expect("late edge consumed");
    assert_eq!(snapshot.input.pressed_gamepad_button_count, 0);
    assert!((snapshot.model_input.stick_left_x - 0.76470584).abs() < 0.0001);
    assert!(snapshot.model_input.stick_left_x < 1.0);
    assert_eq!(snapshot.input.diagnostics.stale_gamepad_events, 1);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_projects_display_relative_cursor_into_model_snapshot() {
    let owner = RuntimeOwner::start(true, 4);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    owner
        .cursor_producer()
        .publish(cursor_sample(25.0, 40.0, 1))
        .expect("cursor accepted");
    let snapshot = client
        .wait_for_cursor_samples(1, TIMEOUT)
        .expect("cursor consumed");
    assert_eq!(snapshot.cursor.sample, Some(cursor_sample(25.0, 40.0, 1)));
    assert_eq!(snapshot.model_input.pointer_x, 0.5);
    assert!((snapshot.model_input.pointer_y - 0.2).abs() < f32::EPSILON);
    assert!((snapshot.model_input.pointer_z + 0.1).abs() < f32::EPSILON);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_smooths_cursor_with_the_injected_monotonic_clock() {
    let clock = Arc::new(ManualClock::default());
    let (owner, _render_consumer) =
        RuntimeOwner::start_with_rendering_and_clock(true, 4, clock.clone());
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let cursor = owner.cursor_producer();

    cursor
        .publish(cursor_sample(50.0, 50.0, 0))
        .expect("initial cursor accepted");
    client
        .wait_for_cursor_samples(1, TIMEOUT)
        .expect("initial cursor consumed");
    cursor
        .publish(cursor_sample(0.0, 50.0, 1))
        .expect("target cursor accepted");
    let targeted = client
        .wait_for_cursor_samples(2, TIMEOUT)
        .expect("target cursor consumed");
    assert_eq!(targeted.model_input.pointer_x, 0.0);

    let frame = Duration::from_secs_f64(1.0 / 60.0);
    clock.set(frame);
    let first_tick = client
        .send(RuntimeCommand::Tick)
        .expect("first tick accepted");
    let first = client
        .wait_for_command(first_tick, TIMEOUT)
        .expect("first tick applied");
    assert!((first.model_input.pointer_x - 0.25).abs() < 1e-6);

    clock.set(frame * 2);
    let second_tick = client
        .send(RuntimeCommand::Tick)
        .expect("second tick accepted");
    let second = client
        .wait_for_command(second_tick, TIMEOUT)
        .expect("second tick applied");
    assert!((second.model_input.pointer_x - 0.4375).abs() < 1e-6);

    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn shutdown_flushes_a_pending_cursor_before_stopped_state() {
    let owner = RuntimeOwner::start(true, 4);
    owner
        .client()
        .wait_for_revision(1, TIMEOUT)
        .expect("runtime ready");
    owner
        .cursor_producer()
        .publish(cursor_sample(75.0, 60.0, 1))
        .expect("pending cursor accepted");
    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert_eq!(stopped.cursor.sample, Some(cursor_sample(75.0, 60.0, 1)));
    assert_eq!(stopped.cursor.transport.consumed, 1);
    assert_eq!(stopped.cursor.transport.pending, 0);
}
