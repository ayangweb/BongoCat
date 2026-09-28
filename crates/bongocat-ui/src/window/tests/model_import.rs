//! Picking a source, converting a legacy model and the import card.

use super::*;

#[test]
fn cancellation_requested_while_starting_reaches_the_created_operation() {
    let (client, _endpoint) = SettingsClient::bounded(1);
    let operation = client
        .start_model_import_blocking(SettingsModelImportRequest {
            title: "custom-model".to_owned(),
            source_root: PathBuf::from("/private/source"),
            selected_mver_modes: Vec::new(),
        })
        .expect("prepared import");
    let draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Starting {
            cancel_requested: true,
        },
        ..ModelImportDraft::default()
    };

    assert!(!operation.is_cancelled());
    draft.apply_starting_cancellation(&operation);
    assert!(operation.is_cancelled());
    // The request has landed, so the card stops offering to send another one.
    assert!(draft.shows_cancel());
    assert!(!draft.is_cancellable());
}

#[test]
fn the_conversion_dialog_defaults_to_the_priority_mode_only() {
    // Standard is the first preference, keyboard the next, gamepad last. The
    // default is exactly one mode, whichever of the three the source carries.
    let all = [
        SettingsMverMode::Standard,
        SettingsMverMode::Keyboard,
        SettingsMverMode::Gamepad,
    ];
    let standard = MverModeDialog::from_available(all.to_vec());
    assert_eq!(
        standard.checked_in_order(),
        vec![SettingsMverMode::Standard]
    );
    assert_eq!(standard.checked.len(), 1, "one mode is checked by default");

    let keyboard_only =
        MverModeDialog::from_available(vec![SettingsMverMode::Gamepad, SettingsMverMode::Keyboard]);
    assert_eq!(
        keyboard_only.checked_in_order(),
        vec![SettingsMverMode::Keyboard]
    );
    assert_eq!(keyboard_only.checked.len(), 1);

    let gamepad_only = MverModeDialog::from_available(vec![SettingsMverMode::Gamepad]);
    assert_eq!(
        gamepad_only.checked_in_order(),
        vec![SettingsMverMode::Gamepad]
    );
    assert_eq!(gamepad_only.checked.len(), 1);
}

#[test]
fn the_selected_modes_keep_the_reported_order_not_the_enum_order() {
    // The request's modes must read in the order the dialog showed them, so a
    // source that reports gamepad first asks the store for gamepad first even
    // though `SettingsMverMode` lists standard first.
    let mut dialog = MverModeDialog::from_available(vec![
        SettingsMverMode::Gamepad,
        SettingsMverMode::Standard,
        SettingsMverMode::Keyboard,
    ]);
    // The default picked standard; add the rest so the whole set is selected
    // and the order it exports is the one the dialog showed.
    dialog.toggle(SettingsMverMode::Standard, true);
    dialog.toggle(SettingsMverMode::Keyboard, true);
    dialog.toggle(SettingsMverMode::Gamepad, true);
    assert_eq!(
        dialog.checked_in_order(),
        vec![
            SettingsMverMode::Gamepad,
            SettingsMverMode::Standard,
            SettingsMverMode::Keyboard,
        ]
    );

    // A mode the source never reported is not offered and so cannot be chosen.
    let mut narrowed = MverModeDialog::from_available(vec![SettingsMverMode::Keyboard]);
    narrowed.toggle(SettingsMverMode::Standard, true);
    narrowed.toggle(SettingsMverMode::Gamepad, true);
    assert!(
        narrowed
            .checked_in_order()
            .contains(&SettingsMverMode::Keyboard)
    );
    assert!(
        !narrowed
            .checked_in_order()
            .contains(&SettingsMverMode::Standard)
    );
    assert!(
        !narrowed
            .checked_in_order()
            .contains(&SettingsMverMode::Gamepad)
    );
}

#[test]
fn the_conversion_dialog_needs_one_checked_mode_to_confirm() {
    let mut dialog = MverModeDialog::from_available(vec![SettingsMverMode::Standard]);
    assert!(dialog.can_confirm());
    dialog.toggle(SettingsMverMode::Standard, false);
    assert!(dialog.checked.is_empty());
    assert!(
        !dialog.can_confirm(),
        "an empty selection cannot start a run"
    );
    dialog.toggle(SettingsMverMode::Standard, true);
    assert!(dialog.can_confirm());
}

