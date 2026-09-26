//! What a model sees, and what it must never see.

use super::*;

#[test]
fn model_snapshot_applies_bindings_without_exposing_pressed_keys() {
    let right = PhysicalKey::from_hid_usage(0x4f);
    let bindings = InputBindings::new(BTreeMap::from([
        (PhysicalKey::KEY_A, HandSide::Left),
        (right, HandSide::Right),
    ]));
    let mut state = InputState::default();
    state.apply(edge(0, 0, A, InputEdge::Down));
    state.apply(edge(1, 1, InputControl::Key(right), InputEdge::Down));
    state.apply(edge(
        2,
        2,
        InputControl::Mouse(MouseButton::Left),
        InputEdge::Down,
    ));
    assert_eq!(
        state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
        ModelInputSnapshot {
            key_presses: {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress::keyboard(
                    PhysicalKey::KEY_A.hid_usage(),
                    KeySide::Left,
                ));
                presses.push(KeyPress::keyboard(right.hid_usage(), KeySide::Right));
                presses
            },
            left_hand_down: true,
            right_hand_down: true,
            mouse_left_down: true,
            mouse_right_down: false,
            ..ModelInputSnapshot::default()
        }
    );
    state.force_reset(InputResetReason::Test);
    assert_eq!(
        state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
        ModelInputSnapshot::default()
    );
}

#[test]
fn source_filters_keep_keyboard_and_gamepad_model_input_independent() {
    let connection = GamepadConnection {
        device_id: 3,
        generation: 1,
    };
    let gamepad = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::South,
    });
    let bindings = InputBindings::with_gamepad_hands(
        BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]),
        BTreeMap::from([(GamepadButton::South, HandSide::Right)]),
    );
    let mut state = InputState::default();
    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        },
    });
    state.apply(edge(1, 1, A, InputEdge::Down));
    state.apply(edge(2, 2, gamepad, InputEdge::Down));

    let keyboard_ignored = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            ignore_keyboard: true,
            ignore_gamepad: false,
        },
    );
    assert!(!keyboard_ignored.left_hand_down);
    assert!(keyboard_ignored.right_hand_down);
    assert_eq!(
        keyboard_ignored
            .key_presses
            .iter()
            .map(|press| (press.key, press.side))
            .collect::<Vec<_>>(),
        vec![(KeyIdentity::Gamepad(GamepadButton::South), KeySide::Right)],
        "the surviving source's own overlay, and only that one"
    );

    let gamepad_ignored = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            ignore_keyboard: false,
            ignore_gamepad: true,
        },
    );
    assert!(gamepad_ignored.left_hand_down);
    assert!(!gamepad_ignored.right_hand_down);
    assert_eq!(
        gamepad_ignored
            .key_presses
            .iter()
            .map(|press| (press.key, press.side))
            .collect::<Vec<_>>(),
        vec![(
            KeyIdentity::Keyboard(PhysicalKey::KEY_A.hid_usage()),
            KeySide::Left
        )],
        "the ignored source contributes no overlay of its own"
    );

    let all_ignored = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            ignore_keyboard: true,
            ignore_gamepad: true,
        },
    );
    assert_eq!(all_ignored, ModelInputSnapshot::default());
}

