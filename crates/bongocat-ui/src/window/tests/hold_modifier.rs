//! Recording the one modifier key the overlay watches for.
//!
//! The recorder's whole difficulty is that it cannot read the keyboard, so these
//! tests drive it the way the product does: through the typed read of the
//! runtime's pressed set, with the answer arriving on the settings channel.

use super::super::hold_modifier::{RECORDER_POLL_INTERVAL, hold_modifier_row_description};
use super::*;
use crate::PressedModifiers;

/// Move the simulated clock past one recorder poll.
///
/// The recorder waits on a background timer, so a test that only runs the
/// executor to a standstill would never see it look at the pressed set.
fn advance_recorder_clock(visual: &mut VisualTestContext) {
    visual.update(|_, cx| {
        cx.background_executor()
            .advance_clock(RECORDER_POLL_INTERVAL)
    });
    visual.run_until_parked();
}

fn overlay_snapshot(revision: u64) -> SettingsSnapshot {
    let mut snapshot = crate::tests::snapshot(revision, false, true);
    snapshot.config_revision = Some(revision);
    snapshot.overlay = SettingsOverlay {
        click_through: true,
        hide_on_pointer_hover: true,
        ..SettingsOverlay::default()
    };
    snapshot
}

/// Give the view a snapshot and the shown state a real window has.
///
/// A newly created settings window is hidden until `reopen`, and a recorder on a
/// hidden window has nothing to record into — the same reason the snapshot poll
/// refuses to run there.
fn show_overlay_page(view: &Entity<SettingsView>, visual: &mut VisualTestContext, revision: u64) {
    view.update(visual, |view, _| {
        view.snapshot = Some(overlay_snapshot(revision));
        view.window_hidden = false;
    });
    visual.run_until_parked();
}

/// The label has to name the side, because the value has one — and it has to
/// name it *before* the key, as a word in the interface language, so `⇧L` cannot
/// be misread as a variant of `⇧`.
///
/// macOS gets the platform's own modifier symbols — the same table the shortcut
/// display reads, so `左 Shift` and a recorded `Control+Shift+A` cannot disagree
/// about what a shift looks like — and every other platform spells the key out.
#[test]
fn the_recorder_names_the_side_of_the_key_it_stores() {
    let chinese = SettingsLanguage::ChineseSimplified;
    let macos = [
        (ModifierKey::LeftControl, "左⌃"),
        (ModifierKey::RightControl, "右⌃"),
        (ModifierKey::LeftShift, "左⇧"),
        (ModifierKey::RightShift, "右⇧"),
        (ModifierKey::LeftAlt, "左⌥"),
        (ModifierKey::RightAlt, "右⌥"),
        (ModifierKey::LeftMeta, "左⌘"),
        (ModifierKey::RightMeta, "右⌘"),
    ];
    for (modifier, expected) in macos {
        assert_eq!(
            format_modifier_key_display(modifier, true, chinese),
            expected
        );
    }

    let spelled_out = [
        (ModifierKey::LeftControl, "左 Control"),
        (ModifierKey::RightControl, "右 Control"),
        (ModifierKey::LeftShift, "左 Shift"),
        (ModifierKey::RightShift, "右 Shift"),
        (ModifierKey::LeftAlt, "左 Alt"),
        (ModifierKey::RightAlt, "右 Alt"),
        (ModifierKey::LeftMeta, "左 Meta"),
        (ModifierKey::RightMeta, "右 Meta"),
    ];
    for (modifier, expected) in spelled_out {
        assert_eq!(
            format_modifier_key_display(modifier, false, chinese),
            expected
        );
    }

    // Every modifier renders differently from every other one, so no two of the
    // eight settings are indistinguishable on the page.
    let mut rendered = spelled_out
        .iter()
        .map(|(modifier, _)| format_modifier_key_display(*modifier, false, chinese))
        .collect::<Vec<_>>();
    assert_eq!(rendered.len(), ModifierKey::ALL.len());
    rendered.sort();
    rendered.dedup();
    assert_eq!(rendered.len(), ModifierKey::ALL.len());
}

/// The description has to carry what the title cannot.
///
/// ADR-0066 permits a row description only where the title and the control leave
/// the behaviour unclear, so this one earns its place by naming the two settings
/// it suspends. Those two switches are the two rows directly above it, so quoting
/// their titles is what connects the sentence to something on screen; describing
/// the effect instead leaves the user to map it back themselves.
///
/// Comparing against the catalog rather than a literal also catches drift in the
/// other direction: renaming a setting without updating this description fails
/// here instead of leaving a row that names a switch the page no longer has.
#[test]
fn the_recorder_description_names_both_settings_it_suspends() {
    for language in SettingsLanguage::ALL {
        let locale = language.catalog_locale();
        let description = hold_modifier_row_description(language);
        assert!(
            !description.trim().is_empty(),
            "{language:?} has no description"
        );
        assert!(
            !description.contains('{') && !description.contains('}'),
            "{language:?} description has an unresolved placeholder: {description}"
        );
        for key in [
            "settings.overlay.hide_on_mouse_hover.label",
            "settings.overlay.click_through.label",
        ] {
            let title = bongocat_i18n::text(locale, key);
            assert!(
                description.contains(title),
                "{language:?} description does not name {key} ({title:?}): {description}"
            );
        }
    }
}

