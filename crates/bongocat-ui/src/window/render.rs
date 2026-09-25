use super::*;
use gpui_kit::{FileDropEvent, canvas};

fn with_search_keywords<I>(items: I, keywords: &[SharedString]) -> impl Iterator<Item = SettingItem>
where
    I: IntoIterator<Item = SettingItem>,
{
    items
        .into_iter()
        .map(move |item| item.keywords(keywords.iter().cloned()))
}

/// `gpui-kit` owns the sidebar selection in window-keyed state. The empty title
/// suffix is the component's public per-page render hook, so it lets the process
/// owner observe the active page without taking over the component's navigation.
fn report_navigation_page(page: SettingsNavigationPage, memory: &SettingsNavigationMemory) {
    memory.select(page);
}

fn page_reporter(
    page: SettingsNavigationPage,
    memory: SettingsNavigationMemory,
) -> impl Fn(&mut Window, &mut App) -> Div {
    move |_, _| {
        report_navigation_page(page, &memory);
        div()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if viewport.width <= px(0.) || viewport.height <= px(0.) {
            return div().size_full().into_any_element();
        }
        // `on_file_drop_exit` is intentionally hitbox-scoped in GPUI. Once the
        // pointer is outside the window, that hitbox is no longer hovered and
        // the element callback can miss the platform's DragLeave/DragExit event.
        // Register a window-level listener from a paint-phase canvas instead;
        // it remains active even when the drag leaves from outside the viewport.
        let drag_exit_view = cx.entity().downgrade();
        let model_drag_exit_listener = canvas(|_bounds, _window, _cx| {}, {
            let drag_exit_view = drag_exit_view.clone();
            move |_bounds, _state, window, _cx| {
                window.on_mouse_event(move |event: &FileDropEvent, _phase, _window, cx| {
                    if matches!(event, FileDropEvent::Exited) {
                        let _ = drag_exit_view.update(cx, |view, cx| view.clear_model_drag(cx));
                    }
                });
            }
        })
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let snapshot = self.snapshot.clone();
        // The appearance is applied on every frame, seeded included: the first frame is
        // painted before the service answers the first snapshot, and it has to be the
        // product's theme rather than the component default.
        let appearance_theme = self.display_appearance_theme();
        self.sync_component_theme(appearance_theme, window, cx);
        if let Some(snapshot) = snapshot.as_ref() {
            self.sync_component_inputs(snapshot, window, cx);
        }
        // Every page renders its rows from the stable states where editing is
        // structurally impossible — no snapshot, unusable configuration, a
        // model import running. The transient in-flight `pending` flag must
        // not feed any gate or row: it flips on and off around every save and
        // visibly dims and re-enables the page on each control change, which
        // reads as the page refreshing. `editing_blocked` is that one
        // predicate, shared by every page's gate — the Appearance page's selects
        // and the startup switch below read it exactly like the gated rows do
        // (see `setting_gate` for the unified rule).
        let editing_blocked = self.editing_blocked(snapshot.as_ref());
        let hover_hide_delay_available = snapshot
            .as_ref()
            .is_some_and(|snapshot| hover_hide_delay_applies(snapshot.overlay));
        // The hover hide delay is inert while the switch above it is off, so
        // its row renders disabled rather than accepting a value nothing
        // reads — the unified gate rule's control arm.
        let hover_hide_delay_gate = SettingGate::new(editing_blocked, hover_hide_delay_available);
        let check_for_updates_interval_gate = SettingGate::new(
            editing_blocked,
            snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.check_for_updates_automatically),
        );
        let random_behavior_gate = SettingGate::new(
            editing_blocked,
            snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.random_behavior.enabled),
        );
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
        self.sync_mver_mode_dialog(window, cx);
        if let Some(error) = self.pending_notification.take() {
            window.push_notification(
                Notification::new()
                    .id::<SettingsServiceErrorNotification>()
                    .message(settings_error(language, error))
                    .with_type(NotificationType::Error),
                cx,
            );
        }
        // Reported before the success below, so a run that installed several
        // models and could not prepare one of them reads as "this one failed"
        // first and "the rest are in" second.
        if self.model_import_failed_pending {
            self.model_import_failed_pending = false;
            window.push_notification(
                Notification::new()
                    .id::<ModelImportFailedNotification>()
                    .message(bongocat_i18n::text(
                        language.catalog_locale(),
                        "models.import.failed",
                    ))
                    .with_type(NotificationType::Error),
                cx,
            );
        }
        if self.model_import_success_pending {
            self.model_import_success_pending = false;
            window.push_notification(
                Notification::new()
                    .id::<ModelImportSuccessNotification>()
                    .message(bongocat_i18n::text(
                        language.catalog_locale(),
                        "models.import.success",
                    ))
                    .with_type(NotificationType::Success),
                cx,
            );
        }
        if self.about_copy_success_pending {
            self.about_copy_success_pending = false;
            window.push_notification(
                Notification::new()
                    .id::<AboutCopySuccessNotification>()
                    .message(bongocat_i18n::text(
                        language.catalog_locale(),
                        "about.software_information.copy_success",
                    ))
                    .with_type(NotificationType::Success),
                cx,
            );
        }
        self.sync_shortcut_row_focus(&shortcuts, active_model, model_entries, editing_blocked, cx);
        let view_entity = cx.entity();
        let navigation_memory = self.navigation_memory.clone();
        let startup_item = startup_item_presentation(
            snapshot.as_ref().map(|snapshot| snapshot.startup_item),
            editing_blocked,
            language,
        );

        // Theme and language stay directly on the first page. Repeating an
        // "Appearance" group below an "Appearance & language" page title would
        // add a level without helping the user find either control.
        let appearance_keywords =
            SettingsNavigationPage::Appearance.search_keywords(language, std::iter::empty());
        let appearance_page = SettingPage::new(SettingsNavigationPage::Appearance.title(language))
            .icon(SettingsNavigationPage::Appearance.icon())
            .default_open(true)
            .title_suffix(page_reporter(
                SettingsNavigationPage::Appearance,
                navigation_memory.clone(),
            ))
            .group(SettingGroup::new().items(with_search_keywords(
                vec![
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
                                    .disabled(editing_blocked)
                                    .into_any_element()
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
                                Select::new(&state)
                                    .disabled(editing_blocked)
                                    .into_any_element()
                            }
                        }),
                    ),
                ],
                &appearance_keywords,
            )));

        // The model window itself: how it behaves on the desktop, how it looks,
        // and how often it draws. Model-wide mirroring, audio, and random
        // behavior live with the model instead of being split across pages.
        let model_window_behavior_keywords = SettingsNavigationPage::ModelWindow
            .search_keywords(language, ["settings.overlay.behavior.title"]);
        let model_window_appearance_keywords = SettingsNavigationPage::ModelWindow
            .search_keywords(language, ["settings.overlay.appearance.title"]);
        let model_window_performance_keywords = SettingsNavigationPage::ModelWindow
            .search_keywords(language, ["settings.overlay.performance.title"]);
        let overlay_page = SettingPage::new(SettingsNavigationPage::ModelWindow.title(language))
            .icon(SettingsNavigationPage::ModelWindow.icon())
            .title_suffix(page_reporter(
                SettingsNavigationPage::ModelWindow,
                navigation_memory.clone(),
            ))
            .groups(vec![
                SettingGroup::new()
                    .title(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.behavior.title",
                    ))
                    .items(with_search_keywords(
                        vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.overlay.hide_model_window.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| !s.overlay_visible)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_overlay_visible(!value, cx)
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
                ],
                        &model_window_behavior_keywords,
                    )),
                SettingGroup::new()
                    .title(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.appearance.title",
                    ))
                    .items(with_search_keywords(
                        vec![
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
                                            view.read(app).snapshot.as_ref().map_or(100.0, |s| {
                                                f64::from(s.overlay.scale_percent)
                                            })
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
                                            view.read(app).snapshot.as_ref().map_or(100.0, |s| {
                                                f64::from(s.overlay.opacity_percent)
                                            })
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
                                            view.read(app).snapshot.as_ref().map_or(0.0, |s| {
                                                f64::from(s.overlay.corner_radius_percent)
                                            })
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
                        ],
                        &model_window_appearance_keywords,
                    )),
                SettingGroup::new()
                    .title(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.overlay.performance.title",
                    ))
                    .items(with_search_keywords(
                        vec![
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
                        ],
                        &model_window_performance_keywords,
                    )),
            ]);

        // Model-wide display and automatic behavior belong with the model the
        // user is choosing. Behavior shortcut bindings remain on Shortcuts:
        // that page owns discrete command mappings and their two gates.
        let model_behavior_keywords =
            SettingsNavigationPage::ModelBehavior.search_keywords(language, std::iter::empty());
        let model_behavior_group = SettingGroup::new().items(with_search_keywords(
            vec![
                SettingItem::new(
                    bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.models.behavior.mirror_model.label",
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
                        "settings.models.behavior.motion_audio.label",
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
                SettingItem::new(
                    bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.models.behavior.random_behavior_enabled.label",
                    ),
                    SettingField::switch(
                        {
                            let view = view_entity.clone();
                            move |app| {
                                view.read(app)
                                    .snapshot
                                    .as_ref()
                                    .is_some_and(|s| s.random_behavior.enabled)
                            }
                        },
                        {
                            let view = view_entity.clone();
                            move |value, app| {
                                view.update(app, |view, cx| {
                                    view.set_random_behavior_enabled(value, cx)
                                });
                            }
                        },
                    ),
                )
                .disabled(random_behavior_gate.disables_switch()),
                SettingItem::new(
                    bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.models.behavior.random_behavior_interval.label",
                    ),
                    SettingField::number_input(
                        NumberFieldOptions {
                            min: f64::from(
                                bongocat_config::MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
                            ),
                            max: f64::from(
                                bongocat_config::MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
                            ),
                            step: 1.0,
                        },
                        {
                            let view = view_entity.clone();
                            move |app| {
                                view.read(app).snapshot.as_ref().map_or(30.0, |snapshot| {
                                    f64::from(snapshot.random_behavior.interval_seconds)
                                })
                            }
                        },
                        {
                            let view = view_entity.clone();
                            move |value, app| {
                                view.update(app, |view, cx| {
                                    view.set_random_behavior_interval_value(value, cx)
                                });
                            }
                        },
                    ),
                )
                .disabled(random_behavior_gate.disables_controls()),
            ],
            &model_behavior_keywords,
        ));

        // Continuous device input and the model's response to it form one
        // task-oriented page. Discrete shortcut bindings stay separate below.
        let mouse_keywords = SettingsNavigationPage::InputInteraction
            .search_keywords(language, ["settings.input_interaction.mouse.title"]);
        let keyboard_keywords = SettingsNavigationPage::InputInteraction
            .search_keywords(language, ["settings.input_interaction.keyboard.title"]);
        let gamepad_keywords = SettingsNavigationPage::InputInteraction
            .search_keywords(language, ["settings.input_interaction.gamepad.title"]);
        let input_interaction_page =
            SettingPage::new(SettingsNavigationPage::InputInteraction.title(language))
                .icon(SettingsNavigationPage::InputInteraction.icon())
                .title_suffix(page_reporter(
                    SettingsNavigationPage::InputInteraction,
                    navigation_memory.clone(),
                ))
                .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input_interaction.mouse.title",
                ))
                .items(with_search_keywords(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input_interaction.mouse.ignore_mouse_input.label",
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
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input_interaction.mouse.mirror_mouse_tracking.label",
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
                ], &mouse_keywords)),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input_interaction.keyboard.title",
                ))
                .items(with_search_keywords(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input_interaction.keyboard.ignore_keyboard_input.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.model_settings.ignore_keyboard)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        if let Some(s) = view.snapshot.as_ref() {
                                            let mut settings = s.model_settings;
                                            settings.ignore_keyboard = value;
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
                            "settings.input_interaction.keyboard.key_release_timeout.label",
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
                        "settings.input_interaction.keyboard.key_release_timeout.description",
                    )),
                ], &keyboard_keywords)),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.input_interaction.gamepad.title",
                ))
                .items(with_search_keywords(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input_interaction.gamepad.ignore_gamepad_input.label",
                        ),
                        SettingField::switch(
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app)
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|s| s.model_settings.ignore_gamepad)
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        if let Some(s) = view.snapshot.as_ref() {
                                            let mut settings = s.model_settings;
                                            settings.ignore_gamepad = value;
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
                            "settings.input_interaction.gamepad.stick_dead_zone.label",
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
                        "settings.input_interaction.gamepad.stick_dead_zone.description",
                    )),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.input_interaction.gamepad.trigger_dead_zone.label",
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
                        "settings.input_interaction.gamepad.trigger_dead_zone.description",
                    )),
                ], &gamepad_keywords)),
        ]);

        // The application's own surface: how it integrates with the desktop,
        // how it updates itself, and how much diagnostic logging it retains.
        let app_desktop_keywords = SettingsNavigationPage::AppSystem
            .search_keywords(language, ["settings.app_system.desktop.title"]);
        let app_updates_keywords = SettingsNavigationPage::AppSystem
            .search_keywords(language, ["settings.app_system.updates.title"]);
        let app_logging_keywords = SettingsNavigationPage::AppSystem
            .search_keywords(language, ["settings.app_system.logging.title"]);
        let app_system_page = SettingPage::new(SettingsNavigationPage::AppSystem.title(language))
            .icon(SettingsNavigationPage::AppSystem.icon())
            .title_suffix(page_reporter(
                SettingsNavigationPage::AppSystem,
                navigation_memory.clone(),
            ))
            .groups(vec![
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.desktop.title",
                ))
                .items(with_search_keywords({
                    // The login item is the first desktop preference because it
                    // describes what happens before the rest of the application.
                    let mut items = Vec::new();
                    let mut open_at_login = SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.app_system.open_at_login.label",
                        ),
                        SettingField::switch(move |_app| startup_item.enabled, {
                            let view = view_entity.clone();
                            move |enabled: bool, app: &mut App| {
                                view.update(app, |view, cx| {
                                    view.set_startup_item_enabled(enabled, cx)
                                });
                            }
                        }),
                    )
                    .disabled(startup_item.disabled);
                    if let Some(description) = startup_item.description {
                        open_at_login = open_at_login.description(description);
                    }
                    items.push(open_at_login);
                    items.push(SettingItem::new(
                        bongocat_i18n::platform_text(
                            language.catalog_locale(),
                            "settings.app_system.status_icon.label",
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
                    ));
                    #[cfg(target_os = "windows")]
                    items.push(SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.app_system.taskbar_icon.label",
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
                }, &app_desktop_keywords)),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.updates.title",
                ))
                .items(with_search_keywords({
                    let mut items = Vec::new();
                    items.push(
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.app_system.auto_update.label",
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
                        .disabled(check_for_updates_interval_gate.disables_switch()),
                    );
                    items.push(
                        SettingItem::new(
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.app_system.auto_update.interval.label",
                            ),
                            SettingField::number_input(
                                check_for_updates_interval_number_field_options(),
                                {
                                    let view = view_entity.clone();
                                    move |app| {
                                        view.read(app).snapshot.as_ref().map_or(
                                            f64::from(
                                                bongocat_config::DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS,
                                            ),
                                            |snapshot| {
                                                f64::from(snapshot.check_for_updates_interval_hours)
                                            },
                                        )
                                    }
                                },
                                {
                                    let view = view_entity.clone();
                                    move |value, app| {
                                        view.update(app, |view, cx| {
                                            view.set_check_for_updates_interval_hours(value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .disabled(check_for_updates_interval_gate.disables_controls()),
                    );
                    items
                }, &app_updates_keywords)),
            SettingGroup::new()
                .title(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.logging.title",
                ))
                .items(with_search_keywords(vec![
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.app_system.logging.level.label",
                        ),
                        SettingField::element({
                            let view = view_entity.clone();
                            move |_: &RenderOptions, _: &mut Window, app: &mut App| {
                                let state = view.read(app).logging_level_select.clone();
                                Select::new(&state)
                                    .disabled(editing_blocked)
                                    .into_any_element()
                            }
                        }),
                    )
                    .disabled(editing_blocked),
                    SettingItem::new(
                        bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.app_system.logging.retention_days.label",
                        ),
                        SettingField::number_input(
                            logging_retention_number_field_options(),
                            {
                                let view = view_entity.clone();
                                move |app| {
                                    view.read(app).snapshot.as_ref().map_or(
                                        f64::from(SettingsLogging::default().retention_days),
                                        |snapshot| f64::from(snapshot.logging.retention_days),
                                    )
                                }
                            },
                            {
                                let view = view_entity.clone();
                                move |value, app| {
                                    view.update(app, |view, cx| {
                                        view.set_logging_retention_days_value(value, cx)
                                    });
                                }
                            },
                        ),
                    )
                    .disabled(editing_blocked),
                ], &app_logging_keywords)),
        ]);

        // The model library is a flat grid of self-drawn cards, so its page drops
        // the window-wide Outline surface. Model behavior is a separate page and
        // keeps the normal settings-card surface; neither page shares a body or
        // a scroll region with the other.
        let model_library_keywords = model_library_search_keywords(
            language,
            snapshot
                .as_ref()
                .into_iter()
                .flat_map(|snapshot| snapshot.model_catalog.entries.iter())
                .map(|entry| entry.title.clone()),
        );
        let model_library_group = SettingGroup::new().variant(GroupBoxVariant::Normal).item(
            SettingItem::render({
                let view = view_entity.clone();
                move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                    let snapshot = view.read(app).snapshot.clone();
                    let tokens = Tokens::from_theme(app);
                    view.update(app, move |view, cx| {
                        models::content(view, window, cx, snapshot.as_ref(), tokens)
                    })
                    .into_any_element()
                }
            })
            .keywords(model_library_keywords),
        );
        let model_library_page =
            SettingPage::new(SettingsNavigationPage::ModelLibrary.title(language))
                .icon(SettingsNavigationPage::ModelLibrary.icon())
                .title_suffix(page_reporter(
                    SettingsNavigationPage::ModelLibrary,
                    navigation_memory.clone(),
                ))
                .group(model_library_group);
        let model_behavior_page =
            SettingPage::new(SettingsNavigationPage::ModelBehavior.title(language))
                .icon(SettingsNavigationPage::ModelBehavior.icon())
                .title_suffix(page_reporter(
                    SettingsNavigationPage::ModelBehavior,
                    navigation_memory.clone(),
                ))
                .group(model_behavior_group);

        // The page's two scopes are two titled groups rather than tabs: a
        // `SettingPage` cannot host child pages, but the settings component
        // renders every titled group of a page with more than one group as a
        // second-level sidebar entry, so both scopes stay directly reachable
        // from the sidebar and the body renders them one after the other.
        let shortcuts_page = SettingPage::new(SettingsNavigationPage::Shortcuts.title(language))
            .icon(SettingsNavigationPage::Shortcuts.icon())
            .title_suffix(page_reporter(
                SettingsNavigationPage::Shortcuts,
                navigation_memory.clone(),
            ))
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

        // About is the final normal settings destination. Its operational rows
        // use the same SettingGroup contract as the rest of the settings
        // window; the page intentionally stays focused on product information
        // and support actions.
        let about_keywords =
            SettingsNavigationPage::About.search_keywords(language, std::iter::empty());
        let about_page = SettingPage::new(SettingsNavigationPage::About.title(language))
            .icon(SettingsNavigationPage::About.icon())
            .title_suffix(page_reporter(
                SettingsNavigationPage::About,
                navigation_memory.clone(),
            ))
            .group(about::operational_group(
                view_entity.clone(),
                snapshot.as_ref(),
                language,
                about_keywords,
                self.request_update.clone(),
            ));

        let settings = Settings::new("bongocat-settings")
            .sidebar_width(px(220.0))
            .default_selected_index(SelectIndex {
                page_ix: self.navigation_memory.page_index(),
                group_ix: None,
            })
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(vec![
                appearance_page,
                model_library_page,
                model_behavior_page,
                overlay_page,
                input_interaction_page,
                shortcuts_page,
                app_system_page,
                about_page,
            ]);
        let model_drag_overlay = self.model_drag.map(|state| {
            let view = cx.entity().downgrade();
            model_drag_overlay::render(state, language, Tokens::from_theme(cx))
                .on_drag_move(
                    cx.listener(|view, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                        let path_count = event.drag(cx).paths().len();
                        view.update_model_drag(path_count, cx);
                    }),
                )
                .on_drop({
                    let view = view.clone();
                    move |paths: &ExternalPaths, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.accept_model_folder_drop(paths.paths(), cx);
                        });
                    }
                })
        });

        div()
            .id("bongocat-settings-root")
            .relative()
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let path_count = event.drag(cx).paths().len();
                    view.update_model_drag(path_count, cx);
                }),
            )
            .on_drop(cx.listener(|view, paths: &ExternalPaths, _, cx| {
                view.accept_model_folder_drop(paths.paths(), cx);
            }))
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
            .child(model_drag_exit_listener)
            .children(model_drag_overlay)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_reporter_updates_the_process_navigation_memory() {
        let memory = SettingsNavigationMemory::new();
        report_navigation_page(SettingsNavigationPage::ModelBehavior, &memory);
        assert_eq!(memory.page_index(), 2);
    }
}
