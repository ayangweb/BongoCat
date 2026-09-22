use super::*;
use gpui_kit::base::TestSupportExt as _;

/// Model cards are a fixed width so the grid stays a grid. Covers arrive at
/// several aspect ratios, and sizing each card around its own artwork would
/// ragged every column. The import card is a cell in the same grid, so it takes
/// its width from here rather than repeating the number.
pub(super) const MODEL_CARD_WIDTH: f32 = 240.0;
/// Height of the cover area. A cover is cropped into this box rather than
/// resized around it, so one unusual image cannot resize the whole page.
const MODEL_CARD_COVER_HEIGHT: f32 = 140.0;
/// The height a model card occupies when its summary is one line: the card's own
/// padding, the cover, the title and the action row.
///
/// It is not the card's height — a row is as tall as its tallest cell, and a card
/// carrying a status line is taller than this — but it is the height the import
/// card floors itself at, so the grid's first cell keeps the shape of the cells
/// beside it even when it is the only cell on its row.
pub(super) const MODEL_CARD_MIN_HEIGHT: f32 = 232.0;

/// The grid every model cell lives in.
///
/// Cells wrap, and every cell on a row is stretched to that row's tallest one, so
/// the import card — the grid's first cell — is exactly as tall as the model
/// cards beside it without either of them carrying the other's height.
/// `content_start` is what keeps a row at its content height: a wrapped flex
/// container stretches its lines to fill the scroll area by default, which would
/// size the cells to the window instead of to the cards.
pub(super) fn model_grid() -> Stateful<Div> {
    div()
        .id("model-catalog")
        .min_h_0()
        .flex_1()
        .flex()
        .flex_wrap()
        .items_stretch()
        .content_start()
        .gap_3()
        .overflow_y_scroll()
}

pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
        snapshot.resolved_language
    });
    let import_running = view.model_import.is_running();
    let picker_open = view.model_import.is_picker_open();
    let model_commands_blocked = import_running || picker_open || view.pending.is_some();
    // A model the running import just installed is withheld until its cover
    // capture reports back, so the grid only ever shows the finished card: the
    // alternative is a card that appears with the source package's placeholder
    // picture and swaps it a moment later.
    let model_entries = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .filter(|entry| {
                    !view
                        .pending_model_reveal
                        .contains(&ModelRowKey::new(entry.origin, &entry.id))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let active_model = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.active_model.as_ref());
    view.sync_model_row_focus(&model_entries, active_model, model_commands_blocked, cx);
    let catalog_error = snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.model_catalog.error.is_some());
    // An unreadable catalog is an error, and an error on this page is a
    // notification. It is pushed on the transition into the failure so a
    // catalog that stays unreadable does not repeat the same message on every
    // snapshot; the grid's placeholder stays neutral for the same reason.
    if catalog_error && !view.model_catalog_error_reported {
        view.model_catalog_error_reported = true;
        window.push_notification(
            Notification::new()
                .id::<ModelCatalogErrorNotification>()
                .message(bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.catalog.load_failed",
                ))
                .with_type(NotificationType::Error),
            cx,
        );
    } else if !catalog_error {
        view.model_catalog_error_reported = false;
    }
    let model_cards = model_entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let model = SettingsModelKey {
                id: entry.id.clone(),
                origin: entry.origin,
            };
            let actions = model_row_actions(&entry, active_model, model_commands_blocked);
            let confirming_delete = view.model_delete_confirmation.as_ref() == Some(&model);
            let editing = view
                .model_edit
                .as_ref()
                .is_some_and(|draft| draft.model == model);
            let focus = view
                .model_row_focus
                .get(&ModelRowKey::new(entry.origin, &entry.id))
                .expect("model card focus is synchronized")
                .clone();
            let tab_index = 40_isize.saturating_add(
                isize::try_from(index)
                    .unwrap_or(isize::MAX / 5)
                    .saturating_mul(5),
            );
            let action_tabs = model_row_action_tab_indices(tab_index);
            // The status line belongs to the card, and the confirmation is a
            // surface anchored to the delete control, so the diagnostic a broken
            // package carries stays visible while the question is on screen.
            let status = model_availability_status(&entry, language);
            let activate_label = if actions.active {
                bongocat_i18n::text(language.catalog_locale(), "models.identity.status.active")
            } else if matches!(
                &entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) {
                bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.identity.status.unavailable",
                )
            } else {
                bongocat_i18n::text(language.catalog_locale(), "models.actions.activate")
            };
            let cover = if editing {
                let draft = view
                    .model_edit
                    .as_ref()
                    .expect("editing card has an open draft");
                draft.cover.clone().or_else(|| entry.cover.clone())
            } else {
                entry.cover.clone()
            };
            let card = div()
                .id(("model-card", index))
                .flex_none()
                .w(px(MODEL_CARD_WIDTH))
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_lg()
                .border_1()
                .border_color(if actions.active {
                    tokens.accent
                } else {
                    tokens.border
                })
                .bg(tokens.canvas)
                .child(model_card_cover(cover, language, tokens));
            // The card is observed like the import card is, so a test can read the
            // two cells' geometry: the grid's whole job is to make them match.
            if editing {
                card.child(edit_model_card(view, window, cx, language, tokens))
                    .test_support()
            } else {
                card.child(model_card_summary(&entry, actions.active, status, tokens))
                    .child(model_card_actions(
                        window,
                        cx,
                        index,
                        &model,
                        activate_label,
                        actions,
                        confirming_delete,
                        action_tabs,
                        &focus,
                        language,
                        tokens,
                    ))
                    .test_support()
            }
        })
        .collect::<Vec<_>>();
    // The page shell renders the title, and the only place progress is drawn is
    // the import card itself — buttons carry their own disabled/cancel state and
    // errors go through notifications — so the content is just the grid, and
    // nothing outside it grows or shrinks as a command comes and goes.
    div()
        .min_w_0()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .text_color(tokens.text)
        .id("models-content")
        .child(
            model_grid()
                .child(model_import_card(view, cx, language))
                .children(model_cards),
        )
}