/// The side word is the row's own copy, so it follows the interface language
/// rather than being spelled the same way everywhere. A single letter would be
/// shorter, but it would also be the one part of a keycap no one can read.
#[test]
fn the_side_word_follows_the_interface_language() {
    let expected = [
        (SettingsLanguage::English, "Left", "Right"),
        (SettingsLanguage::ChineseSimplified, "左", "右"),
        (SettingsLanguage::ChineseTraditional, "左", "右"),
    ];
    for (language, left, right) in expected {
        assert_eq!(
            format_modifier_key_display(ModifierKey::LeftShift, false, language),
            format!("{left} Shift")
        );
        assert_eq!(
            format_modifier_key_display(ModifierKey::RightShift, false, language),
            format!("{right} Shift")
        );
    }

    // Every shipped language has to name both sides, and neither may be empty or
    // a placeholder — a missing side word would render a bare `⇧`, which is
    // exactly the label that cannot tell the two keys apart.
    for language in SettingsLanguage::ALL {
        for side in [
            ModifierKey::LeftShift,
            ModifierKey::RightShift,
            ModifierKey::LeftControl,
            ModifierKey::RightControl,
            ModifierKey::LeftAlt,
            ModifierKey::RightAlt,
            ModifierKey::LeftMeta,
            ModifierKey::RightMeta,
        ] {
            for macos in [true, false] {
                let rendered = format_modifier_key_display(side, macos, language);
                assert!(
                    !rendered.trim().is_empty(),
                    "{language:?} renders an empty label for {side:?}"
                );
                assert!(
                    !rendered.contains('{') && !rendered.contains('}'),
                    "{language:?} renders an unresolved placeholder for {side:?}: {rendered}"
                );
            }
        }
    }
}

/// The row's three states are the shortcut recorder's three states: recording,
/// recorded, and empty. All three need a name in every shipped language.
#[test]
fn the_recorder_copy_is_named_in_every_shipped_language() {
    for language in SettingsLanguage::ALL {
        for key in [
            "settings.overlay.hold_modifier_to_interact.label",
            "settings.overlay.hold_modifier_to_interact.recording",
            "settings.overlay.hold_modifier_to_interact.record_placeholder",
            "settings.overlay.hold_modifier_to_interact.side_left",
            "settings.overlay.hold_modifier_to_interact.side_right",
            "settings.overlay.hold_modifier_to_interact.description",
        ] {
            let text = bongocat_i18n::text(language.catalog_locale(), key);
            assert!(!text.trim().is_empty(), "{language:?} is missing {key}");
        }
    }
}

/// The recorder's defining property: it records a key that is being held, and it
/// reads the runtime's pressed set to learn which one.
#[gpui_kit::test]
fn a_held_modifier_is_recorded_as_the_physical_key_it_is(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 4);

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_hold_modifier_recording(window, cx)
        });
    });
    assert!(view.read_with(visual, |view, _| view.hold_modifier_recording));
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "arming the recorder must not write a configuration"
    );

    // The recorder's only input is the typed read of the pressed set.
    advance_recorder_clock(visual);
    visual.run_until_parked();
    let crate::SettingsCommand::ReadPressedModifiers { reply } = endpoint
        .try_recv()
        .expect("the recorder must ask which modifiers are held")
    else {
        panic!("the recorder must read the runtime's pressed modifiers");
    };

    let mut pressed = PressedModifiers::NONE;
    pressed.insert(ModifierKey::RightShift);
    reply.respond(Ok(pressed)).expect("pressed modifier reply");
    visual.run_until_parked();

    let crate::SettingsCommand::SetOverlaySettings {
        expected_config_revision,
        settings,
        reply,
    } = endpoint
        .try_recv()
        .expect("the recorded modifier must be persisted")
    else {
        panic!("recording must write the overlay settings");
    };
    assert_eq!(expected_config_revision, 4);
    assert_eq!(
        settings.hold_modifier_to_interact,
        Some(ModifierKey::RightShift)
    );
    assert!(
        settings.click_through && settings.hide_on_pointer_hover,
        "the write must carry the rest of the overlay settings, not reset them"
    );

    let mut confirmed = overlay_snapshot(5);
    confirmed.overlay = settings;
    reply
        .respond(Ok(confirmed))
        .expect("overlay settings reply");
    visual.run_until_parked();
    assert!(
        !view.read_with(visual, |view, _| view.hold_modifier_recording),
        "a recorded modifier ends the recording"
    );

    // And the poll stops with it. A recorder that kept listening would overwrite
    // the value with whatever the user pressed next.
    advance_recorder_clock(visual);
    visual.run_until_parked();
    assert!(endpoint.try_recv().is_err());
}