#[test]
fn the_conversion_mode_labels_come_from_the_shared_legacy_keys() {
    for locale in [
        SettingsLanguage::EnglishUnitedStates,
        SettingsLanguage::ChineseSimplified,
    ] {
        for mode in [
            SettingsMverMode::Standard,
            SettingsMverMode::Keyboard,
            SettingsMverMode::Gamepad,
        ] {
            let label = bongocat_i18n::text(locale.catalog_locale(), mver_mode_label_key(mode));
            assert!(!label.is_empty(), "missing label for {mode:?}");
        }
    }
}

#[test]
fn every_model_card_mode_has_a_visible_localized_label() {
    let mut labels = Vec::new();
    for mode in [
        SettingsModelMode::Standard,
        SettingsModelMode::Keyboard,
        SettingsModelMode::Gamepad,
    ] {
        for locale in [
            SettingsLanguage::EnglishUnitedStates,
            SettingsLanguage::ChineseSimplified,
        ] {
            let label = bongocat_i18n::text(
                locale.catalog_locale(),
                super::models::model_mode_label_key(mode),
            );
            assert!(!label.is_empty(), "missing model mode label for {mode:?}");
            assert_ne!(label, super::models::model_mode_label_key(mode));
            labels.push(label);
        }
    }
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), before, "mode labels must be distinguishable");
}

#[test]
fn the_dialog_snapshot_keeps_the_same_priority_default() {
    let all = [
        SettingsMverMode::Standard,
        SettingsMverMode::Keyboard,
        SettingsMverMode::Gamepad,
    ];
    for (available, expected) in [
        (all.to_vec(), SettingsMverMode::Standard),
        (
            vec![SettingsMverMode::Keyboard, SettingsMverMode::Gamepad],
            SettingsMverMode::Keyboard,
        ),
        (vec![SettingsMverMode::Gamepad], SettingsMverMode::Gamepad),
    ] {
        let snapshot = MverDialogSnapshot::from_dialog(&MverModeDialog::from_available(available));
        assert_eq!(snapshot.checked_in_order(), vec![expected]);
        assert!(snapshot.can_confirm());
        assert_eq!(
            snapshot
                .available()
                .iter()
                .copied()
                .filter(|mode| snapshot.is_checked(*mode))
                .count(),
            1,
            "exactly one mode is checked by default"
        );
    }
}

#[test]
fn the_dialog_snapshot_exports_checks_in_display_order() {
    let mut snapshot = MverDialogSnapshot::from_dialog(&MverModeDialog::from_available(vec![
        SettingsMverMode::Gamepad,
        SettingsMverMode::Keyboard,
        SettingsMverMode::Standard,
    ]));
    assert_eq!(
        snapshot.checked_in_order(),
        vec![SettingsMverMode::Standard]
    );
    snapshot.set(SettingsMverMode::Keyboard, true);
    snapshot.set(SettingsMverMode::Gamepad, true);
    assert_eq!(
        snapshot.checked_in_order(),
        vec![
            SettingsMverMode::Gamepad,
            SettingsMverMode::Keyboard,
            SettingsMverMode::Standard,
        ]
    );
}

#[test]
fn the_dialog_snapshot_tracks_checks_and_gates_confirmation() {
    let mut snapshot = MverDialogSnapshot::from_dialog(&MverModeDialog::from_available(vec![
        SettingsMverMode::Keyboard,
    ]));
    assert!(snapshot.is_checked(SettingsMverMode::Keyboard));

    snapshot.set(SettingsMverMode::Standard, true);
    assert!(
        !snapshot.is_checked(SettingsMverMode::Standard),
        "an unavailable mode is refused"
    );

    snapshot.set(SettingsMverMode::Keyboard, false);
    assert!(!snapshot.is_checked(SettingsMverMode::Keyboard));
    assert!(
        !snapshot.can_confirm(),
        "confirmation needs at least one check"
    );

    snapshot.set(SettingsMverMode::Keyboard, true);
    assert!(snapshot.is_checked(SettingsMverMode::Keyboard));
    assert!(snapshot.can_confirm());
}

