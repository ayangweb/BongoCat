//! The in-place model editor, the import gate and cover capture.

use super::*;

/// Opening a card's editor leaves the card, and every row in it, exactly where
/// it was.
///
/// A model card is one cell of a wrapping grid, so a card that grew would drag
/// its whole grid row with it and move every card below: that is the flicker the
/// in-place editor exists to avoid. The title row is literally the same row in
/// both faces — only its child changes, from the name to the field that edits it
/// — and the cover picker is drawn on the cover rather than in a row of its own,
/// so what this pins is the card, the title row, and the picker staying inside
/// the cover's band instead of below it.
#[gpui_kit::test]
fn opening_a_models_editor_does_not_change_the_card(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut seeded = crate::tests::snapshot(1, false, true);
    // An installed, ready model is the only kind that can be edited, and a ready
    // one carries no status line — so the card here is the plainest card the
    // page ever draws, which is exactly the one an editor must not resize.
    seeded.model_catalog.entries = vec![model_entry(
        "editable",
        SettingsModelOrigin::Imported,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    )];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();
    let model = SettingsModelKey {
        id: "editable".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
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

    let card_id = ElementId::from(("model-card", 0usize));
    let cover_id = ElementId::from(("model-card-cover", 0usize));
    let title_id = ElementId::from(("model-card-title", 0usize));
    let mode_badge_id = ElementId::from(("model-mode-badge", 0usize));
    let card_before = rendered_bounds(visual, card_id.clone());
    let cover_before = rendered_bounds(visual, cover_id);
    let title_before = rendered_bounds(visual, title_id.clone());
    let mode_badge_before = rendered_bounds(visual, mode_badge_id.clone());
    assert!(
        mode_badge_before.left() >= cover_before.left()
            && mode_badge_before.top() >= cover_before.top()
            && mode_badge_before.right() <= cover_before.right()
            && mode_badge_before.bottom() <= cover_before.bottom(),
        "the mode badge belongs on the cover and must not create another card row"
    );
    let cover_inset = px(8.0);
    let one_pixel = px(1.0);
    assert!(
        mode_badge_before.top() >= cover_before.top() + cover_inset - one_pixel,
        "the mode badge must stay inset from the cover's top edge"
    );
    assert!(
        mode_badge_before.right() <= cover_before.right() - cover_inset + one_pixel,
        "the mode badge must be anchored to the cover's top-right corner"
    );
    assert!(
        mode_badge_before.left() > cover_before.left() + cover_before.size.width / 2.0,
        "the mode badge must sit on the right half of the cover"
    );

    // The card's own edit action, so the editor opens the way the page opens it.
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Edit, model.clone(), window, cx);
        });
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        view.read_with(visual, |view, _| view.model_edit.is_some()),
        "test premise: the edit action must have opened the card's editor"
    );

    let card_after = rendered_bounds(visual, card_id);
    let title_after = rendered_bounds(visual, title_id);
    let mode_badge_after = rendered_bounds(visual, mode_badge_id);
    let field = rendered_bounds(visual, ElementId::from("model-edit-title-input"));
    let picker = rendered_bounds(visual, ElementId::from("choose-model-cover"));

    assert_eq!(
        card_after, card_before,
        "the card must occupy the same box with its editor open: anything else \
         resizes its whole grid row"
    );
    assert_eq!(
        card_after.size.height,
        px(super::models::MODEL_CARD_MIN_HEIGHT),
        "a card with no status line must really be as tall as the floor says, \
         or the two faces above are being compared inside a stretched cell"
    );
    assert_eq!(
        title_after, title_before,
        "the field must replace the name inside the same row, at the same height"
    );
    assert_eq!(
        mode_badge_after, mode_badge_before,
        "editing a title must leave the mode badge in the same place"
    );
    assert_eq!(
        field, title_before,
        "the field's own box — border included — must be the title row's box, \
         not something sitting inside it"
    );
    assert!(
        picker.bottom() <= title_after.top(),
        "the cover picker must be drawn over the cover, not in a row under it"
    );
}

