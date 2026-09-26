//! The model library page: layout, gating and the card grid.

use super::*;

/// Deleting a model is two steps, and the first one asks nothing of the service.
///
/// The card's delete control opens a confirmation surface and the accept button
/// runs `ModelRowAction::Delete`; this pins both halves of that split — opening
/// and closing only move the confirmation, and only `Delete` reaches the
/// service. The surface itself is covered by `pop_confirm`'s own tests; what is
/// checked here is that the page hands it the right two callbacks.
#[gpui_kit::test]
fn deleting_a_model_asks_the_service_only_after_the_confirmation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let installed = model_entry(
        "duplicate",
        SettingsModelOrigin::Imported,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    );
    let model = SettingsModelKey {
        id: installed.id.clone(),
        origin: installed.origin,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![installed];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();

    // `gpui-component` resolves its overlays through a `Root` at the top of the
    // window, so the page is built as a child of one and the handle is carried out
    // of the builder rather than taken from the root.
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
        view.snapshot = Some(seeded);
        view.sync_model_row_focus(&entries, active.as_ref(), false, cx);
    });

    // Opening the confirmation is a state change on this page and nothing else:
    // a model that is asked about is not a model that has been deleted.
    //
    // Both negative checks below settle the executor before looking at the
    // channel. A command is sent from a spawned task, so an unsettled executor
    // would report an empty channel no matter what the page did, and the check
    // would hold even if opening the question really did delete the model.
    view.update(visual, |view, cx| {
        view.request_model_delete(model.clone(), cx);
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        Some(model.clone()),
        "asking about a model must record which model the question is about"
    );
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "opening the confirmation must not reach the service"
    );

    view.update(visual, |view, cx| {
        view.cancel_model_delete(&model, cx);
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        None,
        "declining must drop the question"
    );
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "declining must not reach the service"
    );

    // The accept button runs the row action, and that is what deletes.
    view.update(visual, |view, cx| {
        view.request_model_delete(model.clone(), cx);
    });
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Delete, model.clone(), window, cx);
        });
    });
    visual.run_until_parked();
    assert!(
        matches!(
            endpoint.try_recv(),
            Ok(crate::SettingsCommand::DeleteModel { .. })
        ),
        "accepting must ask the service to delete the model it named"
    );
}

/// The actual settings list path must keep the catalog visible; a container
/// query used directly inside a setting item collapses to zero height there.
#[gpui_kit::test]
fn the_model_catalog_is_visible_through_the_settings_item_wrapper(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let seeded = crate::tests::snapshot(1, false, true);

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
        view.update(cx, |view, _| view.snapshot = Some(seeded.clone()));
        let page = cx.new(|_| WrappedModelsPageHarness {
            view,
            snapshot: Some(seeded),
        });
        Root::new(page, window, cx)
    });

    visual.update(|window, cx| window.render_frame(cx));
    let import = rendered_bounds(
        visual,
        ElementId::from((ElementId::from("model-import-card"), "trigger")),
    );

    assert!(import.size.width > px(0.0));
    assert!(import.size.height >= px(super::models::MODEL_CARD_MIN_HEIGHT));
}

/// The import card is exactly as tall as the model cards it is laid out beside.
///
/// This is the page's real grid, not a stand-in for it: the model card here
/// carries a status line, which is what makes it taller than the import card's
/// own floor, so a card that kept its own height would come out short. Nothing
/// else about the page is asserted — the card's own tests cover its two faces.
#[gpui_kit::test]
fn the_import_card_is_as_tall_as_the_model_cards_beside_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut seeded = crate::tests::snapshot(1, false, true);
    // An unavailable model is the case that makes the two heights differ: its
    // status line is what pushes a model card past the import card's floor.
    seeded.model_catalog.entries = vec![model_entry(
        "broken",
        SettingsModelOrigin::Imported,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    )];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();
    let page_snapshot = seeded.clone();

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
        let page = cx.new(|_| ModelsPageHarness {
            view,
            snapshot: Some(page_snapshot),
        });
        // The cards own `PopConfirm` surfaces, which resolve through a `Root`.
        Root::new(page, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.sync_model_row_focus(&entries, active.as_ref(), false, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));

    let import = rendered_bounds(
        visual,
        ElementId::from((ElementId::from("model-import-card"), "trigger")),
    );
    let model = rendered_bounds(visual, ElementId::from(("model-card", 0usize)));

    assert_eq!(
        import.origin.y, model.origin.y,
        "the two cells must share a row for their heights to be comparable"
    );
    assert_eq!(
        import.size.height, model.size.height,
        "the grid's first cell must be as tall as the model cards beside it"
    );
    assert!(
        import.size.height > px(super::models::MODEL_CARD_MIN_HEIGHT),
        "a row is as tall as its tallest cell, so the card must be taller than its own floor"
    );
}