/// The cover area of a card: the image when the package ships one, and an
/// explicit placeholder when it does not. A package without a cover is an
/// ordinary package, so the placeholder is neutral rather than a warning.
fn model_card_cover(cover: Option<PathBuf>, language: SettingsLanguage, tokens: Tokens) -> Div {
    let frame = div()
        .flex_none()
        .w_full()
        .h(px(MODEL_CARD_COVER_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas);
    match cover {
        Some(cover) => frame.child(img(cover).w_full().h_full().object_fit(ObjectFit::Cover)),
        None => frame.child(
            div()
                .text_sm()
                .text_color(tokens.muted)
                .child(bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.card.cover_missing",
                )),
        ),
    }
}

fn model_card_summary(
    entry: &SettingsModelEntry,
    active: bool,
    status: Option<SharedString>,
    tokens: Tokens,
) -> Div {
    let mut summary = div().flex().flex_col().gap_1().child(
        div()
            .min_w_0()
            .w_full()
            .truncate()
            .text_color(if active { tokens.accent } else { tokens.text })
            .child(entry.title.clone()),
    );
    // The status line only exists to carry a diagnostic or a delete
    // confirmation; a healthy card shows nothing but its title.
    if let Some(status) = status {
        summary = summary.child(div().text_sm().text_color(tokens.muted).child(status));
    }
    summary
}

