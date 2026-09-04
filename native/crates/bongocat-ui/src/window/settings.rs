use super::*;

impl SettingsView {
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

    pub(super) fn adjust_overlay_scale(&mut self, delta: i16, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready {
            return;
        }
        let settings = stepped_overlay_scale(snapshot.overlay, delta);
        if settings.scale_percent != snapshot.overlay.scale_percent {
            self.set_overlay_settings(settings, cx);
        }
    }

    pub(super) fn set_overlay_scale_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(25.0, 400.0) as u16;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready
            || snapshot.overlay.scale_percent == value
        {
            return;
        }
        let mut settings = snapshot.overlay;
        settings.scale_percent = value;
        self.set_overlay_settings(settings, cx);
    }

    pub(super) fn adjust_overlay_opacity(&mut self, delta: i16, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready {
            return;
        }
        let settings = stepped_overlay_opacity(snapshot.overlay, delta);
        if settings.opacity_percent != snapshot.overlay.opacity_percent {
            self.set_overlay_settings(settings, cx);
        }
    }

    pub(super) fn set_overlay_opacity_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(1.0, 100.0) as u8;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready
            || snapshot.overlay.opacity_percent == value
        {
            return;
        }
        let mut settings = snapshot.overlay;
        settings.opacity_percent = value;
        self.set_overlay_settings(settings, cx);
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

    pub(super) fn set_maximum_fps(&mut self, maximum_fps: u16, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::MaximumFps,
            Some(SettingValue::MaximumFps {
                expected_config_revision,
                maximum_fps,
            }),
            cx,
        );
    }

    pub(super) fn set_maximum_fps_value(&mut self, raw: f64, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(15.0, 240.0) as u16;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready
            || snapshot.maximum_fps == value
        {
            return;
        }
        self.set_maximum_fps(value, cx);
    }

    pub(super) fn adjust_maximum_fps(&mut self, delta: i16, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        self.set_maximum_fps_value(
            f64::from((i32::from(snapshot.maximum_fps) + i32::from(delta)).clamp(15, 240)),
            cx,
        );
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

    pub(super) fn adjust_gamepad_dead_zone(
        &mut self,
        stick: bool,
        delta: i16,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready {
            return;
        }
        let mut settings = snapshot.gamepad_axis_settings;
        let value = if stick {
            &mut settings.stick_dead_zone_percent
        } else {
            &mut settings.trigger_dead_zone_percent
        };
        *value = (i16::from(*value) + delta).clamp(0, 99) as u8;
        self.set_gamepad_axis_settings(settings, cx);
    }

    pub(super) fn set_gamepad_dead_zone_value(
        &mut self,
        stick: bool,
        raw: f64,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let value = raw.round().clamp(0.0, 99.0) as u8;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.configuration_status != SettingsConfigurationStatus::Ready {
            return;
        }
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
        self.set_gamepad_axis_settings(settings, cx);
    }

    pub(super) fn set_gamepad_axis_settings(
        &mut self,
        settings: SettingsGamepadAxisSettings,
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
            PendingOperation::GamepadAxisSettings,
            Some(SettingValue::GamepadAxisSettings {
                expected_config_revision,
                settings,
            }),
            cx,
        );
    }

    pub(super) fn set_startup_item_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::StartupItem,
            Some(SettingValue::StartupItemEnabled(enabled)),
            cx,
        );
    }

    pub(super) fn restore_default_configuration(&mut self, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::RestoreDefaultConfiguration,
            Some(SettingValue::RestoreDefaultConfiguration),
            cx,
        );
    }

    pub(super) fn open_config_backup_location(&mut self, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::OpenConfigBackupLocation,
            Some(SettingValue::OpenConfigBackupLocation),
            cx,
        );
    }

    pub(super) fn export_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::ExportDiagnostics,
            Some(SettingValue::ExportDiagnostics),
            cx,
        );
    }

    pub(super) fn shortcut_commands_available(&self) -> bool {
        self.pending.is_none()
            && !self.model_import.is_running()
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.configuration_status == SettingsConfigurationStatus::Ready
                    && snapshot.config_revision.is_some()
            })
    }

    pub(super) fn restore_default_shortcuts(&mut self, cx: &mut Context<Self>) {
        if !self.shortcut_commands_available() {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::RestoreDefaultShortcuts,
            Some(SettingValue::RestoreDefaultShortcuts {
                expected_config_revision,
            }),
            cx,
        );
    }

    pub(super) fn clear_shortcuts(&mut self, cx: &mut Context<Self>) {
        if !self.shortcut_commands_available() {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::ClearShortcuts,
            Some(SettingValue::Shortcuts {
                expected_config_revision,
                shortcuts: SettingsShortcuts::default(),
            }),
            cx,
        );
    }
}
