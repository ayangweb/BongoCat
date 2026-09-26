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

#[test]
fn runtime_tick_publishes_keyboard_fallback_release_from_runtime_clock() {
    let clock = Arc::new(ManualClock::default());
    let (owner, _consumer) = RuntimeOwner::start_with_rendering_and_clock(false, 8, clock.clone());
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let configured = client
        .send(RuntimeCommand::SetReleaseFallbackTimeout(1_000))
        .expect("fallback setting accepted");
    let configured = client
        .wait_for_command(configured, TIMEOUT)
        .expect("fallback setting published");
    assert_eq!(configured.release_fallback_timeout_ms, 1_000);

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

    clock.set(Duration::from_millis(999));
    let before_deadline = client.send(RuntimeCommand::Tick).expect("tick accepted");
    let before_deadline = client
        .wait_for_command(before_deadline, TIMEOUT)
        .expect("tick published");
    assert_eq!(before_deadline.input.pressed_key_count, 1);

    clock.set(Duration::from_millis(1_000));
    client.send(RuntimeCommand::Tick).expect("tick accepted");
    let deadline = Instant::now() + TIMEOUT;
    let released = loop {
        let current = client.snapshot();
        if current.input.diagnostics.fallback_release == 1 {
            break current;
        }
        assert!(Instant::now() < deadline, "fallback release timed out");
        thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(released.input.pressed_key_count, 0);
    assert_eq!(released.input.diagnostics.reconciled_release, 0);
    assert_eq!(released.input.diagnostics.released_by_reset, 0);
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn release_fallback_timeout_command_rejects_values_above_v1_limit() {
    let owner = RuntimeOwner::start(false, 4);
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let sequence = client
        .send(RuntimeCommand::SetReleaseFallbackTimeout(60_001))
        .expect("invalid command accepted for typed failure");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("invalid command result published");
    assert_eq!(
        rejected.release_fallback_timeout_ms,
        DEFAULT_RELEASE_FALLBACK_TIMEOUT_MS
    );
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::ReleaseFallbackTimeoutInvalid,
        })
    );
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
            mirror_pointer_tracking: false,
            ignore_keyboard: true,
            ignore_gamepad: false,
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
            mirror_pointer_tracking: false,
            ignore_keyboard: false,
            ignore_gamepad: true,
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
