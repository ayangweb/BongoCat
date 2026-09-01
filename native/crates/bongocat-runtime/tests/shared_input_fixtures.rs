use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use bongocat_runtime::{
    CursorPosition, CursorSample, CursorViewport, GamepadAxis, GamepadAxisKey, GamepadAxisSample,
    GamepadButton, GamepadButtonKey, GamepadConnection, HandSide, InputBindings, InputControl,
    InputEdge, InputEvent, InputResetReason, InputSource, MonotonicMillis, MouseButton,
    PhysicalKey, RuntimeCommand, RuntimeOwner, RuntimeState,
};
use serde_json::Value;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_audio::MotionAudioClient;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_render::{ModelCommitFeedback, ModelCommitOutcome, RenderConsumer, RenderFrame};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_runtime::{ExpressionId, MonotonicClock, MotionId, MotionPriority};

const TIMEOUT: Duration = Duration::from_secs(2);

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .to_owned()
}

fn load(path: PathBuf) -> Value {
    serde_json::from_slice(&fs::read(path).expect("fixture bytes")).expect("fixture JSON")
}

fn key(name: &str) -> PhysicalKey {
    match name {
        "ControlLeft" => PhysicalKey::from_hid_usage(0xe0),
        "ShiftLeft" => PhysicalKey::from_hid_usage(0xe1),
        value if value.strip_prefix("Key").is_some_and(|key| key.len() == 1) => {
            let letter = value.as_bytes()[3].to_ascii_uppercase();
            assert!(
                letter.is_ascii_uppercase(),
                "unsupported fixture key {name}"
            );
            PhysicalKey::from_hid_usage(0x04 + u16::from(letter - b'A'))
        }
        _ => panic!("unsupported fixture key {name}"),
    }
}

fn mouse_button(name: &str) -> MouseButton {
    match name {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        "back" => MouseButton::Back,
        "forward" => MouseButton::Forward,
        _ => panic!("unsupported fixture mouse button {name}"),
    }
}

fn gamepad_button(name: &str) -> GamepadButton {
    match name {
        "south" => GamepadButton::South,
        "east" => GamepadButton::East,
        "west" => GamepadButton::West,
        "north" => GamepadButton::North,
        "left_shoulder" => GamepadButton::LeftShoulder,
        "right_shoulder" => GamepadButton::RightShoulder,
        "left_trigger" => GamepadButton::LeftTrigger,
        "right_trigger" => GamepadButton::RightTrigger,
        "select" => GamepadButton::Select,
        "start" => GamepadButton::Start,
        "left_stick" => GamepadButton::LeftStick,
        "right_stick" => GamepadButton::RightStick,
        "dpad_up" => GamepadButton::DpadUp,
        "dpad_down" => GamepadButton::DpadDown,
        "dpad_left" => GamepadButton::DpadLeft,
        "dpad_right" => GamepadButton::DpadRight,
        _ => panic!("unsupported fixture gamepad button {name}"),
    }
}

fn gamepad_button_key(name: &str) -> GamepadButton {
    let suffix = name.strip_prefix("Gamepad").expect("gamepad binding key");
    let mut snake = String::new();
    for (index, character) in suffix.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            snake.push('_');
        }
        snake.push(character.to_ascii_lowercase());
    }
    gamepad_button(&snake)
}

fn gamepad_axis(name: &str) -> GamepadAxis {
    match name {
        "left_stick_x" => GamepadAxis::LeftStickX,
        "left_stick_y" => GamepadAxis::LeftStickY,
        "right_stick_x" => GamepadAxis::RightStickX,
        "right_stick_y" => GamepadAxis::RightStickY,
        "left_trigger" => GamepadAxis::LeftTrigger,
        "right_trigger" => GamepadAxis::RightTrigger,
        _ => panic!("unsupported fixture gamepad axis {name}"),
    }
}

fn reset_reason(name: &str) -> InputResetReason {
    match name {
        "session_lock" => InputResetReason::SessionLock,
        "sleep" => InputResetReason::Sleep,
        "device_removed" => InputResetReason::DeviceRemoved,
        "service_restart" => InputResetReason::ServiceRestart,
        "queue_overflow" => InputResetReason::QueueOverflow,
        "permission_changed" => InputResetReason::PermissionChanged,
        "test" => InputResetReason::Test,
        _ => panic!("unsupported fixture reset reason {name}"),
    }
}

