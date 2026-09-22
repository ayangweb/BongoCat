use super::*;
use gpui_kit::assets::IconName;

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if viewport.width <= px(0.) || viewport.height <= px(0.) {
            return div().size_full().into_any_element();
        }
        let snapshot = self.snapshot.clone();
        // The appearance is applied on every frame, seeded included: the first frame is
        // painted before the service answers the first snapshot, and it has to be the
        // product's theme rather than the component default.
        let appearance_theme = self.display_appearance_theme();
        self.sync_component_theme(appearance_theme, window, cx);
        if let Some(snapshot) = snapshot.as_ref() {
            self.sync_component_inputs(snapshot, window, cx);
        }
        let disabled =
            self.pending.is_some() || snapshot.is_none() || self.model_import.is_running();
        // Every page renders its rows from the stable states where editing is
        // structurally impossible — no snapshot, unusable configuration, a
        // model import running. The transient in-flight `pending` flag must
        // not feed any gate or row: it flips on and off around every save and
        // visibly dims and re-enables the page on each control change, which
        // reads as the page refreshing. The header status is the saving
        // indicator instead (see `setting_gate` for the unified rule).
        let editing_blocked = snapshot.is_none() || self.model_import.is_running();
        let hover_hide_delay_available = snapshot
            .as_ref()
            .is_some_and(|snapshot| hover_hide_delay_applies(snapshot.overlay));
        // The hover hide delay is inert while the switch above it is off, so
        // its row renders disabled rather than accepting a value nothing
        // reads — the unified gate rule's control arm.
        let hover_hide_delay_gate = SettingGate::new(editing_blocked, hover_hide_delay_available);
        // One gate per shortcut scope: the switch that owns the group plus the
        // shared structural editing state. Each scope's rows read their own
        // gate; the gate switches themselves stay operable while off.
        let command_shortcuts_gate = SettingGate::new(
            editing_blocked,
            snapshot
                .as_ref()
                .is_some_and(|snapshot| shortcuts_page::ShortcutScope::Window.is_enabled(snapshot)),
        );
        let behavior_shortcuts_gate = SettingGate::new(
            editing_blocked,
            snapshot
                .as_ref()
                .is_some_and(|snapshot| shortcuts_page::ShortcutScope::Model.is_enabled(snapshot)),
        );
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
        let language = self.display_language();
        if let Some(error) = self.pending_notification.take() {
            window.push_notification(
                Notification::new()
                    .id::<SettingsServiceErrorNotification>()
                    .message(settings_error(language, error))
                    .with_type(NotificationType::Error),
                cx,
            );
        }
        self.sync_shortcut_row_focus(&shortcuts, active_model, model_entries, editing_blocked, cx);
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

        // The landing page keeps the two preferences every user reaches for
        // first. The other four concerns that used to share this page — the
        // model window, how the model reacts, the input devices and the
        // app-level integrations — each have a page now.
        let general_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.general.title",
        ))
        .icon(IconName::Settings)
        .default_open(true)
        .title_suffix(page_reporter(view_entity.clone(), SettingsPage::General))
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
                                Select::new(&state).disabled(disabled).into_any_element()
                            }
                        }),
                    ),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.appearance.language.label",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                let state = view.read(app).language_select.clone();
                                Select::new(&state).disabled(disabled).into_any_element()
                            }
                        }),
                    ),
                ]),
        ]);

        // The model window itself: how it behaves on the desktop, how it looks,
        // and how often it draws. The runtime status moved to Application and
        // the motion audio to Interaction, where they describe those pages.
        let overlay_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.overlay.title",
        ))
        .icon(IconName::AppWindow)
        .title_suffix(page_reporter(view_entity.clone(), SettingsPage::Overlay))
        .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.overlay.behavior.title",
                ))
                .items(vec![
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
                    ),
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
                    ),
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
                    ),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.keep_inside_screen.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.overlay.keep_inside_screen)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        if let Some(snapshot) = view.snapshot.as_ref() {
                                            let mut settings = snapshot.overlay;
                                            settings.keep_inside_screen = value;
                                            view.set_overlay_settings(settings, cx);
                                        }
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.keep_inside_screen.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.hide_on_mouse_hover.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.overlay.hide_on_pointer_hover)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        if let Some(snapshot) = view.snapshot.as_ref() {
                                            let mut settings = snapshot.overlay;
                                            settings.hide_on_pointer_hover = value;
                                            view.set_overlay_settings(settings, cx);
                                        }
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.hide_on_mouse_hover.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.hide_on_mouse_hover_delay.label",
                        ),
                        SettingField::number_input(
                            NumberFieldOptions {
                                min: 0.0,
                                max: f64::from(
                                    bongocat_config::MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS,
                                ),
                                step: 1.0,
                            },
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app).snapshot.as_ref().map_or(0.0, |s| {
                                        f64::from(s.overlay.hide_on_pointer_hover_delay_seconds)
                                    })
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_overlay_hover_hide_delay_value(value, cx)
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.hide_on_mouse_hover_delay.description",
                    ))
                    .disabled(hover_hide_delay_gate.disables_controls()),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.overlay.appearance.title",
                ))
                .items(vec![
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
                            "settings.overlay.corner_radius.label",
                        ),
                        SettingField::number_input(
                            NumberFieldOptions {
                                min: 0.0,
                                max: 50.0,
                                step: 5.0,
                            },
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .map_or(0.0, |s| f64::from(s.overlay.corner_radius_percent))
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_overlay_corner_radius_value(value, cx)
                                    });
                                }
                            },
                        ),
                    )
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.corner_radius.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.overlay.performance.title",
                ))
                .items(vec![
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
        ]);

        // How the model reacts to what the user does: the two mirror switches
        // and the motion audio sit with the model they belong to, and the
        // pointer pair stays apart because it is about following the cursor.
        // The model behaviour shortcut gate is not here: both shortcut gates
        // live on the Shortcuts page, directly above the rows they gate, so
        // nobody has to find the one that silences a list shown elsewhere.
        let interaction_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.interaction.title",
        ))
        .icon(IconName::MousePointer2)
        .title_suffix(page_reporter(
            view_entity.clone(),
            SettingsPage::Interaction,
        ))
        .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.model_interaction.model.title",
                ))
                .items(vec![
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
                    ),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.motion_audio.label",
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
                    ),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.model_interaction.mouse.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.mirror_mouse_tracking.label",
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
                    ),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.model_interaction.ignore_mouse_input.label",
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
                    ),
                ]),
        ]);

        // The two input devices are configured separately: the keyboard's
        // release recovery has nothing in common with the gamepad dead zones.
        let input_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.input.title",
        ))
        .icon(IconName::Gamepad2)
        .title_suffix(page_reporter(view_entity.clone(), SettingsPage::Input))
        .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input.keyboard.title",
                ))
                .items(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input.release_fallback_timeout.label",
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
                        "settings.input.release_fallback_timeout.description",
                    )),
                ]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input.gamepad.title",
                ))
                .items(vec![
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
        ]);

        // The application's own surface: how it is running, how it integrates
        // with the desktop, and how it starts and updates itself. The runtime
        // status came here from the model window group it never belonged to —
        // it reports the app's health, not the window's.
        let application_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.application.title",
        ))
        .icon(IconName::Cog)
        .title_suffix(page_reporter(
            view_entity.clone(),
            SettingsPage::Application,
        ))
        .groups(vec![
            // No group title: the runtime status is the first thing on the page and the only
            // row the group holds, so a heading here would just repeat the row's own label.
            SettingGroup::new().items(vec![SettingItem::new(
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
            )]),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.system.title",
                ))
                .items({
                    // Only Windows adds the taskbar icon below, so the binding is
                    // `mut` on one platform and not the other.
                    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
                    let mut items = vec![SettingItem::new(
                        bongocat_i18n::platform_text(
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
                    )];
                    #[cfg(target_os = "windows")]
                    items.push(SettingItem::new(
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
                    ));
                    items
                }),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.updates.title",
                ))
                .items({
                    // Built by hand instead of `SettingField::switch` so the
                    // switch can carry a tooltip: the row is the only place the
                    // unavailable-in-this-build reason can be shown, and the
                    // packaged switch field cannot take one (ADR-0051). The switch
                    // position already answers its two steady states, so only the
                    // states that still need explaining carry a row description.
                    let mut open_at_login = SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.application.open_at_login.label",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _: &mut Window, _: &mut App| {
                                Switch::new(STARTUP_ITEM_SWITCH_ID)
                                    .checked(startup_item.enabled)
                                    .disabled(startup_item.switch_disabled())
                                    .when_some(startup_item.unavailable_hint, |switch, hint| {
                                        switch.tooltip(hint)
                                    })
                                    .on_change({
                                        let view = view.clone();
                                        move |enabled: &bool, _: &mut Window, cx: &mut App| {
                                            view.update(cx, |view, cx| {
                                                view.set_startup_item_enabled(*enabled, cx)
                                            });
                                        }
                                    })
                                    .into_any_element()
                            }
                        }),
                    );
                    if let Some(description) = startup_item.description {
                        open_at_login = open_at_login.description(description);
                    }
                    let mut items = vec![open_at_login];
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
                    items
                }),
        ]);

        // The page shell already carries the title and description, and the
        // model list is one flat grid, so the group and item render without
        // their own labels — labelling each nesting level is what turned this
        // page into stacked boxes in the first place.
        let models_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.models.title",
        ))
        .icon(IconName::Cat)
        .group(SettingGroup::new().item(SettingItem::render({
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
        })));

        // The page's two scopes are two titled groups rather than tabs: a
        // `SettingPage` cannot host child pages, but the settings component
        // renders every titled group of a page with more than one group as a
        // second-level sidebar entry, so both scopes stay directly reachable
        // from the sidebar and the body renders them one after the other.
        let shortcuts_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.shortcuts.title",
        ))
        .icon(IconName::Keyboard)
        .groups(vec![
            shortcuts_page::group(
                shortcuts_page::ShortcutScope::Window,
                language,
                view_entity.clone(),
                command_shortcuts_gate,
            ),
            shortcuts_page::group(
                shortcuts_page::ShortcutScope::Model,
                language,
                view_entity.clone(),
                behavior_shortcuts_gate,
            ),
        ]);

        let about_page = SettingPage::new(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.about.title",
        ))
        .icon(IconName::Info)
        .group({
            let mut about_group = SettingGroup::new()
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
                    .layout(Axis::Vertical),
                );
            about_group =
                about_group.item(
                    SettingItem::new(
                        bongocat_i18n::text(language.catalog_locale(), "update.about.label"),
                        SettingField::element({
                            let view = view_entity.clone();
                            let label_locale = language.catalog_locale();
                            move |_: &RenderOptions, _window: &mut Window, app: &mut App| {
                                let request_update = view.read(app).request_update.clone();
                                div()
                                    .id("about-check-for-updates")
                                    .on_click(move |_, _, app| (request_update)(app))
                                    .child(Button::new("about-check-for-updates-button").label(
                                        bongocat_i18n::text(label_locale, "update.about.label"),
                                    ))
                                    .into_any_element()
                            }
                        }),
                    )
                    .layout(Axis::Vertical)
                    .description(bongocat_i18n::text(
                        language.catalog_locale(),
                        "update.about.description",
                    )),
                );
            about_group
        });

        let settings = Settings::new("bongocat-settings")
            .sidebar_width(px(220.0))
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(vec![
                general_page,
                models_page,
                overlay_page,
                interaction_page,
                input_page,
                shortcuts_page,
                application_page,
                about_page,
            ]);

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
            .children(Root::render_notification_layer(window, cx))
            .into_any_element()
    }
}

/// Reports the page a rendered header belongs to.
///
/// `SettingsView::page` tracks the visible page. The pages that build custom
/// content already report themselves while building it; pages made of plain
/// setting items get this empty hook in their header.
fn page_reporter(
    view: Entity<SettingsView>,
    page: SettingsPage,
) -> impl Fn(&mut Window, &mut App) -> Div {
    move |_window, cx| {
        view.update(cx, |view, _| view.page = page);
        div()
    }
}
