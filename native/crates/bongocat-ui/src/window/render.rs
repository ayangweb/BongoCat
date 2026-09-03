use super::*;

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if let Some(target) = self.accessibility_focus.take() {
                let static_focus = match target {
                    ACCESSIBILITY_GENERAL => Some(&self.general_focus),
                    ACCESSIBILITY_MODELS => Some(&self.models_focus),
                    ACCESSIBILITY_DIAGNOSTICS => Some(&self.diagnostics_focus),
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
                window.focus(
                    static_focus
                        .or(shortcut_focus)
                        .unwrap_or(&self.general_focus),
                    cx,
                );
            }
            self.update_accessibility(window);
        }
        let tokens = Tokens::from_theme(cx);
        let snapshot = self.snapshot.clone();
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
        self.sync_shortcut_row_focus(&shortcuts, disabled, cx);
        let status: SharedString = match (&self.error, self.pending, &snapshot) {
            (Some(error), _, _) => error.to_string().into(),
            (_, Some(PendingOperation::Refresh), _) => "Refreshing runtime snapshot...".into(),
            (_, Some(_), _) => "Saving changes...".into(),
            (_, None, Some(snapshot)) => {
                let health = match snapshot.runtime_health {
                    RuntimeHealth::Starting => "Starting",
                    RuntimeHealth::Ready => "Ready",
                    RuntimeHealth::Degraded => "Degraded",
                    RuntimeHealth::Stopped => "Stopped",
                };
                format!("Runtime {health} · revision {}", snapshot.revision).into()
            }
            _ => "Connecting to runtime...".into(),
        };
        let status_is_error = self.error.is_some();
        let view_entity = cx.entity();

        let general_page = SettingPage::new("General")
            .default_open(true)
            .description("Configure the overlay, model interaction, input and startup behavior.")
            .groups(vec![
                SettingGroup::new().title("Overlay").items(vec![
                    SettingItem::new(
                        "Runtime status",
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
                    .description("Current runtime connection and configuration revision."),
                    SettingItem::new(
                        "Show desktop cat",
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
                    .description(
                        "Keep the Live2D overlay visible. Search: overlay, cat, visibility.",
                    ),
                    SettingItem::new(
                        "Always on top",
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
                    .description("Keep the Live2D overlay above other windows. Search: topmost."),
                    SettingItem::new(
                        "Click-through overlay",
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
                    .description(
                        "Let pointer input pass through the overlay. Search: pointer, mouse.",
                    ),
                    SettingItem::new(
                        "Motion audio",
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
                    .description("Play audio attached to model motions. Search: sound, audio."),
                    SettingItem::new(
                        "Overlay scale",
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
                    .description(
                        "Resize the Live2D overlay from 25% to 400%. Search: size, scale.",
                    ),
                    SettingItem::new(
                        "Overlay opacity",
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
                    .description(
                        "Adjust the overlay transparency from 1% to 100%. Search: transparent.",
                    ),
                ]),
                SettingGroup::new().title("Model interaction").items(vec![
                    SettingItem::new(
                        "Mirror model",
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
                    .description("Render the model mirrored horizontally. Search: flip."),
                    SettingItem::new(
                        "Mirror pointer tracking",
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.model_settings.mirror_pointer_tracking)
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
                    .description(
                        "Mirror horizontal pointer movement with the model. Search: mouse.",
                    ),
                    SettingItem::new(
                        "Ignore pointer input",
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
                    .description("Do not apply pointer movement to the model. Search: mouse."),
                ]),
                SettingGroup::new().title("Input").items(vec![
                    SettingItem::new(
                        "Gamepad stick dead zone",
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
                                        f64::from(s.gamepad_axis_settings.stick_dead_zone_percent)
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
                    .description(
                        "Ignore small analog stick movement. Search: controller, joystick.",
                    ),
                    SettingItem::new(
                        "Gamepad trigger dead zone",
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
                                        f64::from(s.gamepad_axis_settings.trigger_dead_zone_percent)
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
                    .description("Ignore small trigger movement. Search: controller, gamepad."),
                ]),
                SettingGroup::new().title("Startup").item(
                    SettingItem::new(
                        "Open at login",
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
                    .description("Open BongoCat when you sign in. Search: launch, startup, login."),
                ),
            ]);

        let models_page = SettingPage::new("Models")
            .description("Install, validate and activate Live2D model packages.")
            .group(SettingGroup::new().title("Model catalog").item(SettingItem::new(
                "Installed models",
                SettingField::element({
                    let view = view_entity.clone();
                    move |_: &RenderOptions,
                          window: &mut Window,
                          app: &mut App| {
                        let snapshot = view.read(app).snapshot.clone();
                        let tokens = Tokens::from_theme(app);
                        view.update(app, move |view, cx| {
                            view.page = SettingsPage::Models;
                            models::content(view, window, cx, snapshot.as_ref(), tokens)
                        })
                        .into_any_element()
                    }
                }),
            ).layout(Axis::Vertical).description("Import a model folder, validate package safety and choose the active model. Search: import, activate, Live2D.")));

        let diagnostics_page = SettingPage::new("Diagnostics")
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
                    "Refresh",
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
                command_button("Quit", &self.quit_focus, 31, window, tokens, false)
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