fn device_id(name: &str, ids: &mut BTreeMap<String, u8>) -> u8 {
    if let Some(id) = ids.get(name) {
        return *id;
    }
    let id = u8::try_from(ids.len() + 1).expect("fixture device id range");
    ids.insert(name.to_owned(), id);
    id
}

fn source(value: &str) -> InputSource {
    match value {
        "capture" => InputSource::Capture,
        "reconciliation" => InputSource::Reconciliation,
        _ => panic!("unsupported fixture input source {value}"),
    }
}

fn publish_event(owner: &RuntimeOwner, event: &Value, ids: &mut BTreeMap<String, u8>) {
    let at = MonotonicMillis::new(event["atMs"].as_u64().expect("event time"));
    let input = owner.input_producer();
    match event["type"].as_str().expect("event type") {
        "key_down" | "key_up" => {
            let edge = if event["type"] == "key_down" {
                InputEdge::Down
            } else {
                InputEdge::Up
            };
            input
                .publish(InputEvent::Edge {
                    control: InputControl::Key(key(event["key"].as_str().expect("key"))),
                    edge,
                    source: source(event["source"].as_str().expect("source")),
                    at,
                })
                .expect("publish key event");
        }
        "mouse_down" | "mouse_up" => {
            let edge = if event["type"] == "mouse_down" {
                InputEdge::Down
            } else {
                InputEdge::Up
            };
            input
                .publish(InputEvent::Edge {
                    control: InputControl::Mouse(mouse_button(
                        event["button"].as_str().expect("mouse button"),
                    )),
                    edge,
                    source: source(event["source"].as_str().expect("source")),
                    at,
                })
                .expect("publish mouse event");
        }
        "device_connected" => {
            assert_eq!(event["deviceKind"], "gamepad", "input fixture device kind");
            input
                .publish(InputEvent::GamepadConnected {
                    connection: GamepadConnection {
                        device_id: device_id(event["deviceId"].as_str().expect("device id"), ids),
                        generation: 1,
                    },
                    at,
                })
                .expect("publish gamepad connection");
        }
        "device_disconnected" => {
            assert_eq!(event["deviceKind"], "gamepad", "input fixture device kind");
            input
                .publish(InputEvent::GamepadDisconnected {
                    connection: GamepadConnection {
                        device_id: device_id(event["deviceId"].as_str().expect("device id"), ids),
                        generation: 1,
                    },
                    at,
                })
                .expect("publish gamepad disconnection");
        }
        "gamepad_button" => {
            let value = event["value"].as_f64().expect("gamepad button value");
            input
                .publish(InputEvent::Edge {
                    control: InputControl::Gamepad(GamepadButtonKey {
                        connection: GamepadConnection {
                            device_id: device_id(
                                event["deviceId"].as_str().expect("device id"),
                                ids,
                            ),
                            generation: 1,
                        },
                        button: gamepad_button(event["button"].as_str().expect("button")),
                    }),
                    edge: if value >= 0.5 {
                        InputEdge::Down
                    } else {
                        InputEdge::Up
                    },
                    source: InputSource::Capture,
                    at,
                })
                .expect("publish gamepad button");
        }
        "reset" => {
            input
                .publish(InputEvent::Reset {
                    reason: reset_reason(event["reason"].as_str().expect("reset reason")),
                    at,
                })
                .expect("publish reset");
        }
        "cursor_moved" => {
            let sample = CursorSample::new(
                CursorPosition {
                    x: event["position"]["x"].as_f64().expect("cursor x"),
                    y: event["position"]["y"].as_f64().expect("cursor y"),
                },
                CursorViewport {
                    origin: CursorPosition { x: 0.0, y: 0.0 },
                    width: 100.0,
                    height: 100.0,
                },
                at,
            )
            .expect("cursor sample");
            owner
                .cursor_producer()
                .publish(sample)
                .expect("publish cursor sample");
        }
        "gamepad_axis" => {
            let key = GamepadAxisKey {
                connection: GamepadConnection {
                    device_id: device_id(event["deviceId"].as_str().expect("device id"), ids),
                    generation: 1,
                },
                axis: gamepad_axis(event["axis"].as_str().expect("axis")),
            };
            let sample = GamepadAxisSample::new(
                key,
                event["value"].as_f64().expect("axis value") as f32,
                at,
            )
            .expect("gamepad axis sample");
            owner
                .gamepad_axis_producer()
                .publish(sample)
                .expect("publish gamepad axis");
        }
        other => panic!("unsupported product input fixture event {other}"),
    }
}

