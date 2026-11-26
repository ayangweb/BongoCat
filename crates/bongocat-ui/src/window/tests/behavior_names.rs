//! A name replaces a row's numbered label, and nothing else about the row changes.
//!
//! The rows number behaviors by position because the resource names inside a package
//! are internal numbering the user cannot see, so a name is the only way to find the
//! one they meant. What has to stay true is everything the row already did: its chord
//! capture, its play control and its clear control are the same three controls in the
//! same order, whether or not the row has a name.

use super::*;

fn active_model() -> SettingsModelKey {
    SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    }
}

fn entries() -> Vec<SettingsModelEntry> {
    vec![model_entry(
        "standard",
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Ready {
            behaviors: vec![
                SettingsModelBehavior::Motion {
                    group: "CAT_motion".to_owned(),
                    index: 0,
                },
                SettingsModelBehavior::Motion {
                    group: "CAT_motion".to_owned(),
                    index: 1,
                },
                SettingsModelBehavior::Expression {
                    name: "live2d_expression0.exp3.json".to_owned(),
                },
            ],
        },
    )]
}

fn named(behavior_id: &str, name: &str) -> SettingsModelBehaviorName {
    SettingsModelBehaviorName {
        behavior_id: behavior_id.to_owned(),
        name: name.to_owned(),
    }
}

/// A row the user named shows that name instead of its position.
///
/// This is the whole feature: the user binds a key to "the sleepy one" and sees that
/// word on the row afterwards, rather than having to count to the third motion again.
#[test]
fn a_named_behavior_shows_its_name_instead_of_its_number() {
    let rows = shortcut_behavior_rows(
        &SettingsShortcuts::default(),
        Some(&active_model()),
        &entries(),
        &[named("motion:CAT_motion:1", "the sleepy one")],
    );
    let english = rows
        .iter()
        .map(|row| row.name(SettingsLanguage::English))
        .collect::<Vec<_>>();
    assert_eq!(
        english,
        vec!["Motion 1", "the sleepy one", "Expression 1"],
        "only the named row changes, and the others keep their numbered labels"
    );
    // The name is the user's own text, so it is not a catalog string and no language
    // translates it.
    assert_eq!(
        rows[1].name(SettingsLanguage::ChineseSimplified),
        "the sleepy one"
    );
}

/// A row nobody named keeps the numbered label in every language.
///
/// An absent name is the ordinary case: the field is `Option`, not an empty string, so
/// the page never has to invent a placeholder for it.
#[test]
fn an_unnamed_behavior_keeps_its_numbered_label() {
    let rows = shortcut_behavior_rows(
        &SettingsShortcuts::default(),
        Some(&active_model()),
        &entries(),
        &[],
    );
    assert!(rows.iter().all(|row| row.custom_name.is_none()));
    assert_eq!(rows[0].name(SettingsLanguage::English), "Motion 1");
    assert_eq!(rows[0].name(SettingsLanguage::ChineseSimplified), "动作 1");
}

/// A name is matched by the behavior's identity, not by its position.
///
/// A name belonging to another behavior, or to another model, must not land on this
/// row: the whole value of a name is that it names *one* thing.
#[test]
fn a_name_only_lands_on_the_behavior_it_belongs_to() {
    let rows = shortcut_behavior_rows(
        &SettingsShortcuts::default(),
        Some(&active_model()),
        &entries(),
        &[
            named("motion:CAT_motion:99", "not a declared motion"),
            named("expression:does_not_exist.exp3.json", "not declared either"),
        ],
    );
    assert!(
        rows.iter().all(|row| row.custom_name.is_none()),
        "a name the model does not declare belongs to no row"
    );
}