/// A model import gates the other cards for the whole flow without hiding them.
///
/// The controls must stay in the card so the grid does not change shape while
/// the picker, inspection or dialog is open, and a press while the gate is on
/// must still do nothing. Preset cards have no delete affordance at all; only
/// installed cards keep a visible delete control, disabled for the run.
#[gpui_kit::test]
fn a_model_import_disables_the_other_cards_actions_until_it_finishes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let preset = model_entry("standard", SettingsModelOrigin::BuiltIn, ready.clone());
    let installed = model_entry("editable", SettingsModelOrigin::Imported, ready);
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![preset, installed];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();
    let page_snapshot = seeded.clone();

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
        let page = cx.new(|_| ModelsPageHarness {
            view,
            snapshot: Some(page_snapshot),
        });
        Root::new(page, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.sync_model_row_focus(&entries, active.as_ref(), false, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));

    assert!(
        model_row_actions(&entries[0], active.as_ref(), false).can_edit,
        "test premise: a preset card offers its edit control"
    );
    assert!(
        model_row_actions(&entries[1], active.as_ref(), false).can_edit,
        "test premise: the installed card offers its edit control"
    );
    assert!(
        model_row_actions(&entries[1], active.as_ref(), false).can_delete,
        "test premise: the installed card offers its delete control"
    );
    assert!(
        visual.update(|window, _| {
            window
                .try_find(ElementId::from(("delete-model", 0usize)))
                .is_none()
        }),
        "a preset card must not render a delete control at all"
    );

    let installed_edit = ElementId::from(("edit-model", 1usize));
    let installed_delete = ElementId::from(("delete-model", 1usize));
    let edit = rendered_bounds(visual, installed_edit.clone());
    let delete = rendered_bounds(visual, installed_delete.clone());

    // The installed edit control is live before the run: the first press opens
    // the card's in-place editor. Close it again so the seeded page starts from
    // the non-editing face for the gated phases.
    visual.update(|window, cx| window.click(installed_edit.clone(), cx));
    assert!(
        view.read_with(visual, |view, _| view.model_edit.is_some()),
        "editing an installed model must be possible while the page is idle"
    );
    view.update(visual, |view, cx| {
        view.cancel_model_edit(cx);
        cx.notify();
    });
    visual.update(|window, cx| window.render_frame(cx));

    let cases = [
        (ModelImportState::Picking, None, "the native folder picker"),
        (
            ModelImportState::ValidatingDrop,
            None,
            "dropped-folder validation",
        ),
        (ModelImportState::Inspecting, None, "source inspection"),
        (
            ModelImportState::Inspecting,
            Some(MverModeDialog::from_available(vec![
                SettingsMverMode::Standard,
            ])),
            "the open Mver conversion dialog",
        ),
        (
            ModelImportState::Starting {
                cancel_requested: false,
            },
            None,
            "the running import and capture",
        ),
    ];

    for (state, dialog, phase) in cases {
        // The dialog preview is built during `SettingsView::render`; rendering
        // before clicking proves the card gate covers both the block and the
        // rendered dwell/surface states of that path.
        view.update(visual, |view, cx| {
            view.model_edit = None;
            view.model_import.state = state;
            view.model_import.mver_mode_dialog = dialog;
            cx.notify();
        });
        visual.update(|window, cx| window.render_frame(cx));

        if phase == "the open Mver conversion dialog" {
            assert!(
                view.read_with(visual, |view, _| {
                    view.model_import.has_open_mver_mode_dialog()
                }),
                "the Mver conversion dialog draft must remain open during {phase}"
            );
        }

        assert!(
            visual.update(|window, _| {
                window
                    .try_find(ElementId::from(("delete-model", 0usize)))
                    .is_none()
            }),
            "a preset card must not regain a delete control during {phase}"
        );
        assert_eq!(
            rendered_bounds(visual, installed_edit.clone()),
            edit,
            "the installed edit control must keep its slot during {phase}"
        );
        assert_eq!(
            rendered_bounds(visual, installed_delete.clone()),
            delete,
            "the installed delete control must keep its slot during {phase}"
        );

        visual.update(|window, cx| window.click(installed_edit.clone(), cx));
        assert!(
            view.read_with(visual, |view, _| view.model_edit.is_none()),
            "editing must stay blocked during {phase}"
        );
    }

    view.update(visual, |view, cx| {
        view.model_import.state = ModelImportState::Idle;
        view.model_import.mver_mode_dialog = None;
        cx.notify();
    });
    visual.update(|window, cx| window.render_frame(cx));

    visual.update(|window, cx| window.click(installed_edit.clone(), cx));
    assert!(
        view.read_with(visual, |view, _| view.model_edit.is_some()),
        "finishing the import must re-enable the installed edit control"
    );
}
