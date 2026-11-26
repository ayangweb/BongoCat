//! macOS menus connect framework actions to the existing product owners.

use super::*;
use gpui_kit::component::{
    Theme,
    input::{Copy, Cut, Paste, Redo, SelectAll, Undo},
};
use gpui_kit::{KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

gpui_kit::actions!(
    macos_menu,
    [
        Quit,
        OpenSettings,
        CloseWindow,
        MinimizeWindow,
        ZoomWindow,
        Hide,
        HideOthers,
        ShowAll
    ]
);

pub(crate) fn init(cx: &mut App) {
    if !cx.has_global::<Theme>() {
        gpui_kit::init(cx);
    }
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
    ]);
    // Global handlers can run while the focused window is borrowed for dispatch.
    // Defer mutations until that borrow has been released.
    cx.on_action(|_: &Quit, cx| cx.defer(request_product_quit));
    cx.on_action(|_: &OpenSettings, cx| {
        cx.defer(|cx| {
            if let Err(error) = ensure_settings_window(cx) {
                menu_failure(cx, error);
            }
        });
    });
    cx.on_action(|_: &CloseWindow, cx| close_active_window(cx));
    cx.on_action(|_: &MinimizeWindow, cx| {
        with_active_window(cx, |window| window.minimize_window());
    });
    cx.on_action(|_: &ZoomWindow, cx| {
        with_active_window(cx, |window| window.zoom_window());
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    set_language(cx.global::<ProductCoordinator>().product_language, cx);
}

pub(crate) fn set_language(language: bongocat_ui_protocol::SettingsLanguage, cx: &mut App) {
    let locale = language.catalog_locale();
    let label = |key| bongocat_i18n::text(locale, key).to_owned();
    cx.set_menus(vec![
        Menu::new("BongoCat").items([
            MenuItem::action(
                format!("{}…", label("system_menu.open_settings")),
                OpenSettings,
            ),
            MenuItem::separator(),
            MenuItem::os_submenu(label("application_menu.services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(label("application_menu.hide"), Hide),
            MenuItem::action(label("application_menu.hide_others"), HideOthers),
            MenuItem::action(label("application_menu.show_all"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(label("system_menu.quit"), Quit),
        ]),
        Menu::new(label("application_menu.file")).items([MenuItem::action(
            label("application_menu.close"),
            CloseWindow,
        )]),
        Menu::new(label("application_menu.edit")).items([
            MenuItem::os_action(label("application_menu.undo"), Undo, OsAction::Undo),
            MenuItem::os_action(label("application_menu.redo"), Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(label("application_menu.cut"), Cut, OsAction::Cut),
            MenuItem::os_action(label("application_menu.copy"), Copy, OsAction::Copy),
            MenuItem::os_action(label("application_menu.paste"), Paste, OsAction::Paste),
            MenuItem::os_action(
                label("application_menu.select_all"),
                SelectAll,
                OsAction::SelectAll,
            ),
        ]),
        Menu::new(label("application_menu.window")).items([
            MenuItem::action(label("application_menu.minimize"), MinimizeWindow),
            MenuItem::action(label("application_menu.zoom"), ZoomWindow),
        ]),
    ]);
}

fn with_active_window(cx: &mut App, action: impl FnOnce(&Window) + 'static) {
    if let Some(handle) = cx.active_window() {
        cx.defer(move |cx| {
            if let Err(error) = handle.update(cx, |_, window, _| action(window)) {
                menu_failure(cx, error.to_string());
            }
        });
    }
}

fn close_active_window(cx: &mut App) {
    let Some(active) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let Some(coordinator) = cx.try_global::<ProductCoordinator>() else {
            return;
        };
        if let Some(update) = coordinator.update_window.clone()
            && update.window().window_id() == active.window_id()
        {
            if let Err(error) = update.update(cx, |_, window, _| window.remove_window()) {
                menu_failure(cx, error.to_string());
            }
        } else if let Some(settings) = coordinator.settings_window.clone() {
            match settings.update(cx, |view, window, cx| {
                if window.window_handle() == active {
                    view.close(window, cx)
                } else {
                    Ok(())
                }
            }) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => menu_failure(cx, error),
                Err(error) => menu_failure(cx, error.to_string()),
            }
        }
    });
}

fn menu_failure(cx: &App, error: String) {
    if let Some(coordinator) = cx.try_global::<ProductCoordinator>() {
        record_failure(
            &coordinator.failures,
            format!("macOS application menu: {error}"),
        );
    }
}