/// The globe key travels the same path as every other key.
///
/// Its usage is `0xff03` — Apple's vendor page folded into the same `u16` a
/// Keyboard/Keypad usage uses — so this pins that nothing on the way to the
/// model snapshot narrows it to a byte, indexes a table by it, or treats a
/// usage above `0x00ff` as "not a key". A press without a hand assignment is
/// dropped, so the binding is what makes it observable.
#[test]
fn the_globe_key_projects_through_the_model_snapshot_like_any_other_key() {
    assert_eq!(PhysicalKey::GLOBE.hid_usage(), 0xff03);
    let unbound = InputBindings::new(BTreeMap::new());
    let bindings = InputBindings::new(BTreeMap::from([(PhysicalKey::GLOBE, HandSide::Left)]));
    let mut state = InputState::default();
    state.apply(edge(
        0,
        0,
        InputControl::Key(PhysicalKey::GLOBE),
        InputEdge::Down,
    ));

    assert_eq!(
        state.model_snapshot(&unbound, NormalizedCursorPosition::default()),
        ModelInputSnapshot::default(),
        "an unbound globe key is inert, like any other unbound key"
    );
    assert_eq!(
        state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
        ModelInputSnapshot {
            key_presses: {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress::keyboard(
                    PhysicalKey::GLOBE.hid_usage(),
                    KeySide::Left,
                ));
                presses
            },
            left_hand_down: true,
            ..ModelInputSnapshot::default()
        }
    );

    state.apply(edge(
        1,
        1,
        InputControl::Key(PhysicalKey::GLOBE),
        InputEdge::Up,
    ));
    assert_eq!(
        state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
        ModelInputSnapshot::default(),
        "and it releases like any other key"
    );
}

#[test]
fn gamepad_button_edges_project_to_stick_parameters_and_reset_cleanly() {
    let connection = GamepadConnection {
        device_id: 2,
        generation: 7,
    };
    let left_stick = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::LeftStick,
    });
    let right_stick = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::RightStick,
    });
    let mut state = InputState::default();
    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        },
    });
    state.apply(edge(1, 1, left_stick, InputEdge::Down));
    state.apply(edge(2, 2, right_stick, InputEdge::Down));
    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_gamepad_button_count, 2);
    assert_eq!(snapshot.connected_gamepad_count, 1);
    assert_eq!(
        state.model_snapshot(
            &InputBindings::default(),
            NormalizedCursorPosition::default()
        ),
        ModelInputSnapshot {
            stick_left_down: true,
            stick_right_down: true,
            ..ModelInputSnapshot::default()
        }
    );

    state.force_reset(InputResetReason::DeviceRemoved);
    assert_eq!(state.snapshot().pressed_gamepad_button_count, 0);
    assert_eq!(state.snapshot().connected_gamepad_count, 0);
    assert_eq!(
        state.model_snapshot(
            &InputBindings::default(),
            NormalizedCursorPosition::default()
        ),
        ModelInputSnapshot::default()
    );
}

#[test]
fn reset_rejects_stale_gamepad_edges_until_a_new_generation_connects() {
    let first = GamepadConnection {
        device_id: 1,
        generation: 4,
    };
    let second = GamepadConnection {
        device_id: 1,
        generation: 5,
    };
    let button = |connection| {
        InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::South,
        })
    };
    let mut state = InputState::default();

    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::GamepadConnected {
            connection: first,
            at: MonotonicMillis::new(0),
        },
    });
    state.apply(edge(1, 1, button(first), InputEdge::Down));
    assert_eq!(state.snapshot().pressed_gamepad_button_count, 1);

    state.apply(SequencedInputEvent {
        sequence: 2,
        event: InputEvent::Reset {
            reason: InputResetReason::QueueOverflow,
            at: MonotonicMillis::new(2),
        },
    });
    state.apply(edge(3, 3, button(first), InputEdge::Down));
    assert_eq!(state.snapshot().pressed_gamepad_button_count, 0);
    assert_eq!(state.snapshot().diagnostics.stale_gamepad_events, 1);

    state.apply(SequencedInputEvent {
        sequence: 4,
        event: InputEvent::GamepadConnected {
            connection: second,
            at: MonotonicMillis::new(4),
        },
    });
    state.apply(edge(5, 5, button(second), InputEdge::Down));
    assert_eq!(state.snapshot().connected_gamepad_count, 1);
    assert_eq!(state.snapshot().pressed_gamepad_button_count, 1);
}

