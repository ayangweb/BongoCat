use super::*;

impl SettingsView {
    pub(super) fn sync_component_theme(
        &mut self,
        theme: SettingsTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.applied_theme == Some(theme) {
            return;
        }
        apply_component_theme(theme, window, cx);
        self.applied_theme = Some(theme);
    }

    pub(super) fn sync_component_inputs(
        &mut self,
        snapshot: &SettingsSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.set_window_title(ui_text(snapshot.resolved_language, UiText::Settings));
        self.syncing_component_inputs = true;
        let scale = f64::from(snapshot.overlay.scale_percent);
        let opacity = f64::from(snapshot.overlay.opacity_percent);
        let stick = f64::from(snapshot.gamepad_axis_settings.stick_dead_zone_percent);
        let trigger = f64::from(snapshot.gamepad_axis_settings.trigger_dead_zone_percent);
        self.overlay_scale_input.update(cx, |input, cx| {
            input.set_value(scale.to_string(), window, cx)
        });
        self.overlay_opacity_input.update(cx, |input, cx| {
            input.set_value(opacity.to_string(), window, cx)
        });
        self.stick_dead_zone_input.update(cx, |input, cx| {
            input.set_value(stick.to_string(), window, cx)
        });
        self.trigger_dead_zone_input.update(cx, |input, cx| {
            input.set_value(trigger.to_string(), window, cx)
        });
        self.model_id_input.update(cx, |input, cx| {
            input.set_placeholder(
                ui_text(snapshot.resolved_language, UiText::ModelId),
                window,
                cx,
            );
            input.set_value(&self.model_import.id, window, cx)
        });
        self.language_select.update(cx, |select, cx| {
            select.set_items(
                SearchableVec::new(
                    SettingsLanguage::ALL
                        .into_iter()
                        .map(|language| language.display_name(snapshot.resolved_language))
                        .collect::<Vec<_>>(),
                ),
                window,
                cx,
            );
            select.set_selected_value(
                &snapshot.language.display_name(snapshot.resolved_language),
                window,
                cx,
            )
        });
        self.theme_select.update(cx, |select, cx| {
            select.set_items(
                SearchableVec::new(theme_options(snapshot.resolved_language)),
                window,
                cx,
            );
            select.set_selected_value(
                &theme_display_name(snapshot.appearance_theme, snapshot.resolved_language),
                window,
                cx,
            )
        });
        self.syncing_component_inputs = false;
    }