/// Two modifiers held at once still store one key, and it is the leftmost rather
/// than whichever edge the observer happened to see first.
#[gpui_kit::test]
fn two_held_modifiers_resolve_to_the_same_key_every_time(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 1);

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_hold_modifier_recording(window, cx)
        });
    });
    advance_recorder_clock(visual);
    visual.run_until_parked();
    let crate::SettingsCommand::ReadPressedModifiers { reply } = endpoint
        .try_recv()
        .expect("the recorder must ask which modifiers are held")
    else {
        panic!("the recorder must read the runtime's pressed modifiers");
    };

    let mut pressed = PressedModifiers::NONE;
    pressed.insert(ModifierKey::RightShift);
    pressed.insert(ModifierKey::LeftControl);
    reply.respond(Ok(pressed)).expect("pressed modifier reply");
    visual.run_until_parked();

    let crate::SettingsCommand::SetOverlaySettings { settings, .. } = endpoint
        .try_recv()
        .expect("the recorded modifier must be persisted")
    else {
        panic!("recording must write the overlay settings");
    };
    assert_eq!(
        settings.hold_modifier_to_interact,
        Some(ModifierKey::LeftControl)
    );
}

/// A cancelled recording writes nothing, and the poll that belonged to it cannot
/// write afterwards either.
///
/// The second half is the part a click-to-cancel cannot cover on its own: the
/// poll may already be waiting on the channel when the user gives up, and a
/// modifier pressed a moment later would otherwise be recorded by a session that
/// no longer exists.
#[gpui_kit::test]
fn a_cancelled_recording_writes_nothing_even_if_a_key_is_held_afterwards(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 2);

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_hold_modifier_recording(window, cx)
        });
    });
    advance_recorder_clock(visual);
    visual.run_until_parked();
    let crate::SettingsCommand::ReadPressedModifiers { reply } = endpoint
        .try_recv()
        .expect("the recorder must ask which modifiers are held")
    else {
        panic!("the recorder must read the runtime's pressed modifiers");
    };

    view.update(visual, |view, cx| view.cancel_hold_modifier_recording(cx));
    assert!(!view.read_with(visual, |view, _| view.hold_modifier_recording));

    let mut pressed = PressedModifiers::NONE;
    pressed.insert(ModifierKey::LeftAlt);
    reply.respond(Ok(pressed)).expect("pressed modifier reply");
    visual.run_until_parked();

    assert!(
        endpoint.try_recv().is_err(),
        "a recording that was cancelled must not write the value"
    );
}

/// Clearing is a write of the shipped default rather than a second code path, and
/// it asks for nothing when there is nothing to clear.
#[gpui_kit::test]
fn clearing_writes_the_default_and_an_empty_setting_asks_for_nothing(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 6);
    view.update(visual, |view, _| {
        view.snapshot
            .as_mut()
            .expect("the shown page has a snapshot")
            .overlay
            .hold_modifier_to_interact = Some(ModifierKey::LeftMeta);
    });

    view.update(visual, |view, cx| view.clear_hold_modifier(cx));
    visual.run_until_parked();
    let crate::SettingsCommand::SetOverlaySettings {
        expected_config_revision,
        settings,
        reply,
    } = endpoint
        .try_recv()
        .expect("clearing must persist the default")
    else {
        panic!("clearing must write the overlay settings");
    };
    assert_eq!(expected_config_revision, 6);
    assert_eq!(settings.hold_modifier_to_interact, None);

    let mut confirmed = overlay_snapshot(7);
    confirmed.overlay = settings;
    reply
        .respond(Ok(confirmed))
        .expect("overlay settings reply");
    visual.run_until_parked();

    view.update(visual, |view, cx| view.clear_hold_modifier(cx));
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "clearing a setting that is already off must ask for nothing"
    );
}

/// Structurally blocked editing stops both directions of the change, which is the
/// control arm of the unified gate rule.
#[gpui_kit::test]
fn structural_editing_blocking_refuses_to_record_or_clear(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 3);
    // A model import running makes editing structurally impossible; the snapshot
    // alone is not what blocks it.
    view.update(visual, |view, _| {
        view.model_import.state = ModelImportState::Capturing;
    });
    assert!(view.read_with(visual, |view, _| {
        view.editing_blocked(view.snapshot.as_ref())
    }));

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_hold_modifier_recording(window, cx)
        });
    });
    assert!(
        !view.read_with(visual, |view, _| view.hold_modifier_recording),
        "a blocked page must not arm a recorder"
    );
    view.update(visual, |view, cx| view.clear_hold_modifier(cx));
    visual.run_until_parked();
    assert!(endpoint.try_recv().is_err());
}

