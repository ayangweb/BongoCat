use super::*;

/// Model cards are a fixed width so the grid stays a grid. Covers arrive at
/// several aspect ratios, and sizing each card around its own artwork would
/// ragged every column.
const MODEL_CARD_WIDTH: f32 = 240.0;
/// Height of the cover area. A cover is cropped into this box rather than
/// resized around it, so one unusual image cannot resize the whole page.
const MODEL_CARD_COVER_HEIGHT: f32 = 140.0;

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
    let model_id_disabled = import_running || picker_open;
    let model_commands_blocked = import_running || picker_open || view.pending.is_some();
    let model_entries = snapshot
        .as_ref()
        .map(|snapshot| snapshot.model_catalog.entries.clone())
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
            if editing {
                card.child(edit_model_card(view, window, cx, language, tokens))
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
            }
        })
        .collect::<Vec<_>>();
    // The page shell renders the title, and progress is never rendered on the
    // page: buttons carry their own disabled/cancel state and errors go through
    // notifications, so the content is just the list — nothing that occupies
    // layout space announces a transient state.
    div()
        .min_w_0()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .text_color(tokens.text)
        .id("models-content")
        .child(
            div()
                .id("model-catalog")
                .min_h_0()
                .flex_1()
                .flex()
                .flex_wrap()
                .items_start()
                .gap_3()
                .overflow_y_scroll()
                .child(model_import_card(
                    view,
                    window,
                    cx,
                    import_running,
                    picker_open,
                    model_id_disabled,
                    language,
                    tokens,
                ))
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
    let mut row = div().flex().items_center().gap_1().child(
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
/// Importing is the only way a model reaches the page, so it keeps its whole
/// flow — name, source, commit — in one place instead of spreading it across
/// the toolbar and the cards.
#[allow(clippy::too_many_arguments)]
fn model_import_card(
    view: &SettingsView,
    window: &Window,
    cx: &mut Context<SettingsView>,
    import_running: bool,
    picker_open: bool,
    model_id_disabled: bool,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Stateful<Div> {
    let picker_disabled = import_running || picker_open || view.pending.is_some();
    let picker_button_label = if picker_open {
        bongocat_i18n::text(language.catalog_locale(), "models.import.actions.choosing")
    } else {
        bongocat_i18n::text(
            language.catalog_locale(),
            "models.import.actions.choose_folder",
        )
    };
    let archive_button_label = if picker_open {
        bongocat_i18n::text(language.catalog_locale(), "models.import.actions.choosing")
    } else {
        bongocat_i18n::text(
            language.catalog_locale(),
            "models.import.actions.choose_archive",
        )
    };
    let import_button_label = if import_running {
        bongocat_i18n::text(language.catalog_locale(), "actions.cancel")
    } else {
        bongocat_i18n::text(language.catalog_locale(), "models.import.actions.import")
    };
    let import_disabled =
        !import_running && (!view.model_import.can_import() || view.pending.is_some());
    div()
        .id("model-import-card")
        .flex_none()
        .w(px(MODEL_CARD_WIDTH))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_dashed()
        .border_color(tokens.border)
        .bg(tokens.canvas)
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .py_2()
                .child(
                    Icon::new(IconName::ArrowUp)
                        .w(px(24.0))
                        .h(px(24.0))
                        .text_color(tokens.muted),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(tokens.muted)
                        .child(bongocat_i18n::text(
                            language.catalog_locale(),
                            "models.import.title",
                        )),
                ),
        )
        .child(
            div()
                .id("model-title-input")
                .key_context("SettingsModelTitle")
                .track_focus(&view.model_id_focus)
                .tab_index(20)
                .w_full()
                .on_click(cx.listener(|view, _, window, cx| {
                    if !view.model_import.is_running() {
                        window.focus(&view.model_id_focus, cx);
                    }
                }))
                .child(Input::new(&view.model_id_input))
                .when(model_id_disabled, |input| input.opacity(0.6)),
        )
        .child(
            command_button(
                picker_button_label,
                &view.choose_model_focus,
                21,
                window,
                tokens,
                picker_disabled,
            )
            .w_full()
            .id("choose-model-directory")
            .on_click(cx.listener(|view, _, window, cx| {
                if !view.model_import.is_running()
                    && !view.model_import.is_picker_open()
                    && view.pending.is_none()
                {
                    window.focus(&view.choose_model_focus, cx);
                    view.choose_model_source(ModelSourceKind::Directory, cx);
                }
            }))
            .on_key_down(cx.listener(|view, event, window, cx| {
                if !view.model_import.is_running()
                    && !view.model_import.is_picker_open()
                    && view.pending.is_none()
                    && is_activation_key(event)
                {
                    cx.stop_propagation();
                    window.focus(&view.choose_model_focus, cx);
                    view.choose_model_source(ModelSourceKind::Directory, cx);
                }
            })),
        )
        .child(
            command_button(
                archive_button_label,
                &view.choose_archive_focus,
                22,
                window,
                tokens,
                picker_disabled,
            )
            .w_full()
            .id("choose-model-archive")
            .on_click(cx.listener(|view, _, window, cx| {
                if !view.model_import.is_running()
                    && !view.model_import.is_picker_open()
                    && view.pending.is_none()
                {
                    window.focus(&view.choose_archive_focus, cx);
                    view.choose_model_source(ModelSourceKind::Archive, cx);
                }
            }))
            .on_key_down(cx.listener(|view, event, window, cx| {
                if !view.model_import.is_running()
                    && !view.model_import.is_picker_open()
                    && view.pending.is_none()
                    && is_activation_key(event)
                {
                    cx.stop_propagation();
                    window.focus(&view.choose_archive_focus, cx);
                    view.choose_model_source(ModelSourceKind::Archive, cx);
                }
            })),
        )
        .child(
            command_button(
                import_button_label,
                &view.import_model_focus,
                23,
                window,
                tokens,
                import_disabled,
            )
            .w_full()
            .id("import-model")
            .on_click(cx.listener(move |view, _, window, cx| {
                if !import_disabled {
                    window.focus(&view.import_model_focus, cx);
                    if import_running {
                        view.cancel_model_import(cx);
                    } else {
                        view.start_model_import(cx);
                    }
                }
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if !import_disabled && is_activation_key(event) {
                    cx.stop_propagation();
                    window.focus(&view.import_model_focus, cx);
                    if import_running {
                        view.cancel_model_import(cx);
                    } else {
                        view.start_model_import(cx);
                    }
                }
            })),
        )
}

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
