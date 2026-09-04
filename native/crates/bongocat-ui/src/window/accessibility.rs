use super::*;

impl SettingsView {
    pub(super) fn accessibility_tree(&self) -> AccessibilityTree {
        let focus = match self.page {
            SettingsPage::General => ACCESSIBILITY_GENERAL,
            SettingsPage::Models => ACCESSIBILITY_MODELS,
            SettingsPage::Diagnostics => ACCESSIBILITY_DIAGNOSTICS,
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
        let startup = startup_item_presentation(snapshot.map(|s| s.startup_item), disabled);
        let selected_theme =
            snapshot.map_or(SettingsTheme::System, |snapshot| snapshot.appearance_theme);
        let theme_nodes = [
            (ACCESSIBILITY_THEME_SYSTEM, SettingsTheme::System, "System"),
            (ACCESSIBILITY_THEME_LIGHT, SettingsTheme::Light, "Light"),
            (ACCESSIBILITY_THEME_DARK, SettingsTheme::Dark, "Dark"),
        ]
        .map(|(id, theme, label)| {
            let mut node = AccessibilityNode::new(id, AccessibilityRole::RadioButton, label)
                .with_description("BongoCat settings appearance")
                .with_toggle(if selected_theme == theme {
                    AccessibilityToggle::On
                } else {
                    AccessibilityToggle::Off
                })
                .disabled(disabled);
            if !disabled {
                node = node.clickable().focusable();
            }
            node
        });
        let mut overlay_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY,
            AccessibilityRole::Switch,
            "Show desktop cat",
        )
        .with_value("Keep the Live2D overlay visible")
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
            "Motion audio",
        )
        .with_value("Play audio attached to model motions")
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
            "Mirror model",
        )
        .with_value("Render the model mirrored horizontally")
        .with_toggle(if model_settings.mirror {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut mirror_pointer_node = AccessibilityNode::new(
            ACCESSIBILITY_MIRROR_POINTER,
            AccessibilityRole::Switch,
            "Mirror pointer tracking",
        )
        .with_value("Mirror horizontal pointer tracking with the model")
        .with_toggle(if model_settings.mirror_pointer_tracking {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut ignore_pointer_node = AccessibilityNode::new(
            ACCESSIBILITY_IGNORE_POINTER,
            AccessibilityRole::Switch,
            "Ignore pointer input",
        )
        .with_value("Do not apply pointer movement to the model")
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
            "Gamepad stick dead zone",
        )
        .with_value(format!("{}%", axis_settings.stick_dead_zone_percent))
        .disabled(disabled);
        let mut trigger_node = AccessibilityNode::new(
            ACCESSIBILITY_TRIGGER_DEAD_ZONE,
            AccessibilityRole::Button,
            "Gamepad trigger dead zone",
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
            "Always on top",
        )
        .with_value("Keep the Live2D overlay above other windows")
        .with_toggle(if overlay_settings.always_on_top {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        let mut click_through_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
            AccessibilityRole::Switch,
            "Click-through overlay",
        )
        .with_value("Let pointer input pass through the Live2D overlay")
        .with_toggle(if overlay_settings.click_through {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(disabled);
        if !disabled {
            topmost_node = topmost_node.clickable().focusable();
            click_through_node = click_through_node.clickable().focusable();
        }
        let scale = overlay_settings.scale_percent;
        let opacity = overlay_settings.opacity_percent;
        let mut scale_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
            AccessibilityRole::Button,
            "Decrease overlay scale",
        )
        .with_value(format!("{scale}%"))
        .disabled(disabled || scale <= 25);
        let mut scale_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
            AccessibilityRole::Button,
            "Increase overlay scale",
        )
        .with_value(format!("{scale}%"))
        .disabled(disabled || scale >= 400);
        let mut opacity_decrease_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
            AccessibilityRole::Button,
            "Decrease overlay opacity",
        )
        .with_value(format!("{opacity}%"))
        .disabled(disabled || opacity <= 1);
        let mut opacity_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
            AccessibilityRole::Button,
            "Increase overlay opacity",
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
            "Decrease maximum FPS",
        )
        .with_value(maximum_fps.to_string())
        .disabled(disabled || maximum_fps <= 15);
        let mut maximum_fps_increase_node = AccessibilityNode::new(
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE,
            AccessibilityRole::Button,
            "Increase maximum FPS",
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
        let mut startup_node = AccessibilityNode::new(
            ACCESSIBILITY_STARTUP,
            AccessibilityRole::Switch,
            "Open at login",
        )
        .with_value(startup.description)
        .with_toggle(if startup.enabled {
            AccessibilityToggle::On
        } else {
            AccessibilityToggle::Off
        })
        .disabled(startup.action == StartupItemAction::None);
        if startup.action != StartupItemAction::None {
            startup_node = startup_node.clickable().focusable();
        }
        let mut status_icon_node = AccessibilityNode::new(
            ACCESSIBILITY_STATUS_ICON,
            AccessibilityRole::Switch,
            "Show status icon",
        )
        .with_value("Show BongoCat in the system tray or menu bar")
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
        let mut refresh_node =
            AccessibilityNode::new(ACCESSIBILITY_REFRESH, AccessibilityRole::Button, "Refresh")
                .disabled(self.refresh_is_disabled());
        if !self.refresh_is_disabled() {
            refresh_node = refresh_node.clickable().focusable();
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
            "Restore default configuration",
        )
        .with_value("Archive the invalid configuration and create verified defaults")
        .disabled(!restore_available);
        if restore_available {
            restore_node = restore_node.clickable().focusable();
        }
        let open_backups_available = snapshot.is_some() && self.pending.is_none();
        let mut open_backups_node = AccessibilityNode::new(
            ACCESSIBILITY_OPEN_BACKUPS,
            AccessibilityRole::Button,
            "Open configuration backups folder",
        )
        .with_value("Open the current environment's backup folder")
        .disabled(!open_backups_available);
        if open_backups_available {
            open_backups_node = open_backups_node.clickable().focusable();
        }
        let export_available = snapshot.is_some() && self.pending.is_none();
        let mut diagnostics_export_node = AccessibilityNode::new(
            ACCESSIBILITY_EXPORT_DIAGNOSTICS,
            AccessibilityRole::Button,
            "Export diagnostics",
        )
        .with_value("Write an anonymous diagnostics report to the current environment logs")
        .disabled(!export_available);
        if export_available {
            diagnostics_export_node = diagnostics_export_node.clickable().focusable();
        }
        let mut restore_shortcuts_node = AccessibilityNode::new(
            ACCESSIBILITY_RESTORE_SHORTCUTS,
            AccessibilityRole::Button,
            "Restore default shortcuts",
        )
        .with_value("Replace custom shortcut bindings with the verified defaults")
        .disabled(disabled);
        if !disabled {
            restore_shortcuts_node = restore_shortcuts_node.clickable().focusable();
        }
        let mut clear_shortcuts_node = AccessibilityNode::new(
            ACCESSIBILITY_CLEAR_SHORTCUTS,
            AccessibilityRole::Button,
            "Clear all shortcuts",
        )
        .with_value("Remove all custom shortcut bindings")
        .disabled(
            disabled
                || snapshot.is_none_or(|snapshot| {
                    snapshot.shortcuts.commands.is_empty()
                        && snapshot.shortcuts.model_behaviors.is_empty()
                }),
        );
        if !disabled
            && snapshot.is_some_and(|snapshot| {
                !snapshot.shortcuts.commands.is_empty()
                    || !snapshot.shortcuts.model_behaviors.is_empty()
            })
        {
            clear_shortcuts_node = clear_shortcuts_node.clickable().focusable();
        }
        let shortcut_rows = snapshot
            .map(|snapshot| shortcut_accessibility_rows(&snapshot.shortcuts))
            .unwrap_or_default();
        let shortcut_node_ids = (0..shortcut_rows.len())
            .map(shortcut_accessibility_node_id)
            .collect::<Vec<_>>();
        let shortcut_nodes = shortcut_rows
            .into_iter()
            .enumerate()
            .map(|(index, (target, label, value))| {
                let capturing = self.shortcut_capture.as_ref() == Some(&target);
                let mut node = AccessibilityNode::new(
                    shortcut_accessibility_node_id(index),
                    AccessibilityRole::Button,
                    label,
                )
                .with_value(if capturing {
                    "Waiting for a key combination".to_owned()
                } else {
                    value
                })
                .disabled(disabled);
                if !disabled {
                    node = node.clickable().focusable();
                }
                node
            })
            .collect::<Vec<_>>();
        let mut root_children = vec![
            ACCESSIBILITY_GENERAL,
            ACCESSIBILITY_MODELS,
            ACCESSIBILITY_DIAGNOSTICS,
            ACCESSIBILITY_THEME_SYSTEM,
            ACCESSIBILITY_THEME_LIGHT,
            ACCESSIBILITY_THEME_DARK,
            ACCESSIBILITY_OVERLAY,
            ACCESSIBILITY_OVERLAY_TOPMOST,
            ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
            ACCESSIBILITY_MAXIMUM_FPS_DECREASE,
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE,
            ACCESSIBILITY_AUDIO,
            ACCESSIBILITY_MIRROR,
            ACCESSIBILITY_MIRROR_POINTER,
            ACCESSIBILITY_IGNORE_POINTER,
            ACCESSIBILITY_STICK_DEAD_ZONE,
            ACCESSIBILITY_TRIGGER_DEAD_ZONE,
            ACCESSIBILITY_STATUS_ICON,
            ACCESSIBILITY_STARTUP,
            ACCESSIBILITY_OPEN_BACKUPS,
            ACCESSIBILITY_RESTORE_DEFAULTS,
            ACCESSIBILITY_EXPORT_DIAGNOSTICS,
            ACCESSIBILITY_RESTORE_SHORTCUTS,
            ACCESSIBILITY_CLEAR_SHORTCUTS,
        ];
        root_children.extend(shortcut_node_ids);
        root_children.extend([ACCESSIBILITY_REFRESH, ACCESSIBILITY_QUIT]);
        let mut nodes = vec![
            AccessibilityNode::new(
                ACCESSIBILITY_ROOT,
                AccessibilityRole::Window,
                "BongoCat Settings",
            )
            .with_children(root_children),
            AccessibilityNode::new(ACCESSIBILITY_GENERAL, AccessibilityRole::Button, "General")
                .clickable()
                .focusable(),
            AccessibilityNode::new(ACCESSIBILITY_MODELS, AccessibilityRole::Button, "Models")
                .clickable()
                .focusable(),
            AccessibilityNode::new(
                ACCESSIBILITY_DIAGNOSTICS,
                AccessibilityRole::Button,
                "Diagnostics",
            )
            .clickable()
            .focusable(),
            theme_nodes[0].clone(),
            theme_nodes[1].clone(),
            theme_nodes[2].clone(),
            overlay_node,
            topmost_node,
            click_through_node,
            scale_decrease_node,
            scale_increase_node,
            opacity_decrease_node,
            opacity_increase_node,
            maximum_fps_decrease_node,
            maximum_fps_increase_node,
            audio_node,
            mirror_node,
            mirror_pointer_node,
            ignore_pointer_node,
            stick_node,
            trigger_node,
            status_icon_node,
            startup_node,
            open_backups_node,
            restore_node,
            diagnostics_export_node,
            restore_shortcuts_node,
            clear_shortcuts_node,
            refresh_node,
            AccessibilityNode::new(ACCESSIBILITY_QUIT, AccessibilityRole::Button, "Quit")
                .clickable()
                .focusable(),
        ];
        nodes.extend(shortcut_nodes);
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
            shortcut_target_for_accessibility_node(&snapshot.shortcuts, request.target)
        });
        self.accessibility_focus = Some(request.target);
        if request.action != AccessibilityAction::Click {
            return;
        }
        match request.target {
            ACCESSIBILITY_GENERAL => self.page = SettingsPage::General,
            ACCESSIBILITY_MODELS => self.page = SettingsPage::Models,
            ACCESSIBILITY_DIAGNOSTICS => self.page = SettingsPage::Diagnostics,
            ACCESSIBILITY_THEME_SYSTEM => self.set_appearance_theme(SettingsTheme::System, cx),
            ACCESSIBILITY_THEME_LIGHT => self.set_appearance_theme(SettingsTheme::Light, cx),
            ACCESSIBILITY_THEME_DARK => self.set_appearance_theme(SettingsTheme::Dark, cx),
            ACCESSIBILITY_STATUS_ICON => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_status_icon_visible(!snapshot.status_icon_visible, cx);
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
            ACCESSIBILITY_OVERLAY_SCALE_DECREASE => self.adjust_overlay_scale(-25, cx),
            ACCESSIBILITY_OVERLAY_SCALE_INCREASE => self.adjust_overlay_scale(25, cx),
            ACCESSIBILITY_OVERLAY_OPACITY_DECREASE => self.adjust_overlay_opacity(-10, cx),
            ACCESSIBILITY_OVERLAY_OPACITY_INCREASE => self.adjust_overlay_opacity(10, cx),
            ACCESSIBILITY_MAXIMUM_FPS_DECREASE => self.adjust_maximum_fps(-15, cx),
            ACCESSIBILITY_MAXIMUM_FPS_INCREASE => self.adjust_maximum_fps(15, cx),
            ACCESSIBILITY_AUDIO => {
                if let Some(snapshot) = self.snapshot.as_ref() {
                    self.set_motion_audio_enabled(!snapshot.motion_audio_enabled, cx);
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
            )
            .action
            {
                StartupItemAction::SetEnabled(enabled) => {
                    self.set_startup_item_enabled(enabled, cx)
                }
                StartupItemAction::Retry => self.refresh(cx),
                StartupItemAction::None => {}
            },
            ACCESSIBILITY_OPEN_BACKUPS => self.open_config_backup_location(cx),
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
            ACCESSIBILITY_EXPORT_DIAGNOSTICS => self.export_diagnostics(cx),
            ACCESSIBILITY_RESTORE_SHORTCUTS => self.restore_default_shortcuts(cx),
            ACCESSIBILITY_CLEAR_SHORTCUTS => self.clear_shortcuts(cx),
            ACCESSIBILITY_REFRESH => self.refresh(cx),
            ACCESSIBILITY_QUIT => (self.request_quit)(cx),
            _ => {
                if let Some(target) = shortcut_target
                    && self.pending.is_none()
                    && self.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.configuration_status == SettingsConfigurationStatus::Ready
                    })
                {
                    self.shortcut_capture = Some(target);
                    self.shortcut_capture_error = None;
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
    pub(super) fn update_accessibility(&mut self, window: &Window) {
        let focus = [
            (ACCESSIBILITY_GENERAL, &self.general_focus),
            (ACCESSIBILITY_MODELS, &self.models_focus),
            (ACCESSIBILITY_DIAGNOSTICS, &self.diagnostics_focus),
            (ACCESSIBILITY_OVERLAY, &self.overlay_focus),
            (ACCESSIBILITY_OVERLAY_TOPMOST, &self.overlay_topmost_focus),
            (
                ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
                &self.overlay_click_through_focus,
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
            (ACCESSIBILITY_AUDIO, &self.audio_focus),
            (ACCESSIBILITY_STATUS_ICON, &self.status_icon_focus),
            (ACCESSIBILITY_MIRROR, &self.mirror_focus),
            (ACCESSIBILITY_MIRROR_POINTER, &self.mirror_pointer_focus),
            (ACCESSIBILITY_IGNORE_POINTER, &self.ignore_pointer_focus),
            (ACCESSIBILITY_STICK_DEAD_ZONE, &self.stick_dead_zone_focus),
            (
                ACCESSIBILITY_TRIGGER_DEAD_ZONE,
                &self.trigger_dead_zone_focus,
            ),
            (ACCESSIBILITY_STARTUP, &self.startup_item_focus),
            (ACCESSIBILITY_OPEN_BACKUPS, &self.open_backups_focus),
            (ACCESSIBILITY_RESTORE_DEFAULTS, &self.restore_defaults_focus),
            (
                ACCESSIBILITY_RESTORE_SHORTCUTS,
                &self.restore_shortcuts_focus,
            ),
            (ACCESSIBILITY_CLEAR_SHORTCUTS, &self.clear_shortcuts_focus),
            (
                ACCESSIBILITY_EXPORT_DIAGNOSTICS,
                &self.export_diagnostics_focus,
            ),
            (ACCESSIBILITY_REFRESH, &self.refresh_focus),
            (ACCESSIBILITY_QUIT, &self.quit_focus),
        ]
        .into_iter()
        .find_map(|(id, handle)| handle.is_focused(window).then_some(id))
        .or_else(|| {
            self.snapshot.as_ref().and_then(|snapshot| {
                shortcut_targets(&snapshot.shortcuts)
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
        .unwrap_or(match self.page {
            SettingsPage::General => ACCESSIBILITY_GENERAL,
            SettingsPage::Models => ACCESSIBILITY_MODELS,
            SettingsPage::Diagnostics => ACCESSIBILITY_DIAGNOSTICS,
        });
        let tree = self.accessibility_tree_with_focus(focus);
        if let Some(bridge) = self.accessibility.as_mut() {
            let _ = bridge.update(tree);
        }
        let _ = window;
    }
}
