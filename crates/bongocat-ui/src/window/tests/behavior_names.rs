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
