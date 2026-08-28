mod text_input;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyBinding, Menu, MenuItem, Render,
    SharedString, SystemMenuType, Timer, TitlebarOptions, Window, WindowAppearance, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use std::time::Duration;
use text_input::{
    Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, TextInput,
};

actions!(gpui_settings_spike, [Quit, Tab, TabPrevious]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy)]
struct Tokens {
    canvas: gpui::Rgba,
    sidebar: gpui::Rgba,
    surface: gpui::Rgba,
    surface_selected: gpui::Rgba,
    border: gpui::Rgba,
    text: gpui::Rgba,
    text_muted: gpui::Rgba,
    accent: gpui::Rgba,
}

impl Tokens {
    fn dark() -> Self {
        Self {
            canvas: rgb(0x202329),
            sidebar: rgb(0x17191d),
            surface: rgb(0x292d35),
            surface_selected: rgb(0x303b4d),
            border: rgb(0x414754),
            text: rgb(0xf6f7f9),
            text_muted: rgb(0xaab1bd),
            accent: rgb(0x6da0ff),
        }
    }

    fn light() -> Self {
        Self {
            canvas: rgb(0xf3f4f6),
            sidebar: rgb(0xe6e8ec),
            surface: rgb(0xffffff),
            surface_selected: rgb(0xdce7fa),
            border: rgb(0xc5cad2),
            text: rgb(0x1d2129),
            text_muted: rgb(0x5d6571),
            accent: rgb(0x235ebd),
        }
    }
}

struct SettingsWindow {
    selected_section: SharedString,
    theme_mode: ThemeMode,
    theme_focus: Vec<FocusHandle>,
    root_focus: FocusHandle,
    model_name: gpui::Entity<TextInput>,
}

