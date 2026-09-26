//! The one request that carries a pending change to the service.
//!
//! One function rather than one per setting, because the ordering is the point:
//! a second change is refused while one is in flight, a refused write for a stale
//! revision is retried against a fresh snapshot, and each debouncer is told what
//! was sent only if the write succeeded. Splitting the match per setting would
//! scatter that ordering across a dozen places.

use super::*;

impl SettingsView {
    pub(crate) fn start_request(
        &mut self,
        operation: PendingOperation,
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let is_refresh = operation == PendingOperation::Refresh;
        if !is_refresh {
            self.pending = Some(operation);
            cx.notify();
        }
        let client = self.client.clone();
        let sent_check_for_updates_interval = match value.as_ref() {
            Some(SettingValue::CheckForUpdatesIntervalHours { interval_hours, .. }) => {
                Some(*interval_hours)
            }
            _ => None,
        };
        let sent_overlay_scale = match value.as_ref() {
            Some(SettingValue::OverlayScale { scale_percent, .. }) => Some(*scale_percent),
            _ => None,
        };
        let sent_overlay_opacity = match value.as_ref() {
            Some(SettingValue::OverlayOpacity {
                opacity_percent, ..
            }) => Some(*opacity_percent),
            _ => None,
        };
        let sent_overlay_corner_radius = match value.as_ref() {
            Some(SettingValue::OverlayCornerRadius {
                corner_radius_percent,
                ..
            }) => Some(*corner_radius_percent),
            _ => None,
        };
        let sent_overlay_hover_hide_delay = match value.as_ref() {
            Some(SettingValue::OverlayHoverHideDelay {
                hide_on_pointer_hover_delay_seconds,
                ..
            }) => Some(*hide_on_pointer_hover_delay_seconds),
            _ => None,
        };
        let sent_gamepad_dead_zone = match value.as_ref() {
            Some(SettingValue::GamepadAxisSettings { settings, .. }) => Some(*settings),
            _ => None,
        };
        let sent_maximum_fps = match value.as_ref() {
            Some(SettingValue::MaximumFps { maximum_fps, .. }) => Some(*maximum_fps),
            _ => None,
        };
        let sent_release_fallback_timeout = match value.as_ref() {
            Some(SettingValue::ReleaseFallbackTimeout { timeout_ms, .. }) => Some(*timeout_ms),
            _ => None,
        };
        let sent_random_behavior = match value.as_ref() {
            Some(SettingValue::RandomBehaviorSettings { settings, .. }) => Some(*settings),
            _ => None,
        };
        let sent_logging_settings = match value.as_ref() {
            Some(SettingValue::LoggingSettings { settings, .. }) => Some(*settings),
            _ => None,
        };
        cx.spawn(async move |this, cx| {
            let result = match value {
                None => client.read_snapshot().await,
                Some(SettingValue::AppearanceTheme {
                    expected_config_revision,
                    theme,
                }) => {
                    client
                        .set_appearance_theme(expected_config_revision, theme)
                        .await
                }
                Some(SettingValue::Language {
                    expected_config_revision,
                    language,
                }) => {
                    client
                        .set_language(expected_config_revision, language)
                        .await
                }
                Some(SettingValue::StatusIconVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_status_icon_visible(expected_config_revision, visible)
                        .await
                }
                #[cfg(target_os = "windows")]
                Some(SettingValue::TaskbarIconVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_taskbar_icon_visible(expected_config_revision, visible)
                        .await
                }
                Some(SettingValue::CheckForUpdatesAutomatically {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_check_for_updates_automatically(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::CheckForUpdatesIntervalHours {
                    expected_config_revision,
                    interval_hours,
                }) => {
                    client
                        .set_check_for_updates_interval_hours(
                            expected_config_revision,
                            interval_hours,
                        )
                        .await
                }
                Some(SettingValue::LoggingSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_logging_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_overlay_visible(expected_config_revision, visible)
                        .await
                }
                Some(SettingValue::OverlaySettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayScale {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayOpacity {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayCornerRadius {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayHoverHideDelay {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::MotionAudioEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_motion_audio_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::CommandShortcutsEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_command_shortcuts_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::BehaviorShortcutsEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_behavior_shortcuts_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::RandomBehaviorSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_random_behavior_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::MaximumFps {
                    expected_config_revision,
                    maximum_fps,
                }) => {
                    client
                        .set_maximum_fps(expected_config_revision, maximum_fps)
                        .await
                }
                Some(SettingValue::ReleaseFallbackTimeout {
                    expected_config_revision,
                    timeout_ms,
                }) => {
                    client
                        .set_release_fallback_timeout(expected_config_revision, timeout_ms)
                        .await
                }
                Some(SettingValue::ModelSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_model_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::GamepadAxisSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_gamepad_axis_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::GamepadAutoSwitch {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_gamepad_auto_switch(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::StartupItemEnabled(enabled)) => {
                    client.set_startup_item_enabled(enabled).await
                }
                Some(SettingValue::Shortcuts {
                    expected_config_revision,
                    shortcuts,
                }) => {
                    client
                        .set_shortcuts(expected_config_revision, shortcuts)
                        .await
                }
            };
            let refreshed = if result
                .as_ref()
                .is_err_and(|error| error.code() == SettingsErrorCode::SnapshotOutdated)
            {
                client.read_snapshot().await.ok()
            } else {
                None
            };
            let _ = this.update(cx, |view, cx| {
                let mut snapshot_changed = false;
                if !is_refresh {
                    view.pending = None;
                }
                if result.is_ok()
                    && let Some(interval_hours) = sent_check_for_updates_interval
                {
                    view.check_for_updates_interval_debouncer
                        .mark_sent(&interval_hours);
                    if view.check_for_updates_interval_debouncer.is_pending() {
                        view.schedule_check_for_updates_interval_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(scale_percent) = sent_overlay_scale
                {
                    view.overlay_scale_debouncer.mark_sent(&scale_percent);
                    if view.overlay_scale_debouncer.is_pending() {
                        view.schedule_overlay_scale_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(opacity_percent) = sent_overlay_opacity
                {
                    view.overlay_opacity_debouncer.mark_sent(&opacity_percent);
                    if view.overlay_opacity_debouncer.is_pending() {
                        view.schedule_overlay_opacity_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(corner_radius_percent) = sent_overlay_corner_radius
                {
                    view.overlay_corner_radius_debouncer
                        .mark_sent(&corner_radius_percent);
                    if view.overlay_corner_radius_debouncer.is_pending() {
                        view.schedule_overlay_corner_radius_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(hide_on_pointer_hover_delay_seconds) = sent_overlay_hover_hide_delay
                {
                    view.overlay_hover_hide_delay_debouncer
                        .mark_sent(&hide_on_pointer_hover_delay_seconds);
                    if view.overlay_hover_hide_delay_debouncer.is_pending() {
                        view.schedule_overlay_hover_hide_delay_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(settings) = sent_gamepad_dead_zone
                {
                    view.gamepad_dead_zone_debouncer.mark_sent(&settings);
                    if view.gamepad_dead_zone_debouncer.is_pending() {
                        view.schedule_gamepad_dead_zone_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(maximum_fps) = sent_maximum_fps
                {
                    view.maximum_fps_debouncer.mark_sent(&maximum_fps);
                    if view.maximum_fps_debouncer.is_pending() {
                        view.schedule_maximum_fps_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(timeout_ms) = sent_release_fallback_timeout
                {
                    view.release_fallback_timeout_debouncer
                        .mark_sent(&timeout_ms);
                    if view.release_fallback_timeout_debouncer.is_pending() {
                        view.schedule_release_fallback_timeout_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(settings) = sent_random_behavior
                {
                    view.random_behavior_debouncer.mark_sent(&settings);
                    if view.random_behavior_debouncer.is_pending() {
                        view.schedule_random_behavior_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(settings) = sent_logging_settings
                {
                    view.logging_settings_debouncer.mark_sent(&settings);
                    if view.logging_settings_debouncer.is_pending() {
                        view.schedule_logging_settings_flush(cx);
                    }
                }
                if result.is_err() {
                    if operation == PendingOperation::AppearanceTheme {
                        view.applied_theme = None;
                    }
                    // Keep failed debounced patches alive and retry after the stable window.
                    // The debouncer only clears a value after a successful acknowledgement.
                    if sent_check_for_updates_interval.is_some() {
                        view.schedule_check_for_updates_interval_flush(cx);
                    }
                    if sent_overlay_scale.is_some() {
                        view.schedule_overlay_scale_flush(cx);
                    }
                    if sent_overlay_opacity.is_some() {
                        view.schedule_overlay_opacity_flush(cx);
                    }
                    if sent_overlay_corner_radius.is_some() {
                        view.schedule_overlay_corner_radius_flush(cx);
                    }
                    if sent_overlay_hover_hide_delay.is_some() {
                        view.schedule_overlay_hover_hide_delay_flush(cx);
                    }
                    if sent_gamepad_dead_zone.is_some() {
                        view.schedule_gamepad_dead_zone_flush(cx);
                    }
                    if sent_maximum_fps.is_some() {
                        view.schedule_maximum_fps_flush(cx);
                    }
                    if sent_release_fallback_timeout.is_some() {
                        view.schedule_release_fallback_timeout_flush(cx);
                    }
                    if sent_random_behavior.is_some() {
                        view.schedule_random_behavior_flush(cx);
                    }
                    if sent_logging_settings.is_some() {
                        view.schedule_logging_settings_flush(cx);
                    }
                }
                if sent_check_for_updates_interval.is_none()
                    && view.check_for_updates_interval_debouncer.is_pending()
                {
                    view.schedule_check_for_updates_interval_flush(cx);
                }
                if sent_random_behavior.is_none() && view.random_behavior_debouncer.is_pending() {
                    view.schedule_random_behavior_flush(cx);
                }
                if sent_logging_settings.is_none() && view.logging_settings_debouncer.is_pending() {
                    view.schedule_logging_settings_flush(cx);
                }
                if let Some(snapshot) = refreshed
                    && accepts_snapshot_revision(
                        view.snapshot.as_ref().map(|current| current.revision),
                        snapshot.revision,
                    )
                    && view.snapshot.as_ref() != Some(&snapshot)
                {
                    view.snapshot = Some(snapshot);
                    snapshot_changed = true;
                }
                match result {
                    Ok(ref snapshot)
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) =>
                    {
                        if view.snapshot.as_ref() != Some(snapshot) {
                            view.snapshot = Some(snapshot.clone());
                            snapshot_changed = true;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if view.pending_notification.as_ref() != Some(&error) {
                            snapshot_changed = true;
                        }
                        view.pending_notification = Some(error);
                    }
                }
                // The next shutdown patch must use the revision returned by this request.
                if view.flush_pending_requested {
                    if result.is_ok() {
                        view.flush_pending_setting_patches(cx);
                    } else {
                        view.flush_pending_requested = false;
                        view.quit_after_flush = false;
                    }
                }
                if !is_refresh || snapshot_changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
