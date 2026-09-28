//! A reset is complete, and a disconnect is scoped.

use super::*;

#[test]
fn reset_clears_keyboard_and_mouse_together() {
    let mut state = InputState::default();
    state.apply(edge(0, 0, A, InputEdge::Down));
    state.apply(edge(
        1,
        1,
        InputControl::Mouse(MouseButton::Left),
        InputEdge::Down,
    ));
    state.apply(SequencedInputEvent {
        sequence: 2,
        event: InputEvent::Reset {
            reason: InputResetReason::QueueOverflow,
            at: MonotonicMillis::new(2),
        },
    });
    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_key_count, 0);
    assert_eq!(snapshot.pressed_mouse_button_count, 0);
    assert_eq!(snapshot.diagnostics.released_by_reset, 2);
}

#[test]
fn disconnect_is_scoped_and_keyboard_reconciliation_ignores_gamepads() {
    let first = GamepadConnection {
        device_id: 0,
        generation: 1,
    };
    let second = GamepadConnection {
        device_id: 1,
        generation: 1,
    };
    let first_button = InputControl::Gamepad(GamepadButtonKey {
        connection: first,
        button: GamepadButton::South,
    });
    let second_button = InputControl::Gamepad(GamepadButtonKey {
        connection: second,
        button: GamepadButton::East,
    });
    let mut state = InputState::default();
    for (sequence, connection) in [(0, first), (1, second)] {
        state.apply(SequencedInputEvent {
            sequence,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(sequence),
            },
        });
    }
    state.apply(edge(2, 2, first_button, InputEdge::Down));
    state.apply(edge(3, 3, second_button, InputEdge::Down));
    state.apply(SequencedInputEvent {
        sequence: 4,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::new(),
            at: MonotonicMillis::new(4),
        },
    });
    state.apply(SequencedInputEvent {
        sequence: 5,
        event: InputEvent::GamepadDisconnected {
            connection: first,
            at: MonotonicMillis::new(5),
        },
    });

    let snapshot = state.snapshot();
    assert_eq!(snapshot.connected_gamepad_count, 1);
    assert_eq!(snapshot.pressed_gamepad_button_count, 1);
    assert!(state.record(first_button).is_none());
    assert!(state.record(second_button).is_some());
    assert_eq!(snapshot.diagnostics.released_by_disconnect, 1);
}