impl SettingsWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dark_theme = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let model_name = cx.new(|cx| TextInput::new(cx, dark_theme));

        cx.observe_window_appearance(window, |this, window, cx| {
            if this.theme_mode == ThemeMode::System {
                let dark_theme = matches!(
                    window.appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                );
                this.model_name.update(cx, |input, _| {
                    input.set_dark_theme(dark_theme);
                });
            }
            window.refresh();
            cx.notify();
        })
        .detach();
        let theme_focus = (2..=4)
            .map(|index| cx.focus_handle().tab_index(index).tab_stop(true))
            .collect();

        Self {
            selected_section: "Appearance".into(),
            theme_mode: ThemeMode::System,
            theme_focus,
            root_focus: cx.focus_handle(),
            model_name,
        }
    }

    fn resolved_dark(&self, window: &Window) -> bool {
        match self.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }

    fn on_tab(&mut self, _: &Tab, window: &mut Window, _: &mut Context<Self>) {
        window.focus_next();
    }

    fn on_tab_previous(&mut self, _: &TabPrevious, window: &mut Window, _: &mut Context<Self>) {
        window.focus_prev();
    }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = if self.resolved_dark(window) {
            Tokens::dark()
        } else {
            Tokens::light()
        };
        let system_appearance = match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => "Dark",
            WindowAppearance::Light | WindowAppearance::VibrantLight => "Light",
        };
        let character_count = self.model_name.read(cx).content().chars().count();

        let sidebar = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(180.0))
            .p_4()
            .bg(tokens.sidebar)
            .child(div().text_xl().text_color(tokens.text).child("BongoCat"))
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.text_muted)
                    .child("Native settings"),
            )
            .child(div().h(px(12.0)))
            .child(
                div()
                    .rounded_md()
                    .bg(tokens.surface_selected)
                    .p_3()
                    .text_color(tokens.text)
                    .child(self.selected_section.clone()),
            )
            .child(div().p_3().text_color(tokens.text_muted).child("Models"))
            .child(div().p_3().text_color(tokens.text_muted).child("Shortcuts"));

        let theme_options = div().flex().gap_2().children(
            ThemeMode::ALL
                .into_iter()
                .zip(self.theme_focus.clone())
                .map(|(mode, focus)| {
                    let selected = self.theme_mode == mode;
                    let focused = focus.is_focused(window);
                    div()
                        .id(mode.label())
                        .track_focus(&focus)
                        .tab_index((mode as isize) + 2)
                        .flex_1()
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_color(if focused || selected {
                            tokens.accent
                        } else {
                            tokens.border
                        })
                        .bg(if selected {
                            tokens.surface_selected
                        } else {
                            tokens.surface
                        })
                        .text_color(if selected { tokens.accent } else { tokens.text })
                        .hover(|style| style.bg(tokens.surface_selected).cursor_pointer())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            window.focus(&focus);
                            this.theme_mode = mode;
                            this.model_name.update(cx, |input, _| {
                                input.set_dark_theme(this.resolved_dark(window));
                            });
                            cx.notify();
                        }))
                        .child(mode.label())
                }),
        );

        let content = div()
            .flex()
            .flex_col()
            .gap_4()
            .flex_1()
            .p_8()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            .child(div().text_2xl().child("Appearance"))
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.text_muted)
                    .child("GPUI interaction and platform behavior probe"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .border_1()
                    .border_color(tokens.border)
                    .rounded_md()
                    .p_4()
                    .bg(tokens.surface)
                    .child(div().child("Theme"))
                    .child(theme_options)
                    .child(
                        div()
                            .text_sm()
                            .text_color(tokens.text_muted)
                            .child(format!("System appearance: {system_appearance}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_1()
                    .border_color(tokens.border)
                    .rounded_md()
                    .p_4()
                    .bg(tokens.surface)
                    .child(div().child("Model display name"))
                    .child(self.model_name.clone())
                    .child(div().text_sm().text_color(tokens.text_muted).child(format!(
                        "{character_count} characters · supports selection and clipboard"
                    ))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.text_muted)
                    .child("Tab and Shift-Tab move focus; focused controls use an accent border."),
            );

        div()
            .id("settings-root")
            .track_focus(&self.root_focus)
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_tab_previous))
            .flex()
            .size_full()
            .child(sidebar)
            .child(content)
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
        |window, cx| cx.new(|cx| SettingsWindow::new(window, cx)),
    );

    match result {
        Ok(window) => {
            window
                .update(cx, |settings, window, cx| {
                    window.focus(&settings.model_name.focus_handle(cx));
                })
                .ok();
            println!("gpui-settings-spike: window opened");
        }
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
        cx.bind_keys([
            KeyBinding::new("tab", Tab, None),
            KeyBinding::new("shift-tab", TabPrevious, None),
            KeyBinding::new("backspace", Backspace, Some("SettingsTextInput")),
            KeyBinding::new("delete", Delete, Some("SettingsTextInput")),
            KeyBinding::new("left", Left, Some("SettingsTextInput")),
            KeyBinding::new("right", Right, Some("SettingsTextInput")),
            KeyBinding::new("shift-left", SelectLeft, Some("SettingsTextInput")),
            KeyBinding::new("shift-right", SelectRight, Some("SettingsTextInput")),
            KeyBinding::new("home", Home, Some("SettingsTextInput")),
            KeyBinding::new("end", End, Some("SettingsTextInput")),
        ]);
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-a", SelectAll, Some("SettingsTextInput")),
            KeyBinding::new("cmd-v", Paste, Some("SettingsTextInput")),
            KeyBinding::new("cmd-c", Copy, Some("SettingsTextInput")),
            KeyBinding::new("cmd-x", Cut, Some("SettingsTextInput")),
        ]);
        #[cfg(target_os = "windows")]
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-a", SelectAll, Some("SettingsTextInput")),
            KeyBinding::new("ctrl-v", Paste, Some("SettingsTextInput")),
            KeyBinding::new("ctrl-c", Copy, Some("SettingsTextInput")),
            KeyBinding::new("ctrl-x", Cut, Some("SettingsTextInput")),
        ]);
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