fn assert_checkpoint(
    snapshot: &bongocat_runtime::RuntimeSnapshot,
    checkpoint: &Value,
    gamepad_bindings: &BTreeMap<GamepadButton, HandSide>,
) {
    let expected_input = &checkpoint["input"];
    assert_eq!(
        snapshot.input.pressed_key_count,
        expected_input["pressedKeys"]
            .as_array()
            .expect("pressed keys")
            .len()
    );
    assert_eq!(
        snapshot.input.pressed_mouse_button_count,
        expected_input["pressedMouseButtons"]
            .as_array()
            .expect("pressed mouse buttons")
            .len()
    );
    assert_eq!(
        snapshot.input.connected_gamepad_count,
        expected_input["connectedDevices"]
            .as_array()
            .expect("connected devices")
            .len()
    );
    let expected_reset = expected_input["lastResetReason"].as_str();
    assert_eq!(
        snapshot
            .input
            .last_reset_reason
            .map(|reason| format!("{reason:?}").to_ascii_lowercase()),
        expected_reset.map(|reason| reason.replace('_', "")),
        "reset reason at {}ms",
        checkpoint["atMs"]
    );
    match expected_input
        .get("cursorPosition")
        .and_then(Value::as_object)
    {
        Some(expected_cursor) => {
            let actual = snapshot.cursor.sample.expect("cursor sample");
            assert_eq!(actual.position.x, expected_cursor["x"]);
            assert_eq!(actual.position.y, expected_cursor["y"]);
        }
        None => assert!(snapshot.cursor.sample.is_none()),
    }
    let expected_model = &checkpoint["model"];
    let model = snapshot.model_input;
    assert_eq!(
        model.left_hand_down,
        expected_model["leftHandDown"].as_bool().expect("left hand")
    );
    assert_eq!(
        model.right_hand_down,
        expected_model["rightHandDown"]
            .as_bool()
            .expect("right hand")
    );
    assert!(expected_model["activeMotion"].is_null());
    assert!(expected_model["activeExpression"].is_null());
    let parameters = expected_model["parameters"]
        .as_object()
        .expect("parameters");
    for (name, value) in parameters {
        let expected = value.as_f64().expect("parameter value") as f32;
        let actual = match name.as_str() {
            "CatParamLeftHandDown" => model.left_hand_down as u8 as f32,
            "CatParamRightHandDown" => model.right_hand_down as u8 as f32,
            "ParamMouseLeftDown" => model.mouse_left_down as u8 as f32,
            "ParamMouseRightDown" => model.mouse_right_down as u8 as f32,
            other if other.starts_with("Gamepad") && other.ends_with("Down") => {
                let binding = gamepad_button_key(other.trim_end_matches("Down"));
                let pressed = match binding {
                    GamepadButton::LeftStick => model.stick_left_down,
                    GamepadButton::RightStick => model.stick_right_down,
                    button => match gamepad_bindings.get(&button) {
                        Some(HandSide::Left) => model.left_hand_down,
                        Some(HandSide::Right) => model.right_hand_down,
                        None => false,
                    },
                };
                pressed as u8 as f32
            }
            other => panic!("unsupported product parameter {other}"),
        };
        assert_eq!(
            actual, expected,
            "parameter {name} at {}ms",
            checkpoint["atMs"]
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Default)]
struct FixtureClock(std::sync::Mutex<Duration>);

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FixtureClock {
    fn set_ms(&self, value: u64) {
        *self.0.lock().expect("fixture clock") = Duration::from_millis(value);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl MonotonicClock for FixtureClock {
    fn now(&self) -> Duration {
        *self.0.lock().expect("fixture clock")
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn wait_for_render_frame(
    consumer: &RenderConsumer,
    predicate: impl Fn(&RenderFrame) -> bool,
) -> RenderFrame {
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(frame) = consumer.take_latest()
            && predicate(&frame)
        {
            return frame;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "render frame timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn activate_fixture_model(
    client: &bongocat_runtime::RuntimeClient,
    consumer: &RenderConsumer,
    model_id: &str,
) -> bongocat_runtime::RuntimeSnapshot {
    let actual_id = match model_id {
        "fixture-model-a" => "standard",
        "fixture-model-b" => "keyboard",
        other => panic!("unsupported fixture model {other}"),
    };
    let model = PresetModelCatalog::open(
        repository_root().join("native/resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse(actual_id).expect("model id"))
    .expect("preset model");
    let sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(model)))
        .expect("activate fixture model");
    let frame = wait_for_render_frame(consumer, |frame| {
        frame
            .model_commit
            .is_some_and(|token| token.command_sequence == sequence)
    });
    let token = frame.model_commit.expect("model commit token");
    consumer
        .report_model_commit(ModelCommitFeedback {
            token,
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("report model commit");
    client
        .wait_for_command(sequence, TIMEOUT)
        .expect("model committed")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn fixture_motion(motion_id: &str) -> MotionId {
    let (group, index) = match motion_id {
        "walk" => ("CAT_motion", 0),
        "idle" => ("CAT_motion", 1),
        "blink" => ("CAT_motion_lock", 0),
        other => panic!("unsupported fixture motion {other}"),
    };
    MotionId::new(group, index).expect("fixture motion id")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn model_motion_expression_audio_fixture_matches_product_runtime() {
    let root = repository_root().join("shared/fixtures");
    let sequence = load(root.join("input-sequences/model-motion-expression-audio.json"));
    let expected = load(root.join("expected-state/model-motion-expression-audio.json"));
    let clock = Arc::new(FixtureClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_audio_and_clock(
        true,
        true,
        32,
        MotionAudioClient::unavailable(),
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");

    let mut event_index = 0;
    let mut audio_triggers = 0;
    for checkpoint in expected["checkpoints"].as_array().expect("checkpoints") {
        let at_ms = checkpoint["atMs"].as_u64().expect("checkpoint time");
        while event_index < sequence["events"].as_array().expect("events").len()
            && sequence["events"][event_index]["atMs"]
                .as_u64()
                .expect("event time")
                <= at_ms
        {
            let event = &sequence["events"][event_index];
            let event_at = event["atMs"].as_u64().expect("event time");
            clock.set_ms(event_at);
            match event["type"].as_str().expect("event type") {
                "model_switch" => {
                    activate_fixture_model(
                        &client,
                        &consumer,
                        event["modelId"].as_str().expect("model id"),
                    );
                }
                "motion_start" => {
                    let priority = match event["priority"].as_str().expect("priority") {
                        "idle" => MotionPriority::Idle,
                        "normal" => MotionPriority::Normal,
                        "force" => MotionPriority::Force,
                        other => panic!("unsupported fixture priority {other}"),
                    };
                    let command = client
                        .send(RuntimeCommand::StartMotion {
                            motion: fixture_motion(event["motionId"].as_str().expect("motion id")),
                            priority,
                        })
                        .expect("start fixture motion");
                    client
                        .wait_for_command(command, TIMEOUT)
                        .expect("motion command");
                }
                "motion_stop" => {
                    let command = client
                        .send(RuntimeCommand::StopMotion(fixture_motion(
                            event["motionId"].as_str().expect("motion id"),
                        )))
                        .expect("stop fixture motion");
                    client
                        .wait_for_command(command, TIMEOUT)
                        .expect("stop command");
                }
                "expression_set" => {
                    let expression = match event["expressionId"].as_str().expect("expression id") {
                        "smile" => "live2d_expression1.exp3.json",
                        other => panic!("unsupported fixture expression {other}"),
                    };
                    let command = client
                        .send(RuntimeCommand::SetExpression(
                            ExpressionId::new(expression).expect("expression id"),
                        ))
                        .expect("set fixture expression");
                    client
                        .wait_for_command(command, TIMEOUT)
                        .expect("expression command");
                }
                "audio_trigger" => audio_triggers += 1,
                other => panic!("unexpected non-model event {other}"),
            }
            event_index += 1;
        }
        clock.set_ms(at_ms);
        let tick = client.send(RuntimeCommand::Tick).expect("fixture tick");
        let snapshot = client
            .wait_for_command(tick, TIMEOUT)
            .expect("tick snapshot");
        let model = snapshot.active_model.as_ref().expect("active model");
        let expected_model = &checkpoint["model"];
        let expected_id = match expected_model["selectedModelId"]
            .as_str()
            .expect("model id")
        {
            "fixture-model-a" => "standard",
            "fixture-model-b" => "keyboard",
            other => panic!("unsupported expected model {other}"),
        };
        assert_eq!(model.id.as_str(), expected_id, "model at {at_ms}ms");
        let expected_motion = expected_model["activeMotion"].as_str();
        assert_eq!(
            snapshot
                .active_motion
                .as_ref()
                .map(|active| active.motion.index()),
            expected_motion.map(|id| fixture_motion(id).index()),
            "motion at {at_ms}ms"
        );
        let expected_expression = expected_model["activeExpression"]
            .as_str()
            .map(|id| match id {
                "smile" => "live2d_expression1.exp3.json",
                other => panic!("unsupported expected expression {other}"),
            });
        assert_eq!(
            snapshot
                .active_expression
                .as_ref()
                .map(|active| active.expression.name()),
            expected_expression,
            "expression at {at_ms}ms"
        );
    }
    assert_eq!(audio_triggers, 1);
    assert!(client.snapshot().motion_audio.rejected_after_shutdown >= 1);
    owner.shutdown(TIMEOUT).expect("fixture runtime shutdown");
}

#[test]
fn shared_input_fixtures_match_product_runtime_projection() {
    let root = repository_root().join("shared/fixtures");
    let input_dir = root.join("input-sequences");
    let expected_dir = root.join("expected-state");
    for name in [
        "cursor-does-not-block-release",
        "gamepad-reconnect-reset",
        "input-recovery-lifecycle",
        "keyboard-modifiers-and-repeat",
        "keyboard-reconciled-release",
        "keyboard-single-key",
        "lifecycle-reset",
        "mouse-drag-and-cursor",
    ] {
        let sequence = load(input_dir.join(format!("{name}.json")));
        let expected = load(expected_dir.join(format!("{name}.json")));
        let mut key_bindings = BTreeMap::new();
        let mut gamepad_bindings = BTreeMap::new();
        for (name, side) in sequence["context"]["keySides"]
            .as_object()
            .expect("key sides")
        {
            let hand = match side.as_str().expect("hand side") {
                "left" => HandSide::Left,
                "right" => HandSide::Right,
                _ => panic!("unsupported hand side"),
            };
            if name.starts_with("Gamepad") {
                gamepad_bindings.insert(gamepad_button_key(name), hand);
            } else {
                key_bindings.insert(key(name), hand);
            }
        }
        let owner = RuntimeOwner::start(true, 64);
        let client = owner.client();
        client
            .wait_for_state(RuntimeState::Ready, TIMEOUT)
            .expect("runtime ready");
        client
            .send(RuntimeCommand::SetInputBindings(std::sync::Arc::new(
                InputBindings::with_gamepad_hands(key_bindings, gamepad_bindings.clone()),
            )))
            .expect("set fixture bindings");
        let mut event_index = 0;
        let mut ids = BTreeMap::new();
        for checkpoint in expected["checkpoints"].as_array().expect("checkpoints") {
            let at_ms = checkpoint["atMs"].as_u64().expect("checkpoint time");
            while event_index < sequence["events"].as_array().expect("events").len()
                && sequence["events"][event_index]["atMs"]
                    .as_u64()
                    .expect("event time")
                    <= at_ms
            {
                publish_event(&owner, &sequence["events"][event_index], &mut ids);
                event_index += 1;
            }
            let tick = client
                .send(RuntimeCommand::Tick)
                .expect("tick fixture runtime");
            let snapshot = client
                .wait_for_command(tick, TIMEOUT)
                .expect("tick snapshot");
            assert_checkpoint(&snapshot, checkpoint, &gamepad_bindings);
        }
        owner.shutdown(TIMEOUT).expect("fixture runtime shutdown");
    }
}
