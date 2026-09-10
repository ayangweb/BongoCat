use super::*;

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if let Some(target) = self.accessibility_focus.take() {
                if target == ACCESSIBILITY_LANGUAGE || target == ACCESSIBILITY_THEME {
                    let focus = if target == ACCESSIBILITY_LANGUAGE {
                        self.language_select.read(cx).focus_handle(cx)
                    } else {
                        self.theme_select.read(cx).focus_handle(cx)
                    };
                    focus.focus(window, cx);
                }
                let static_focus = match target {
                    ACCESSIBILITY_GENERAL => Some(&self.general_focus),
                    ACCESSIBILITY_MODELS => Some(&self.models_focus),
                    ACCESSIBILITY_SHORTCUTS => Some(&self.shortcuts_focus),
                    ACCESSIBILITY_DIAGNOSTICS => Some(&self.diagnostics_focus),
                    ACCESSIBILITY_ABOUT => Some(&self.about_focus),
                    ACCESSIBILITY_OVERLAY => Some(&self.overlay_focus),
                    ACCESSIBILITY_OVERLAY_TOPMOST => Some(&self.overlay_topmost_focus),
                    ACCESSIBILITY_OVERLAY_CLICK_THROUGH => Some(&self.overlay_click_through_focus),
                    ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA => {
                        Some(&self.overlay_keep_inside_work_area_focus)
                    }
                    ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK => {
                        Some(&self.automatic_update_check_focus)
                    }
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
                    ACCESSIBILITY_RELEASE_FALLBACK_DECREASE => {
                        Some(&self.release_fallback_decrease_focus)
                    }
                    ACCESSIBILITY_RELEASE_FALLBACK_INCREASE => {
                        Some(&self.release_fallback_increase_focus)
                    }
                    ACCESSIBILITY_AUDIO => Some(&self.audio_focus),
                    ACCESSIBILITY_BEHAVIOR_SHORTCUTS => Some(&self.behavior_shortcuts_focus),
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
                    shortcut_target_for_accessibility_node(
                        &snapshot.shortcuts,
                        snapshot.active_model.as_ref(),
                        &snapshot.model_catalog.entries,
                        target,
                    )
                    .and_then(|target| self.shortcut_row_focus.get(&target))
                    .or_else(|| {
                        shortcut_clear_target_for_accessibility_node(
                            &snapshot.shortcuts,
                            snapshot.active_model.as_ref(),
                            &snapshot.model_catalog.entries,
                            target,
                        )
                        .and_then(|target| self.shortcut_clear_focus.get(&target))
                    })
                });
                if target != ACCESSIBILITY_LANGUAGE && target != ACCESSIBILITY_THEME {
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
        let active_model = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.active_model.as_ref());
        let model_entries = snapshot
            .as_ref()
            .map(|snapshot| snapshot.model_catalog.entries.as_slice())
            .unwrap_or_default();
        let language = snapshot
            .as_ref()
            .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                snapshot.resolved_language
            });
        if let Some(error) = self.pending_notification.take() {
            window.push_notification(
                Notification::new()
                    .id::<SettingsServiceErrorNotification>()
                    .message(settings_error(language, error))
                    .with_type(NotificationType::Error),
                cx,
            );
        }
        self.sync_shortcut_row_focus(&shortcuts, active_model, model_entries, disabled, cx);
        let status: SharedString = match (self.pending, &snapshot) {
            (Some(PendingOperation::Refresh), _) => {
                bongocat_i18n::text(language.catalog_locale(), "status.refreshing").into()
            }
            (Some(_), _) => bongocat_i18n::text(language.catalog_locale(), "status.saving").into(),
            (None, Some(snapshot)) => {
                let health = match snapshot.runtime_health {
                    RuntimeHealth::Starting => {
                        bongocat_i18n::text(language.catalog_locale(), "status.starting")
                    }
                    RuntimeHealth::Ready => {
                        bongocat_i18n::text(language.catalog_locale(), "status.ready")
                    }
                    RuntimeHealth::Degraded => {
                        bongocat_i18n::text(language.catalog_locale(), "status.degraded")
                    }
                    RuntimeHealth::Stopped => {
                        bongocat_i18n::text(language.catalog_locale(), "status.stopped")
                    }
                };
                runtime_status(language, health, snapshot.revision).into()
            }
            _ => bongocat_i18n::text(language.catalog_locale(), "status.connecting").into(),
        };
        let status_is_error = false;
        let view_entity = cx.entity();
        let startup_item = startup_item_presentation(
            snapshot.as_ref().map(|snapshot| snapshot.startup_item),
            disabled,
            language,
        );

        let general_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.general.title",
        ))
        .default_open(true)
        .description(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.general.description",
        ))
        .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.appearance.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.appearance.theme.label",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                let state = view.read(app).theme_select.clone();
                                Select::new(&state)
                                    .accessibility_label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.appearance.theme.label",
                                    ))
                                    .disabled(disabled)
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.appearance.theme.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.appearance.language.label",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                let state = view.read(app).language_select.clone();
                                Select::new(&state)
                                    .accessibility_label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.appearance.language.label",
                                    ))
                                    .disabled(disabled)
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.appearance.language.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.overlay.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(language.catalog_locale(), "settings.runtime.title"),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.runtime.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.visibility.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.visibility.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.always_on_top.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.always_on_top.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.click_through.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.click_through.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.keep_inside_work_area.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.overlay.keep_inside_work_area)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        if let Some(snapshot) = view.snapshot.as_ref() {
                                            let mut settings = snapshot.overlay;
                                            settings.keep_inside_work_area = value;
                                            view.set_overlay_settings(settings, cx);
                                        }
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.keep_inside_work_area.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.motion_audio.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.motion_audio.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.scale.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.scale.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.opacity.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.opacity.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.maximum_fps.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.maximum_fps.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.model_interaction.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.behavior_shortcuts.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.behavior_shortcuts_enabled)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_behavior_shortcuts_enabled(value, cx)
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.model_interaction.behavior_shortcuts.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.mirror_model.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.model_interaction.mirror_model.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.mirror_pointer_tracking.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.model_interaction.mirror_pointer_tracking.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.ignore_pointer_input.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.model_interaction.ignore_pointer_input.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.release_fallback_timeout.label",
                        ),
                        SettingField::number_input(
                            NumberFieldOptions {
                                min: 0.0,
                                max: 60_000.0,
                                step: 250.0,
                            },
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .map_or(500.0, |s| f64::from(s.release_fallback_timeout_ms))
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_release_fallback_timeout_value(value, cx)
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.release_fallback_timeout.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input.gamepad_stick_dead_zone.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.input.gamepad_stick_dead_zone.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input.gamepad_trigger_dead_zone.label",
                        ),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.input.gamepad_trigger_dead_zone.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.title",
                ))
                .items({
                    let mut items = vec![
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.application.status_icon.label",
                            ),
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
                        .description(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.application.status_icon.description",
                        )),
                    ];
                    #[cfg(target_os = "windows")]
                    items.push(
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.application.taskbar_icon.label",
                            ),
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
                        .description(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.application.taskbar_icon.description",
                        )),
                    );
                    items.push(
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.application.auto_update.label",
                            ),
                            SettingField::switch(
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app)
                                            .snapshot
                                            .as_ref()
                                            .is_some_and(|s| s.check_for_updates_automatically)
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_check_for_updates_automatically(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.application.auto_update.description",
                        )),
                    );
                    items.push(
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.application.open_at_login.label",
                            ),
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

        let models_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.models.title",
        ))
        .description(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.models.description",
        ))
        .group(
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.catalog.title",
                ))
                .item(
                    SettingItem::new(
                        bongocat_i18n::text(language.catalog_locale(), "models.installed.title"),
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
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "models.installed.description",
                    )),
                ),
        );

        let shortcuts_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.shortcuts.title",
        ))
        .description(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.shortcuts.description",
        ))
        .group(
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "navigation.shortcuts.title",
                ))
                .item(
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "navigation.shortcuts.title",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                                let snapshot = view.read(app).snapshot.clone();
                                let tokens = Tokens::from_theme(app);
                                view.update(app, move |view, cx| {
                                    view.page = SettingsPage::Shortcuts;
                                    shortcuts_page::content(
                                        view,
                                        window,
                                        cx,
                                        snapshot.as_ref(),
                                        disabled,
                                        tokens,
                                    )
                                })
                                .into_any_element()
                            }
                        }),
                    )
                    .layout(Axis::Vertical)
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "navigation.shortcuts.description",
                    )),
                ),
        );

        let diagnostics_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.diagnostics.title",
        ))
        .description(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.diagnostics.description",
        ))
        .group(
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.runtime_and_input.title",
                ))
                .item(
                    SettingItem::new(
                        bongocat_i18n::text(language.catalog_locale(), "diagnostics.runtime.title"),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                                let snapshot = view.read(app).snapshot.clone();
                                let tokens = Tokens::from_theme(app);
                                view.update(app, move |view, cx| {
                                    view.page = SettingsPage::Diagnostics;
                                    diagnostics::content(
                                        view,
                                        window,
                                        cx,
                                        snapshot.as_ref(),
                                        disabled,
                                        tokens,
                                    )
                                })
                                .into_any_element()
                            }
                        }),
                    )
                    .layout(Axis::Vertical)
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "diagnostics.runtime.description",
                    )),
                ),
        );

        let about_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.about.title",
        ))
        .description(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.about.description",
        ))
        .group(
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "about.page_title",
                ))
                .item(
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "about.product_information.title",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _window: &mut Window, app: &mut App| {
                                let snapshot = view.read(app).snapshot.clone();
                                view.update(app, move |view, _cx| {
                                    view.page = SettingsPage::About;
                                    about::content(snapshot.as_ref())
                                })
                                .into_any_element()
                            }
                        }),
                    )
                    .layout(Axis::Vertical)
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "about.product_information.description",
                    )),
                ),
        );

        let settings = Settings::new("bongocat-settings")
            .sidebar_width(px(220.0))
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(vec![
                general_page,
                models_page,
                shortcuts_page,
                diagnostics_page,
                about_page,
            ]);
        let footer = div()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .p_4()
            .border_t_1()
            .border_color(tokens.border)
            .child(
                icon_command_button(
                    "refresh-settings-control",
                    bongocat_i18n::text(language.catalog_locale(), "actions.refresh"),
                    IconName::RotateCw,
                    &self.refresh_focus,
                    30,
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
                icon_command_button(
                    "quit-application-control",
                    bongocat_i18n::text(language.catalog_locale(), "actions.quit"),
                    IconName::Close,
                    &self.quit_focus,
                    31,
                    false,
                )
                .id("quit-application")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.quit_focus, cx);
                    view.request_quit_after_flush(cx);
                })),
            );

        div()
            .id("bongocat-settings-root")
            .on_key_down(cx.listener(|view, event, window, cx| {
                if view.shortcut_capture.is_some() {
                    cx.stop_propagation();
                    view.capture_shortcut(event, window, cx);
                }
            }))
            .on_key_up(cx.listener(|view, event, window, cx| {
                view.update_shortcut_capture_on_key_up(event, window, cx);
            }))
            .on_modifiers_changed(cx.listener(
                |view, event: &gpui_kit::ModifiersChangedEvent, window, cx| {
                    view.update_shortcut_capture_modifiers(event.modifiers, window, cx);
                },
            ))
            .size_full()
            .flex()
            .flex_col()
            .child(div().min_h_0().w_full().flex_1().child(settings))
            .child(footer)
            .children(Root::render_notification_layer(window, cx))
    }
}
