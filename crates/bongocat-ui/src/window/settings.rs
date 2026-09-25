use super::*;

pub(super) fn random_behavior_settings_after_toggle(
    persisted: SettingsRandomBehavior,
    pending: Option<SettingsRandomBehavior>,
    enabled: bool,
) -> SettingsRandomBehavior {
    let mut settings = pending.unwrap_or(persisted);
    settings.enabled = enabled;
    settings
}

impl SettingsView {
    /// Keep the rendered snapshot current while the window is on screen.
    ///
    /// The window is created for each open and destroyed on close. The poll still
    /// stops while it is being prepared or closed: nothing it renders can be seen,
    /// the runtime keeps running, and a newly created window refreshes before it is
    /// shown. Without this a window waiting for its first snapshot could ask the
    /// service for a full snapshot — model catalog scan included — while invisible.
    pub(super) fn start_snapshot_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |view, cx| {
                        if !view.window_hidden() {
                            view.refresh(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_is_disabled() {
            return;
        }
        self.start_request(PendingOperation::Refresh, None, cx);
    }

    pub(super) fn refresh_is_disabled(&self) -> bool {
        matches!(self.pending, Some(PendingOperation::Refresh))
            || self.model_import.is_running()
            || self.model_import.is_picker_open()
    }

    pub(super) fn set_language(&mut self, language: SettingsLanguage, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.language == language)
        {
            return;
        }
        self.start_request(
            PendingOperation::Language,
            Some(SettingValue::Language {
                expected_config_revision,
                language,
            }),
            cx,
        );
    }

    pub(super) fn set_appearance_theme(&mut self, theme: SettingsTheme, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.appearance_theme)
            == Some(theme)
        {
            return;
        }
        apply_optimistic_component_theme(theme, cx);
        self.start_request(
            PendingOperation::AppearanceTheme,
            Some(SettingValue::AppearanceTheme {
                expected_config_revision,
                theme,
            }),
            cx,
        );
    }

