use super::*;

pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    disabled: bool,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
        snapshot.resolved_language
    });
    let tab = view.shortcut_tab;
    let view_entity = cx.entity();
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
        .id("shortcuts-content")
        .child(div().text_2xl().child(ui_text(language, UiText::Shortcuts)))
        .child(
            div()
                .text_sm()
                .text_color(tokens.muted)
                .child(ui_text(language, UiText::ShortcutsDescription)),
        )
        .child(
            TabBar::new("shortcut-settings-tabs")
                .segmented()
                .selected_index(match tab {
                    ShortcutSettingsTab::Window => 0,
                    ShortcutSettingsTab::Model => 1,
                })
                .child(Tab::new().label(ui_text(language, UiText::WindowShortcuts)))
                .child(Tab::new().label(ui_text(language, UiText::ModelShortcuts)))
                .on_click(move |index, _, app| {
                    let tab = if *index == 0 {
                        ShortcutSettingsTab::Window
                    } else {
                        ShortcutSettingsTab::Model
                    };
                    view_entity.update(app, |view, cx| {
                        view.shortcut_tab = tab;
                        view.shortcut_capture = None;
                        view.shortcut_capture_error = None;
                        cx.notify();
                    });
                }),
        )
        .when_some(snapshot, |content, snapshot| {
            let rows = match tab {
                ShortcutSettingsTab::Window => window_shortcut_rows(&snapshot.shortcuts),
                ShortcutSettingsTab::Model => shortcut_behavior_rows(
                    &snapshot.shortcuts,
                    snapshot.active_model.as_ref(),
                    &snapshot.model_catalog.entries,
                ),
            };
            let row_offset = match tab {
                ShortcutSettingsTab::Window => 0,
                ShortcutSettingsTab::Model => window_shortcut_rows(&snapshot.shortcuts).len(),
            };
            content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().text_sm().text_color(tokens.muted).child(match tab {
                            ShortcutSettingsTab::Window => {
                                ui_text(language, UiText::WindowShortcuts)
                            }
                            ShortcutSettingsTab::Model => ui_text(language, UiText::ModelShortcuts),
                        }))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    command_button(
                                        ui_text(language, UiText::RestoreDefaults),
                                        &view.restore_shortcuts_focus,
                                        33,
                                        window,
                                        tokens,
                                        disabled,
                                    )
                                    .id("restore-default-shortcuts")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        window.focus(&view.restore_shortcuts_focus, cx);
                                        view.restore_default_shortcuts(cx);
                                    }))
                                    .on_key_down(cx.listener(|view, event, window, cx| {
                                        if is_activation_key(event) {
                                            cx.stop_propagation();
                                            window.focus(&view.restore_shortcuts_focus, cx);
                                            view.restore_default_shortcuts(cx);
                                        }
                                    })),
                                )
                                .child(
                                    command_button(
                                        ui_text(language, UiText::ClearAll),
                                        &view.clear_shortcuts_focus,
                                        34,
                                        window,
                                        tokens,
                                        disabled
                                            || snapshot.shortcuts.commands.is_empty()
                                                && snapshot.shortcuts.model_behaviors.is_empty(),
                                    )
                                    .id("clear-shortcuts")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        window.focus(&view.clear_shortcuts_focus, cx);
                                        view.clear_shortcuts(cx);
                                    }))
                                    .on_key_down(cx.listener(|view, event, window, cx| {
                                        if is_activation_key(event) {
                                            cx.stop_propagation();
                                            window.focus(&view.clear_shortcuts_focus, cx);
                                            view.clear_shortcuts(cx);
                                        }
                                    })),
                                ),
                        ),
                )
                .when_some(view.shortcut_capture_error, |content, error| {
                    content.child(
                        div()
                            .text_sm()
                            .text_color(tokens.danger)
                            .child(shortcut_capture_error(language, error)),
                    )
                })
                .when_some(view.shortcut_capture.clone(), |content, target| {
                    content.child(
                        div()
                            .text_sm()
                            .text_color(tokens.accent)
                            .child(match target {
                                ShortcutCaptureTarget::Command(_) => {
                                    ui_text(language, UiText::PressCommandShortcut)
                                }
                                ShortcutCaptureTarget::ModelBehavior { .. } => {
                                    ui_text(language, UiText::PressBehaviorShortcut)
                                }
                            }),
                    )
                })
                .when(rows.is_empty(), |content| {
                    content.child(
                        div()
                            .text_sm()
                            .text_color(tokens.muted)
                            .child(ui_text(language, UiText::NoModelBehaviors)),
                    )
                })
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    shortcut_row(
                        view,
                        window,
                        cx,
                        language,
                        disabled,
                        tokens,
                        row,
                        row_offset + index,
                        tab,
                        index,
                    )
                }))
        })
}

#[allow(clippy::too_many_arguments)]
fn shortcut_row(
    view: &SettingsView,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    disabled: bool,
    tokens: Tokens,
    row: ShortcutRow,
    row_index: usize,
    tab: ShortcutSettingsTab,
    tab_index: usize,
) -> Div {
    let target = row.target;
    let target_name = shortcut_target_name(language, &target);
    let capturing = view.shortcut_capture.as_ref() == Some(&target);
    let focus = view
        .shortcut_row_focus
        .get(&target)
        .expect("shortcut row focus is synchronized")
        .clone();
    let clear_focus = view
        .shortcut_clear_focus
        .get(&target)
        .expect("shortcut clear focus is synchronized")
        .clone();
    let clear_key_focus = clear_focus.clone();
    let keyboard_target = target.clone();
    let clear_target = target.clone();
    let clear_key_target = target.clone();
    let capture_id = match tab {
        ShortcutSettingsTab::Window => ("capture-window-shortcut", tab_index),
        ShortcutSettingsTab::Model => ("capture-model-shortcut", tab_index),
    };
    let clear_id = match tab {
        ShortcutSettingsTab::Window => ("clear-window-shortcut", tab_index),
        ShortcutSettingsTab::Model => ("clear-model-shortcut", tab_index),
    };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .text_sm()
        .child(div().min_w_0().flex_1().child(target_name))
        .child(
            div().text_color(tokens.muted).child(
                row.shortcut
                    .clone()
                    .unwrap_or_else(|| ui_text(language, UiText::NotSet).to_owned()),
            ),
        )
        .child(
            command_button(
                ui_text(
                    language,
                    if capturing {
                        UiText::PressKey
                    } else {
                        UiText::Capture
                    },
                ),
                &focus,
                shortcut_capture_tab_index(row_index),
                window,
                tokens,
                disabled,
            )
            .id(capture_id)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.begin_shortcut_capture(target.clone(), window, cx);
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if view.shortcut_capture.is_none() && is_activation_key(event) {
                    cx.stop_propagation();
                    view.begin_shortcut_capture(keyboard_target.clone(), window, cx);
                }
            })),
        )
        .when(row.shortcut.is_some(), |actions| {
            actions.child(
                command_button(
                    ui_text(language, UiText::Clear),
                    &clear_focus,
                    shortcut_clear_tab_index(row_index),
                    window,
                    tokens,
                    disabled,
                )
                .id(clear_id)
                .on_click(cx.listener(move |view, _, window, cx| {
                    window.focus(&clear_focus, cx);
                    view.clear_shortcut(clear_target.clone(), cx);
                }))
                .on_key_down(cx.listener(move |view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&clear_key_focus, cx);
                        view.clear_shortcut(clear_key_target.clone(), cx);
                    }
                })),
            )
        })
}