/// A preset's card opens the same in-place editor an imported model's does.
///
/// Editing keeps no origin exception (ADR-0047 决策 2), and the page used to
/// carry two of them that no compile error could catch: the edit action refused
/// a preset outright, and the next re-projection abandoned a draft whose model
/// was not installed. Both would leave the button drawing but doing nothing, so
/// this opens the editor on a preset and then re-projects the catalog under it.
#[gpui_kit::test]
fn a_preset_models_card_opens_the_same_in_place_editor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelAvailability::Ready {
                behaviors: Vec::new(),
            },
        ),
        model_entry(
            "imported",
            SettingsModelOrigin::Imported,
            SettingsModelAvailability::Ready {
                behaviors: Vec::new(),
            },
        ),
    ];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();
    let model = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
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

    // The row must offer the control at all: this is what the page draws, not
    // what the service would answer.
    assert!(
        model_row_actions(&entries[0], active.as_ref(), false).can_edit,
        "test premise: a preset card offers its edit control"
    );

    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Edit, model.clone(), window, cx);
        });
    });

    let (edited_model, title) = view.read_with(visual, |view, _| {
        view.model_edit
            .as_ref()
            .map(|draft| (draft.model.clone(), draft.title.clone()))
            .expect(
                "a preset's editor must be open after the edit action — which \
                 includes surviving any re-projection the action itself triggers",
            )
    });
    assert_eq!(edited_model, model);
    assert_eq!(
        title, entries[0].title,
        "the field starts from the name the card shows"
    );

    // Rendering re-syncs the row state from the catalog, so the draft has to
    // survive that too: one that only lasts until the next projection is a card
    // that opens and closes by itself.
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        view.read_with(visual, |view, _| view.model_edit.is_some()),
        "a preset's open edit must survive a re-projection"
    );

    let input = rendered_bounds(visual, ElementId::from("model-edit-title-input"));
    let picker = rendered_bounds(visual, ElementId::from("choose-model-cover"));
    assert!(
        input.bottom() <= picker.top() || picker.bottom() <= input.top(),
        "the card draws both halves of the editor: the title field and the cover picker"
    );
}

/// The in-flight command flag never feeds the shared visual-gate predicate
/// (ADR-0053).
///
/// `select_model`, `open_model_location` and `delete_model` hold `pending` for
/// the round trip. Gating a page on it disabled and re-enabled every control
/// around each command — every click read as the page refreshing. Every page
/// now reads one predicate, `SettingsView::editing_blocked`, so this pins the
/// predicate itself (it must stay false while a command merely waits), and the
/// models page as a consumer of it: an open delete question survives a
/// re-projection rendered under `pending`, and a press on a card's control
/// during the wait still reaches nothing.
#[gpui_kit::test]
fn an_in_flight_command_never_flickers_the_models_page_gate(cx: &mut TestAppContext) {
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
    view.update(visual, |view, _| {
        view.snapshot = Some(seeded);
        view.pending = Some(PendingOperation::ModelSelection);
        // `request_model_delete` refuses while a command is in flight, so the
        // question is planted the way the confirm button leaves it: open, on a
        // card whose delete control the gate still draws.
        view.model_delete_confirmation = Some(model.clone());
    });
    visual.update(|window, cx| window.render_frame(cx));

    // The predicate itself: with a snapshot on hand, an idle import and no
    // picker, a merely waiting command must not block any page.
    assert!(
        !view.read_with(visual, |view, _| view
            .editing_blocked(view.snapshot.as_ref())),
        "the shared gate must not read the in-flight flag"
    );

    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        Some(model.clone()),
        "the page's gate must not read the in-flight flag: the question's \
         control stays drawn while a command waits"
    );

    // Keeping the controls drawn must not reopen the door for a second
    // command: the press is refused by the command methods, not by the paint.
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Activate, model.clone(), window, cx);
        });
    });
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "a press while a command is in flight must not reach the service"
    );
}

/// The frames before the first snapshot render the seeded appearance.
///
/// The window is created and shown while its first snapshot is still in flight, so
/// every frame in that gap has no snapshot to read. Resolving the defaults there is
/// what made a Simplified Chinese window paint one frame of English and then
/// re-render itself, which is the animation this pins down: the seed a window is
/// opened with has to answer until the snapshot replaces it, and the snapshot has
/// to win once it exists.
#[gpui_kit::test]
fn the_frames_before_the_first_snapshot_render_the_seeded_appearance(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::ChineseSimplified,
                    appearance_theme: SettingsTheme::Dark,
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
        assert!(
            view.snapshot.is_none(),
            "test premise: no snapshot has arrived yet"
        );
        assert_eq!(view.display_language(), SettingsLanguage::ChineseSimplified);
        assert_eq!(view.display_appearance_theme(), SettingsTheme::Dark);
        cx.notify();
    });
    visual.run_until_parked();

    // The snapshot is what the window renders from once it exists, seed or not.
    view.update(visual, |view, cx| {
        view.snapshot = Some(crate::tests::snapshot(1, true, false));
        cx.notify();
    });
    visual.run_until_parked();
    view.update(visual, |view, cx| {
        assert_eq!(
            view.display_language(),
            SettingsLanguage::EnglishUnitedStates,
            "an arrived snapshot must replace the seed"
        );
        cx.notify();
    });
    visual.run_until_parked();
}

