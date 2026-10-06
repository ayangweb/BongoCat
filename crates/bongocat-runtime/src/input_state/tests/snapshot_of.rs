//! What a model sees, and what it must never see.

use super::*;

/// The modifier projection answers per physical key, and nothing else does.
///
/// The counts beside it cannot say which shift is down, and the model projection
/// cannot either: it drops any key the current model's artwork cannot draw, and
/// modifiers are exactly the keys a model leaves out. So this is the only place
/// the overlay can learn that the key the user configured is being held.
#[test]
fn the_pressed_modifier_projection_keeps_the_two_sides_apart() {
    let mut state = InputState::default();
    assert!(
        state.snapshot().pressed_modifiers.is_empty(),
        "an empty pressed set holds no modifier"
    );

    let right_shift = InputControl::Key(ModifierKey::RightShift.physical_key());
    state.apply(edge(0, 0, right_shift, InputEdge::Down));
    let held = state.snapshot().pressed_modifiers;
    assert!(held.holds(ModifierKey::RightShift));
    assert!(
        !held.holds(ModifierKey::LeftShift),
        "the right shift must not answer for the left one"
    );
    assert_eq!(state.snapshot().pressed_key_count, 1);

    // A key that is not a modifier contributes to the count and nothing else.
    state.apply(edge(1, 1, A, InputEdge::Down));
    assert_eq!(state.snapshot().pressed_key_count, 2);
    assert_eq!(state.snapshot().pressed_modifiers, held);

    // Both sides at once are two keys, and the projection says so rather than
    // collapsing them the way a modifier bitmask would.
    state.apply(edge(
        2,
        2,
        InputControl::Key(ModifierKey::LeftShift.physical_key()),
        InputEdge::Down,
    ));
    let both = state.snapshot().pressed_modifiers;
    assert!(both.holds(ModifierKey::LeftShift) && both.holds(ModifierKey::RightShift));
    assert_eq!(
        both.iter().collect::<Vec<_>>(),
        vec![ModifierKey::LeftShift, ModifierKey::RightShift],
        "read back in keyboard order, so a two-modifier hold resolves the same way every time"
    );

    state.apply(edge(3, 3, right_shift, InputEdge::Up));
    let after_release = state.snapshot().pressed_modifiers;
    assert!(
        !after_release.holds(ModifierKey::RightShift),
        "a release ends that side's hold, which is what makes it a hold"
    );
    assert!(
        after_release.holds(ModifierKey::LeftShift),
        "and leaves the other side alone"
    );
    assert_eq!(state.snapshot().pressed_key_count, 2);

    state.apply(edge(
        4,
        4,
        InputControl::Key(ModifierKey::LeftShift.physical_key()),
        InputEdge::Up,
    ));
    assert_eq!(state.snapshot().pressed_modifiers, PressedModifiers::NONE);
    assert_eq!(state.snapshot().pressed_key_count, 1);
}

/// A reset has to clear the projection for the same reason it clears the pressed
/// set: a modifier left "held" after a lock, a sleep or a device removal would
/// keep the overlay reachable with no key under the user's fingers.
#[test]
fn a_reset_clears_the_pressed_modifier_projection() {
    let mut state = InputState::default();
    state.apply(edge(0, 0, ALT, InputEdge::Down));
    assert!(
        state
            .snapshot()
            .pressed_modifiers
            .holds(ModifierKey::LeftAlt)
    );

    state.force_reset(InputResetReason::SessionLock);
    assert_eq!(state.snapshot().pressed_modifiers, PressedModifiers::NONE);
}

/// The two documented routes to "the configured key is not held" both have to
/// leave the projection empty: a release that never arrived, and a reconcile that
/// confirms the key is gone. This is the issue #47 shape applied to the hold.
#[test]
fn a_reconciled_release_ends_the_modifier_hold() {
    let mut state = InputState::default();
    state.apply(edge(0, 0, CTRL, InputEdge::Down));
    let mut confirmations = 0_u8;
    while state
        .snapshot()
        .pressed_modifiers
        .holds(ModifierKey::LeftControl)
    {
        state.apply(SequencedInputEvent {
            sequence: u64::from(confirmations) + 1,
            event: InputEvent::Reconcile {
                pressed: BTreeSet::new(),
                at: MonotonicMillis::new(u64::from(confirmations) + 1),
            },
        });
        confirmations += 1;
        assert!(
            confirmations < 8,
            "a reconcile that never clears the hold would strand the overlay"
        );
    }
    assert!(
        confirmations > 1,
        "the policy asks for more than one confirmation, so one missing release does not end the hold"
    );
    assert!(state.snapshot().diagnostics.reconciled_release > 0);
}

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
            show_all_pressed_keys: false,
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
            show_all_pressed_keys: false,
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
            show_all_pressed_keys: false,
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