/// A name changes the label and nothing else about the row.
///
/// The row is a capture target, a play control and a clear control; renaming it must not
/// move the chord, make the row playable, or attach a binding to a different behavior.
#[test]
fn naming_a_behavior_leaves_its_binding_and_play_control_alone() {
    let mut shortcuts = SettingsShortcuts::default();
    let target = model_behavior_id(&SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 1,
    });
    shortcuts.model_behaviors = vec![SettingsModelBehaviorBinding {
        model: active_model(),
        behavior_id: target.clone(),
        shortcut: "Ctrl+Alt+3".to_owned(),
    }];

    let plain = shortcut_behavior_rows(&shortcuts, Some(&active_model()), &entries(), &[]);
    let renamed = shortcut_behavior_rows(
        &shortcuts,
        Some(&active_model()),
        &entries(),
        &[named("motion:CAT_motion:1", "the sleepy one")],
    );

    assert_eq!(
        renamed.iter().map(|row| &row.target).collect::<Vec<_>>(),
        plain.iter().map(|row| &row.target).collect::<Vec<_>>(),
        "the capture target is the same behavior either way"
    );
    assert_eq!(
        renamed
            .iter()
            .map(|row| row.shortcut.clone())
            .collect::<Vec<_>>(),
        plain
            .iter()
            .map(|row| row.shortcut.clone())
            .collect::<Vec<_>>(),
        "renaming must not move a recorded chord"
    );
    assert_eq!(
        renamed
            .iter()
            .map(|row| row.playable.is_some())
            .collect::<Vec<_>>(),
        plain
            .iter()
            .map(|row| row.playable.is_some())
            .collect::<Vec<_>>(),
        "the play control follows the row, not the name"
    );
    assert_ne!(
        plain[1].name(SettingsLanguage::English),
        renamed[1].name(SettingsLanguage::English),
        "and only the label differs"
    );
}

/// An application command is not a model behavior, so it has nothing to be named.
#[test]
fn an_application_command_is_never_named() {
    let rows = shortcut_rows(
        &SettingsShortcuts::default(),
        Some(&active_model()),
        &entries(),
        &[named("motion:CAT_motion:0", "a motion's name")],
    );
    let commands = rows
        .iter()
        .filter(|row| matches!(row.target, ShortcutCaptureTarget::Command(_)))
        .collect::<Vec<_>>();
    assert!(!commands.is_empty());
    assert!(
        commands.iter().all(|row| row.custom_name.is_none()),
        "a command's label is the command, not a name the user gave a behavior"
    );
}

/// The stored name is bounded and printable, and the field says so.
#[test]
fn a_stored_name_is_bounded_and_carries_no_control_characters() {
    assert_eq!(
        sanitize_behavior_name_input("   spaced out   "),
        "spaced out",
        "surrounding whitespace is dropped rather than stored"
    );
    assert_eq!(
        sanitize_behavior_name_input("a\nb\tc"),
        "abc",
        "a control character would be the row's own problem"
    );
    assert!(
        sanitize_behavior_name_input(&"n".repeat(BEHAVIOR_NAME_MAXIMUM_CHARS + 20)).len()
            <= BEHAVIOR_NAME_MAXIMUM_CHARS,
        "the field cannot be wider than the document accepts"
    );
    assert_eq!(
        BEHAVIOR_NAME_MAXIMUM_CHARS,
        bongocat_config::MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS,
        "the field and the document agree on the bound, so the page never sends a value the service rejects"
    );
}

// ── The inline editor ────────────────────────────────────────────────────────
//
// The rename surface is the row's own label, so these tests render the page and
// drive the label, the pencil and the field the way a pointer and a keyboard do.

use super::behavior_name::{behavior_name_key, behavior_name_part_id};

/// The model scope of the shortcuts page, rendered with one ready model.
///
/// Returns the view, the context it renders into, the endpoint its commands arrive
/// at, and the second motion's `behavior_id` — the row a rename is driven on.
fn editor_harness(
    cx: &mut TestAppContext,
) -> (
    Entity<SettingsView>,
    &mut VisualTestContext,
    crate::SettingsServiceEndpoint,
    String,
) {
    let mut snapshot = crate::tests::snapshot(4, false, true);
    snapshot.model_catalog.entries = entries();
    let behavior_id = "motion:CAT_motion:1".to_owned();
    let (view, visual, endpoint) = rendered_shortcuts_scope(cx, snapshot, ShortcutScope::Model);
    visual.update(|window, cx| window.render_frame(cx));
    (view, visual, endpoint, behavior_id)
}

fn editor_id(behavior_id: &str, part: &str) -> gpui_kit::ElementId {
    behavior_name_part_id(
        &behavior_name_key(&ShortcutCaptureTarget::ModelBehavior {
            model: active_model(),
            behavior_id: behavior_id.to_owned(),
        }),
        part,
    )
}

fn editor_is_open(visual: &mut VisualTestContext, behavior_id: &str) -> bool {
    visual.update(|window, _| window.try_find(editor_id(behavior_id, "field")).is_some())
}

