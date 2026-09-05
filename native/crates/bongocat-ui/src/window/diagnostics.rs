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
        .id("diagnostics-content")
        .child(
            div()
                .text_2xl()
                .child(ui_text(language, UiText::Diagnostics)),
        )
        .child(
            div()
                .text_sm()
                .text_color(if view.error.is_some() {
                    tokens.danger
                } else {
                    tokens.muted
                })
                .child(match view.error {
                    Some(error) => diagnostics_unavailable(language, error),
                    None if snapshot.is_none() => {
                        ui_text(language, UiText::LoadingDiagnostics).to_owned()
                    }
                    None => ui_text(language, UiText::InputReliabilityCounters).to_owned(),
                }),
        )
        .child(
            div()
                .id("input-diagnostics")
                .min_h_0()
                .flex_1()
                .overflow_y_scroll()
                .when_some(snapshot.as_ref(), |content, snapshot| {
                    let metrics = input_diagnostic_metrics(language, snapshot.input_diagnostics);
                    let input_service =
                        input_service_presentation(snapshot.input_diagnostics, language);
                    let runtime_diagnostics =
                        runtime_diagnostics_presentation(snapshot.runtime_diagnostics, language);
                    let recovery = config_recovery_presentation(
                        snapshot.configuration_status,
                        snapshot.config_recovery,
                        language,
                    );
                    let config_action_disabled = view.pending.is_some();
                    let shortcut_action_disabled = disabled;
                    content
                        .child(
                            div()
                                .id("build-information")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(tokens.muted)
                                        .child(ui_text(language, UiText::BuildInformation)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_sm()
                                        .child(build_info_detail(language, &snapshot.build_info)),
                                ),
                        )
                        .child(
                            div()
                                .id("runtime-diagnostics")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(tokens.muted)
                                                .child(ui_text(language, UiText::RuntimeRenderer)),
                                        )
                                        .child(div().text_sm().child(runtime_diagnostics.title)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_sm()
                                        .text_color(if runtime_diagnostics.attention {
                                            tokens.danger
                                        } else {
                                            tokens.muted
                                        })
                                        .child(runtime_diagnostics.detail),
                                ),
                        )
                        .child(
                            div()
                                .id("diagnostics-export")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div().text_sm().text_color(tokens.muted).child(
                                                ui_text(language, UiText::DiagnosticsExport),
                                            ),
                                        )
                                        .child(
                                            div().text_sm().child(diagnostics_export_status(
                                                language,
                                                snapshot
                                                    .diagnostics_export
                                                    .map(|status| status.bytes_written),
                                            )),
                                        ),
                                )
                                .child(
                                    command_button(
                                        ui_text(language, UiText::Export),
                                        &view.export_diagnostics_focus,
                                        32,
                                        window,
                                        tokens,
                                        config_action_disabled,
                                    )
                                    .id("export-diagnostics")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        if view.pending.is_none() {
                                            window.focus(&view.export_diagnostics_focus, cx);
                                            view.export_diagnostics(cx);
                                        }
                                    }))
                                    .on_key_down(cx.listener(|view, event, window, cx| {
                                        if view.pending.is_none() && is_activation_key(event) {
                                            cx.stop_propagation();
                                            window.focus(&view.export_diagnostics_focus, cx);
                                            view.export_diagnostics(cx);
                                        }
                                    })),
                                ),
                        )
                        .child(
                            div()
                                .id("shortcut-diagnostics")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
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
                                                .text_sm()
                                                .text_color(tokens.muted)
                                                .child(ui_text(language, UiText::Shortcuts)),
                                        )
                                        .child(
                                            command_button(
                                                ui_text(language, UiText::RestoreDefaults),
                                                &view.restore_shortcuts_focus,
                                                33,
                                                window,
                                                tokens,
                                                shortcut_action_disabled,
                                            )
                                            .id("restore-default-shortcuts")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                if view.pending.is_none() {
                                                    window.focus(&view.restore_shortcuts_focus, cx);
                                                    view.restore_default_shortcuts(cx);
                                                }
                                            }))
                                            .on_key_down(cx.listener(|view, event, window, cx| {
                                                if view.pending.is_none()
                                                    && is_activation_key(event)
                                                {
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
                                                shortcut_action_disabled
                                                    || snapshot.shortcuts.commands.is_empty()
                                                        && snapshot
                                                            .shortcuts
                                                            .model_behaviors
                                                            .is_empty(),
                                            )
                                            .id("clear-shortcuts")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                if view.pending.is_none() {
                                                    window.focus(&view.clear_shortcuts_focus, cx);
                                                    view.clear_shortcuts(cx);
                                                }
                                            }))
                                            .on_key_down(cx.listener(|view, event, window, cx| {
                                                if view.pending.is_none()
                                                    && is_activation_key(event)
                                                {
                                                    cx.stop_propagation();
                                                    window.focus(&view.clear_shortcuts_focus, cx);
                                                    view.clear_shortcuts(cx);
                                                }
                                            })),
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
                                    content.child(div().text_sm().text_color(tokens.accent).child(
                                        match target {
                                            ShortcutCaptureTarget::Command(_) => {
                                                ui_text(language, UiText::PressCommandShortcut)
                                            }
                                            ShortcutCaptureTarget::ModelBehavior { .. } => {
                                                ui_text(language, UiText::PressBehaviorShortcut)
                                            }
                                        },
                                    ))
                                })
                                .children(snapshot.shortcuts.commands.iter().enumerate().map(
                                    |(index, binding)| {
                                        let target =
                                            ShortcutCaptureTarget::Command(binding.command.clone());
                                        let target_name = shortcut_target_name(language, &target);
                                        let capturing =
                                            view.shortcut_capture.as_ref() == Some(&target);
                                        let disabled = shortcut_action_disabled;
                                        let focus = view
                                            .shortcut_row_focus
                                            .get(&target)
                                            .expect("shortcut row focus is synchronized")
                                            .clone();
                                        let tab_index = shortcut_capture_tab_index(index);
                                        let keyboard_target = target.clone();
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_3()
                                            .text_sm()
                                            .child(div().min_w_0().flex_1().child(target_name))
                                            .child(
                                                div()
                                                    .text_color(tokens.muted)
                                                    .child(binding.shortcut.clone()),
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
                                                    tab_index,
                                                    window,
                                                    tokens,
                                                    disabled,
                                                )
                                                .id(("capture-command", index))
                                                .on_click(cx.listener(
                                                    move |view, _, window, cx| {
                                                        view.begin_shortcut_capture(
                                                            target.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_key_down(cx.listener(
                                                    move |view, event, window, cx| {
                                                        if view.shortcut_capture.is_none()
                                                            && is_activation_key(event)
                                                        {
                                                            cx.stop_propagation();
                                                            view.begin_shortcut_capture(
                                                                keyboard_target.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    },
                                                )),
                                            )
                                    },
                                ))
                                .children(
                                    snapshot.shortcuts.model_behaviors.iter().enumerate().map(
                                        |(index, binding)| {
                                            let target = ShortcutCaptureTarget::ModelBehavior {
                                                model_id: binding.model_id.clone(),
                                                behavior_id: binding.behavior_id.clone(),
                                            };
                                            let target_name =
                                                shortcut_target_name(language, &target);
                                            let capturing =
                                                view.shortcut_capture.as_ref() == Some(&target);
                                            let disabled = shortcut_action_disabled;
                                            let focus = view
                                                .shortcut_row_focus
                                                .get(&target)
                                                .expect("shortcut row focus is synchronized")
                                                .clone();
                                            let tab_index = shortcut_capture_tab_index(
                                                snapshot.shortcuts.commands.len() + index,
                                            );
                                            let keyboard_target = target.clone();
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap_3()
                                                .text_sm()
                                                .child(div().min_w_0().flex_1().child(target_name))
                                                .child(
                                                    div()
                                                        .text_color(tokens.muted)
                                                        .child(binding.shortcut.clone()),
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
                                                        tab_index,
                                                        window,
                                                        tokens,
                                                        disabled,
                                                    )
                                                    .id(("capture-behavior", index))
                                                    .on_click(cx.listener(
                                                        move |view, _, window, cx| {
                                                            view.begin_shortcut_capture(
                                                                target.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .on_key_down(cx.listener(
                                                        move |view, event, window, cx| {
                                                            if view.shortcut_capture.is_none()
                                                                && is_activation_key(event)
                                                            {
                                                                cx.stop_propagation();
                                                                view.begin_shortcut_capture(
                                                                    keyboard_target.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        },
                                                    )),
                                                )
                                        },
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .id("input-service-diagnostics")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(tokens.muted)
                                                .child(ui_text(language, UiText::InputService)),
                                        )
                                        .child(div().text_sm().child(input_service.title)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_sm()
                                        .text_color(if input_service.attention {
                                            tokens.danger
                                        } else if input_service.running {
                                            tokens.accent
                                        } else {
                                            tokens.muted
                                        })
                                        .child(input_service.detail),
                                ),
                        )
                        .child(
                            div()
                                .id("config-recovery-diagnostics")
                                .pb_3()
                                .mb_3()
                                .border_b_1()
                                .border_color(tokens.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(tokens.muted)
                                                .child(ui_text(language, UiText::Configuration)),
                                        )
                                        .child(div().text_sm().child(recovery.title)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .text_sm()
                                        .text_color(if recovery.attention {
                                            tokens.danger
                                        } else if recovery.recovered {
                                            tokens.accent
                                        } else {
                                            tokens.muted
                                        })
                                        .child(recovery.detail)
                                        .child(
                                            command_button(
                                                ui_text(language, UiText::Backups),
                                                &view.open_backups_focus,
                                                28,
                                                window,
                                                tokens,
                                                config_action_disabled,
                                            )
                                            .id("open-config-backups")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                if view.pending.is_none() {
                                                    window.focus(&view.open_backups_focus, cx);
                                                    view.open_config_backup_location(cx);
                                                }
                                            }))
                                            .on_key_down(cx.listener(|view, event, window, cx| {
                                                if view.pending.is_none()
                                                    && is_activation_key(event)
                                                {
                                                    cx.stop_propagation();
                                                    window.focus(&view.open_backups_focus, cx);
                                                    view.open_config_backup_location(cx);
                                                }
                                            })),
                                        )
                                        .when(recovery.can_restore, |content| {
                                            content.child(
                                                command_button(
                                                    ui_text(language, UiText::RestoreDefaults),
                                                    &view.restore_defaults_focus,
                                                    29,
                                                    window,
                                                    tokens,
                                                    config_action_disabled,
                                                )
                                                .id("restore-default-configuration")
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    if view.pending.is_none() {
                                                        window.focus(
                                                            &view.restore_defaults_focus,
                                                            cx,
                                                        );
                                                        view.restore_default_configuration(cx);
                                                    }
                                                }))
                                                .on_key_down(cx.listener(
                                                    |view, event, window, cx| {
                                                        if view.pending.is_none()
                                                            && is_activation_key(event)
                                                        {
                                                            cx.stop_propagation();
                                                            window.focus(
                                                                &view.restore_defaults_focus,
                                                                cx,
                                                            );
                                                            view.restore_default_configuration(cx);
                                                        }
                                                    },
                                                )),
                                            )
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap_5()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .child(diagnostic_group(
                                            ui_text(language, UiText::CurrentState),
                                            &metrics[..2],
                                            tokens,
                                        ))
                                        .child(diagnostic_group(
                                            ui_text(language, UiText::InputProcessing),
                                            &metrics[2..10],
                                            tokens,
                                        )),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .child(diagnostic_group(
                                            ui_text(language, UiText::SequenceRecovery),
                                            &metrics[10..15],
                                            tokens,
                                        ))
                                        .child(diagnostic_group(
                                            ui_text(language, UiText::Transport),
                                            &metrics[15..],
                                            tokens,
                                        )),
                                ),
                        )
                }),
        )
}