/// The action row of a card.
///
/// Activating is the only labelled control: it is the page's core action, and
/// the icon actions are the ones that leave the page. Deleting is the one that
/// cannot be undone, so its control owns a confirmation surface rather than
/// acting on the first press — the row keeps all four controls either way, and
/// the question appears next to the control that asked it.
#[allow(clippy::too_many_arguments)]
fn model_card_actions(
    window: &Window,
    cx: &mut Context<SettingsView>,
    index: usize,
    model: &SettingsModelKey,
    activate_label: &'static str,
    actions: ModelRowActions,
    confirming_delete: bool,
    action_tabs: ModelRowActionTabIndices,
    focus: &ModelRowFocus,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Div {
    let activate_model = model.clone();
    let activate_key_model = model.clone();
    // The grid stretches a card to its row's tallest cell, so the controls are
    // pushed to the card's bottom edge: every card on a row then lines its
    // buttons up, whatever its own summary turned out to be worth.
    let mut row = div().flex().items_center().gap_1().mt_auto().child(
        command_button(
            activate_label,
            &focus.activate,
            action_tabs.activate,
            window,
            tokens,
            !actions.can_activate,
        )
        .flex_1()
        .id(("activate-model", index))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.run_model_row_action(ModelRowAction::Activate, activate_model.clone(), window, cx);
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if is_activation_key(event) {
                cx.stop_propagation();
                view.run_model_row_action(
                    ModelRowAction::Activate,
                    activate_key_model.clone(),
                    window,
                    cx,
                );
            }
        })),
    );
    let location_model = model.clone();
    let location_key_model = model.clone();
    row = row.child(
        icon_command_button(
            "open-model-location-control",
            bongocat_i18n::text(language.catalog_locale(), "models.actions.open_location"),
            IconName::FolderOpen,
            &focus.open_location,
            action_tabs.open_location,
            !actions.can_open_location,
        )
        .id(("open-model-location", index))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.run_model_row_action(
                ModelRowAction::OpenLocation,
                location_model.clone(),
                window,
                cx,
            );
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if is_activation_key(event) {
                cx.stop_propagation();
                view.run_model_row_action(
                    ModelRowAction::OpenLocation,
                    location_key_model.clone(),
                    window,
                    cx,
                );
            }
        })),
    );
    if actions.can_edit {
        let edit_model = model.clone();
        let edit_key_model = model.clone();
        row = row.child(
            icon_command_button(
                "edit-model-control",
                bongocat_i18n::text(language.catalog_locale(), "models.actions.edit"),
                gpui_kit::assets::IconName::SquarePen,
                &focus.edit,
                action_tabs.edit,
                !actions.can_edit,
            )
            .id(("edit-model", index))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.run_model_row_action(ModelRowAction::Edit, edit_model.clone(), window, cx);
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    view.run_model_row_action(
                        ModelRowAction::Edit,
                        edit_key_model.clone(),
                        window,
                        cx,
                    );
                }
            })),
        );
    }
    if actions.can_delete {
        let confirm_model = model.clone();
        let open_model = model.clone();
        let close_model = model.clone();
        let delete_label = bongocat_i18n::text(language.catalog_locale(), "models.actions.delete");
        row = row.child(
            PopConfirm::new(
                ("delete-model-confirmation", index),
                bongocat_i18n::text(language.catalog_locale(), "models.delete_confirmation"),
            )
            // The control sits at the trailing edge of the card, so the surface
            // grows back over the card instead of past the window edge.
            .anchor(Anchor::TopRight)
            .icon(gpui_kit::assets::IconName::TriangleAlert, tokens.danger)
            .confirm_label(bongocat_i18n::text(
                language.catalog_locale(),
                "actions.confirm",
            ))
            .cancel_label(bongocat_i18n::text(
                language.catalog_locale(),
                "actions.cancel",
            ))
            .trigger(
                icon_command_button(
                    "delete-model-control",
                    delete_label,
                    gpui_kit::assets::IconName::Trash,
                    &focus.delete,
                    action_tabs.delete,
                    false,
                )
                // The wrapper owns the queryable id; the inner control gets a
                // derived one so the two registrations cannot be ambiguous.
                .id(("delete-model", index)),
            )
            .open(confirming_delete)
            // The surface reports every transition — the control's press or
            // Enter, Escape, a press outside — and this is where the page decides
            // which of them are its business. The page's own `open` value is not
            // echoed back, so writing the state here cannot loop.
            .on_open_change(cx.listener(move |view, open: &bool, _, cx| {
                if *open {
                    view.request_model_delete(open_model.clone(), cx);
                } else {
                    view.cancel_model_delete(&close_model, cx);
                }
            }))
            .on_confirm({
                let view = cx.entity().downgrade();
                move |window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.run_model_row_action(
                            ModelRowAction::Delete,
                            confirm_model.clone(),
                            window,
                            cx,
                        );
                    });
                }
            }),
        );
    }
    row
}

/// The body of a card that is being edited: the cover picker, the title field
/// and the save/cancel pair.
///
/// Editing stays inside the card it belongs to, so the model under edit is
/// always the model the card already shows and there is no separate edit view
/// that could disagree with the catalog it was opened from.
fn edit_model_card(
    view: &SettingsView,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Div {
    let Some(draft) = view.model_edit.as_ref() else {
        return div();
    };
    let cover_label = if draft.cover.is_some() {
        bongocat_i18n::text(language.catalog_locale(), "models.edit.cover.replace")
    } else {
        bongocat_i18n::text(language.catalog_locale(), "models.edit.cover.choose")
    };
    let cover_focus = draft.cover_focus.clone();
    let cover_key_focus = cover_focus.clone();
    let save_focus = draft.save_focus.clone();
    let save_key_focus = save_focus.clone();
    let cancel_focus = draft.cancel_focus.clone();
    let cancel_key_focus = cancel_focus.clone();
    let input_focus = draft.input_focus.clone();
    let input = draft.input.clone();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            command_button(
                cover_label,
                &cover_focus,
                MODEL_EDIT_COVER_TAB_INDEX,
                window,
                tokens,
                draft.picking,
            )
            .w_full()
            .id("choose-model-cover")
            .on_click(cx.listener(move |view, _, window, cx| {
                if !view.model_edit.as_ref().is_some_and(|draft| draft.picking) {
                    window.focus(&cover_focus, cx);
                    view.choose_model_cover(cx);
                }
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if is_activation_key(event)
                    && !view.model_edit.as_ref().is_some_and(|draft| draft.picking)
                {
                    cx.stop_propagation();
                    window.focus(&cover_key_focus, cx);
                    view.choose_model_cover(cx);
                }
            })),
        )
        .child(
            div()
                .id("model-edit-title-input")
                .key_context("SettingsModelEditTitle")
                .track_focus(&input_focus)
                .tab_index(MODEL_EDIT_TITLE_TAB_INDEX)
                .w_full()
                .on_click(cx.listener(move |view, _, window, cx| {
                    if let Some(draft) = view.model_edit.as_ref() {
                        let focus = draft.input_focus.clone();
                        window.focus(&focus, cx);
                    }
                }))
                .child(Input::new(&input)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    command_button(
                        bongocat_i18n::text(language.catalog_locale(), "models.actions.save"),
                        &save_focus,
                        MODEL_EDIT_SAVE_TAB_INDEX,
                        window,
                        tokens,
                        false,
                    )
                    .flex_1()
                    .id("save-model-edit")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&save_focus, cx);
                        view.save_model_edit(cx);
                    }))
                    .on_key_down(cx.listener(
                        move |view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                window.focus(&save_key_focus, cx);
                                view.save_model_edit(cx);
                            }
                        },
                    )),
                )
                .child(
                    command_button(
                        bongocat_i18n::text(language.catalog_locale(), "actions.cancel"),
                        &cancel_focus,
                        MODEL_EDIT_CANCEL_TAB_INDEX,
                        window,
                        tokens,
                        false,
                    )
                    .id("cancel-model-edit")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&cancel_focus, cx);
                        view.cancel_model_edit(cx);
                    }))
                    .on_key_down(cx.listener(
                        move |view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                window.focus(&cancel_key_focus, cx);
                                view.cancel_model_edit(cx);
                            }
                        },
                    )),
                ),
        )
}