#[gpui_kit::test]
fn model_import_success_waits_for_every_queued_cover_capture(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let first = SettingsModelKey {
        id: "first-installed".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let second = SettingsModelKey {
        id: "second-installed".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![
        model_entry(&first.id, first.origin, ready.clone()),
        model_entry(&second.id, second.origin, ready),
    ];
    let first_row = ModelRowKey::new(first.origin, &first.id);
    let second_row = ModelRowKey::new(second.origin, &second.id);

    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.model_import.baseline_models = BTreeSet::new();
        view.pending_model_reveal = BTreeSet::from([first_row.clone(), second_row.clone()]);
        view.model_import.state = ModelImportState::Capturing;
        view.model_import_success_pending = false;

        view.finish_model_cover_capture(&first, true, cx);
        assert!(!view.model_import_success_pending);
        assert!(matches!(
            &view.model_import.state,
            ModelImportState::Capturing
        ));
        assert_eq!(view.pending_model_reveal, BTreeSet::from([second_row]));

        view.finish_model_cover_capture(&second, true, cx);
        assert!(view.pending_model_reveal.is_empty());
        assert!(matches!(&view.model_import.state, ModelImportState::Idle));
        assert!(view.model_import_success_pending);
        assert!(!view.model_import_failed_pending);
    });
}

/// A capture that could not prepare the model abandons the import instead of
/// publishing it: the model is never revealed, the run is not reported as a
/// success, and the failure is reported in its place. The capture renders
/// through the same GPU path the overlay uses, so a model it cannot prepare is
/// a model the user could only fail to activate.
#[gpui_kit::test]
fn a_failed_cover_capture_abandons_the_import(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    let installed = SettingsModelKey {
        id: "unpreparable".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![model_entry(
        &installed.id,
        installed.origin,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    )];
    let row = ModelRowKey::new(installed.origin, &installed.id);

    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.model_import.baseline_models = BTreeSet::new();
        view.pending_model_reveal = BTreeSet::from([row]);
        view.model_import.state = ModelImportState::Capturing;
        view.model_import_success_pending = false;
        view.model_import_failed_pending = false;

        view.finish_model_cover_capture(&installed, false, cx);

        assert!(
            view.pending_model_reveal.is_empty(),
            "the model must stop being withheld so the card is never revealed"
        );
        assert!(matches!(&view.model_import.state, ModelImportState::Idle));
        assert!(
            view.model_import_failed_pending,
            "the failed capture must be reported to the user"
        );
        assert!(
            !view.model_import_success_pending,
            "an abandoned import is not a successful one"
        );
    });
}

#[gpui_kit::test]
fn cover_capture_completed_before_import_reply_skips_capturing(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    let installed = SettingsModelKey {
        id: "installed".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![model_entry(
        &installed.id,
        installed.origin,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    )];
    let installed_row = ModelRowKey::new(installed.origin, &installed.id);

    view.update(visual, |view, _cx| {
        view.snapshot = Some(seeded);
        view.completed_model_cover_captures = BTreeSet::from([installed_row]);
        view.model_import.baseline_models = BTreeSet::new();
        view.model_import.state = ModelImportState::Starting {
            cancel_requested: false,
        };
        view.model_import_success_pending = false;

        let reveal_complete = view.begin_model_reveal();
        if reveal_complete {
            view.model_import_success_pending = true;
        }

        assert!(reveal_complete);
        assert!(matches!(&view.model_import.state, ModelImportState::Idle));
        assert!(view.pending_model_reveal.is_empty());
        assert!(view.model_import_success_pending);
        assert!(view.completed_model_cover_captures.is_empty());
    });
}

#[gpui_kit::test]
fn empty_successful_import_resets_and_notifies_immediately(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);

    view.update(visual, |view, _cx| {
        view.snapshot = Some(crate::tests::snapshot(1, false, true));
        view.model_import.baseline_models = BTreeSet::new();
        view.model_import.state = ModelImportState::Starting {
            cancel_requested: false,
        };
        view.model_import_success_pending = false;

        let reveal_complete = view.begin_model_reveal();
        if reveal_complete {
            view.model_import_success_pending = true;
        }

        assert!(reveal_complete);
        assert!(matches!(&view.model_import.state, ModelImportState::Idle));
        assert!(view.pending_model_reveal.is_empty());
        assert!(view.model_import_success_pending);
    });
}