/// Closing the window ends the recording. The view is recreated for each open, so
/// a recording that outlived the window would have nothing to write into and would
/// keep polling a channel for a row nobody can see.
#[gpui_kit::test]
fn closing_the_window_ends_the_recording(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    show_overlay_page(&view, visual, 9);

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_hold_modifier_recording(window, cx)
        });
    });
    assert!(view.read_with(visual, |view, _| view.hold_modifier_recording));

    view.update(visual, |view, cx| view.prepare_close(cx));
    assert!(!view.read_with(visual, |view, _| view.hold_modifier_recording));
    advance_recorder_clock(visual);
    visual.run_until_parked();

    // Closing flushes whatever was in flight, so the channel is not empty. What
    // must not appear is the recorder writing a value; a poll that was already on
    // its way may still ask once, and its answer has to be dropped.
    while let Ok(command) = endpoint.try_recv() {
        match command {
            crate::SettingsCommand::ReadPressedModifiers { reply } => {
                let mut pressed = PressedModifiers::NONE;
                pressed.insert(ModifierKey::LeftMeta);
                reply
                    .respond(Ok(pressed))
                    .expect("late pressed modifier reply");
            }
            other => assert!(
                !matches!(other, crate::SettingsCommand::SetOverlaySettings { .. }),
                "a closed window must not write a modifier"
            ),
        }
    }
    visual.run_until_parked();
    // And it stops there: a poll that kept going would ask again.
    advance_recorder_clock(visual);
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "a closed window must stop asking which modifiers are held"
    );
}
/// The recorder's own element: pressing the frame arms it, and pressing the clear
/// control inside the frame does not.
///
/// The frame starts a recording on any click inside it, so the clear control has
/// to stop the press the same way the shortcut row's controls do. The row is
/// rendered through the packaged `SettingItem::render` wrapper because that is
/// what supplies `RenderOptions`, and the probe below is what makes the "asked
/// for nothing" assertions meaningful.
#[gpui_kit::test]
fn the_recorder_frame_arms_the_recording_and_the_clear_control_does_not(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(8);
    let snapshot = overlay_snapshot(12);
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let harness_snapshot = snapshot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.snapshot = Some(harness_snapshot.clone());
            view.window_hidden = false;
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(
            cx.new(|_| OverlayRowHarness {
                view,
                editing_blocked: false,
            }),
            window,
            cx,
        )
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");

    visual.update(|window, cx| window.render_frame(cx));
    let frame = rendered_bounds(visual, ElementId::from("hold-modifier-recorder"));
    assert!(
        frame.size.width > px(0.) && frame.size.height > px(0.),
        "the row must have drawn its recorder, not an empty frame"
    );

    let clear = rendered_bounds(visual, ElementId::from("hold-modifier-clear"));
    visual.simulate_click(clear.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(
        !view.read_with(visual, |view, _| view.hold_modifier_recording),
        "pressing clear must not start a recording"
    );
    assert!(
        endpoint.try_recv().is_err(),
        "clearing an unset modifier must ask the service for nothing"
    );

    visual.simulate_click(frame.center(), Modifiers::default());
    assert!(
        view.read_with(visual, |view, _| view.hold_modifier_recording),
        "pressing the recorder must start a recording"
    );

    // Escape is the way out from the keyboard, the same one the shortcut recorder
    // uses, and it must leave the stored value alone.
    visual.simulate_keystrokes("escape");
    assert!(
        !view.read_with(visual, |view, _| view.hold_modifier_recording),
        "escape must end the recording"
    );
    assert!(
        endpoint.try_recv().is_err(),
        "ending a recording must not write anything"
    );
}

/// A blocked page renders the recorder inert rather than merely dimmed.
#[gpui_kit::test]
fn structural_editing_blocking_renders_the_recorder_inert(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(8);
    let snapshot = overlay_snapshot(12);
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let harness_snapshot = snapshot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.snapshot = Some(harness_snapshot.clone());
            view.window_hidden = false;
            view.model_import.state = ModelImportState::Capturing;
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(
            cx.new(|_| OverlayRowHarness {
                view,
                editing_blocked: true,
            }),
            window,
            cx,
        )
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");

    visual.update(|window, cx| window.render_frame(cx));
    let frame = rendered_bounds(visual, ElementId::from("hold-modifier-recorder"));
    visual.simulate_click(frame.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(
        !view.read_with(visual, |view, _| view.hold_modifier_recording),
        "a row the page cannot edit must not register a recording"
    );
    assert!(endpoint.try_recv().is_err());
}
