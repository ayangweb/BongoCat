use super::*;

pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    tokens: Tokens,
) -> Stateful<Div> {
    let (import_status, import_failed) = model_import_status(&view.model_import);
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
    let mut model_rows = model_entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let model = SettingsModelKey {
                id: entry.id.clone(),
                origin: entry.origin,
            };
            let actions = model_row_actions(&entry, active_model, model_commands_blocked);
            let confirming_delete = view.model_delete_confirmation.as_ref() == Some(&model);
            let focus = view
                .model_row_focus
                .get(&ModelRowKey::new(entry.origin, &entry.id))
                .expect("model row focus is synchronized")
                .clone();
            let tab_index = 40_isize.saturating_add(
                isize::try_from(index)
                    .unwrap_or(isize::MAX / 3)
                    .saturating_mul(3),
            );
            let action_tabs = model_row_action_tab_indices(tab_index, confirming_delete);
            let availability = if confirming_delete {
                format!(
                    "{} · Confirm deletion",
                    model_availability_status(&entry, actions.active)
                )
                .into()
            } else {
                model_availability_status(&entry, actions.active)
            };
            let activate_label = if actions.active {
                "Active"
            } else if matches!(
                entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) {
                "Unavailable"
            } else {
                "Activate"
            };
            let activate_model = model.clone();
            let activate_key_model = model.clone();
            let mut actions_row = div().flex().items_center().gap_2().child(
                command_button(
                    activate_label,
                    &focus.activate,
                    action_tabs.activate,
                    window,
                    tokens,
                    !actions.can_activate,
                )
                .w(px(92.0))
                .id(("activate-model", index))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.run_model_row_action(
                        ModelRowAction::Activate,
                        activate_model.clone(),
                        window,
                        cx,
                    );
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
            if actions.can_delete {
                if confirming_delete {
                    let cancel_model = model.clone();
                    let cancel_key_model = model.clone();
                    actions_row = actions_row.child(
                        command_button(
                            "Cancel",
                            &focus.cancel_delete,
                            action_tabs.cancel_delete,
                            window,
                            tokens,
                            false,
                        )
                        .id(("cancel-delete-model", index))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.run_model_row_action(
                                ModelRowAction::CancelDelete,
                                cancel_model.clone(),
                                window,
                                cx,
                            );
                        }))
                        .on_key_down(cx.listener(
                            move |view, event, window, cx| {
                                if is_activation_key(event) {
                                    cx.stop_propagation();
                                    view.run_model_row_action(
                                        ModelRowAction::CancelDelete,
                                        cancel_key_model.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            },
                        )),
                    );
                }
                let delete_model = model.clone();
                let delete_key_model = model.clone();
                actions_row = actions_row.child(
                    command_button(
                        if confirming_delete {
                            "Confirm"
                        } else {
                            "Delete"
                        },
                        &focus.delete,
                        action_tabs.delete,
                        window,
                        tokens,
                        false,
                    )
                    .id(("delete-model", index))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.run_model_row_action(
                            ModelRowAction::Delete,
                            delete_model.clone(),
                            window,
                            cx,
                        );
                    }))
                    .on_key_down(cx.listener(
                        move |view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                view.run_model_row_action(
                                    ModelRowAction::Delete,
                                    delete_key_model.clone(),
                                    window,
                                    cx,
                                );
                            }
                        },
                    )),
                );
            }
            GroupBox::new().outline().child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_color(tokens.text)
                                    .child(entry.id),
                            )
                            .child(actions_row),
                    )
                    .child(div().text_sm().text_color(tokens.muted).child(availability)),
            )
        })
        .collect::<Vec<_>>();
    let catalog_error = snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.model_catalog.error.is_some());
    if model_rows.is_empty() {
        let empty_status = if snapshot.is_none() {
            "Loading models..."
        } else if catalog_error {
            "Model catalog is unavailable"
        } else {
            "No models available"
        };
        model_rows.push(
            GroupBox::new().outline().child(
                div()
                    .py_4()
                    .text_sm()
                    .text_color(if catalog_error {
                        tokens.danger
                    } else {
                        tokens.muted
                    })
                    .child(empty_status),
            ),
        )
    }
    let (management_status, management_failed): (SharedString, bool) =
        match (&view.error, view.pending, catalog_error) {
            (Some(error), _, _) => (error.to_string().into(), true),
            (_, Some(PendingOperation::ModelSelection), _) => ("Activating model...".into(), false),
            (_, Some(PendingOperation::ModelDeletion), _) => ("Deleting model...".into(), false),
            (_, Some(PendingOperation::Refresh), _) => ("Refreshing models...".into(), false),
            (_, _, true) => ("Catalog unavailable".into(), true),
            _ => ("".into(), false),
        };
    let picker_disabled = import_running || picker_open || view.pending.is_some();
    let picker_button_label = if picker_open {
        "Choosing..."
    } else {
        "Choose folder"
    };
    let import_button_label = if import_running { "Cancel" } else { "Import" };
    let import_disabled =
        !import_running && (!view.model_import.can_import() || view.pending.is_some());
    div()
        .min_w_0()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .bg(tokens.canvas)
        .text_color(tokens.text)
        .id("models-content")
        .child(div().text_2xl().child("Models"))
        .child(
            GroupBox::new().outline().child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(
                                div().min_w_0().flex_1().flex().flex_col().gap_1().child(
                                    div()
                                        .id("model-id-input")
                                        .key_context("SettingsModelId")
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
                                ),
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
                                .w(px(112.0))
                                .id("choose-model-directory")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    if !view.model_import.is_running()
                                        && !view.model_import.is_picker_open()
                                        && view.pending.is_none()
                                    {
                                        window.focus(&view.choose_model_focus, cx);
                                        view.choose_model_directory(cx);
                                    }
                                }))
                                .on_key_down(cx.listener(
                                    |view, event, window, cx| {
                                        if !view.model_import.is_running()
                                            && !view.model_import.is_picker_open()
                                            && view.pending.is_none()
                                            && is_activation_key(event)
                                        {
                                            cx.stop_propagation();
                                            window.focus(&view.choose_model_focus, cx);
                                            view.choose_model_directory(cx);
                                        }
                                    },
                                )),
                            )
                            .child(
                                command_button(
                                    import_button_label,
                                    &view.import_model_focus,
                                    22,
                                    window,
                                    tokens,
                                    import_disabled,
                                )
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
                                .on_key_down(cx.listener(
                                    move |view, event, window, cx| {
                                        if !import_disabled && is_activation_key(event) {
                                            cx.stop_propagation();
                                            window.focus(&view.import_model_focus, cx);
                                            if import_running {
                                                view.cancel_model_import(cx);
                                            } else {
                                                view.start_model_import(cx);
                                            }
                                        }
                                    },
                                )),
                            ),
                    )
                    .child(if import_failed {
                        Tag::danger().child(import_status).into_any_element()
                    } else {
                        Tag::secondary().child(import_status).into_any_element()
                    }),
            ),
        )
        .child(
            div()
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .text_sm()
                .child(div().text_color(tokens.muted).child("Available models"))
                .child(
                    div()
                        .min_w_0()
                        .text_color(if management_failed {
                            tokens.danger
                        } else {
                            tokens.muted
                        })
                        .child(management_status),
                ),
        )
        .child(
            div()
                .id("model-catalog")
                .min_h_0()
                .flex_1()
                .overflow_y_scroll()
                .children(model_rows),
        )
}
