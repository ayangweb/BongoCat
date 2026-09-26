//! A release that never arrives, and a stale one that must not apply.

use super::*;

#[test]
fn issue_47_lost_release_is_reconciled_without_clearing_held_keys_early() {
    let mut state = InputState::default();
    state.apply(edge(0, 0, CTRL, InputEdge::Down));
    state.apply(edge(1, 1, ALT, InputEdge::Down));
    state.apply(edge(2, 2, A, InputEdge::Down));
    state.apply(edge(3, 3, ALT, InputEdge::Up));
    state.apply(edge(4, 4, CTRL, InputEdge::Up));

    state.apply(SequencedInputEvent {
        sequence: 5,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::new(),
            at: MonotonicMillis::new(250),
        },
    });
    assert!(state.record(A).is_some());
    state.apply(SequencedInputEvent {
        sequence: 6,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::new(),
            at: MonotonicMillis::new(500),
        },
    });

    assert_eq!(state.snapshot().pressed_key_count, 0);
    assert_eq!(state.snapshot().diagnostics.reconciled_release, 1);
}

#[test]
fn sequence_gap_resets_unknown_state_before_current_edge() {
    let mut state = InputState::default();
    state.apply(edge(10, 0, CTRL, InputEdge::Down));
    assert_eq!(
        state.apply(edge(12, 1, A, InputEdge::Down)),
        InputDisposition::AppliedAfterSequenceGap { missing: 1 }
    );
    assert!(state.record(CTRL).is_none());
    assert!(state.record(A).is_some());
    assert_eq!(
        state.snapshot().last_reset_reason,
        Some(InputResetReason::SequenceGap)
    );
}

#[test]
fn input_sequence_tracker_handles_u64_wraparound() {
    let mut state = InputState::default();
    state.apply(edge(u64::MAX - 2, 0, A, InputEdge::Down));
    assert_eq!(
        state.apply(edge(u64::MAX, 1, A, InputEdge::Up)),
        InputDisposition::AppliedAfterSequenceGap { missing: 1 }
    );
    assert_eq!(
        state.apply(edge(0, 2, A, InputEdge::Down)),
        InputDisposition::Applied
    );
    assert_eq!(state.snapshot().pressed_key_count, 1);
    assert_eq!(state.snapshot().diagnostics.missing_sequence_count, 1);
    assert_eq!(
        state.apply(edge(u64::MAX, 3, A, InputEdge::Up)),
        InputDisposition::OutOfOrderSequence
    );
}

#[test]
fn duplicate_and_out_of_order_sequences_never_apply_release() {
    let mut state = InputState::default();
    state.apply(edge(4, 10, A, InputEdge::Down));
    assert_eq!(
        state.apply(edge(4, 11, A, InputEdge::Up)),
        InputDisposition::DuplicateSequence
    );
    assert_eq!(
        state.apply(edge(3, 12, A, InputEdge::Up)),
        InputDisposition::OutOfOrderSequence
    );
    assert!(state.record(A).is_some());
}

#[test]
fn non_monotonic_time_resets_pressed_state() {
    let mut state = InputState::default();
    state.apply(edge(0, 10, A, InputEdge::Down));
    assert_eq!(
        state.apply(edge(1, 9, CTRL, InputEdge::Down)),
        InputDisposition::ResetForNonMonotonicTime
    );
    assert_eq!(state.snapshot().pressed_key_count, 0);
    assert_eq!(
        state.snapshot().last_reset_reason,
        Some(InputResetReason::NonMonotonicTime)
    );
}

#[test]
fn pressed_record_retains_source_and_monotonic_times() {
    let mut state = InputState::default();
    state.apply(edge(0, 10, A, InputEdge::Down));
    state.apply(SequencedInputEvent {
        sequence: 1,
        event: InputEvent::Reconcile {
            pressed: BTreeSet::from([A]),
            at: MonotonicMillis::new(250),
        },
    });
    assert_eq!(
        state.record(A),
        Some(PressedRecord {
            source: InputSource::Capture,
            pressed_at: MonotonicMillis::new(10),
            last_reconciled_at: Some(MonotonicMillis::new(250)),
            runtime_observed_at: Duration::from_millis(10),
        })
    );
}

#[test]
fn reconciliation_cannot_synthesize_a_pressed_control() {
    let mut state = InputState::default();
    state.apply(SequencedInputEvent {
        sequence: 0,
        event: InputEvent::Edge {
            control: A,
            edge: InputEdge::Down,
            source: InputSource::Reconciliation,
            at: MonotonicMillis::new(0),
        },
    });
    assert_eq!(state.snapshot().pressed_key_count, 0);
    assert_eq!(state.snapshot().diagnostics.invalid_source, 1);
}
