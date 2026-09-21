use super::*;
use crate::SettingsModelCatalog;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn model_import_accessibility_nodes(
    draft: &ModelImportDraft,
    commands_pending: bool,
    configuration_ready: bool,
    language: SettingsLanguage,
) -> [AccessibilityNode; 4] {
    let import_running = draft.is_running();
    let picker_open = draft.is_picker_open();
    let pickers_disabled =
        commands_pending || import_running || picker_open || !configuration_ready;
    let mut choose_folder_node = AccessibilityNode::new(
        ACCESSIBILITY_MODEL_CHOOSE_FOLDER,
        AccessibilityRole::Button,
        bongocat_i18n::text(
            language.catalog_locale(),
            "models.import.actions.choose_folder",
        ),
    )
    .disabled(pickers_disabled);
    if !pickers_disabled {
        choose_folder_node = choose_folder_node.clickable().focusable();
    }
    let mut choose_archive_node = AccessibilityNode::new(
        ACCESSIBILITY_MODEL_CHOOSE_ARCHIVE,
        AccessibilityRole::Button,
        bongocat_i18n::text(
            language.catalog_locale(),
            "models.import.actions.choose_archive",
        ),
    )
    .disabled(pickers_disabled);
    if !pickers_disabled {
        choose_archive_node = choose_archive_node.clickable().focusable();
    }

    let import_status = super::model_import_status(draft, language);
    let import_disabled =
        !import_running && (commands_pending || !configuration_ready || !draft.can_import());
    let mut import_node = AccessibilityNode::new(
        ACCESSIBILITY_MODEL_IMPORT,
        AccessibilityRole::Button,
        if import_running {
            bongocat_i18n::text(language.catalog_locale(), "actions.cancel")
        } else {
            bongocat_i18n::text(language.catalog_locale(), "models.import.actions.import")
        },
    )
    .with_value(import_status.to_string())
    .disabled(import_disabled);
    if !import_disabled {
        import_node = import_node.clickable().focusable();
    }

    let import_status_node = AccessibilityNode::new(
        ACCESSIBILITY_MODEL_IMPORT_STATUS,
        AccessibilityRole::Status,
        bongocat_i18n::text(language.catalog_locale(), "models.import.actions.import"),
    )
    .with_value(import_status.to_string());
    [
        choose_folder_node,
        choose_archive_node,
        import_node,
        import_status_node,
    ]
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn model_catalog_accessibility_status_node(
    catalog: Option<&SettingsModelCatalog>,
    language: SettingsLanguage,
) -> Option<AccessibilityNode> {
    let status = match catalog {
        None => Some(
            bongocat_i18n::text(language.catalog_locale(), "models.catalog.loading").to_owned(),
        ),
        Some(catalog) if catalog.error.is_some() || catalog.entries.is_empty() => {
            Some(super::models::empty_model_catalog_status(Some(catalog), language).to_owned())
        }
        Some(_) => None,
    }?;
    Some(
        AccessibilityNode::new(
            ACCESSIBILITY_MODEL_CATALOG_STATUS,
            AccessibilityRole::Status,
            bongocat_i18n::text(language.catalog_locale(), "models.catalog.available"),
        )
        .with_value(status),
    )
}

/// The accessibility node of one scope's shortcut gate.
///
/// Both gates are configured positively and rendered as "enable …" switches, so
/// the switch reports exactly the configuration field — the same thing the
/// visible row shows and the same thing the command writes. The label comes from
/// the same source the row uses ([`shortcuts_page::ShortcutScope::gate_label`]),
/// and the node carries no value because the row carries no description.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn shortcut_gate_accessibility_node(
    scope: shortcuts_page::ShortcutScope,
    id: AccessibilityNodeId,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    disabled: bool,
) -> AccessibilityNode {
    let mut node =
        AccessibilityNode::new(id, AccessibilityRole::Switch, scope.gate_label(language))
            .with_toggle(
                if snapshot.is_some_and(|snapshot| scope.is_enabled(snapshot)) {
                    AccessibilityToggle::On
                } else {
                    AccessibilityToggle::Off
                },
            )
            .disabled(disabled);
    if !disabled {
        node = node.clickable().focusable();
    }
    node
}