#[gpui_kit::test]
fn the_mver_dialog_can_open_during_the_settings_render(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::EnglishUnitedStates,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(view, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");

    view.update(visual, |view, cx| {
        view.model_import.title = "mver-model".to_owned();
        view.model_import.source_root = Some(PathBuf::from("/tmp/mver-model"));
        view.model_import.mver_mode_dialog = Some(MverModeDialog::from_available(vec![
            SettingsMverMode::Standard,
            SettingsMverMode::Keyboard,
            SettingsMverMode::Gamepad,
        ]));
        cx.notify();
    });

    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(
            window.has_active_dialog(cx),
            "the Mver dialog must open without re-borrowing SettingsView"
        );
    });
}

#[test]
fn the_import_card_never_contains_the_selected_path() {
    // The card reports the step it is on, never the source: a path on screen
    // would be the one place the page leaks where a user keeps their files.
    for state in [
        ModelImportState::Idle,
        ModelImportState::Picking,
        ModelImportState::ValidatingDrop,
        ModelImportState::Inspecting,
        ModelImportState::Starting {
            cancel_requested: false,
        },
        ModelImportState::Capturing,
    ] {
        let draft = ModelImportDraft {
            title: "custom-model".to_owned(),
            source_root: Some(PathBuf::from("/private/secret/model")),
            state,
            ..ModelImportDraft::default()
        };
        for step in super::models::import_card_step(&draft, SettingsLanguage::EnglishUnitedStates)
            .into_iter()
        {
            assert!(
                !step.contains("private") && !step.contains("secret"),
                "a step label must not name the source path: {step}"
            );
        }
    }
}

#[test]
fn an_open_picker_blocks_starting_another_import() {
    let draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Picking,
        ..ModelImportDraft::default()
    };

    assert!(draft.is_picker_open());
    assert!(!draft.can_import());
    // A dialog cannot be cancelled from the card, so it offers no control.
    assert!(!draft.shows_cancel());
    assert_eq!(
        super::models::import_card_step(&draft, SettingsLanguage::EnglishUnitedStates).as_deref(),
        Some("Opening the file picker…"),
        "an open dialog is the step the card reports"
    );
}

#[test]
fn picker_and_drop_paths_share_the_folder_reading_step() {
    let dropped = ModelImportDraft {
        state: ModelImportState::ValidatingDrop,
        ..ModelImportDraft::default()
    };
    let picked = ModelImportDraft {
        state: ModelImportState::Inspecting,
        ..ModelImportDraft::default()
    };

    for draft in [dropped, picked] {
        assert_eq!(
            super::models::import_card_step(&draft, SettingsLanguage::ChineseSimplified).as_deref(),
            Some("正在读取模型文件夹…"),
            "picker and drop sources must use the same folder-reading copy"
        );
        assert_eq!(
            super::models::import_card_step(&draft, SettingsLanguage::EnglishUnitedStates)
                .as_deref(),
            Some("Reading the model folder…")
        );
    }
}

#[gpui_kit::test]
fn selecting_a_folder_enters_the_shared_source_reading_state(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    visual.update(|window, cx| window.render_frame(cx));
    let source = std::env::temp_dir().join("bongocat-picker-model");

    view.update(visual, |view, cx| {
        view.model_import.state = ModelImportState::Picking;
        let source_root = view
            .apply_model_source_result(Ok(ModelSourcePickerOutcome::Selected(source.clone())))
            .expect("the picker result should produce a source");
        assert!(matches!(view.model_import.state, ModelImportState::Picking));

        view.inspect_model_source(source_root, cx);
        assert!(
            matches!(view.model_import.state, ModelImportState::Inspecting),
            "a selected folder must enter the same source-reading phase as a drop"
        );
    });
    visual.run_until_parked();

    assert!(matches!(
        endpoint.try_recv().expect("picker inspection command"),
        crate::SettingsCommand::InspectModelSource { source_root, .. }
            if source_root == source
    ));
}

#[test]
fn the_suggested_title_is_the_chosen_folders_own_name() {
    let root = PathBuf::from("/private/我的猫 · 标准模式");
    assert_eq!(suggested_model_title(&root), "我的猫 · 标准模式");
    // A path with no name of its own has nothing to suggest, so the page falls
    // back to the placeholder the service also uses.
    assert_eq!(suggested_model_title(&PathBuf::from("/")), "custom-model");
}