/// Every held key keeps its own overlay, stacked oldest first so the key pressed
/// last is the one drawn on top.
///
/// This is the mode issue #965 asks for: without it a chord collapses into the
/// one picture its hand last saw, and a viewer cannot tell a fast four-key roll
/// from a single tap.
#[test]
fn the_stacked_mode_keeps_every_held_key_oldest_first() {
    let right = PhysicalKey::from_hid_usage(0x4f);
    let d = PhysicalKey::from_hid_usage(0x07);
    let bindings = InputBindings::new(BTreeMap::from([
        (PhysicalKey::KEY_A, HandSide::Left),
        (d, HandSide::Left),
        (right, HandSide::Right),
    ]));
    let mut state = InputState::default();
    // `A` and `D` share a millisecond on purpose: the clock has millisecond
    // resolution, so a chord can and does land inside one, and the sequence of
    // the two edges is the only thing that tells them apart.
    state.apply(edge(0, 0, A, InputEdge::Down));
    state.apply(edge(1, 0, InputControl::Key(d), InputEdge::Down));
    state.apply(edge(2, 4, InputControl::Key(right), InputEdge::Down));

    let stacked = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            show_all_pressed_keys: true,
            ..ModelInputFilter::default()
        },
    );
    assert_eq!(
        stacked
            .key_presses
            .iter()
            .map(|press| (press.key, press.side))
            .collect::<Vec<_>>(),
        vec![
            (
                KeyIdentity::Keyboard(PhysicalKey::KEY_A.hid_usage()),
                KeySide::Left
            ),
            (KeyIdentity::Keyboard(d.hid_usage()), KeySide::Left),
            (KeyIdentity::Keyboard(right.hid_usage()), KeySide::Right),
        ],
        "both left-hand keys survive, in the order they were pressed, and the renderer draws the last one on top"
    );
    assert!(stacked.left_hand_down && stacked.right_hand_down);
    assert_eq!(
        state
            .model_snapshot(&bindings, NormalizedCursorPosition::default())
            .key_presses
            .iter()
            .count(),
        2,
        "the compatibility mode still collapses a hand to the key it last saw"
    );

    // Releasing the newest key must leave the older picture underneath it alone
    // rather than promoting a substitute the user is not holding.
    state.apply(edge(3, 8, InputControl::Key(right), InputEdge::Up));
    let after_release = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            show_all_pressed_keys: true,
            ..ModelInputFilter::default()
        },
    );
    assert_eq!(
        after_release
            .key_presses
            .iter()
            .map(|press| press.key)
            .collect::<Vec<_>>(),
        vec![
            KeyIdentity::Keyboard(PhysicalKey::KEY_A.hid_usage()),
            KeyIdentity::Keyboard(d.hid_usage()),
        ]
    );
    assert!(after_release.left_hand_down && !after_release.right_hand_down);
}

/// The paw parameters must not follow the overlay count.
///
/// A hand is down while *any* key bound to it is held, so turning the stack on
/// cannot make the paw depend on how many pictures happen to be visible.
#[test]
fn the_stacked_mode_does_not_change_which_hands_are_down() {
    let d = PhysicalKey::from_hid_usage(0x07);
    let bindings = InputBindings::new(BTreeMap::from([
        (PhysicalKey::KEY_A, HandSide::Left),
        (d, HandSide::Left),
    ]));
    let mut state = InputState::default();
    state.apply(edge(0, 0, A, InputEdge::Down));
    let compatibility = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter::default(),
    );
    state.apply(edge(1, 3, InputControl::Key(d), InputEdge::Down));
    let stacked = state.model_snapshot_with_filter(
        &bindings,
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            show_all_pressed_keys: true,
            ..ModelInputFilter::default()
        },
    );
    assert_eq!(
        (stacked.left_hand_down, stacked.right_hand_down),
        (compatibility.left_hand_down, compatibility.right_hand_down)
    );
}

/// The key-image layer is bounded, and a device holding more keys than it can
/// draw loses the oldest ones rather than the ones in the user's hands.
#[test]
fn the_stacked_mode_drops_the_oldest_presses_past_the_layer_capacity() {
    let mut bindings = BTreeMap::new();
    let mut expected = Vec::new();
    let mut state = InputState::default();
    for index in 0..=KeyPressSet::CAPACITY {
        let usage = 0x04 + u16::try_from(index).expect("usage offset fits a keyboard page");
        let key = PhysicalKey::from_hid_usage(usage);
        bindings.insert(key, HandSide::Left);
        expected.push(KeyIdentity::Keyboard(usage));
        state.apply(edge(
            index as u64,
            index as u64,
            InputControl::Key(key),
            InputEdge::Down,
        ));
    }
    let stacked = state.model_snapshot_with_filter(
        &InputBindings::new(bindings),
        NormalizedCursorPosition::default(),
        ModelInputFilter {
            show_all_pressed_keys: true,
            ..ModelInputFilter::default()
        },
    );
    assert_eq!(stacked.key_presses.iter().count(), KeyPressSet::CAPACITY);
    assert_eq!(
        stacked
            .key_presses
            .iter()
            .map(|press| press.key)
            .collect::<Vec<_>>(),
        expected[expected.len() - KeyPressSet::CAPACITY..],
        "the oldest press is the one that falls off the bottom of the stack"
    );
}