impl SettingsView {
    pub(super) fn accessibility_tree(&self) -> AccessibilityTree {
        let focus = match self.page {
            SettingsPage::General => ACCESSIBILITY_GENERAL,
            SettingsPage::Models => ACCESSIBILITY_MODELS,
            SettingsPage::Overlay => ACCESSIBILITY_OVERLAY_PAGE,
            SettingsPage::Interaction => ACCESSIBILITY_INTERACTION,
            SettingsPage::Input => ACCESSIBILITY_INPUT,
            SettingsPage::Shortcuts => ACCESSIBILITY_SHORTCUTS,
            SettingsPage::Application => ACCESSIBILITY_APPLICATION,
            SettingsPage::About => ACCESSIBILITY_ABOUT,
        };
        self.accessibility_tree_with_focus(focus)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn accessibility_tree_with_focus(
        &self,
        focus: AccessibilityNodeId,
    ) -> AccessibilityTree {
        let snapshot = self.snapshot.as_ref();
        let configuration_ready = snapshot.is_some_and(|snapshot| {
            snapshot.configuration_status == SettingsConfigurationStatus::Ready
        });
        let disabled = self.pending.is_some()
            || snapshot.is_none()
            || self.model_import.is_running()
            || !configuration_ready;
        let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
            snapshot.resolved_language
        });
        let startup =
            startup_item_presentation(snapshot.map(|s| s.startup_item), disabled, language);
        let selected_theme =
            snapshot.map_or(SettingsTheme::System, |snapshot| snapshot.appearance_theme);
        let mut theme_node = AccessibilityNode::new(
            ACCESSIBILITY_THEME,
            AccessibilityRole::ComboBox,
            bongocat_i18n::text(language.catalog_locale(), "settings.appearance.theme.label"),
        )
        .with_value(theme_display_name(selected_theme, language))
        .disabled(disabled);
        if !disabled {
            theme_node = theme_node.clickable().focusable();
        }
        let mut language_node = AccessibilityNode::new(
            ACCESSIBILITY_LANGUAGE,
            AccessibilityRole::ComboBox,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.appearance.language.label",
            ),
        )
        .with_value(
            snapshot
                .map_or(SettingsLanguage::System, |snapshot| snapshot.language)
                .display_name(language),
        )
        .disabled(disabled);
        if !disabled {
            language_node = language_node.clickable().focusable();
        }
        let mut overlay_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.visibility.label",
            ),
        )
        .with_toggle(if snapshot.is_some_and(|s| s.overlay_visible) {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        if !disabled {
            overlay_node = overlay_node.clickable().focusable();
        }
        let mut audio_node = AccessibilityNode::new(
            ACCESSIBILITY_AUDIO,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.model_interaction.motion_audio.label",
            ),
        )
        .with_toggle(if snapshot.is_some_and(|s| s.motion_audio_enabled) {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        if !disabled {
            audio_node = audio_node.clickable().focusable();
        }
        let model_settings = snapshot
            .map(|snapshot| snapshot.model_settings)
            .unwrap_or_default();
        let mut mirror_node = AccessibilityNode::new(
            ACCESSIBILITY_MIRROR,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.model_interaction.mirror_model.label",
            ),
        )
        .with_toggle(if model_settings.mirror {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut mirror_pointer_node = AccessibilityNode::new(
            ACCESSIBILITY_MIRROR_POINTER,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.model_interaction.mirror_mouse_tracking.label",
            ),
        )
        .with_toggle(if model_settings.mirror_pointer_tracking {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut ignore_pointer_node = AccessibilityNode::new(
            ACCESSIBILITY_IGNORE_POINTER,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.model_interaction.ignore_mouse_input.label",
            ),
        )
        .with_toggle(if model_settings.ignore_pointer {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        if !disabled {
            mirror_node = mirror_node.clickable().focusable();
            mirror_pointer_node = mirror_pointer_node.clickable().focusable();
            ignore_pointer_node = ignore_pointer_node.clickable().focusable();
        }
        let axis_settings = snapshot
            .map(|snapshot| snapshot.gamepad_axis_settings)
            .unwrap_or_default();
        let mut stick_node = AccessibilityNode::new(
            ACCESSIBILITY_STICK_DEAD_ZONE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.input.gamepad_stick_dead_zone.label",
            ),
        )
        .with_value(format!("{}%", axis_settings.stick_dead_zone_percent))
        .disabled(disabled);
        let mut trigger_node = AccessibilityNode::new(
            ACCESSIBILITY_TRIGGER_DEAD_ZONE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.input.gamepad_trigger_dead_zone.label",
            ),
        )
        .with_value(format!("{}%", axis_settings.trigger_dead_zone_percent))
        .disabled(disabled);
        if !disabled {
            stick_node = stick_node.clickable().focusable();
            trigger_node = trigger_node.clickable().focusable();
        }
        let overlay_settings = snapshot
            .map(|snapshot| snapshot.overlay)
            .unwrap_or_default();
        let mut topmost_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_TOPMOST,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.always_on_top.label",
            ),
        )
        .with_toggle(if overlay_settings.always_on_top {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut click_through_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.click_through.label",
            ),
        )
        .with_toggle(if overlay_settings.click_through {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut keep_inside_screen_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.keep_inside_screen.label",
            ),
        )
        .with_value(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.keep_inside_screen.description",
        ))
        .with_toggle(if overlay_settings.keep_inside_screen {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut hide_on_pointer_hover_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.hide_on_mouse_hover.label",
            ),
        )
        .with_value(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hide_on_mouse_hover.description",
        ))
        .with_toggle(if overlay_settings.hide_on_pointer_hover {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let hover_hide_delay_seconds = overlay_settings.hide_on_pointer_hover_delay_seconds;
        // The same availability the row renders from, through the unified gate
        // rule: the delay is inert while the hide-on-hover switch is off, so
        // both steppers report themselves unusable.
        let hover_hide_delay_gate =
            SettingGate::new(disabled, hover_hide_delay_applies(overlay_settings));
        let hover_hide_delay_available = !hover_hide_delay_gate.disables_controls();
        let mut hover_hide_delay_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_DECREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.decrease_hide_on_mouse_hover_delay",
            ),
        )
        .with_description(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hide_on_mouse_hover_delay.description",
        ))
        .with_value(format!("{hover_hide_delay_seconds}s"))
        .disabled(hover_hide_delay_gate.disables_controls() || hover_hide_delay_seconds == 0);
        let mut hover_hide_delay_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_INCREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.increase_hide_on_mouse_hover_delay",
            ),
        )
        .with_description(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hide_on_mouse_hover_delay.description",
        ))
        .with_value(format!("{hover_hide_delay_seconds}s"))
        .disabled(
            hover_hide_delay_gate.disables_controls()
                || hover_hide_delay_seconds
                    >= bongocat_config::MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS,
        );
        if !disabled {
            topmost_node = topmost_node.clickable().focusable();
            click_through_node = click_through_node.clickable().focusable();
            keep_inside_screen_node = keep_inside_screen_node.clickable().focusable();
            hide_on_pointer_hover_node = hide_on_pointer_hover_node.clickable().focusable();
            if hover_hide_delay_available && hover_hide_delay_seconds > 0 {
                hover_hide_delay_decrease_node =
                    hover_hide_delay_decrease_node.clickable().focusable();
            }
            if hover_hide_delay_available
                && hover_hide_delay_seconds
                    < bongocat_config::MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS
            {
                hover_hide_delay_increase_node =
                    hover_hide_delay_increase_node.clickable().focusable();
            }
        }
        let scale = overlay_settings.scale_percent;
        let opacity = overlay_settings.opacity_percent;
        let mut scale_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.decrease_overlay_scale",
            ),
        )
        .with_value(format!("{scale}%"))
        .disabled(disabled || scale <= 25);
        let mut scale_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.increase_overlay_scale",
            ),
        )
        .with_value(format!("{scale}%"))
        .disabled(disabled || scale >= 400);
        let mut opacity_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.decrease_overlay_opacity",
            ),
        )
        .with_value(format!("{opacity}%"))
        .disabled(disabled || opacity <= 1);
        let mut opacity_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.increase_overlay_opacity",
            ),
        )
        .with_value(format!("{opacity}%"))
        .disabled(disabled || opacity >= 100);
        if !disabled {
            if scale > 25 {
                scale_decrease_node = scale_decrease_node.clickable().focusable();
            }
            if scale < 400 {
                scale_increase_node = scale_increase_node.clickable().focusable();
            }
            if opacity > 1 {
                opacity_decrease_node = opacity_decrease_node.clickable().focusable();
            }
            if opacity < 100 {
                opacity_increase_node = opacity_increase_node.clickable().focusable();
            }
        }
        let maximum_fps = snapshot.map_or(60, |snapshot| snapshot.maximum_fps);
        let mut maximum_fps_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_MAXIMUM_FPS_DECREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.decrease_maximum_fps",
            ),
        )
        .with_value(maximum_fps.to_string())
        .disabled(disabled || maximum_fps <= 15);
        let mut maximum_fps_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.increase_maximum_fps",
            ),
        )
        .with_value(maximum_fps.to_string())
        .disabled(disabled || maximum_fps >= 240);
        if !disabled {
            if maximum_fps > 15 {
                maximum_fps_decrease_node = maximum_fps_decrease_node.clickable().focusable();
            }
            if maximum_fps < 240 {
                maximum_fps_increase_node = maximum_fps_increase_node.clickable().focusable();
            }
        }
        let release_fallback_timeout_ms =
            snapshot.map_or(500, |snapshot| snapshot.release_fallback_timeout_ms);
        let mut release_fallback_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_RELEASE_FALLBACK_DECREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.decrease_release_fallback_timeout",
            ),
        )
        .with_description(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.input.release_fallback_timeout.description",
        ))
        .with_value(release_fallback_timeout_ms.to_string())
        .disabled(disabled || release_fallback_timeout_ms == 0);
        let mut release_fallback_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_RELEASE_FALLBACK_INCREASE,
            AccessibilityRole::Button,
            bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.actions.increase_release_fallback_timeout",
            ),
        )
        .with_description(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.input.release_fallback_timeout.description",
        ))
        .with_value(release_fallback_timeout_ms.to_string())
        .disabled(disabled || release_fallback_timeout_ms >= 60_000);
        if !disabled {
            if release_fallback_timeout_ms > 0 {
                release_fallback_decrease_node =
                    release_fallback_decrease_node.clickable().focusable();
            }
            if release_fallback_timeout_ms < 60_000 {
                release_fallback_increase_node =
                    release_fallback_increase_node.clickable().focusable();
            }
        }
        let mut startup_node = AccessibilityNode::new(
            ACCESSIBILITY_STARTUP,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.application.open_at_login.label",
            ),
        )
        .with_toggle(if startup.enabled {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(startup.action == StartupItemAction::None);
        // The switch's own state is what a screen reader needs from the value; the
        // row copy joins it only for the states that still need explaining.
        if let Some(description) = startup.description {
            startup_node = startup_node.with_value(description);
        }
        if startup.action != StartupItemAction::None {
            startup_node = startup_node.clickable().focusable();
        }
        let mut status_icon_node = AccessibilityNode::new(
            ACCESSIBILITY_STATUS_ICON,
            AccessibilityRole::Switch,
            bongocat_i18n::platform_text(
                language.catalog_locale(),
                "settings.application.status_icon.label",
            ),
        )
        .with_toggle(
            if snapshot.is_some_and(|snapshot| snapshot.status_icon_visible) {
                AccessibilityToggle::On
            } else {
                AccessibilityToggle::Off
            },
        )
        .disabled(disabled);
        if !disabled {
            status_icon_node = status_icon_node.clickable().focusable();
        }
        let mut automatic_update_check_node = AccessibilityNode::new(
            ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.application.auto_update.label",
            ),
        )
        .with_value(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.application.auto_update.description",
        ))
        .with_toggle(
            if snapshot.is_some_and(|snapshot| snapshot.check_for_updates_automatically) {
                AccessibilityToggle::On
            } else {
                AccessibilityToggle::Off
            },
        )
        .disabled(disabled);
        if !disabled {
            automatic_update_check_node = automatic_update_check_node.clickable().focusable();
        }
        #[cfg(target_os = "windows")]
        let mut taskbar_icon_node = AccessibilityNode::new(
            ACCESSIBILITY_TASKBAR_ICON,
            AccessibilityRole::Switch,
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.application.taskbar_icon.label",
            ),
        )
        .with_toggle(
            if snapshot.is_some_and(|snapshot| snapshot.taskbar_icon_visible) {
                AccessibilityToggle::On
            } else {
                AccessibilityToggle::Off
            },
        )
        .disabled(disabled);
        #[cfg(target_os = "windows")]
        if !disabled {
            taskbar_icon_node = taskbar_icon_node.clickable().focusable();
        }
        let restore_available = snapshot.is_some_and(|snapshot| {
            matches!(
                snapshot.configuration_status,
                SettingsConfigurationStatus::RecoveryRequired { .. }
            )
        }) && self.pending.is_none();
        let mut restore_node = AccessibilityNode::new(
            ACCESSIBILITY_RESTORE_DEFAULTS,
            AccessibilityRole::Button,
            config_recovery_restore_label(language),
        )
        .with_value(bongocat_i18n::text(
            language.catalog_locale(),
            "diagnostics.configuration.restore_defaults_description",
        ))
        .disabled(!restore_available);
        // The notice's own text, so a screen reader hears what the restore action was about and
        // what it did. Present only while the configuration is unusable: "loaded normally" is
        // not a status worth announcing on every window.
        let recovery_status_node = snapshot
            .filter(|snapshot| snapshot.configuration_status != SettingsConfigurationStatus::Ready)
            .map(|snapshot| {
                let recovery = config_recovery_presentation(
                    snapshot.configuration_status,
                    snapshot.config_recovery,
                    language,
                );
                AccessibilityNode::new(
                    ACCESSIBILITY_CONFIG_RECOVERY,
                    AccessibilityRole::Status,
                    recovery.title,
                )
                .with_value(recovery.detail)
            });
        if restore_available {
            restore_node = restore_node.clickable().focusable();
        }
        let model_configuration_ready = snapshot.is_some_and(|snapshot| {
            snapshot.configuration_status == SettingsConfigurationStatus::Ready
        });
        let [
            choose_folder_node,
            choose_archive_node,
            import_node,
            import_status_node,
        ] = model_import_accessibility_nodes(
            &self.model_import,
            self.pending.is_some(),
            model_configuration_ready,
            language,
        );
        let catalog_status_node = model_catalog_accessibility_status_node(
            snapshot.map(|snapshot| &snapshot.model_catalog),
            language,
        );
        let shortcut_rows = snapshot
            .map(|snapshot| {
                shortcut_accessibility_rows(
                    &snapshot.shortcuts,
                    snapshot.active_model.as_ref(),
                    &snapshot.model_catalog.entries,
                    language,
                )
            })
            .unwrap_or_default();
        let shortcut_node_ids = (0..shortcut_rows.len())
            .map(shortcut_accessibility_node_id)
            .collect::<Vec<_>>();
        let shortcut_nodes = shortcut_rows
            .into_iter()
            .enumerate()
            .map(|(index, (target, label, value))| {
                let capture = self
                    .shortcut_capture
                    .as_ref()
                    .filter(|capture| capture.target == target);
                // The unified gate rule's accessibility arm: a row whose
                // scope's switch is off reports itself disabled even though
                // the global editing state is fine.
                let row_disabled = disabled
                    || snapshot.is_none_or(|snapshot| {
                        !shortcuts_page::ShortcutScope::for_target(&target).is_enabled(snapshot)
                    });
                let mut node = AccessibilityNode::new(
                    shortcut_accessibility_node_id(index),
                    AccessibilityRole::Button,
                    label,
                )
                .with_value(if let Some(capture) = capture {
                    shortcut_capture_preview(&capture.modifiers, &capture.keys)
                        .map(|shortcut| shortcut_display(&shortcut))
                        .unwrap_or_else(|| {
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "shortcuts.capture.waiting",
                            )
                            .to_owned()
                        })
                } else {
                    value
                })
                .disabled(row_disabled);
                if !row_disabled {
                    node = node.clickable().focusable();
                }
                node
            })
            .collect::<Vec<_>>();
        let shortcut_clear_rows = snapshot
            .map(|snapshot| {
                shortcut_clear_accessibility_rows(
                    &snapshot.shortcuts,
                    snapshot.active_model.as_ref(),
                    &snapshot.model_catalog.entries,
                    language,
                )
            })
            .unwrap_or_default();
        let shortcut_clear_node_ids = (0..shortcut_clear_rows.len())
            .map(shortcut_clear_accessibility_node_id)
            .collect::<Vec<_>>();
        let shortcut_clear_nodes = shortcut_clear_rows
            .into_iter()
            .enumerate()
            .map(|(index, (target, label))| {
                let row_disabled = disabled
                    || snapshot.is_none_or(|snapshot| {
                        !shortcuts_page::ShortcutScope::for_target(&target).is_enabled(snapshot)
                    });
                let mut node = AccessibilityNode::new(
                    shortcut_clear_accessibility_node_id(index),
                    AccessibilityRole::Button,
                    label,
                )
                .disabled(row_disabled);
                if !row_disabled {
                    node = node.clickable().focusable();
                }
                node
            })
            .collect::<Vec<_>>();
        // Both gates are the first row of their scope's group, directly above
        // the rows they gate, so the tree lists them in the same order: window
        // gate, window rows, model gate, model rows. The combined row list is
        // split at the window scope's row count — the same offset the page
        // numbers its tab order and accessibility node ids from.
        let window_row_count = snapshot
            .map(|snapshot| window_shortcut_rows(&snapshot.shortcuts).len())
            .unwrap_or_default();
        let command_shortcuts_node = shortcut_gate_accessibility_node(
            shortcuts_page::ShortcutScope::Window,
            ACCESSIBILITY_COMMAND_SHORTCUTS,
            snapshot,
            language,
            disabled,
        );
        let behavior_shortcuts_node = shortcut_gate_accessibility_node(
            shortcuts_page::ShortcutScope::Model,
            ACCESSIBILITY_BEHAVIOR_SHORTCUTS,
            snapshot,
            language,
            disabled,
        );
        let mut window_shortcut_nodes = shortcut_nodes;
        let model_shortcut_nodes =
            window_shortcut_nodes.split_off(window_row_count.min(window_shortcut_nodes.len()));
        let mut window_shortcut_node_ids = shortcut_node_ids;
        let model_shortcut_node_ids = window_shortcut_node_ids
            .split_off(window_row_count.min(window_shortcut_node_ids.len()));
        let mut root_children = vec![
            ACCESSIBILITY_GENERAL,
            ACCESSIBILITY_MODELS,
            ACCESSIBILITY_OVERLAY_PAGE,
            ACCESSIBILITY_INTERACTION,
            ACCESSIBILITY_INPUT,
            ACCESSIBILITY_SHORTCUTS,
            ACCESSIBILITY_APPLICATION,
            ACCESSIBILITY_ABOUT,
            ACCESSIBILITY_THEME,
            ACCESSIBILITY_LANGUAGE,
            ACCESSIBILITY_OVERLAY,
            ACCESSIBILITY_OVERLAY_TOPMOST,
            ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
            ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA,
            ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER,
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_DECREASE,
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_INCREASE,
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
            ACCESSIBILITY_MAXIMUM_FPS_DECREASE,
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE,
            ACCESSIBILITY_RELEASE_FALLBACK_DECREASE,
            ACCESSIBILITY_RELEASE_FALLBACK_INCREASE,
            ACCESSIBILITY_AUDIO,
            ACCESSIBILITY_MIRROR,
            ACCESSIBILITY_MIRROR_POINTER,
            ACCESSIBILITY_IGNORE_POINTER,
            ACCESSIBILITY_STICK_DEAD_ZONE,
            ACCESSIBILITY_TRIGGER_DEAD_ZONE,
            ACCESSIBILITY_STATUS_ICON,
            #[cfg(target_os = "windows")]
            ACCESSIBILITY_TASKBAR_ICON,
            ACCESSIBILITY_STARTUP,
            ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK,
            ACCESSIBILITY_RESTORE_DEFAULTS,
            ACCESSIBILITY_MODEL_CHOOSE_FOLDER,
            ACCESSIBILITY_MODEL_CHOOSE_ARCHIVE,
            ACCESSIBILITY_MODEL_IMPORT,
            ACCESSIBILITY_MODEL_IMPORT_STATUS,
        ];
        if catalog_status_node.is_some() {
            root_children.push(ACCESSIBILITY_MODEL_CATALOG_STATUS);
        }
        if recovery_status_node.is_some() {
            root_children.push(ACCESSIBILITY_CONFIG_RECOVERY);
        }
        root_children.push(ACCESSIBILITY_COMMAND_SHORTCUTS);
        root_children.extend(window_shortcut_node_ids);
        root_children.push(ACCESSIBILITY_BEHAVIOR_SHORTCUTS);
        root_children.extend(model_shortcut_node_ids);
        root_children.extend(shortcut_clear_node_ids);
        let mut nodes = std::iter::once(
            AccessibilityNode::new(
                ACCESSIBILITY_ROOT,
                AccessibilityRole::Window,
                bongocat_i18n::text(language.catalog_locale(), "navigation.settings.title"),
            )
            .with_children(root_children),
        )
        .chain(navigation_accessibility_nodes(language))
        .chain([
            theme_node,
            language_node,
            overlay_node,
            topmost_node,
            click_through_node,
            keep_inside_screen_node,
            hide_on_pointer_hover_node,
            hover_hide_delay_decrease_node,
            hover_hide_delay_increase_node,
            scale_decrease_node,
            scale_increase_node,
            opacity_decrease_node,
            opacity_increase_node,
            maximum_fps_decrease_node,
            maximum_fps_increase_node,
            release_fallback_decrease_node,
            release_fallback_increase_node,
            audio_node,
            mirror_node,
            mirror_pointer_node,
            ignore_pointer_node,
            stick_node,
            trigger_node,
            status_icon_node,
            #[cfg(target_os = "windows")]
            taskbar_icon_node,
            startup_node,
            automatic_update_check_node,
            restore_node,
            choose_folder_node,
            choose_archive_node,
            import_node,
            import_status_node,
        ])
        .collect::<Vec<_>>();
        nodes.push(command_shortcuts_node);
        nodes.extend(window_shortcut_nodes);
        nodes.push(behavior_shortcuts_node);
        nodes.extend(model_shortcut_nodes);
        nodes.extend(shortcut_clear_nodes);
        if let Some(catalog_status_node) = catalog_status_node {
            nodes.push(catalog_status_node);
        }
        if let Some(recovery_status_node) = recovery_status_node {
            nodes.push(recovery_status_node);
        }
        if !nodes.iter().any(|node| node.id == focus) {
            nodes.push(AccessibilityNode::new(focus, AccessibilityRole::Status, ""));
        }
        AccessibilityTree {
            root: ACCESSIBILITY_ROOT,
            focus,
            nodes,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn handle_accessibility_action(
        &mut self,
        request: AccessibilityActionRequest,
        cx: &mut Context<Self>,
    ) {
        let shortcut_target = self.snapshot.as_ref().and_then(|snapshot| {
            shortcut_target_for_accessibility_node(
                &snapshot.shortcuts,
                snapshot.active_model.as_ref(),
                &snapshot.model_catalog.entries,
                request.target,
            )
        });
        let shortcut_clear_target = self.snapshot.as_ref().and_then(|snapshot| {
            shortcut_clear_target_for_accessibility_node(
                &snapshot.shortcuts,
                snapshot.active_model.as_ref(),
                &snapshot.model_catalog.entries,
                request.target,
            )
        });
        self.accessibility_focus = Some(request.target);
        let capture_target_changed = self
            .shortcut_capture
            .as_ref()
            .is_some_and(|capture| shortcut_target.as_ref() != Some(&capture.target));
        if capture_target_changed || self.pending == Some(PendingOperation::BeginShortcutCapture) {
            self.cancel_shortcut_capture(cx);
        }
        if request.action != AccessibilityAction::Click {
            return;
        }
        match request.target {
            ACCESSIBILITY_GENERAL => self.page = SettingsPage::General,
            ACCESSIBILITY_MODELS => self.page = SettingsPage::Models,
            ACCESSIBILITY_OVERLAY_PAGE => self.page = SettingsPage::Overlay,
            ACCESSIBILITY_INTERACTION => self.page = SettingsPage::Interaction,
            ACCESSIBILITY_INPUT => self.page = SettingsPage::Input,
            ACCESSIBILITY_SHORTCUTS => self.page = SettingsPage::Shortcuts,
            ACCESSIBILITY_APPLICATION => self.page = SettingsPage::Application,
            ACCESSIBILITY_ABOUT => self.page = SettingsPage::About,
            ACCESSIBILITY_THEME => {
                if let Some(current) = self
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.appearance_theme)
                {
                    let next = theme_from_index((theme_index(current) + 1) % 3)
                        .expect("theme index is bounded by the theme options");
                    self.set_appearance_theme(next, cx);
                }
            }
            ACCESSIBILITY_LANGUAGE => {
                if let Some(current) = self.snapshot.as_ref().map(|snapshot| snapshot.language) {
                    let index = SettingsLanguage::ALL
                        .iter()
                        .position(|language| *language == current)
                        .unwrap_or_default();
                    let next = SettingsLanguage::ALL[(index + 1) % SettingsLanguage::ALL.len()];
                    self.set_language(next, cx);
                }
            }
            ACCESSIBILITY_STATUS_ICON => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_status_icon_visible(!snapshot.status_icon_visible, cx);
                }
            }
            #[cfg(target_os = "windows")]
            ACCESSIBILITY_TASKBAR_ICON => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_taskbar_icon_visible(!snapshot.taskbar_icon_visible, cx);
                }
            }
            ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_check_for_updates_automatically(
                        !snapshot.check_for_updates_automatically,
                        cx,
                    );
                }
            }
            ACCESSIBILITY_OVERLAY => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_overlay_visible(!snapshot.overlay_visible, cx);
                }
            }
            ACCESSIBILITY_OVERLAY_TOPMOST => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    let mut settings = snapshot.overlay;
                    settings.always_on_top = !settings.always_on_top;
                    self.set_overlay_settings(settings, cx);
                }
            }
            ACCESSIBILITY_OVERLAY_CLICK_THROUGH => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    let mut settings = snapshot.overlay;
                    settings.click_through = !settings.click_through;
                    self.set_overlay_settings(settings, cx);
                }
            }
            ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    let mut settings = snapshot.overlay;
                    settings.keep_inside_screen = !settings.keep_inside_screen;
                    self.set_overlay_settings(settings, cx);
                }
            }
            ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    let mut settings = snapshot.overlay;
                    settings.hide_on_pointer_hover = !settings.hide_on_pointer_hover;
                    self.set_overlay_settings(settings, cx);
                }
            }
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_DECREASE => {
                self.adjust_overlay_hover_hide_delay(-1, cx)
            }
            ACCESSIBILITY_OVERLAY_HOVER_DELAY_INCREASE => {
                self.adjust_overlay_hover_hide_delay(1, cx)
            }
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE => self.adjust_overlay_scale(-25, cx),
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE => self.adjust_overlay_scale(25, cx),
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE => self.adjust_overlay_opacity(-10, cx),
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE => self.adjust_overlay_opacity(10, cx),
            ACCESSIBILITY_MAXIMUM_FPS_DECREASE => self.adjust_maximum_fps(-15, cx),
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE => self.adjust_maximum_fps(15, cx),
            ACCESSIBILITY_RELEASE_FALLBACK_DECREASE => {
                self.adjust_release_fallback_timeout(-250, cx)
            }
            ACCESSIBILITY_RELEASE_FALLBACK_INCREASE => {
                self.adjust_release_fallback_timeout(250, cx)
            }
            ACCESSIBILITY_AUDIO => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_motion_audio_enabled(!snapshot.motion_audio_enabled, cx);
                }
            }
            ACCESSIBILITY_COMMAND_SHORTCUTS => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_command_shortcuts_enabled(!snapshot.command_shortcuts_enabled, cx);
                }
            }
            ACCESSIBILITY_BEHAVIOR_SHORTCUTS => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_behavior_shortcuts_enabled(!snapshot.behavior_shortcuts_enabled, cx);
                }
            }
            ACCESSIBILITY_MIRROR | ACCESSIBILITY_MIRROR_POINTER | ACCESSIBILITY_IGNORE_POINTER => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    let mut settings = snapshot.model_settings;
                    match request.target {
                        ACCESSIBILITY_MIRROR => settings.mirror = !settings.mirror,
                        ACCESSIBILITY_MIRROR_POINTER => {
                            settings.mirror_pointer_tracking = !settings.mirror_pointer_tracking
                        }
                        ACCESSIBILITY_IGNORE_POINTER => {
                            settings.ignore_pointer = !settings.ignore_pointer
                        }
                        _ => unreachable!(),
                    }
                    self.set_model_settings(settings, cx);
                }
            }
            ACCESSIBILITY_STICK_DEAD_ZONE => self.adjust_gamepad_dead_zone(true, 5, cx),
            ACCESSIBILITY_TRIGGER_DEAD_ZONE => self.adjust_gamepad_dead_zone(false, 5, cx),
            ACCESSIBILITY_STARTUP => match startup_item_presentation(
                self.snapshot.as_ref().map(|s| s.startup_item),
                self.pending.is_some() || self.snapshot.is_none(),
                self.snapshot
                    .as_ref()
                    .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                        snapshot.resolved_language
                    }),
            )
            .action
            {
                StartupItemAction::SetEnabled(enabled) => {
                    self.set_startup_item_enabled(enabled, cx)
                }
                StartupItemAction::Retry => self.refresh(cx),
                StartupItemAction::None => {}
            },
            ACCESSIBILITY_RESTORE_DEFAULTS => {
                if self.snapshot.as_ref().is_some_and(|snapshot| {
                    matches!(
                        snapshot.configuration_status,
                        SettingsConfigurationStatus::RecoveryRequired { .. }
                    )
                }) {
                    self.restore_default_configuration(cx);
                }
            }
            ACCESSIBILITY_MODEL_CHOOSE_FOLDER => {
                if self.pending.is_none()
                    && !self.model_import.is_running()
                    && !self.model_import.is_picker_open()
                {
                    self.choose_model_source(ModelSourceKind::Directory, cx);
                }
            }
            ACCESSIBILITY_MODEL_CHOOSE_ARCHIVE => {
                if self.pending.is_none()
                    && !self.model_import.is_running()
                    && !self.model_import.is_picker_open()
                {
                    self.choose_model_source(ModelSourceKind::Archive, cx);
                }
            }
            ACCESSIBILITY_MODEL_IMPORT => {
                if self.model_import.is_running() {
                    self.cancel_model_import(cx);
                } else {
                    self.start_model_import(cx);
                }
            }
            _ => {
                if let Some(target) = shortcut_clear_target {
                    self.clear_shortcut(target, cx);
                } else if let Some(target) = shortcut_target
                    && self.pending.is_none()
                    && self.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.configuration_status == SettingsConfigurationStatus::Ready
                    })
                {
                    self.begin_shortcut_capture_from_accessibility(target, cx);
                }
            }
        }
        cx.notify();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn start_accessibility_actions(
        &mut self,
        receiver: async_channel::Receiver<AccessibilityActionRequest>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Ok(request) = receiver.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.handle_accessibility_action(request, cx);
                });
            }
        })
        .detach();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn update_accessibility(&mut self, window: &Window, cx: &App) {
        let language_is_focused = self
            .language_select
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let focus = [
            (ACCESSIBILITY_GENERAL, &self.general_focus),
            (ACCESSIBILITY_MODELS, &self.models_focus),
            (ACCESSIBILITY_SHORTCUTS, &self.shortcuts_focus),
            (ACCESSIBILITY_ABOUT, &self.about_focus),
            (ACCESSIBILITY_OVERLAY, &self.overlay_focus),
            (ACCESSIBILITY_OVERLAY_TOPMOST, &self.overlay_topmost_focus),
            (
                ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
                &self.overlay_click_through_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA,
                &self.overlay_keep_inside_screen_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER,
                &self.overlay_hide_on_pointer_hover_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_HOVER_DELAY_DECREASE,
                &self.overlay_hover_hide_delay_decrease_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_HOVER_DELAY_INCREASE,
                &self.overlay_hover_hide_delay_increase_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
                &self.overlay_scale_decrease_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
                &self.overlay_scale_increase_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
                &self.overlay_opacity_decrease_focus,
            ),
            (
                ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
                &self.overlay_opacity_increase_focus,
            ),
            (
                ACCESSIBILITY_MAXIMUM_FPS_DECREASE,
                &self.maximum_fps_decrease_focus,
            ),
            (
                ACCESSIBILITY_MAXIMUM_FPS_INCREASE,
                &self.maximum_fps_increase_focus,
            ),
            (
                ACCESSIBILITY_RELEASE_FALLBACK_DECREASE,
                &self.release_fallback_decrease_focus,
            ),
            (
                ACCESSIBILITY_RELEASE_FALLBACK_INCREASE,
                &self.release_fallback_increase_focus,
            ),
            (ACCESSIBILITY_AUDIO, &self.audio_focus),
            (
                ACCESSIBILITY_COMMAND_SHORTCUTS,
                &self.command_shortcuts_focus,
            ),
            (
                ACCESSIBILITY_BEHAVIOR_SHORTCUTS,
                &self.behavior_shortcuts_focus,
            ),
            (ACCESSIBILITY_STATUS_ICON, &self.status_icon_focus),
            #[cfg(target_os = "windows")]
            (ACCESSIBILITY_TASKBAR_ICON, &self.taskbar_icon_focus),
            (
                ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK,
                &self.automatic_update_check_focus,
            ),
            (ACCESSIBILITY_MIRROR, &self.mirror_focus),
            (ACCESSIBILITY_MIRROR_POINTER, &self.mirror_pointer_focus),
            (ACCESSIBILITY_IGNORE_POINTER, &self.ignore_pointer_focus),
            (ACCESSIBILITY_STICK_DEAD_ZONE, &self.stick_dead_zone_focus),
            (
                ACCESSIBILITY_TRIGGER_DEAD_ZONE,
                &self.trigger_dead_zone_focus,
            ),
            (ACCESSIBILITY_STARTUP, &self.startup_item_focus),
            (ACCESSIBILITY_RESTORE_DEFAULTS, &self.restore_defaults_focus),
            (ACCESSIBILITY_MODEL_CHOOSE_FOLDER, &self.choose_model_focus),
            (
                ACCESSIBILITY_MODEL_CHOOSE_ARCHIVE,
                &self.choose_archive_focus,
            ),
            (ACCESSIBILITY_MODEL_IMPORT, &self.import_model_focus),
        ]
        .into_iter()
        .find_map(|(id, handle)| handle.is_focused(window).then_some(id))
        .or_else(|| language_is_focused.then_some(ACCESSIBILITY_LANGUAGE))
        .or_else(|| {
            self.snapshot.as_ref().and_then(|snapshot| {
                shortcut_targets(
                    &snapshot.shortcuts,
                    snapshot.active_model.as_ref(),
                    &snapshot.model_catalog.entries,
                )
                .into_iter()
                .enumerate()
                .find_map(|(index, target)| {
                    self.shortcut_row_focus
                        .get(&target)
                        .is_some_and(|focus| focus.is_focused(window))
                        .then_some(shortcut_accessibility_node_id(index))
                })
            })
        })
        .or_else(|| {
            self.snapshot.as_ref().and_then(|snapshot| {
                shortcut_clear_accessibility_rows(
                    &snapshot.shortcuts,
                    snapshot.active_model.as_ref(),
                    &snapshot.model_catalog.entries,
                    snapshot.resolved_language,
                )
                .into_iter()
                .enumerate()
                .find_map(|(index, (target, _))| {
                    self.shortcut_clear_focus
                        .get(&target)
                        .is_some_and(|focus| focus.is_focused(window))
                        .then_some(shortcut_clear_accessibility_node_id(index))
                })
            })
        })
        .unwrap_or(match self.page {
            SettingsPage::General => ACCESSIBILITY_GENERAL,
            SettingsPage::Models => ACCESSIBILITY_MODELS,
            SettingsPage::Overlay => ACCESSIBILITY_OVERLAY_PAGE,
            SettingsPage::Interaction => ACCESSIBILITY_INTERACTION,
            SettingsPage::Input => ACCESSIBILITY_INPUT,
            SettingsPage::Shortcuts => ACCESSIBILITY_SHORTCUTS,
            SettingsPage::Application => ACCESSIBILITY_APPLICATION,
            SettingsPage::About => ACCESSIBILITY_ABOUT,
        });
        let tree = self.accessibility_tree_with_focus(focus);
        if let Some(bridge) = self.accessibility.as_mut() {
            let _ = bridge.update(tree);
        }
        let _ = window;
    }
}
