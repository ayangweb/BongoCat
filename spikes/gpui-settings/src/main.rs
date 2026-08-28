use gpui::{
    App, Application, Bounds, Context, Render, SharedString, Timer, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use std::time::Duration;

struct SettingsWindow {
    selected_section: SharedString,
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

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.on_app_quit(|_| async {
            println!("gpui-settings-spike: stopped");
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(760.0), px(520.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| SettingsWindow {
                    selected_section: "Appearance".into(),
                })
            },
        )
        .expect("open GPUI settings window");
        println!("gpui-settings-spike: window opened");
        cx.activate(true);

        if let Ok(value) = std::env::var("BONGOCAT_SPIKE_AUTO_QUIT_MS") {
            match value.parse::<u64>() {
                Ok(milliseconds) => {
                    cx.spawn(async move |cx| {
                        Timer::after(Duration::from_millis(milliseconds)).await;
                        let _ = cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
                Err(error) => eprintln!("invalid BONGOCAT_SPIKE_AUTO_QUIT_MS: {error}"),
            }
        }
    });
}
