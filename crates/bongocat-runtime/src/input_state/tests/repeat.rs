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