/// The import entry, as the first cell of the grid.
///
/// Importing is the only way a model reaches the page, so one card owns the
/// whole flow: a press opens the folder picker, the selection starts the run on
/// the spot, and the same card reports the step until the new model is ready to
/// be shown.
fn model_import_card(
    view: &SettingsView,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
) -> ModelImportCard {
    let locale = language.catalog_locale();
    // A native dialog or another page command in flight leaves the card visible
    // but inert; during the run itself it shows progress instead of the prompt.
    let ready_for_input = !view.model_import.is_running()
        && !view.model_import.is_picker_open()
        && view.pending.is_none();
    let mut card = ModelImportCard::new(
        "model-import-card",
        bongocat_i18n::text(locale, "models.import.title"),
    )
    .hint(bongocat_i18n::text(locale, "models.import.hint"))
    .step(import_card_step(&view.model_import, language))
    .interactive(ready_for_input)
    .track_focus(Some(view.import_card_focus.clone()))
    .tab_index(20)
    .on_open({
        let view = cx.entity().downgrade();
        move |_, cx| {
            let _ = view.update(cx, |view, cx| view.choose_model_source(cx));
        }
    });
    if view.model_import.shows_cancel() {
        card = card.cancel(
            bongocat_i18n::text(locale, "actions.cancel"),
            !view.model_import.is_cancellable(),
            {
                let view = cx.entity().downgrade();
                move |_, cx| {
                    let _ = view.update(cx, |view, cx| view.cancel_model_import(cx));
                }
            },
        );
    }
    card
}

/// The step the import card shows, or `None` while the card is the upload prompt.
///
/// The phases are named rather than counted, so a wait with two halves says which
/// half it is in — but only the half that is running comes back. The capture
/// replaces the import line instead of being appended under it: the card is one
/// grid cell the size of a model card, and a growing list of finished steps would
/// turn a two-phase wait into a log that cell cannot hold.
pub(super) fn import_card_step(
    draft: &ModelImportDraft,
    language: SettingsLanguage,
) -> Option<SharedString> {
    let step = |key: &str| Some(bongocat_i18n::text(language.catalog_locale(), key).into());
    match &draft.state {
        ModelImportState::Idle => None,
        ModelImportState::Picking => step("models.import.step.choosing"),
        ModelImportState::Starting { .. } | ModelImportState::Running(_) => {
            step("models.import.step.importing")
        }
        ModelImportState::Capturing => step("models.import.step.capturing"),
    }
}

#[cfg(test)]
pub(super) fn empty_model_catalog_status(
    catalog: Option<&crate::SettingsModelCatalog>,
    language: SettingsLanguage,
) -> &'static str {
    match catalog {
        None => bongocat_i18n::text(language.catalog_locale(), "models.catalog.loading"),
        Some(catalog) if catalog.error.is_some() => {
            bongocat_i18n::text(language.catalog_locale(), "models.catalog.unavailable")
        }
        Some(_) => bongocat_i18n::text(language.catalog_locale(), "models.catalog.empty"),
    }
}
