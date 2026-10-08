use super::*;
use gpui_kit::component::color_picker::ColorPicker;

pub(super) fn rgb_color(rgb: [u8; 3]) -> Hsla {
    gpui_kit::rgb((u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])).into()
}

pub(super) fn window_mode_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
) -> SettingItem {
    let read_view = view.clone();
    let write_view = view.clone();
    SettingItem::new(
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.window_mode.label",
        ),
        SettingField::switch(
            move |app| {
                read_view
                    .read(app)
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.overlay.window_mode)
            },
            move |value, app| {
                write_view.update(app, |view, cx| {
                    if let Some(snapshot) = &view.snapshot {
                        let mut settings = snapshot.overlay;
                        settings.window_mode = value;
                        view.set_overlay_settings(settings, cx);
                    }
                })
            },
        ),
    )
}

pub(super) fn background_color_row(
    picker: &Entity<gpui_kit::component::color_picker::ColorPickerState>,
    language: SettingsLanguage,
    gate: SettingGate,
) -> SettingItem {
    let picker = picker.clone();
    SettingItem::new(
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.window_background_color.label",
        ),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                if options.is_disabled() {
                    let color = picker
                        .read(app)
                        .value()
                        .unwrap_or_else(|| rgb_color([0, 255, 0]));
                    // The base ColorSwatch is unstyled: its color is a value,
                    // not a background fill. A passive preview needs no input.
                    div()
                        .id("window-background-disabled")
                        .size_8()
                        .bg(color)
                        .border_1()
                        .border_color(app.theme().input)
                        .rounded(app.theme().radius)
                        .into_any_element()
                } else {
                    ColorPicker::new(&picker)
                        .featured_colors(vec![
                            rgb_color([0, 255, 0]),
                            rgb_color([0, 0, 255]),
                            rgb_color([255, 0, 255]),
                        ])
                        .into_any_element()
                }
            },
        ),
    )
    .disabled(gate.disables_controls())
}

impl SettingsView {
    pub(super) fn set_window_background_color(&mut self, color: Hsla, cx: &mut Context<Self>) {
        if self.syncing_component_inputs || self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if !snapshot.overlay.window_mode {
            return;
        }
        let rgba = color.to_rgb();
        let rgb =
            [rgba.r, rgba.g, rgba.b].map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8);
        if self
            .window_background_debouncer
            .pending_value()
            .copied()
            .unwrap_or(snapshot.overlay.window_background_color)
            == rgb
        {
            return;
        }
        self.window_background_debouncer
            .observe(rgb, Instant::now());
        self.schedule_window_background_flush(cx);
    }

    pub(super) fn flush_window_background_color(
        &mut self,
        cx: &mut Context<Self>,
        now: Instant,
        force: bool,
    ) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let color = if force {
            self.window_background_debouncer.flush(now)
        } else {
            self.window_background_debouncer.ready(now)
        };
        let Some(color) = color else {
            return false;
        };
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let mut settings = snapshot.overlay;
        settings.window_background_color = color;
        self.set_overlay_settings(settings, cx);
        self.pending.is_some()
    }

    pub(super) fn schedule_window_background_flush(&mut self, cx: &mut Context<Self>) {
        self.window_background_timer_generation =
            self.window_background_timer_generation.saturating_add(1);
        let generation = self.window_background_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if generation == view.window_background_timer_generation {
                    view.flush_window_background_color(cx, Instant::now(), false);
                }
            });
        })
        .detach();
    }
}
