//! The per-row actions on a model card and their confirmation.

use super::*;

#[test]
fn model_row_actions_preserve_origin_availability_and_active_identity() {
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let preset = model_entry("duplicate", SettingsModelOrigin::BuiltIn, ready.clone());
    let installed = model_entry("duplicate", SettingsModelOrigin::Imported, ready);
    let active_preset = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };

    // A preset is app-bundled content: it can be activated and edited — its
    // name and cover are recorded on the user's side — but never deleted.
    assert_eq!(
        model_row_actions(&preset, Some(&active_preset), false),
        ModelRowActions {
            active: true,
            can_activate: false,
            can_delete: false,
            can_edit: true,
            can_open_location: false,
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: true,
            can_delete: true,
            can_edit: true,
            can_open_location: false,
        }
    );

    // An imported model keeps its delete control while it is the one on screen:
    // deleting it switches the runtime to the standard preset first, so the
    // control cannot disappear from a card the user imported.
    let active_installed = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    assert_eq!(
        model_row_actions(&installed, Some(&active_installed), false),
        ModelRowActions {
            active: true,
            can_activate: false,
            can_delete: true,
            can_edit: true,
            can_open_location: false,
        }
    );

    // "Open location" needs a directory that actually resolved, so an entry
    // whose files are gone offers no button instead of a dead one.
    let located = SettingsModelEntry {
        directory: Some(PathBuf::from("/private/models/duplicate")),
        ..installed.clone()
    };
    assert_eq!(
        model_row_actions(&located, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: true,
            can_delete: true,
            can_edit: true,
            can_open_location: true,
        }
    );

    let invalid = SettingsModelEntry {
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
        },
        ..installed.clone()
    };
    assert_eq!(
        model_row_actions(&invalid, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: false,
            can_delete: true,
            can_edit: true,
            can_open_location: false,
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), true),
        ModelRowActions {
            active: false,
            can_activate: false,
            can_delete: false,
            can_edit: false,
            can_open_location: false,
        }
    );
}

/// An open delete question is dropped as soon as deletion becomes unavailable.
///
/// The page drops the delete question when deletion would no longer be possible:
/// the card remains in place with a disabled control, but an open question must
/// not outlive the state that made it valid. A question that did would appear
/// already open the moment deletion became possible again.
#[test]
fn an_open_delete_question_lives_only_while_its_control_would() {
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let preset = model_entry("duplicate", SettingsModelOrigin::BuiltIn, ready.clone());
    let installed = model_entry("duplicate", SettingsModelOrigin::Imported, ready);
    let active_preset = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let target = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let catalog = [preset, installed.clone()];

    assert!(model_delete_confirmation_is_valid(
        &catalog,
        Some(&active_preset),
        false,
        &target
    ));

    // The target can stop being deletable in two ways, and each one drops the
    // question: it leaves the catalog, or editing is structurally blocked (an
    // import running, a picker open). Becoming the active model is not one of
    // them — an imported model keeps deletion available while it is the one on
    // screen — and neither is an in-flight command, which never feeds the visual
    // gate (ADR-0053).
    assert!(
        model_delete_confirmation_is_valid(&catalog, Some(&target), false, &target),
        "an imported model that became active still offers deletion"
    );
    assert!(
        !model_delete_confirmation_is_valid(&catalog[..1], Some(&active_preset), false, &target),
        "a model that left the catalog has no card left to ask on"
    );
    assert!(
        !model_delete_confirmation_is_valid(&catalog, Some(&active_preset), true, &target),
        "structural blocking closes the delete confirmation"
    );

    // Availability is not part of it: a package that failed to load is exactly the
    // one a user wants to remove.
    let broken = SettingsModelEntry {
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
        },
        ..installed
    };
    assert!(model_delete_confirmation_is_valid(
        &[broken],
        Some(&active_preset),
        false,
        &target
    ));
}

#[test]
fn behavior_targets_stay_scoped_to_model_and_behavior_identity() {
    let motion = SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    };
    let expression = SettingsModelBehavior::Expression {
        name: "live2d_expression0.exp3.json".to_owned(),
    };
    let entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelAvailability::Ready {
                behaviors: vec![motion.clone(), expression],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelAvailability::Ready {
                behaviors: vec![motion],
            },
        ),
    ];
    // Behavior identity now lives with the shortcut rows the page that owns them
    // renders, so this is where "a motion and an expression are different
    // targets, and the same motion on two models is two targets" has to hold.
    let targets = |id: &str| {
        shortcut_behavior_rows(
            &SettingsShortcuts::default(),
            Some(&SettingsModelKey {
                id: id.to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            }),
            &entries,
        )
        .into_iter()
        .map(|row| row.target)
        .collect::<Vec<_>>()
    };

    let standard = targets("standard");
    let keyboard = targets("keyboard");
    assert_eq!(standard.len(), 2);
    assert_ne!(standard[0], standard[1]);
    assert_eq!(keyboard.len(), 1);
    assert_ne!(keyboard[0], standard[0]);
}

#[test]
fn active_model_behavior_preview_targets_exclude_inactive_and_invalid_models() {
    let active = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let behavior = SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    };
    let entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelAvailability::Ready {
                behaviors: vec![behavior.clone()],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelAvailability::Ready {
                behaviors: vec![SettingsModelBehavior::Expression {
                    name: "inactive".to_owned(),
                }],
            },
        ),
    ];

    // The page no longer previews behaviors: the shortcuts page owns that list,
    // so the targets are only reached through the shortcut rows it renders.
    assert_eq!(
        shortcut_behavior_rows(&SettingsShortcuts::default(), Some(&active), &entries).len(),
        1
    );
    assert!(shortcut_behavior_rows(&SettingsShortcuts::default(), None, &entries).is_empty());
}