/// Selects everything in the focused input, with the keystroke the compiled
/// platform actually bound.
///
/// The input binds `cmd-a` on macOS and `ctrl-a` everywhere else, so pressing the
/// other one is a silent no-op — and the edit that follows would append to the
/// prefilled label instead of replacing it. That is exactly how the first Windows
/// CI run of this suite failed: the stored name arrived as "the sleepy oneMotion 2".
fn select_all_in_focus(visual: &mut VisualTestContext) {
    let keystroke = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    visual.update(|window, cx| window.press(keystroke, cx));
}

/// A confirmed rename reaches the service as its own typed command.
///
/// It carries the revision the page was rendered from, so a stale page cannot overwrite
/// a name someone else just changed, and the row's whole identity — model plus
/// behavior — rather than a position, because a position means a different behavior
/// after the package changes.
#[gpui_kit::test]
fn a_confirmed_rename_sends_the_row_identity_it_was_opened_from(cx: &mut TestAppContext) {
    let (_view, visual, endpoint, behavior_id) = editor_harness(cx);
    let pencil = editor_id(&behavior_id, "edit");
    visual.update(|window, cx| window.click(pencil, cx));
    assert!(
        editor_is_open(visual, &behavior_id),
        "the pencil opens the field where the name was"
    );

    // The field opens on the label the row already shows, so the user edits the
    // thing they can see rather than retyping a name they have to remember.
    select_all_in_focus(visual);
    visual.update(|window, cx| window.input("the sleepy one", cx));
    visual.update(|window, cx| window.press("enter", cx));
    visual.run_until_parked();

    let crate::SettingsCommand::SetModelBehaviorName {
        expected_config_revision,
        model,
        behavior_id,
        name,
        reply,
    } = endpoint
        .try_recv()
        .expect("a confirmed rename must reach the service as its own command")
    else {
        panic!("the rename must use the typed model-behavior-name command");
    };
    assert_eq!(expected_config_revision, 4);
    assert_eq!(model.id, "standard");
    assert_eq!(behavior_id, "motion:CAT_motion:1");
    assert_eq!(name, "the sleepy one");

    let mut confirmed = crate::tests::snapshot(5, false, true);
    confirmed.config_revision = Some(5);
    confirmed.model_behavior_names = vec![crate::SettingsModelBehaviorName {
        behavior_id: "motion:CAT_motion:1".to_owned(),
        name: "the sleepy one".to_owned(),
    }];
    reply.respond(Ok(confirmed)).expect("rename reply");
    visual.run_until_parked();
    assert!(
        !editor_is_open(visual, "motion:CAT_motion:1"),
        "a confirmed rename closes the field"
    );
    assert!(endpoint.try_recv().is_err());
}

/// The pencil is the only entry, and the name itself never starts an edit.
///
/// Reading a row — and selecting its text with the pointer — must not turn the row
/// into a field: an edit is something the pencil asks for, not something that happens
/// because the pointer passed over a name.
#[gpui_kit::test]
fn the_pencil_opens_the_field_and_the_name_does_not(cx: &mut TestAppContext) {
    let (_view, visual, _endpoint, behavior_id) = editor_harness(cx);
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "label"), cx));
    assert!(
        !editor_is_open(visual, &behavior_id),
        "clicking the name starts nothing"
    );
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "edit"), cx));
    assert!(
        editor_is_open(visual, &behavior_id),
        "the pencil opens the field without a second surface"
    );
    assert!(
        visual.update(|window, _| window.try_find(editor_id(&behavior_id, "label")).is_none()),
        "the field replaces the name rather than sitting beside a second copy of it"
    );
}

/// The name and the pencil that names it editable sit on one center line.
///
/// The pencil is an icon inside its own button box and the name is a line of text, so
/// "beside" is only true if their vertical centers agree: an icon a pixel or two off
/// the text's center reads as a misaligned row even though nothing overlaps.
#[gpui_kit::test]
fn the_name_and_the_pencil_share_a_center_line(cx: &mut TestAppContext) {
    let (_view, visual, _endpoint, behavior_id) = editor_harness(cx);
    let name_bounds =
        visual.update(|window, _| window.find(editor_id(&behavior_id, "name")).bounds());
    let pencil_bounds =
        visual.update(|window, _| window.find(editor_id(&behavior_id, "edit")).bounds());
    let row = visual.update(|window, _| window.find(editor_id(&behavior_id, "label")).bounds());
    let offset = (name_bounds.center().y - pencil_bounds.center().y).abs();
    assert!(
        offset <= px(1.0),
        "the name's center {:?} and the pencil's center {:?} must share the row's line {:?}, off by {offset:?}",
        name_bounds.center().y,
        pencil_bounds.center().y,
        row.center().y,
    );
}

