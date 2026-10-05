//! Linux product controls. Both windows use the existing GPUI component framework.
use crate::SettingsLanguage;
use bongocat_platform::{LinuxSystemMenuItem, SystemMenuAction, SystemMenuPresentation};
use gpui_kit::component::{
    Root, WindowExt,
    dialog::DialogButtonProps,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{
    App, AppContext, Bounds, Context, DismissEvent, Entity, Focusable, Render, Subscription,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, prelude::*, px, size,
};
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

struct MenuView {
    menu: Entity<PopupMenu>,
    _subscriptions: Vec<Subscription>,
}
impl Render for MenuView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.menu.clone())
    }
}

/// Independent menu window: Wayland chooses its placement because the overlay
/// belongs to a separate connection, so it cannot be a GPUI popup parent.
pub fn open_linux_context_menu(
    presentation: SystemMenuPresentation,
    action: impl Fn(SystemMenuAction, &mut App) + 'static,
    cx: &mut App,
) -> Result<WindowHandle<Root>, String> {
    let action = Rc::new(action);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(280.), px(180.)),
                cx,
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_resizable: false,
            ..Default::default()
        },
        move |window, cx| {
            let menu = PopupMenu::build(window, cx, |mut menu, _, _| {
                for item in presentation.linux_items() {
                    menu = match item {
                        LinuxSystemMenuItem::Separator => menu.separator(),
                        LinuxSystemMenuItem::Action {
                            action: command,
                            label,
                            checked,
                        } => {
                            let action = action.clone();
                            menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                                move |_, window, cx| {
                                    window.remove_window();
                                    action(command, cx);
                                },
                            ))
                        }
                    };
                }
                menu
            });
            menu.focus_handle(cx).focus(window, cx);
            let view = cx.new(|cx| {
                let dismiss =
                    cx.subscribe_in(&menu, window, |_, _, _: &DismissEvent, window, _| {
                        window.remove_window();
                    });
                let deactivate = cx.observe_window_activation(window, |_, window, _| {
                    if !window.is_window_active() {
                        window.remove_window();
                    }
                });
                MenuView {
                    menu,
                    _subscriptions: vec![dismiss, deactivate],
                }
            });
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
    .map_err(|error| error.to_string())
}
