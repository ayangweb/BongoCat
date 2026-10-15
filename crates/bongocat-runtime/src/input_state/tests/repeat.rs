//! The keyboard's own repeat, which is not a release.
//!
//! A held key sends a down edge for every repeat the platform generates. Each
//! one is counted as a duplicate, and none of them may change the pressed set:
//! the only things that clear a control are a release, a reconciliation and a
//! reset.

use super::*;

#[test]
fn a_repeated_down_edge_is_a_duplicate_and_keeps_the_key_pressed() {
    let mut state = InputState::default();
    state.apply(edge(0, 900, A, InputEdge::Down));
    state.apply(edge(1, 901, A, InputEdge::Down));
    state.apply(edge(2, 902, A, InputEdge::Down));

    let snapshot = state.snapshot();
    assert_eq!(
        snapshot.pressed_key_count, 1,
        "a repeat is not a second press and not a release"
    );
    assert_eq!(snapshot.diagnostics.captured_down, 1);
    assert_eq!(snapshot.diagnostics.duplicate_down, 2);
    assert_eq!(snapshot.diagnostics.captured_up, 0);

    state.apply(edge(3, 903, A, InputEdge::Up));
    let released = state.snapshot();
    assert_eq!(released.pressed_key_count, 0);
    assert_eq!(released.diagnostics.captured_up, 1);
    assert_eq!(released.diagnostics.unmatched_release, 0);
}

#[test]
fn a_release_with_nothing_pressed_is_counted_rather_than_ignored() {
    let mut state = InputState::default();
    state.apply(edge(0, 1, A, InputEdge::Up));

    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_key_count, 0);
    assert_eq!(snapshot.diagnostics.unmatched_release, 1);
    assert_eq!(snapshot.diagnostics.captured_up, 1);
}

#[test]
fn anonymous_device_edges_count_press_and_release_per_device() {
    let b = InputControl::Key(PhysicalKey::from_hid_usage(0x05));
    let mut state = InputState::default();
    state.apply(edge(0, 0, A, InputEdge::Down));
    // A repeat is the platform re-sending a press, not a second one.
    state.apply(edge(1, 1, A, InputEdge::Down));
    state.apply(edge(
        2,
        2,
        InputControl::Mouse(MouseButton::Left),
        InputEdge::Down,
    ));
    state.apply(edge(
        3,
        3,
        InputControl::Mouse(MouseButton::Left),
        InputEdge::Up,
    ));
    state.apply(edge(4, 4, A, InputEdge::Up));
    // B goes down and the platform never reports its release, which is exactly
    // the case the reconciliation exists for.
    state.apply(edge(5, 5, b, InputEdge::Down));
    state.apply(SequencedInputEvent {
        sequence: 6,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::new(),
            at: MonotonicMillis::new(6),
        },
    });
    state.apply(SequencedInputEvent {
        sequence: 7,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::new(),
            at: MonotonicMillis::new(7),
        },
    });
    let counted = state.snapshot().diagnostics;
    assert_eq!(
        counted.keyboard_activity, 2,
        "the repeat is not a second press, but the second key is"
    );
    assert_eq!(
        counted.keyboard_release, 1,
        "the release the runtime invented is not a device edge"
    );
    assert_eq!(counted.mouse_activity, 1);
    assert_eq!(counted.mouse_release, 1);
    assert_eq!(
        counted.reconciled_release, 1,
        "a reconciliation is the runtime's own correction, not a device edge"
    );
    state.apply(SequencedInputEvent {
        sequence: 8,
        event: InputEvent::Reset {
            reason: InputResetReason::SequenceGap,
            at: MonotonicMillis::new(8),
        },
    });
    state.apply(edge(9, 9, A, InputEdge::Down));
    let after_reset = state.snapshot().diagnostics;
    assert_eq!(after_reset.keyboard_activity, 3);
    assert_eq!(
        after_reset.keyboard_release, 1,
        "a reset clears the pressed set, and the release it invents is not a device edge"
    );
}