/// Escape leaves the field without writing anything.
#[gpui_kit::test]
fn escape_leaves_the_field_without_writing(cx: &mut TestAppContext) {
    let (_view, visual, endpoint, behavior_id) = editor_harness(cx);
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "edit"), cx));
    select_all_in_focus(visual);
    visual.update(|window, cx| window.input("a name nobody asked for", cx));
    visual.update(|window, cx| window.press("escape", cx));
    visual.run_until_parked();

    assert!(
        !editor_is_open(visual, &behavior_id),
        "escape closes the field"
    );
    assert!(
        endpoint.try_recv().is_err(),
        "a discarded edit must not reach the service"
    );
}

/// Saving the label a row already shows writes nothing.
///
/// Enter reads the same predicate the page would offer a save control, so the
/// keyboard path cannot send a request that would store what the row already shows.
#[gpui_kit::test]
fn saving_an_unchanged_label_writes_nothing(cx: &mut TestAppContext) {
    let (_view, visual, endpoint, behavior_id) = editor_harness(cx);
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "edit"), cx));
    visual.update(|window, cx| window.press("enter", cx));
    visual.run_until_parked();

    assert!(
        !editor_is_open(visual, &behavior_id),
        "the field closes either way"
    );
    assert!(
        endpoint.try_recv().is_err(),
        "a rename that changes nothing must not reach the service"
    );
}

/// Renaming changes the label and nothing else about the row.
///
/// The field takes the label's place; the row's three controls are the same three
/// controls in the same order, and none of the rename surface joins their tab order.
/// The field carries no controls of its own — Enter saves, Escape discards, blur saves.
#[gpui_kit::test]
fn the_field_replaces_the_label_and_the_row_keeps_its_controls(cx: &mut TestAppContext) {
    let (_view, visual, endpoint, behavior_id) = editor_harness(cx);
    // The row under test is the second motion, and the model scope's rows are
    // numbered after the window scope's, so its controls are the ones row 6 names.
    let row_index = ShortcutScope::Model.row_index_offset(&SettingsShortcuts::default()) + 1;
    let controls = [
        ("capture-model-shortcut", row_index),
        ("play-model-shortcut", row_index),
        ("clear-model-shortcut", row_index),
    ]
    .map(gpui_kit::ElementId::from);
    for id in &controls {
        assert!(
            visual.update(|window, _| window.try_find(id.clone()).is_some()),
            "{id:?} is drawn before the rename starts"
        );
    }
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "edit"), cx));
    for id in &controls {
        assert!(
            visual.update(|window, _| window.try_find(id.clone()).is_some()),
            "{id:?} is still drawn while the rename is open"
        );
    }
    assert!(
        visual.update(|window, _| window
            .try_find(editor_id(&behavior_id, "confirm"))
            .is_none()),
        "the field grows no controls of its own: Enter and blur do the saving"
    );
    assert!(
        endpoint.try_recv().is_err(),
        "opening the field sends nothing"
    );
}

/// Opening another row's editor commits the one that was open.
///
/// The editor is one at a time, so the alternative to committing is discarding a name
/// the user had already typed — and leaving a row because the next one caught their
/// eye is not a reason to throw their typing away.
#[gpui_kit::test]
fn opening_another_rows_editor_commits_the_one_that_was_open(cx: &mut TestAppContext) {
    let (_view, visual, endpoint, behavior_id) = editor_harness(cx);
    visual.update(|window, cx| window.click(editor_id(&behavior_id, "edit"), cx));
    select_all_in_focus(visual);
    visual.update(|window, cx| window.input("the sleepy one", cx));

    visual.update(|window, cx| window.click(editor_id("motion:CAT_motion:0", "edit"), cx));
    visual.run_until_parked();

    let crate::SettingsCommand::SetModelBehaviorName {
        behavior_id, name, ..
    } = endpoint
        .try_recv()
        .expect("the first row's edit must be committed, not dropped")
    else {
        panic!("the commit must use the typed model-behavior-name command");
    };
    assert_eq!(behavior_id, "motion:CAT_motion:1");
    assert_eq!(name, "the sleepy one");
    assert!(
        editor_is_open(visual, "motion:CAT_motion:0"),
        "and the second row's field is the one on screen"
    );
    assert!(endpoint.try_recv().is_err());
}
