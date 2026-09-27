//! A table change re-arms exactly the bindings it unregistered.

use super::*;

#[test]
fn a_table_change_re_arms_bindings_it_unregistered() {
    let mut pressed = PressEdges::default();
    assert!(pressed.observe(HotKeyState::Pressed, 7));
    // The chord leaves the table while it is still held, so no release
    // ever arrives for it.
    pressed.retain(&BTreeSet::from([9]));
    assert!(
        pressed.observe(HotKeyState::Pressed, 7),
        "a chord that left the table is armed again"
    );
    // A change that keeps the id retains the held state.
    pressed.retain(&BTreeSet::from([7]));
    assert!(!pressed.observe(HotKeyState::Pressed, 7));
}

#[test]
fn a_model_switch_hands_the_shared_chords_to_the_incoming_model() {
    // Both models count their behavior chords from the primary
    // modifier's first digit, so the live model binds the same chords
    // before and after a switch and only the model behind them changes.
    let open_settings = command("Meta+O", ShortcutCommand::OpenSettings);
    let outgoing = behavior("Control+1", "standard");
    let incoming = behavior("Control+1", "keyboard");
    assert_eq!(outgoing.hotkey.id, incoming.hotkey.id);
    let registrar = FakeRegistrar::default();
    let mut registered = HashMap::new();
    mirror(
        &registrar,
        &mut registered,
        &[open_settings.clone(), outgoing],
    );

    mirror(
        &registrar,
        &mut registered,
        &[open_settings.clone(), incoming.clone()],
    );

    assert_eq!(
        registered
            .get(&incoming.hotkey.id)
            .map(|entry| &entry.target),
        Some(&incoming.target),
        "the incoming model must answer the chord it shares with the outgoing one"
    );
    assert_eq!(
        registrar.bound.borrow().len(),
        2,
        "the shared chord stayed registered instead of being registered twice"
    );
}

#[test]
fn a_chord_that_left_the_table_is_registered_again_when_it_returns() {
    let standard = behavior("Control+1", "standard");
    let keyboard = behavior("Control+2", "keyboard");
    let registrar = FakeRegistrar::default();
    let mut registered = HashMap::new();
    mirror(&registrar, &mut registered, std::slice::from_ref(&standard));
    // The switch to the other model drops the chord entirely.
    mirror(&registrar, &mut registered, std::slice::from_ref(&keyboard));
    assert!(!registered.contains_key(&standard.hotkey.id));

    // Switching back binds the same chord again.
    mirror(&registrar, &mut registered, std::slice::from_ref(&standard));

    assert_eq!(
        registered
            .get(&standard.hotkey.id)
            .map(|entry| &entry.target),
        Some(&standard.target),
        "a chord that re-enters the table must be registered again"
    );
    assert!(registrar.bound.borrow().contains(&standard.hotkey.id));
}

#[test]
fn an_unchanged_table_leaves_the_registrations_alone() {
    let desired = vec![
        command("Meta+O", ShortcutCommand::OpenSettings),
        behavior("Control+1", "standard"),
    ];
    let registrar = FakeRegistrar::default();
    let mut registered = HashMap::new();
    mirror(&registrar, &mut registered, &desired);
    mirror(&registrar, &mut registered, &desired);

    assert_eq!(registered.len(), desired.len());
    assert_eq!(registrar.bound.borrow().len(), desired.len());
}
