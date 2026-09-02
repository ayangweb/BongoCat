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
                );
            }
            self.update_accessibility(window);
        }
        let tokens = Tokens::from_theme(cx);
        let snapshot = self.snapshot.clone();
        if let Some(snapshot) = snapshot.as_ref() {
            self.sync_component_inputs(snapshot, window, cx);
        }
        let overlay_visible = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.overlay_visible);
        let overlay_settings = snapshot
            .as_ref()
            .map(|snapshot| snapshot.overlay)
            .unwrap_or_default();
        let motion_audio_enabled = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.motion_audio_enabled);
        let model_settings = snapshot
            .as_ref()
            .map(|snapshot| snapshot.model_settings)
            .unwrap_or_default();
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
            (_, Some(PendingOperation::Refresh), _) => "Refreshing...".into(),
            (
                _,
                Some(
                    PendingOperation::OverlayVisibility
                    | PendingOperation::OverlaySettings
                    | PendingOperation::MotionAudio
                    | PendingOperation::ModelSettings,
                ),
                _,
            ) => "Saving...".into(),
            (_, Some(PendingOperation::StartupItem), _) => "Updating login startup...".into(),
            (_, Some(PendingOperation::ModelSelection), _) => "Activating model...".into(),
            (_, Some(PendingOperation::ModelDeletion), _) => "Deleting model...".into(),
            (_, Some(PendingOperation::OpenConfigBackupLocation), _) => {
                "Opening configuration backups...".into()
            }
            (_, Some(PendingOperation::RestoreDefaultConfiguration), _) => {
                "Restoring default configuration...".into()
            }
            (_, Some(PendingOperation::RestoreDefaultShortcuts), _) => {
                "Restoring default shortcuts...".into()
            }
            (_, Some(PendingOperation::ClearShortcuts), _) => "Clearing shortcuts...".into(),
            (_, Some(PendingOperation::SetShortcuts), _) => "Saving shortcut...".into(),
            (_, Some(PendingOperation::ExportDiagnostics), _) => "Exporting diagnostics...".into(),
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
        let active_model: SharedString = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.active_model.as_ref())
            .map(|model| model.id.clone())
            .unwrap_or_else(|| "No active model".to_owned())
            .into();

        let general_selected = self.page == SettingsPage::General;
        let models_selected = self.page == SettingsPage::Models;
        let diagnostics_selected = self.page == SettingsPage::Diagnostics;
        let sidebar = div()
            .w(px(164.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(tokens.sidebar)
            .child(div().text_xl().text_color(tokens.text).child("BongoCat"))
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.muted)
                    .child("Native settings"),
            )
            .child(div().h(px(12.0)))
            .child(
                navigation_item(
                    "General",
                    general_selected,
                    &self.general_focus,
                    1,
                    window,
                    tokens,
                )
                .id("general-page")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.general_focus);
                    view.page = SettingsPage::General;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&view.general_focus);
                        view.page = SettingsPage::General;
                        cx.notify();
                    }
                })),
            )
            .child(
                navigation_item(
                    "Models",
                    models_selected,
                    &self.models_focus,
                    2,
                    window,
                    tokens,
                )
                .id("models-page")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.models_focus);
                    view.page = SettingsPage::Models;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&view.models_focus);
                        view.page = SettingsPage::Models;
                        cx.notify();
                    }
                })),
            )
            .child(div().p_3().text_color(tokens.muted).child("Shortcuts"))
            .child(
                navigation_item(
                    "Diagnostics",
                    diagnostics_selected,
                    &self.diagnostics_focus,
                    3,
                    window,
                    tokens,
                )
                .id("diagnostics-page")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.diagnostics_focus);
                    view.page = SettingsPage::Diagnostics;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&view.diagnostics_focus);
                        view.page = SettingsPage::Diagnostics;
                        cx.notify();
                    }
                })),
            );

        let general_content = general::content(
            self,
            window,
            cx,
            snapshot.as_ref(),
            disabled,
            overlay_visible,
            overlay_settings,
            motion_audio_enabled,
            model_settings,
            active_model,
            status,
            tokens,
        );
        let models_content = models::content(self, window, cx, snapshot.as_ref(), tokens);
        let diagnostics_content =
            diagnostics::content(self, window, cx, snapshot.as_ref(), disabled, tokens);
        let content = match self.page {
            SettingsPage::General => general_content,
            SettingsPage::Models => models_content,
            SettingsPage::Diagnostics => diagnostics_content,
        }
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    command_button(
                        "Refresh",
                        &self.refresh_focus,
                        30,
                        window,
                        tokens,
                        self.pending.is_some()
                            || self.model_import.is_running()
                            || self.model_import.is_picker_open(),
                    )
                    .id("refresh-settings")
                    .on_click(cx.listener(|view, _, window, cx| {
                        if view.pending.is_none()
                            && !view.model_import.is_running()
                            && !view.model_import.is_picker_open()
                        {
                            window.focus(&view.refresh_focus);
                            view.refresh(cx);
                        }
                    }))
                    .on_key_down(cx.listener(|view, event, window, cx| {
                        if view.pending.is_none()
                            && !view.model_import.is_running()
                            && !view.model_import.is_picker_open()
                            && is_activation_key(event)
                        {
                            cx.stop_propagation();
                            window.focus(&view.refresh_focus);
                            view.refresh(cx);
                        }
                    })),
                )
                .child(
                    command_button("Quit", &self.quit_focus, 31, window, tokens, false)
                        .id("quit-application")
                        .on_click(cx.listener(|view, _, window, cx| {
                            window.focus(&view.quit_focus);
                            (view.request_quit)(cx);
                        }))
                        .on_key_down(cx.listener(|view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                window.focus(&view.quit_focus);
                                (view.request_quit)(cx);
                            }
                        })),
                ),
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
            .child(sidebar)
            .child(content)
    }
}