    pub(super) fn set_overlay_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::OverlayVisibility,
            Some(SettingValue::OverlayVisible {
                expected_config_revision,
                visible,
            }),
            cx,
        );
    }

    pub(super) fn set_status_icon_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status_icon_visible == visible)
        {
            return;
        }
        self.start_request(
            PendingOperation::StatusIconVisibility,
            Some(SettingValue::StatusIconVisible {
                expected_config_revision,
                visible,
            }),
            cx,
        );
    }

    #[cfg(target_os = "windows")]
    pub(super) fn set_taskbar_icon_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.taskbar_icon_visible == visible)
        {
            return;
        }
        self.start_request(
            PendingOperation::TaskbarIconVisibility,
            Some(SettingValue::TaskbarIconVisible {
                expected_config_revision,
                visible,
            }),
            cx,
        );
    }

    pub(super) fn set_check_for_updates_automatically(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.check_for_updates_automatically == enabled)
        {
            return;
        }
        if !enabled {
            // A value still waiting in the debounce window belongs to the old
            // enabled schedule. Do not let it race the switch-off command and
            // resurrect an edit after automatic checks have been disabled.
            self.check_for_updates_interval_debouncer.discard_pending();
            self.check_for_updates_interval_timer_generation = self
                .check_for_updates_interval_timer_generation
                .saturating_add(1);
        }
        self.start_request(
            PendingOperation::AutomaticUpdateCheck,
            Some(SettingValue::CheckForUpdatesAutomatically {
                expected_config_revision,
                enabled,
            }),
            cx,
        );
    }

    pub(super) fn set_check_for_updates_interval_hours(
        &mut self,
        raw: f64,
        cx: &mut Context<Self>,
    ) {
        if self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let value = normalize_check_for_updates_interval_hours(raw);
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if !snapshot.check_for_updates_automatically
            || snapshot.check_for_updates_interval_hours == value
        {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let should_send = self
            .check_for_updates_interval_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|interval_hours| {
                expected_config_revision.map(|expected_config_revision| {
                    self.start_request(
                        PendingOperation::CheckForUpdatesIntervalHours,
                        Some(SettingValue::CheckForUpdatesIntervalHours {
                            expected_config_revision,
                            interval_hours,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_check_for_updates_interval_flush(cx);
        }
    }

    pub(super) fn set_logging_level(&mut self, level: SettingsLogLevel, cx: &mut Context<Self>) {
        let Some((persisted, _)) = self
            .snapshot
            .as_ref()
            .map(|snapshot| (snapshot.logging, snapshot.config_revision))
        else {
            return;
        };
        let mut settings = self
            .logging_settings_debouncer
            .pending_value()
            .copied()
            .unwrap_or(persisted);
        settings.level = level;
        self.queue_logging_settings(settings, cx);
    }

    pub(super) fn set_logging_retention_days_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let Some((persisted, _)) = self
            .snapshot
            .as_ref()
            .map(|snapshot| (snapshot.logging, snapshot.config_revision))
        else {
            return;
        };
        let mut settings = self
            .logging_settings_debouncer
            .pending_value()
            .copied()
            .unwrap_or(persisted);
        settings.retention_days = normalize_logging_retention_days(raw);
        self.queue_logging_settings(settings, cx);
    }

    fn queue_logging_settings(&mut self, settings: SettingsLogging, cx: &mut Context<Self>) {
        let Some((persisted, expected_config_revision)) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| Some((snapshot.logging, snapshot.config_revision?)))
        else {
            return;
        };
        if settings == persisted && self.pending != Some(PendingOperation::LoggingSettings) {
            self.logging_settings_debouncer.discard_pending();
            return;
        }
        let should_send = self
            .logging_settings_debouncer
            .observe(settings, Instant::now())
            .filter(|_| self.pending.is_none())
            .map(|settings| {
                self.start_request(
                    PendingOperation::LoggingSettings,
                    Some(SettingValue::LoggingSettings {
                        expected_config_revision,
                        settings,
                    }),
                    cx,
                );
            })
            .is_some();
        if !should_send {
            self.schedule_logging_settings_flush(cx);
        }
    }

    pub(super) fn set_overlay_settings(
        &mut self,
        settings: SettingsOverlay,
        cx: &mut Context<Self>,
    ) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::OverlaySettings,
            Some(SettingValue::OverlaySettings {
                expected_config_revision,
                settings,
            }),
            cx,
        );
    }

    pub(super) fn set_overlay_scale_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(25.0, 400.0) as u16;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.overlay.scale_percent == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let current_overlay = snapshot.overlay;
        let should_send = self
            .overlay_scale_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|scale_percent| {
                expected_config_revision.map(|expected_config_revision| {
                    let mut settings = current_overlay;
                    settings.scale_percent = scale_percent;
                    self.start_request(
                        PendingOperation::OverlayScale,
                        Some(SettingValue::OverlayScale {
                            expected_config_revision,
                            scale_percent,
                            settings,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_overlay_scale_flush(cx);
        }
    }

    pub(super) fn set_overlay_opacity_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(1.0, 100.0) as u8;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.overlay.opacity_percent == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let current_overlay = snapshot.overlay;
        let should_send = self
            .overlay_opacity_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|opacity_percent| {
                expected_config_revision.map(|expected_config_revision| {
                    let mut settings = current_overlay;
                    settings.opacity_percent = opacity_percent;
                    self.start_request(
                        PendingOperation::OverlayOpacity,
                        Some(SettingValue::OverlayOpacity {
                            expected_config_revision,
                            opacity_percent,
                            settings,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_overlay_opacity_flush(cx);
        }
    }

    /// Apply a corner radius typed or stepped in the overlay settings page.
    ///
    /// The legacy window rounding was a percentage of the window box, so the
    /// value is clamped to the range the configuration accepts: `0` keeps square
    /// corners and `50` is the full inscribed ellipse.
    pub(super) fn set_overlay_corner_radius_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(0.0, 50.0) as u8;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.overlay.corner_radius_percent == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let current_overlay = snapshot.overlay;
        let should_send = self
            .overlay_corner_radius_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|corner_radius_percent| {
                expected_config_revision.map(|expected_config_revision| {
                    let mut settings = current_overlay;
                    settings.corner_radius_percent = corner_radius_percent;
                    self.start_request(
                        PendingOperation::OverlayCornerRadius,
                        Some(SettingValue::OverlayCornerRadius {
                            expected_config_revision,
                            corner_radius_percent,
                            settings,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_overlay_corner_radius_flush(cx);
        }
    }

    /// Apply a hover-hide delay typed or stepped in the overlay settings page.
    ///
    /// The delay is the time the pointer has to rest on the overlay before it
    /// hides, so the value is clamped to the range the configuration accepts:
    /// `0` hides as soon as the pointer enters, and the upper bound is the
    /// shared `hide_on_pointer_hover_delay_seconds` limit rather than a
    /// UI-local number. The field is whole-second valued, so a fractional entry
    /// is rounded before it is compared with the current configuration.
    pub(super) fn set_overlay_hover_hide_delay_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(
            0.0,
            f64::from(bongocat_config::MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS),
        ) as u32;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.overlay.hide_on_pointer_hover_delay_seconds == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let current_overlay = snapshot.overlay;
        let should_send = self
            .overlay_hover_hide_delay_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|hide_on_pointer_hover_delay_seconds| {
                expected_config_revision.map(|expected_config_revision| {
                    let mut settings = current_overlay;
                    settings.hide_on_pointer_hover_delay_seconds =
                        hide_on_pointer_hover_delay_seconds;
                    self.start_request(
                        PendingOperation::OverlayHoverHideDelay,
                        Some(SettingValue::OverlayHoverHideDelay {
                            expected_config_revision,
                            hide_on_pointer_hover_delay_seconds,
                            settings,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_overlay_hover_hide_delay_flush(cx);
        }
    }

    pub(super) fn set_motion_audio_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::MotionAudio,
            Some(SettingValue::MotionAudioEnabled {
                expected_config_revision,
                enabled,
            }),
            cx,
        );
    }

    /// Whether the application command bindings reach the platform table.
    ///
    /// The Shortcuts page renders this gate as "disable window shortcuts", so
    /// the row calls this with the configuration truth: `enabled` is what the
    /// config stores, not what the switch shows.
    pub(super) fn set_command_shortcuts_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.command_shortcuts_enabled == enabled)
        {
            return;
        }
        self.start_request(
            PendingOperation::CommandShortcuts,
            Some(SettingValue::CommandShortcutsEnabled {
                expected_config_revision,
                enabled,
            }),
            cx,
        );
    }

    pub(super) fn set_behavior_shortcuts_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.behavior_shortcuts_enabled == enabled)
        {
            return;
        }
        self.start_request(
            PendingOperation::BehaviorShortcuts,
            Some(SettingValue::BehaviorShortcutsEnabled {
                expected_config_revision,
                enabled,
            }),
            cx,
        );
    }

    pub(super) fn set_random_behavior_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let settings = random_behavior_settings_after_toggle(
            snapshot.random_behavior,
            self.random_behavior_debouncer.pending_value().copied(),
            enabled,
        );
        if settings == snapshot.random_behavior && self.pending.is_none() {
            self.random_behavior_debouncer.discard_pending();
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        let should_send = self
            .random_behavior_debouncer
            .observe(settings, Instant::now())
            .filter(|_| self.pending.is_none())
            .is_some();
        if should_send {
            self.start_request(
                PendingOperation::RandomBehavior,
                Some(SettingValue::RandomBehaviorSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else {
            self.schedule_random_behavior_flush(cx);
        }
    }

    pub(super) fn set_random_behavior_interval_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let value = raw.round().clamp(
            f64::from(bongocat_config::MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS),
            f64::from(bongocat_config::MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS),
        ) as u32;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if !snapshot.random_behavior.enabled {
            return;
        }
        let mut settings = self
            .random_behavior_debouncer
            .pending_value()
            .copied()
            .unwrap_or(snapshot.random_behavior);
        if settings.interval_seconds == value {
            return;
        }
        settings.interval_seconds = value;
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        let should_send = self
            .random_behavior_debouncer
            .observe(settings, Instant::now())
            .filter(|_| self.pending.is_none())
            .is_some();
        if should_send {
            self.start_request(
                PendingOperation::RandomBehavior,
                Some(SettingValue::RandomBehaviorSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else {
            self.schedule_random_behavior_flush(cx);
        }
    }

    pub(super) fn set_maximum_fps_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(15.0, 240.0) as u16;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.maximum_fps == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let should_send = self
            .maximum_fps_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|maximum_fps| {
                expected_config_revision.map(|expected_config_revision| {
                    self.start_request(
                        PendingOperation::MaximumFps,
                        Some(SettingValue::MaximumFps {
                            expected_config_revision,
                            maximum_fps,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_maximum_fps_flush(cx);
        }
    }

    pub(super) fn set_release_fallback_timeout_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(0.0, 60_000.0) as u32;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.release_fallback_timeout_ms == value {
            return;
        }
        let expected_config_revision = snapshot.config_revision;
        let should_send = self
            .release_fallback_timeout_debouncer
            .observe(value, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|timeout_ms| {
                expected_config_revision.map(|expected_config_revision| {
                    self.start_request(
                        PendingOperation::ReleaseFallbackTimeout,
                        Some(SettingValue::ReleaseFallbackTimeout {
                            expected_config_revision,
                            timeout_ms,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_release_fallback_timeout_flush(cx);
        }
    }

    pub(super) fn set_model_settings(
        &mut self,
        settings: SettingsModelSettings,
        cx: &mut Context<Self>,
    ) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::ModelSettings,
            Some(SettingValue::ModelSettings {
                expected_config_revision,
                settings,
            }),
            cx,
        );
    }

    pub(super) fn set_gamepad_dead_zone_value(
        &mut self,
        stick: bool,
        raw: f64,
        cx: &mut Context<Self>,
    ) {
        if self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(0.0, 99.0) as u8;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut settings = snapshot.gamepad_axis_settings;
        let current = if stick {
            &mut settings.stick_dead_zone_percent
        } else {
            &mut settings.trigger_dead_zone_percent
        };
        if *current == value {
            return;
        }
        *current = value;
        let expected_config_revision = snapshot.config_revision;
        let should_send = self
            .gamepad_dead_zone_debouncer
            .observe(settings, Instant::now())
            .filter(|_| self.pending.is_none())
            .and_then(|settings| {
                expected_config_revision.map(|expected_config_revision| {
                    self.start_request(
                        PendingOperation::GamepadAxisSettings,
                        Some(SettingValue::GamepadAxisSettings {
                            expected_config_revision,
                            settings,
                        }),
                        cx,
                    );
                })
            })
            .is_some();
        if !should_send {
            self.schedule_gamepad_dead_zone_flush(cx);
        }
    }

    pub(super) fn set_startup_item_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::StartupItem,
            Some(SettingValue::StartupItemEnabled(enabled)),
            cx,
        );
    }

    pub(super) fn shortcut_commands_available(&self) -> bool {
        self.pending.is_none()
            && !self.model_import.is_running()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.config_revision.is_some())
    }
}
