//! Sending a change once the user has stopped making it.
//!
//! Every bounded setting gets its own debouncer, because they are independent:
//! dragging the overlay scale must not also restart the pending update interval.
//! Each writer records what it sent, so a value the user changed again while the
//! write was in flight is sent once more rather than being lost.

use super::*;

impl SettingsView {
    pub(crate) fn schedule_check_for_updates_interval_flush(&mut self, cx: &mut Context<Self>) {
        self.check_for_updates_interval_timer_generation = self
            .check_for_updates_interval_timer_generation
            .saturating_add(1);
        let generation = self.check_for_updates_interval_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.check_for_updates_interval_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(interval_hours) = view
                    .check_for_updates_interval_debouncer
                    .ready(Instant::now())
                else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::CheckForUpdatesIntervalHours,
                    Some(SettingValue::CheckForUpdatesIntervalHours {
                        expected_config_revision,
                        interval_hours,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_overlay_scale_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_scale_timer_generation = self.overlay_scale_timer_generation.saturating_add(1);
        let generation = self.overlay_scale_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_scale_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(scale_percent) = view.overlay_scale_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.scale_percent = scale_percent;
                view.start_request(
                    PendingOperation::OverlayScale,
                    Some(SettingValue::OverlayScale {
                        expected_config_revision,
                        scale_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_overlay_opacity_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_opacity_timer_generation =
            self.overlay_opacity_timer_generation.saturating_add(1);
        let generation = self.overlay_opacity_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_opacity_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(opacity_percent) = view.overlay_opacity_debouncer.ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.opacity_percent = opacity_percent;
                view.start_request(
                    PendingOperation::OverlayOpacity,
                    Some(SettingValue::OverlayOpacity {
                        expected_config_revision,
                        opacity_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_overlay_corner_radius_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_corner_radius_timer_generation = self
            .overlay_corner_radius_timer_generation
            .saturating_add(1);
        let generation = self.overlay_corner_radius_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_corner_radius_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(corner_radius_percent) =
                    view.overlay_corner_radius_debouncer.ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.corner_radius_percent = corner_radius_percent;
                view.start_request(
                    PendingOperation::OverlayCornerRadius,
                    Some(SettingValue::OverlayCornerRadius {
                        expected_config_revision,
                        corner_radius_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_overlay_hover_hide_delay_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_hover_hide_delay_timer_generation = self
            .overlay_hover_hide_delay_timer_generation
            .saturating_add(1);
        let generation = self.overlay_hover_hide_delay_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_hover_hide_delay_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(hide_on_pointer_hover_delay_seconds) = view
                    .overlay_hover_hide_delay_debouncer
                    .ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.hide_on_pointer_hover_delay_seconds = hide_on_pointer_hover_delay_seconds;
                view.start_request(
                    PendingOperation::OverlayHoverHideDelay,
                    Some(SettingValue::OverlayHoverHideDelay {
                        expected_config_revision,
                        hide_on_pointer_hover_delay_seconds,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_gamepad_dead_zone_flush(&mut self, cx: &mut Context<Self>) {
        self.gamepad_dead_zone_timer_generation =
            self.gamepad_dead_zone_timer_generation.saturating_add(1);
        let generation = self.gamepad_dead_zone_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.gamepad_dead_zone_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(settings) = view.gamepad_dead_zone_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::GamepadAxisSettings,
                    Some(SettingValue::GamepadAxisSettings {
                        expected_config_revision,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_maximum_fps_flush(&mut self, cx: &mut Context<Self>) {
        self.maximum_fps_timer_generation = self.maximum_fps_timer_generation.saturating_add(1);
        let generation = self.maximum_fps_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.maximum_fps_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(maximum_fps) = view.maximum_fps_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::MaximumFps,
                    Some(SettingValue::MaximumFps {
                        expected_config_revision,
                        maximum_fps,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_release_fallback_timeout_flush(&mut self, cx: &mut Context<Self>) {
        self.release_fallback_timeout_timer_generation = self
            .release_fallback_timeout_timer_generation
            .saturating_add(1);
        let generation = self.release_fallback_timeout_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.release_fallback_timeout_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(timeout_ms) = view
                    .release_fallback_timeout_debouncer
                    .ready(Instant::now())
                else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::ReleaseFallbackTimeout,
                    Some(SettingValue::ReleaseFallbackTimeout {
                        expected_config_revision,
                        timeout_ms,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_random_behavior_flush(&mut self, cx: &mut Context<Self>) {
        self.random_behavior_timer_generation =
            self.random_behavior_timer_generation.saturating_add(1);
        let generation = self.random_behavior_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.random_behavior_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(settings) = view.random_behavior_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::RandomBehavior,
                    Some(SettingValue::RandomBehaviorSettings {
                        expected_config_revision,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn schedule_logging_settings_flush(&mut self, cx: &mut Context<Self>) {
        self.logging_settings_timer_generation =
            self.logging_settings_timer_generation.saturating_add(1);
        let generation = self.logging_settings_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.logging_settings_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(settings) = view.logging_settings_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::LoggingSettings,
                    Some(SettingValue::LoggingSettings {
                        expected_config_revision,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(crate) fn flush_pending_setting_patches(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let now = Instant::now();
        let Some(snapshot) = self.snapshot.clone() else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
            return;
        };
        let Some(expected_config_revision) = snapshot.config_revision else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
            return;
        };
        if let Some(interval_hours) = self.check_for_updates_interval_debouncer.flush(now) {
            self.start_request(
                PendingOperation::CheckForUpdatesIntervalHours,
                Some(SettingValue::CheckForUpdatesIntervalHours {
                    expected_config_revision,
                    interval_hours,
                }),
                cx,
            );
        } else if let Some(scale_percent) = self.overlay_scale_debouncer.flush(now) {
            let mut settings = snapshot.overlay;
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
        } else if let Some(opacity_percent) = self.overlay_opacity_debouncer.flush(now) {
            let mut settings = snapshot.overlay;
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
        } else if let Some(corner_radius_percent) = self.overlay_corner_radius_debouncer.flush(now)
        {
            let mut settings = snapshot.overlay;
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
        } else if let Some(hide_on_pointer_hover_delay_seconds) =
            self.overlay_hover_hide_delay_debouncer.flush(now)
        {
            let mut settings = snapshot.overlay;
            settings.hide_on_pointer_hover_delay_seconds = hide_on_pointer_hover_delay_seconds;
            self.start_request(
                PendingOperation::OverlayHoverHideDelay,
                Some(SettingValue::OverlayHoverHideDelay {
                    expected_config_revision,
                    hide_on_pointer_hover_delay_seconds,
                    settings,
                }),
                cx,
            );
        } else if let Some(settings) = self.gamepad_dead_zone_debouncer.flush(now) {
            self.start_request(
                PendingOperation::GamepadAxisSettings,
                Some(SettingValue::GamepadAxisSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else if let Some(maximum_fps) = self.maximum_fps_debouncer.flush(now) {
            self.start_request(
                PendingOperation::MaximumFps,
                Some(SettingValue::MaximumFps {
                    expected_config_revision,
                    maximum_fps,
                }),
                cx,
            );
        } else if let Some(timeout_ms) = self.release_fallback_timeout_debouncer.flush(now) {
            self.start_request(
                PendingOperation::ReleaseFallbackTimeout,
                Some(SettingValue::ReleaseFallbackTimeout {
                    expected_config_revision,
                    timeout_ms,
                }),
                cx,
            );
        } else if let Some(settings) = self.random_behavior_debouncer.flush(now) {
            self.start_request(
                PendingOperation::RandomBehavior,
                Some(SettingValue::RandomBehaviorSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else if let Some(settings) = self.logging_settings_debouncer.flush(now) {
            self.start_request(
                PendingOperation::LoggingSettings,
                Some(SettingValue::LoggingSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
        }
    }
}

impl SettingsView {
    pub(super) fn flush_pending_settings(&mut self, cx: &mut Context<Self>) {
        self.flush_pending_requested = true;
        self.flush_pending_setting_patches(cx);
    }
}

impl SettingsView {
    pub(super) fn request_quit_after_flush(&mut self, cx: &mut Context<Self>) {
        self.flush_pending_requested = true;
        self.quit_after_flush = true;
        self.flush_pending_setting_patches(cx);
    }
}

impl SettingsView {
    /// The one visual-gate predicate every settings page reads (ADR-0053).
    ///
    /// True only where editing is structurally impossible: no snapshot yet, a
    /// model import running, or any source or conversion surface
    /// covering the window. The transient in-flight `pending` flag deliberately never
    /// feeds it — it flips on and off around every command, so gating on it
    /// dims and re-enables a whole page on each control change, which reads
    /// as the page refreshing. Re-entrancy is refused by the command guards
    /// instead (`start_request`, the model command methods).
    pub(crate) fn editing_blocked(&self, snapshot: Option<&SettingsSnapshot>) -> bool {
        snapshot.is_none()
            || self.model_import.is_running()
            || self.model_import.is_source_surface_open()
    }
}
