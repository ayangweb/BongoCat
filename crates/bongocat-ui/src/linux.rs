//! Linux permission explanation and theme colors for the Wayland menu.
use crate::SettingsLanguage;
use bongocat_platform::SystemMenuPalette;
use gpui_kit::component::{ActiveTheme, WindowExt, dialog::DialogButtonProps};
use gpui_kit::{App, Hsla, ParentElement, Rgba, Window};
use std::rc::Rc;

pub fn show_linux_input_permission(
    language: SettingsLanguage,
    confirm: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let locale = language.catalog_locale();
    let title = bongocat_i18n::text(locale, "startup_permission.linux.title").to_owned();
    let description =
        bongocat_i18n::text(locale, "startup_permission.linux.description").to_owned();
    let confirm_label = bongocat_i18n::text(locale, "startup_permission.linux.confirm").to_owned();
    let later = bongocat_i18n::text(locale, "startup_permission.later").to_owned();
    let confirm = Rc::new(confirm);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let confirm = confirm.clone();
        dialog
            .confirm()
            .title(title.clone())
            .child(description.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text(confirm_label.clone())
                    .cancel_text(later.clone())
                    .show_cancel(true),
            )
            .on_ok(move |_, _, cx| {
                confirm(cx);
                true
            })
    });
}

/// Use the same semantic colors as the GPUI popup menu.
pub fn linux_system_menu_palette(cx: &App) -> SystemMenuPalette {
    let theme = cx.theme();
    let rgba = |color: Hsla| {
        let color = Rgba::from(color);
        [color.r, color.g, color.b, color.a].map(|component| (component * 255.).round() as u8)
    };
    SystemMenuPalette {
        surface: rgba(theme.popover),
        foreground: rgba(theme.popover_foreground),
        muted_foreground: rgba(theme.muted_foreground),
        separator: rgba(theme.border),
        hover_background: rgba(theme.accent),
        hover_foreground: rgba(theme.accent_foreground),
    }
}