    pub(super) fn new(
        client: SettingsClient,
        request_quit: Rc<dyn Fn(&mut App)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let overlay_scale_input = cx.new(|cx| InputState::new(window, cx).placeholder("100"));
        let overlay_opacity_input = cx.new(|cx| InputState::new(window, cx).placeholder("100"));
        let stick_dead_zone_input = cx.new(|cx| InputState::new(window, cx).placeholder("15"));
        let trigger_dead_zone_input = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        let model_id_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ui_text(
                SettingsLanguage::EnglishUnitedStates,
                UiText::ModelId,
            ))
        });
        let language_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    SettingsLanguage::ALL
                        .into_iter()
                        .map(|language| {
                            language.display_name(SettingsLanguage::EnglishUnitedStates)
                        })
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let theme_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(theme_options(SettingsLanguage::EnglishUnitedStates)),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        cx.subscribe(
            &overlay_scale_input,
            |view, input, event: &NumberInputEvent, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let current = input.read(cx).value().parse::<f64>().unwrap_or(100.0);
                let value = match event {
                    NumberInputEvent::Step(StepAction::Increment) => current + 25.0,
                    NumberInputEvent::Step(StepAction::Decrement) => current - 25.0,
                };
                view.set_overlay_scale_value(value, cx);
            },
        )
        .detach();
        cx.subscribe(
            &overlay_scale_input,
            |view, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && !view.syncing_component_inputs
                    && let Ok(value) = input.read(cx).value().parse::<f64>()
                {
                    view.set_overlay_scale_value(value, cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &overlay_opacity_input,
            |view, input, event: &NumberInputEvent, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let current = input.read(cx).value().parse::<f64>().unwrap_or(100.0);
                let value = match event {
                    NumberInputEvent::Step(StepAction::Increment) => current + 10.0,
                    NumberInputEvent::Step(StepAction::Decrement) => current - 10.0,
                };
                view.set_overlay_opacity_value(value, cx);
            },
        )
        .detach();
        cx.subscribe(
            &overlay_opacity_input,
            |view, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && !view.syncing_component_inputs
                    && let Ok(value) = input.read(cx).value().parse::<f64>()
                {
                    view.set_overlay_opacity_value(value, cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &stick_dead_zone_input,
            |view, input, event: &NumberInputEvent, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let current = input.read(cx).value().parse::<f64>().unwrap_or(15.0);
                let value = match event {
                    NumberInputEvent::Step(StepAction::Increment) => current + 5.0,
                    NumberInputEvent::Step(StepAction::Decrement) => current - 5.0,
                };
                view.set_gamepad_dead_zone_value(true, value, cx);
            },
        )
        .detach();
        cx.subscribe(
            &stick_dead_zone_input,
            |view, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && !view.syncing_component_inputs
                    && let Ok(value) = input.read(cx).value().parse::<f64>()
                {
                    view.set_gamepad_dead_zone_value(true, value, cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &trigger_dead_zone_input,
            |view, input, event: &NumberInputEvent, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let current = input.read(cx).value().parse::<f64>().unwrap_or(0.0);
                let value = match event {
                    NumberInputEvent::Step(StepAction::Increment) => current + 5.0,
                    NumberInputEvent::Step(StepAction::Decrement) => current - 5.0,
                };
                view.set_gamepad_dead_zone_value(false, value, cx);
            },
        )
        .detach();
        cx.subscribe(
            &trigger_dead_zone_input,
            |view, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && !view.syncing_component_inputs
                    && let Ok(value) = input.read(cx).value().parse::<f64>()
                {
                    view.set_gamepad_dead_zone_value(false, value, cx);
                }
            },
        )
        .detach();
        cx.subscribe(&model_id_input, |view, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if view.syncing_component_inputs || view.model_import.is_running() {
                    return;
                }
                let value = input.read(cx).value();
                view.model_import.id = sanitize_model_id_input(&value);
                view.model_import.reset_result_state();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(
            &language_select,
            |view, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let display_language = view
                    .snapshot
                    .as_ref()
                    .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                        snapshot.resolved_language
                    });
                if let SelectEvent::Confirm(Some(name)) = event
                    && let Some(language) =
                        SettingsLanguage::from_display_name(name, display_language)
                {
                    view.set_language(language, cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &theme_select,
            |view, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let display_language = view
                    .snapshot
                    .as_ref()
                    .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                        snapshot.resolved_language
                    });
                if let SelectEvent::Confirm(Some(name)) = event
                    && let Some(theme) = theme_from_display_name(name, display_language)
                {
                    view.set_appearance_theme(theme, cx);
                }
            },
        )
        .detach();
        Self {
            client,
            snapshot: None,
            pending: None,
            error: None,
            page: SettingsPage::General,
            model_import: ModelImportDraft::default(),
            model_delete_confirmation: None,
            model_row_focus: BTreeMap::new(),
            model_behavior_preview_focus: BTreeMap::new(),
            shortcut_capture: None,
            shortcut_capture_error: None,
            shortcut_row_focus: BTreeMap::new(),
            shortcut_clear_focus: BTreeMap::new(),
            window_hidden: false,
            applied_theme: None,
            language_select,
            theme_select,
            request_quit,
            general_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            models_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            diagnostics_focus: cx.focus_handle().tab_index(3).tab_stop(true),
            status_icon_focus: cx.focus_handle().tab_index(9).tab_stop(true),
            #[cfg(target_os = "windows")]
            taskbar_icon_focus: cx.focus_handle().tab_index(38).tab_stop(true),
            automatic_update_check_focus: cx.focus_handle().tab_index(45).tab_stop(true),
            overlay_focus: cx.focus_handle().tab_index(10).tab_stop(true),
            overlay_topmost_focus: cx.focus_handle().tab_index(13).tab_stop(true),
            overlay_click_through_focus: cx.focus_handle().tab_index(14).tab_stop(true),
            overlay_keep_inside_work_area_focus: cx.focus_handle().tab_index(44).tab_stop(true),
            overlay_scale_decrease_focus: cx.focus_handle().tab_index(15).tab_stop(true),
            overlay_scale_increase_focus: cx.focus_handle().tab_index(16).tab_stop(true),
            overlay_opacity_decrease_focus: cx.focus_handle().tab_index(17).tab_stop(true),
            overlay_opacity_increase_focus: cx.focus_handle().tab_index(18).tab_stop(true),
            maximum_fps_decrease_focus: cx.focus_handle().tab_index(24).tab_stop(true),
            maximum_fps_increase_focus: cx.focus_handle().tab_index(25).tab_stop(true),
            release_fallback_decrease_focus: cx.focus_handle().tab_index(39).tab_stop(true),
            release_fallback_increase_focus: cx.focus_handle().tab_index(40).tab_stop(true),
            audio_focus: cx.focus_handle().tab_index(11).tab_stop(true),
            behavior_shortcuts_focus: cx.focus_handle().tab_index(19).tab_stop(true),
            mirror_focus: cx.focus_handle().tab_index(6).tab_stop(true),
            mirror_pointer_focus: cx.focus_handle().tab_index(7).tab_stop(true),
            ignore_pointer_focus: cx.focus_handle().tab_index(8).tab_stop(true),
            stick_dead_zone_focus: cx.focus_handle().tab_index(26).tab_stop(true),
            trigger_dead_zone_focus: cx.focus_handle().tab_index(27).tab_stop(true),
            startup_item_focus: cx.focus_handle().tab_index(12).tab_stop(true),
            model_id_focus: cx.focus_handle().tab_index(20).tab_stop(true),
            choose_model_focus: cx.focus_handle().tab_index(21).tab_stop(true),
            import_model_focus: cx.focus_handle().tab_index(22).tab_stop(true),
            open_backups_focus: cx.focus_handle().tab_index(28).tab_stop(true),
            restore_defaults_focus: cx.focus_handle().tab_index(29).tab_stop(true),
            restore_shortcuts_focus: cx.focus_handle().tab_index(33).tab_stop(true),
            clear_shortcuts_focus: cx.focus_handle().tab_index(34).tab_stop(true),
            export_diagnostics_focus: cx.focus_handle().tab_index(32).tab_stop(true),
            refresh_focus: cx.focus_handle().tab_index(30).tab_stop(true),
            quit_focus: cx.focus_handle().tab_index(31).tab_stop(true),
            overlay_scale_input,
            overlay_opacity_input,
            stick_dead_zone_input,
            trigger_dead_zone_input,
            model_id_input,
            syncing_component_inputs: false,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            accessibility: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            accessibility_focus: None,
        }
    }
}