/// A bound gamepad button projects both halves of the reaction the keyboard
/// path already had: the paw and the button's own overlay. The overlay is
/// the half that was missing — a gamepad press used to reach the renderer as
/// a bare HID usage, which no gamepad button can be, so the model moved a paw
/// for every button and showed the pressed button for none of them.
#[test]
fn configured_gamepad_buttons_project_to_the_bound_hand_and_its_overlay() {
    let connection = GamepadConnection {
        device_id: 4,
        generation: 2,
    };
    let south = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::South,
    });
    let east = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::East,
    });
    let bindings = InputBindings::with_gamepad_hands(
        BTreeMap::new(),
        BTreeMap::from([
            (GamepadButton::South, HandSide::Left),
            (GamepadButton::East, HandSide::Right),
        ]),
    );
    let overlays = |state: &InputState| {
        state
            .model_snapshot(&bindings, NormalizedCursorPosition::default())
            .key_presses
            .iter()
            .map(|press| (press.key, press.side))
            .collect::<Vec<_>>()
    };
    let mut state = InputState::default();
    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        },
    });
    state.apply(edge(1, 1, south, InputEdge::Down));
    assert_eq!(
        state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
        ModelInputSnapshot {
            key_presses: {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress::gamepad(GamepadButton::South, KeySide::Left));
                presses
            },
            left_hand_down: true,
            ..ModelInputSnapshot::default()
        }
    );
    state.apply(edge(2, 2, east, InputEdge::Down));
    assert!(
        state
            .model_snapshot(&bindings, NormalizedCursorPosition::default())
            .right_hand_down
    );
    assert_eq!(
        overlays(&state),
        vec![
            (KeyIdentity::Gamepad(GamepadButton::South), KeySide::Left),
            (KeyIdentity::Gamepad(GamepadButton::East), KeySide::Right),
        ],
        "one overlay per hand, each the button that hand last saw pressed"
    );
    state.apply(edge(3, 3, south, InputEdge::Up));
    let after_release = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
    assert!(!after_release.left_hand_down);
    assert!(after_release.right_hand_down);
    assert_eq!(
        overlays(&state),
        vec![(KeyIdentity::Gamepad(GamepadButton::East), KeySide::Right)],
        "releasing one button leaves the other hand's overlay alone"
    );
}

/// A gamepad button the model has no artwork for must be inert, exactly like
/// an unbound key (ADR-0042), and the two stick buttons must keep their own
/// parameters whether or not they are bound.
#[test]
fn an_unbound_gamepad_button_is_inert_and_the_sticks_keep_their_parameters() {
    let connection = GamepadConnection {
        device_id: 5,
        generation: 1,
    };
    let control = |button| InputControl::Gamepad(GamepadButtonKey { connection, button });
    let bindings = InputBindings::with_gamepad_hands(
        BTreeMap::new(),
        BTreeMap::from([(GamepadButton::Start, HandSide::Right)]),
    );
    let mut state = InputState::default();
    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        },
    });
    state.apply(edge(1, 1, control(GamepadButton::Select), InputEdge::Down));
    let unbound = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
    assert!(!unbound.left_hand_down);
    assert!(!unbound.right_hand_down);
    assert_eq!(unbound.key_presses.iter().count(), 0);

    state.apply(edge(
        2,
        2,
        control(GamepadButton::LeftStick),
        InputEdge::Down,
    ));
    let stick = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
    assert!(
        stick.stick_left_down,
        "a stick press drives the stick artwork, not a paw"
    );
    assert_eq!(stick.key_presses.iter().count(), 0);

    state.apply(edge(3, 3, control(GamepadButton::Start), InputEdge::Down));
    let bound = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
    assert!(bound.right_hand_down);
    assert_eq!(
        bound
            .key_presses
            .iter()
            .map(|press| (press.key, press.side))
            .collect::<Vec<_>>(),
        vec![(KeyIdentity::Gamepad(GamepadButton::Start), KeySide::Right)]
    );
}
