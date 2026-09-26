//! The keyboard's own repeat, which is not a release.

use super::*;

#[test]
fn keyboard_fallback_expires_at_deadline_and_repeat_refreshes_it() {
    let mut state = InputState::default();
    state.apply_observed(edge(0, 900, A, InputEdge::Down), Duration::from_millis(10));
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_millis(509), 500),
        0
    );
    state.apply_observed(edge(1, 901, A, InputEdge::Down), Duration::from_millis(400));
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_millis(899), 500),
        0
    );
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_millis(900), 500),
        1
    );
    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_key_count, 0);
    assert_eq!(snapshot.diagnostics.fallback_release, 1);
    assert_eq!(snapshot.diagnostics.duplicate_down, 1);
}

#[test]
fn keyboard_fallback_zero_and_non_monotonic_clock_do_not_release() {
    let mut state = InputState::default();
    state.apply_observed(edge(0, 1, A, InputEdge::Down), Duration::from_secs(5));
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_secs(60), 0),
        0
    );
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_secs(4), 500),
        0
    );
    assert_eq!(state.snapshot().pressed_key_count, 1);
}

#[test]
fn keyboard_fallback_never_expires_mouse_or_gamepad_controls() {
    let connection = GamepadConnection {
        device_id: 0,
        generation: 1,
    };
    let gamepad = InputControl::Gamepad(GamepadButtonKey {
        connection,
        button: GamepadButton::South,
    });
    let mut state = InputState::default();
    state.apply_observed(
        SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            },
        },
        Duration::ZERO,
    );
    state.apply_observed(
        edge(
            1,
            1,
            InputControl::Mouse(MouseButton::Left),
            InputEdge::Down,
        ),
        Duration::ZERO,
    );
    state.apply_observed(edge(2, 2, gamepad, InputEdge::Down), Duration::ZERO);
    assert_eq!(
        state.expire_keyboard_fallback(Duration::from_secs(60), 1),
        0
    );
    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_mouse_button_count, 1);
    assert_eq!(snapshot.pressed_gamepad_button_count, 1);
}
