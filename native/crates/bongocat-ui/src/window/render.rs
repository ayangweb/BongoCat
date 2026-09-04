use super::*;

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if let Some(target) = self.accessibility_focus.take() {
                if target == ACCESSIBILITY_LANGUAGE {
                    let focus = self.language_select.read(cx).focus_handle(cx);
                    focus.focus(window, cx);
                }
                let static_focus = match target {
                    ACCESSIBILITY_GENERAL => Some(&self.general_focus),
                    ACCESSIBILITY_MODELS => Some(&self.models_focus),
                    ACCESSIBILITY_DIAGNOSTICS => Some(&self.diagnostics_focus),
                    ACCESSIBILITY_THEME_SYSTEM => Some(&self.theme_system_focus),
                    ACCESSIBILITY_THEME_LIGHT => Some(&self.theme_light_focus),
                    ACCESSIBILITY_THEME_DARK => Some(&self.theme_dark_focus),
                    ACCESSIBILITY_OVERLAY => Some(&self.overlay_focus),
                    ACCESSIBILITY_OVERLAY_TOPMOST => Some(&self.overlay_topmost_focus),
                    ACCESSIBILITY_OVERLAY_CLICK_THROUGH => Some(&self.overlay_click_through_focus),
                    ACCESSIBILITY_OVERLAY_SCALE_DECREASE => {
                        Some(&self.overlay_scale_decrease_focus)
                    }
                    ACCESSIBILITY_OVERLAY_SCALE_INCREASE => {
                        Some(&self.overlay_scale_increase_focus)
                    }
                    ACCESSIBILITY_OVERLAY_OPACITY_DECREASE => {
                        Some(&self.overlay_opacity_decrease_focus)
                    }
                    ACCESSIBILITY_OVERLAY_OPACITY_INCREASE => {
                        Some(&self.overlay_opacity_increase_focus)
                    }
                    ACCESSIBILITY_MAXIMUM_FPS_DECREASE => Some(&self.maximum_fps_decrease_focus),
                    ACCESSIBILITY_MAXIMUM_FPS_INCREASE => Some(&self.maximum_fps_increase_focus),
                    ACCESSIBILITY_AUDIO => Some(&self.audio_focus),
                    ACCESSIBILITY_MIRROR => Some(&self.mirror_focus),
                    ACCESSIBILITY_MIRROR_POINTER => Some(&self.mirror_pointer_focus),
                    ACCESSIBILITY_IGNORE_POINTER => Some(&self.ignore_pointer_focus),
                    ACCESSIBILITY_STICK_DEAD_ZONE => Some(&self.stick_dead_zone_focus),
                    ACCESSIBILITY_TRIGGER_DEAD_ZONE => Some(&self.trigger_dead_zone_focus),
                    ACCESSIBILITY_STARTUP => Some(&self.startup_item_focus),
                    ACCESSIBILITY_OPEN_BACKUPS => Some(&self.open_backups_focus),
                    ACCESSIBILITY_RESTORE_DEFAULTS => Some(&self.restore_defaults_focus),
                    ACCESSIBILITY_RESTORE_SHORTCUTS => Some(&self.restore_shortcuts_focus),
                    ACCESSIBILITY_CLEAR_SHORTCUTS => Some(&self.clear_shortcuts_focus),
                    ACCESSIBILITY_EXPORT_DIAGNOSTICS => Some(&self.export_diagnostics_focus),
                    ACCESSIBILITY_REFRESH => Some(&self.refresh_focus),
                    ACCESSIBILITY_QUIT => Some(&self.quit_focus),
                    _ => None,
                };
                let shortcut_focus = self.snapshot.as_ref().and_then(|snapshot| {
                    shortcut_target_for_accessibility_node(&snapshot.shortcuts, target)
                        .and_then(|target| self.shortcut_row_focus.get(&target))
                });
                if target != ACCESSIBILITY_LANGUAGE {
                    window.focus(
                        static_focus
                            .or(shortcut_focus)
                            .unwrap_or(&self.general_focus),
                        cx,
                    );
                }
            }
            self.update_accessibility(window, cx);
        }
        let snapshot = self.snapshot.clone();
        if let Some(snapshot) = snapshot.as_ref() {
            self.sync_component_theme(snapshot.appearance_theme, window, cx);
        }
        let tokens = Tokens::from_theme(cx);
        if let Some(snapshot) = snapshot.as_ref() {
            self.sync_component_inputs(snapshot, window, cx);
        }
        let configuration_ready = snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.configuration_status == SettingsConfigurationStatus::Ready
        });
        let disabled = self.pending.is_some()
            || snapshot.is_none()
            || self.model_import.is_running()
            || !configuration_ready;
        let shortcuts = snapshot
            .as_ref()
            .map(|snapshot| snapshot.shortcuts.clone())
            .unwrap_or_default();
        let language = snapshot
            .as_ref()
            .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                snapshot.resolved_language
            });
        self.sync_shortcut_row_focus(&shortcuts, disabled, cx);
        let status: SharedString = match (&self.error, self.pending, &snapshot) {
            (Some(error), _, _) => settings_error(language, *error).into(),
            (_, Some(PendingOperation::Refresh), _) => ui_text(language, UiText::Refreshing).into(),
            (_, Some(_), _) => ui_text(language, UiText::Saving).into(),
            (_, None, Some(snapshot)) => {
                let health = match snapshot.runtime_health {
                    RuntimeHealth::Starting => ui_text(language, UiText::Starting),
                    RuntimeHealth::Ready => ui_text(language, UiText::Ready),
                    RuntimeHealth::Degraded => ui_text(language, UiText::Degraded),
                    RuntimeHealth::Stopped => ui_text(language, UiText::Stopped),
                };
                runtime_status(language, health, snapshot.revision).into()
            }
            _ => ui_text(language, UiText::Connecting).into(),
        };
        let status_is_error = self.error.is_some();
        let view_entity = cx.entity();
        let startup_item = startup_item_presentation(
            snapshot.as_ref().map(|snapshot| snapshot.startup_item),
            disabled,
            language,
        );

        let general_page = SettingPage::new(ui_text(language, UiText::General))
            .default_open(true)
            .description(ui_text(language, UiText::GeneralDescription))
            .groups(vec![
                SettingGroup::new()
                    .title(ui_text(language, UiText::Appearance))
                    .items(vec![
                        SettingItem::new(
                            ui_text(language, UiText::Theme),
                            SettingField::element({
                                let view = view_entity.clone();
                                move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                    let selected = view
                                        .read(app)
                                        .snapshot
                                        .as_ref()
                                        .map(|snapshot| theme_index(snapshot.appearance_theme));
                                    let view_for_change = view.clone();
                                    RadioGroup::horizontal("appearance-theme")
                                        .children([
                                            Radio::new("appearance-theme-system")
                                                .label(ui_text(language, UiText::System))
                                                .tab_index(4),
                                            Radio::new("appearance-theme-light")
                                                .label(ui_text(language, UiText::Light))
                                                .tab_index(5),
                                            Radio::new("appearance-theme-dark")
                                                .label(ui_text(language, UiText::Dark))
                                                .tab_index(6),
                                        ])
                                        .selected_index(selected)
                                        .disabled(disabled)
                                        .on_click(move |index, _, app| {
                                            let Some(theme) = theme_from_index(*index) else {
                                                return;
                                            };
                                            view_for_change.update(app, |view, cx| {
                                                view.set_appearance_theme(theme, cx)
                                            });
                                        })
                                        .into_any_element()
                                }
                            }),
                        )
                        .description(ui_text(language, UiText::ThemeDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::Language),
                            SettingField::element({
                                let view = view_entity.clone();
                                move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                    let state = view.read(app).language_select.clone();
                                    Select::new(&state)
                                        .accessibility_label(ui_text(language, UiText::Language))
                                        .disabled(disabled)
                                        .into_any_element()
                                }
                            }),
                        )
                        .description(ui_text(language, UiText::LanguageDescription)),
                    ]),
                SettingGroup::new()
                    .title(ui_text(language, UiText::Overlay))
                    .items(vec![
                        SettingItem::new(
                            ui_text(language, UiText::RuntimeStatus),
                            SettingField::element({
                                let status = status.clone();
                                move |_: &RenderOptions, _: &mut Window, _: &mut App| {
                                    if status_is_error {
                                        Tag::danger().child(status.clone()).into_any_element()
                                    } else {
                                        Tag::secondary().child(status.clone()).into_any_element()
                                    }
                                }
                            }),
                        )
                        .description(ui_text(language, UiText::RuntimeStatusDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::ShowDesktopCat),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.overlay_visible)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_overlay_visible(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::ShowDesktopCatDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::AlwaysOnTop),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.overlay.always_on_top)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            if let Some(snapshot) = view.snapshot.as_ref() {
                                                let mut settings = snapshot.overlay;
                                                settings.always_on_top = value;
                                                view.set_overlay_settings(settings, cx);
                                            }
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::AlwaysOnTopDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::ClickThroughOverlay),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.overlay.click_through)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            if let Some(snapshot) = view.snapshot.as_ref() {
                                                let mut settings = snapshot.overlay;
                                                settings.click_through = value;
                                                view.set_overlay_settings(settings, cx);
                                            }
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::ClickThroughOverlayDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::MotionAudio),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.motion_audio_enabled)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_motion_audio_enabled(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::MotionAudioDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::OverlayScale),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 25.0,
                                    max: 400.0,
                                    step: 25.0,
                                },
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .map_or(100.0, |s| f64::from(s.overlay.scale_percent))
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_overlay_scale_value(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::OverlayScaleDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::OverlayOpacity),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 1.0,
                                    max: 100.0,
                                    step: 10.0,
                                },
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .map_or(100.0, |s| f64::from(s.overlay.opacity_percent))
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_overlay_opacity_value(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::OverlayOpacityDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::MaximumFps),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 15.0,
                                    max: 240.0,
                                    step: 15.0,
                                },
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .map_or(60.0, |s| f64::from(s.maximum_fps))
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_maximum_fps_value(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::MaximumFpsDescription)),
                    ]),
                SettingGroup::new()
                    .title(ui_text(language, UiText::ModelInteraction))
                    .items(vec![
                        SettingItem::new(
                            ui_text(language, UiText::MirrorModel),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.model_settings.mirror)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            if let Some(s) = view.snapshot.as_ref() {
                                                let mut settings = s.model_settings;
                                                settings.mirror = value;
                                                view.set_model_settings(settings, cx);
                                            }
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::MirrorModelDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::MirrorPointerTracking),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app).snapshot.as_ref().is_some_and(|s| {
                                            s.model_settings.mirror_pointer_tracking
                                        })
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            if let Some(s) = view.snapshot.as_ref() {
                                                let mut settings = s.model_settings;
                                                settings.mirror_pointer_tracking = value;
                                                view.set_model_settings(settings, cx);
                                            }
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::MirrorPointerTrackingDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::IgnorePointerInput),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.model_settings.ignore_pointer)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            if let Some(s) = view.snapshot.as_ref() {
                                                let mut settings = s.model_settings;
                                                settings.ignore_pointer = value;
                                                view.set_model_settings(settings, cx);
                                            }
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::IgnorePointerInputDescription)),
                    ]),
                SettingGroup::new()
                    .title(ui_text(language, UiText::Input))
                    .items(vec![
                        SettingItem::new(
                            ui_text(language, UiText::GamepadStickDeadZone),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 99.0,
                                    step: 5.0,
                                },
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app).snapshot.as_ref().map_or(15.0, |s| {
                                            f64::from(
                                                s.gamepad_axis_settings.stick_dead_zone_percent,
                                            )
                                        })
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_gamepad_dead_zone_value(true, value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::GamepadStickDeadZoneDescription)),
                        SettingItem::new(
                            ui_text(language, UiText::GamepadTriggerDeadZone),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 99.0,
                                    step: 5.0,
                                },
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app).snapshot.as_ref().map_or(0.0, |s| {
                                            f64::from(
                                                s.gamepad_axis_settings.trigger_dead_zone_percent,
                                            )
                                        })
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_gamepad_dead_zone_value(false, value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ui_text(language, UiText::GamepadTriggerDeadZoneDescription)),
                    ]),
                SettingGroup::new()
                    .title(ui_text(language, UiText::Application))
                    .items({
                        let mut items = vec![
                            SettingItem::new(
                                ui_text(language, UiText::ShowStatusIcon),
                                SettingField::switch(
                                    {
                                        let view = view_entity.clone();
                                        move |app| {
                                            view.read(app)
                                                .snapshot
                                                .as_ref()
                                                .is_some_and(|s| s.status_icon_visible)
                                        }
                                    },
                                    {
                                        let view = view_entity.clone();
                                        move |value, app| {
                                            view.update(app, |view, cx| {
                                                view.set_status_icon_visible(value, cx)
                                            });
                                        }
                                    },
                                ),
                            )
                            .description(ui_text(language, UiText::ShowStatusIconDescription)),
                        ];
                        #[cfg(target_os = "windows")]
                        items.push(
                            SettingItem::new(
                                ui_text(language, UiText::ShowTaskbarIcon),
                                SettingField::switch(
                                    {
                                        let view = view_entity.clone();
                                        move |app| {
                                            view.read(app)
                                                .snapshot
                                                .as_ref()
                                                .is_some_and(|s| s.taskbar_icon_visible)
                                        }
                                    },
                                    {
                                        let view = view_entity.clone();
                                        move |value, app| {
                                            view.update(app, |view, cx| {
                                                view.set_taskbar_icon_visible(value, cx)
                                            });
                                        }
                                    },
                                ),
                            )
                            .description(ui_text(language, UiText::ShowTaskbarIconDescription)),
                        );
                        items.push(
                            SettingItem::new(
                                ui_text(language, UiText::OpenAtLogin),
                                SettingField::switch(
                                    {
                                        let view = view_entity.clone();
                                        move |app| {
                                            view.read(app).snapshot.as_ref().is_some_and(|s| {
                                                matches!(
                                            s.startup_item,
                                            SettingsStartupItemStatus::State(
                                                SettingsStartupItemState::Enabled
                                                    | SettingsStartupItemState::RequiresApproval
                                            )
                                        )
                                            })
                                        }
                                    },
                                    {
                                        let view = view_entity.clone();
                                        move |value, app| {
                                            view.update(app, |view, cx| {
                                                view.set_startup_item_enabled(value, cx)
                                            });
                                        }
                                    },
                                ),
                            )
                            .description(startup_item.description),
                        );
                        items
                    }),
            ]);

        let models_page = SettingPage::new(ui_text(language, UiText::Models))
            .description(ui_text(language, UiText::ModelsDescription))
            .group(
                SettingGroup::new()
                    .title(ui_text(language, UiText::ModelCatalog))
                    .item(
                        SettingItem::new(
                            ui_text(language, UiText::InstalledModels),
                            SettingField::element({
                                let view = view_entity.clone();
                                move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                                    let snapshot = view.read(app).snapshot.clone();
                                    let tokens = Tokens::from_theme(app);
                                    view.update(app, move |view, cx| {
                                        view.page = SettingsPage::Models;
                                        models::content(view, window, cx, snapshot.as_ref(), tokens)
                                    })
                                    .into_any_element()
                                }
                            }),
                        )
                        .layout(Axis::Vertical)
                        .description(ui_text(language, UiText::InstalledModelsDescription)),
                    ),
            );

        let diagnostics_page = SettingPage::new(ui_text(language, UiText::Diagnostics))
            .description("Inspect runtime health, input reliability and shortcut bindings.")
            .group(SettingGroup::new().title("Runtime and input").item(SettingItem::new(
                "Runtime diagnostics",
                SettingField::element({
                    let view = view_entity.clone();
                    move |_: &RenderOptions,
                          window: &mut Window,
                          app: &mut App| {
                        let snapshot = view.read(app).snapshot.clone();
                        let tokens = Tokens::from_theme(app);
                        view.update(app, move |view, cx| {
                            view.page = SettingsPage::Diagnostics;
                            diagnostics::content(view, window, cx, snapshot.as_ref(), disabled, tokens)
                        })
                        .into_any_element()
                    }
                }),
            ).layout(Axis::Vertical).description("Review renderer status, input counters and keyboard shortcuts. Search: renderer, input, shortcut.")));

        let settings = Settings::new("bongocat-settings")
            .sidebar_width(px(220.0))
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(vec![general_page, models_page, diagnostics_page]);
        let footer = div()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .p_4()
            .border_t_1()
            .border_color(tokens.border)
            .child(
                command_button(
                    ui_text(language, UiText::Refresh),
                    &self.refresh_focus,
                    30,
                    window,
                    tokens,
                    self.refresh_is_disabled(),
                )
                .id("refresh-settings")
                .on_click(cx.listener(|view, _, window, cx| {
                    if view.pending.is_none()
                        && !view.model_import.is_running()
                        && !view.model_import.is_picker_open()
                    {
                        window.focus(&view.refresh_focus, cx);
                        view.refresh(cx);
                    }
                })),
            )
            .child(
                command_button(
                    ui_text(language, UiText::Quit),
                    &self.quit_focus,
                    31,
                    window,
                    tokens,
                    false,
                )
                .id("quit-application")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.quit_focus, cx);
                    (view.request_quit)(cx);
                })),
            );

        div()
            .id("bongocat-settings-root")
            .on_key_down(cx.listener(|view, event, _, cx| {
                if view.shortcut_capture.is_some() {
                    cx.stop_propagation();
                    view.capture_shortcut(event, cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(div().min_h_0().w_full().flex_1().child(settings))
            .child(footer)
    }
}
