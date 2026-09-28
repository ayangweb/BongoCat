//! One press is one event, however many times the platform repeats it.

use super::*;

#[test]
fn repeated_pressed_events_for_a_held_chord_dispatch_their_target_once() {
    let registration = command("Control+B", ShortcutCommand::ToggleOverlay);
    let registered = HashMap::from([(registration.hotkey.id, registration.clone())]);
    let mut pressed = PressEdges::default();
    let event = |state| GlobalHotKeyEvent {
        id: registration.hotkey.id,
        state,
    };
    assert_eq!(
        resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered).cloned(),
        Some(registration.clone())
    );
    for _ in 0..5 {
        assert_eq!(
            resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered),
            None,
            "an OS key repeat must not dispatch the target again"
        );
    }
    assert_eq!(
        resolve_trigger(&mut pressed, event(HotKeyState::Released), &registered),
        None
    );
    assert_eq!(
        resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered).cloned(),
        Some(registration.clone())
    );
    // A chord that is no longer bound reports no release, so its events
    // must not leave held state behind either.
    assert_eq!(
        resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &HashMap::new()),
        None
    );
    assert_eq!(
        resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered),
        None,
        "the still-held chord stays silent after an unbound event"
    );
}

#[test]
fn one_hold_dispatches_a_single_press_edge_however_many_repeats_arrive() {
    let mut pressed = PressEdges::default();
    assert!(pressed.observe(HotKeyState::Pressed, 7));
    for _ in 0..5 {
        assert!(
            !pressed.observe(HotKeyState::Pressed, 7),
            "an OS key repeat must not dispatch a second trigger"
        );
    }
    assert!(!pressed.observe(HotKeyState::Released, 7));
    assert!(
        pressed.observe(HotKeyState::Pressed, 7),
        "the next physical press dispatches again"
    );
}

#[test]
fn releases_and_idle_bindings_do_not_interfere() {
    let mut pressed = PressEdges::default();
    assert!(!pressed.observe(HotKeyState::Released, 7));
    assert!(pressed.observe(HotKeyState::Pressed, 7));
    assert!(pressed.observe(HotKeyState::Pressed, 9));
    assert!(!pressed.observe(HotKeyState::Pressed, 9));
    assert!(!pressed.observe(HotKeyState::Pressed, 7));
    assert!(!pressed.observe(HotKeyState::Released, 9));
    assert!(pressed.observe(HotKeyState::Pressed, 9));
}
