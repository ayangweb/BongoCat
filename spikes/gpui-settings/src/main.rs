use gpui::{
    App, Application, Bounds, Context, KeyBinding, Menu, MenuItem, Render, SharedString,
    SystemMenuType, Timer, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, rgb, size,
};
use std::time::Duration;

actions!(gpui_settings_spike, [Quit]);

struct SettingsWindow {
    selected_section: SharedString,
}

fn auto_quit_delay() -> Option<Duration> {
    let argument = std::env::args()
        .skip_while(|argument| argument != "--auto-quit-ms")
        .nth(1);
    let value = argument.or_else(|| std::env::var("BONGOCAT_SPIKE_AUTO_QUIT_MS").ok())?;

    match value.parse::<u64>() {
        Ok(milliseconds) => Some(Duration::from_millis(milliseconds)),
        Err(error) => {
            eprintln!("invalid auto-quit duration: {error}");
            None
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(180.0))
            .p_4()
            .bg(rgb(0x17191d))
            .child(div().text_xl().text_color(rgb(0xf3f4f6)).child("BongoCat"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x9ca3af))
                    .child("Native settings"),
            )
            .child(div().h(px(12.0)))
            .child(
                div()
                    .rounded_md()
                    .bg(rgb(0x2b313b))
                    .p_3()
                    .text_color(rgb(0xffffff))
                    .child(self.selected_section.clone()),
            )
            .child(div().p_3().text_color(rgb(0x9ca3af)).child("Models"))
            .child(div().p_3().text_color(rgb(0x9ca3af)).child("Shortcuts"));

        let content = div()
            .flex()
            .flex_col()
            .gap_4()
            .flex_1()
            .p_8()
            .bg(rgb(0x22252b))
            .child(
                div()
                    .text_2xl()
                    .text_color(rgb(0xf9fafb))
                    .child("Appearance"),
            )
            .child(div().text_base().text_color(rgb(0xc4c9d4)).child(
                "A small GPUI surface for validating layout, typography, and focus behavior.",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_1()
                    .border_color(rgb(0x3b414d))
                    .rounded_md()
                    .p_4()
                    .child(div().text_color(rgb(0xffffff)).child("Theme"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x9ca3af))
                            .child("System default"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_1()
                    .border_color(rgb(0x3b414d))
                    .rounded_md()
                    .p_4()
                    .child(div().text_color(rgb(0xffffff)).child("Overlay"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x9ca3af))
                            .child("The Live2D overlay is owned by the platform renderer."),
                    ),
            );

        div().flex().size_full().child(sidebar).child(content)
    }
}

fn open_settings_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(760.0), px(520.0)), cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("BongoCat Settings".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |_, cx| {
            cx.new(|_| SettingsWindow {
                selected_section: "Appearance".into(),
            })
        },
    );

    match result {
        Ok(_) => println!("gpui-settings-spike: window opened"),
        Err(error) => eprintln!("gpui-settings-spike: failed to open window: {error:#}"),
    }
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn main() {
    let auto_quit_delay = auto_quit_delay();
    let application = Application::new();

    application.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_settings_window(cx);
        }
        cx.activate(true);
    });

    application.run(move |cx: &mut App| {
        cx.on_app_quit(|_| async {
            println!("gpui-settings-spike: stopped");
        })
        .detach();

        cx.on_action(quit);
        #[cfg(target_os = "macos")]
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        #[cfg(target_os = "windows")]
        cx.bind_keys([KeyBinding::new("ctrl-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "BongoCat GPUI Spike".into(),
            items: vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit BongoCat GPUI Spike", Quit),
            ],
        }]);

        open_settings_window(cx);
        cx.activate(true);

        if let Some(delay) = auto_quit_delay {
            cx.spawn(async move |cx| {
                Timer::after(delay).await;
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
}
