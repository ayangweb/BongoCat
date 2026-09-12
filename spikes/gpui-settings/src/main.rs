mod accessibility;
#[cfg(target_os = "macos")]
mod macos_menu;
mod platform_ui_probe;
mod runtime_bridge;
mod text_input;

use accessibility::{
    AccessibilityAction, AccessibilityBridge, AccessibilityFocus, AccessibilitySnapshot,
    AccessibilityTheme,
};
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyBinding, Menu, MenuItem, Render,
    SharedString, SystemMenuType, Timer, TitlebarOptions, Window, WindowAppearance, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rgb, rgba, size,
};
use runtime_bridge::{RuntimeBridge, RuntimeProbeMode, RuntimeSnapshot, run_runtime};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use text_input::{
    Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, TextInput,
};

actions!(
    gpui_settings_spike,
    [
        Quit,
        HideApplication,
        HideOtherApplications,
        ShowAllApplications,
        MinimizeWindow,
        ZoomWindow,
        Tab,
        TabPrevious,
        ActivateFocused,
        DismissDialog
    ]
);

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

#[derive(Clone, Default)]
struct UiProbeState {
    failed: Arc<AtomicBool>,
    tooltip_seen: Arc<AtomicBool>,
    tooltip_hovered: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Default)]
struct UiProbeOptions {
    menu: bool,
    tooltip: bool,
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
    refresh_focus: FocusHandle,
    reset_focus: FocusHandle,
    dialog_cancel_focus: FocusHandle,
    dialog_confirm_focus: FocusHandle,
    reset_dialog_open: bool,
    model_name: gpui::Entity<TextInput>,
    runtime_bridge: RuntimeBridge,
    runtime_snapshot: Option<RuntimeSnapshot>,
    runtime_request_in_flight: bool,
    runtime_error: Option<SharedString>,
    accessibility: AccessibilityBridge,
    accessibility_actions: Option<async_channel::Receiver<AccessibilityAction>>,
    ui_probe: UiProbeState,
    tooltip_probe_enabled: bool,
}

impl SettingsWindow {
    fn new(
        window: &mut Window,
        runtime_bridge: RuntimeBridge,
        accessibility: AccessibilityBridge,
        accessibility_actions: async_channel::Receiver<AccessibilityAction>,
        ui_probe: UiProbeState,
        tooltip_probe_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Self {
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
            refresh_focus: cx.focus_handle().tab_index(5).tab_stop(true),
            reset_focus: cx.focus_handle().tab_index(6).tab_stop(true),
            dialog_cancel_focus: cx.focus_handle().tab_index(7).tab_stop(true),
            dialog_confirm_focus: cx.focus_handle().tab_index(8).tab_stop(true),
            reset_dialog_open: false,
            model_name,
            runtime_bridge,
            runtime_snapshot: None,
            runtime_request_in_flight: false,
            runtime_error: None,
            accessibility,
            accessibility_actions: Some(accessibility_actions),
            ui_probe,
            tooltip_probe_enabled,
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
        if self.reset_dialog_open && self.dialog_confirm_focus.is_focused(window) {
            window.focus(&self.dialog_cancel_focus);
            return;
        }
        window.focus_next();
    }

    fn on_tab_previous(&mut self, _: &TabPrevious, window: &mut Window, _: &mut Context<Self>) {
        if self.reset_dialog_open && self.dialog_cancel_focus.is_focused(window) {
            window.focus(&self.dialog_confirm_focus);
            return;
        }
        window.focus_prev();
    }

    fn on_activate_focused(
        &mut self,
        _: &ActivateFocused,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reset_dialog_open {
            if self.dialog_confirm_focus.is_focused(window) {
                self.confirm_reset(window, cx);
            } else if self.dialog_cancel_focus.is_focused(window) {
                self.close_reset_dialog(window, cx);
            }
            return;
        }
        if let Some(mode) = ThemeMode::ALL
            .into_iter()
            .zip(&self.theme_focus)
            .find_map(|(mode, focus)| focus.is_focused(window).then_some(mode))
        {
            self.set_theme_mode(mode, window, cx);
        } else if self.refresh_focus.is_focused(window) {
            self.request_runtime_snapshot(cx);
        } else if self.reset_focus.is_focused(window) {
            self.open_reset_dialog(window, cx);
        }
    }

    fn on_dismiss_dialog(
        &mut self,
        _: &DismissDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reset_dialog_open {
            self.close_reset_dialog(window, cx);
        }
    }

    fn on_minimize_window(
        &mut self,
        _: &MinimizeWindow,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.minimize_window();
    }

    fn on_zoom_window(&mut self, _: &ZoomWindow, window: &mut Window, _: &mut Context<Self>) {
        window.zoom_window();
    }

    fn open_reset_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_dialog_open = true;
        window.focus(&self.dialog_cancel_focus);
        cx.notify();
    }

    fn close_reset_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_dialog_open = false;
        window.focus(&self.reset_focus);
        cx.notify();
    }

    fn confirm_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = ThemeMode::System;
        self.model_name.update(cx, |input, cx| {
            input.set_content("", window, cx);
            input.set_dark_theme(self.resolved_dark(window));
        });
        self.close_reset_dialog(window, cx);
    }

    fn set_theme_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = mode;
        self.model_name.update(cx, |input, _| {
            input.set_dark_theme(self.resolved_dark(window));
        });
        cx.notify();
    }

    fn start_accessibility_action_task(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(receiver) = self.accessibility_actions.take() else {
            return;
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            while let Ok(action) = receiver.recv().await {
                let updated = window_handle.update(cx, |_, window, cx| {
                    this.update(cx, |settings, cx| {
                        settings.apply_accessibility_action(action, window, cx);
                    })
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    #[cfg(target_os = "macos")]
    fn start_menu_probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        const PROBE_TEXT: &str = "Menu Probe";

        self.model_name.update(cx, |input, cx| {
            input.set_content(PROBE_TEXT, window, cx);
        });
        window.focus(&self.model_name.focus_handle(cx));
        if let Err(error) = macos_menu::verify_structure() {
            self.fail_ui_probe(error, cx);
            return;
        }

        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(100)).await;
            let focused = window_handle.update(cx, |_, window, cx| {
                this.update(cx, |settings, cx| {
                    window.focus(&settings.model_name.focus_handle(cx));
                })
            });
            if !matches!(focused, Ok(Ok(()))) {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe("settings window closed before menu probe".into(), cx);
                });
                return;
            }
            Timer::after(Duration::from_millis(100)).await;
            let select_all = match cx.update(|_| {
                macos_menu::schedule_perform("Edit", "Select All", Duration::from_millis(10))
            }) {
                Ok(result) => result,
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = select_all {
                let _ = this.update(cx, |settings, cx| settings.fail_ui_probe(error, cx));
                return;
            }
            Timer::after(Duration::from_millis(100)).await;
            let cut = match cx
                .update(|_| macos_menu::schedule_perform("Edit", "Cut", Duration::from_millis(10)))
            {
                Ok(result) => result,
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = cut {
                let _ = this.update(cx, |settings, cx| settings.fail_ui_probe(error, cx));
                return;
            }
            Timer::after(Duration::from_millis(100)).await;
            let cut_state = window_handle
                .update(cx, |_, _, cx| {
                    this.update(cx, |settings, cx| {
                        (
                            settings.model_name.read(cx).content().is_empty(),
                            cx.read_from_clipboard()
                                .and_then(|item| item.text())
                                .as_deref()
                                == Some(PROBE_TEXT),
                        )
                    })
                    .unwrap_or((false, false))
                })
                .unwrap_or((false, false));
            if cut_state != (true, true) {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe(
                        format!(
                            "Cut result text_empty={} clipboard_matches={}",
                            cut_state.0, cut_state.1
                        ),
                        cx,
                    );
                });
                return;
            }
            let paste = match cx.update(|_| {
                macos_menu::schedule_perform("Edit", "Paste", Duration::from_millis(10))
            }) {
                Ok(result) => result,
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = paste {
                let _ = this.update(cx, |settings, cx| settings.fail_ui_probe(error, cx));
                return;
            }
            Timer::after(Duration::from_millis(100)).await;
            let paste_verified = window_handle
                .update(cx, |_, _, cx| {
                    this.update(cx, |settings, cx| {
                        settings.model_name.read(cx).content() == PROBE_TEXT
                    })
                    .unwrap_or(false)
                })
                .unwrap_or(false);
            if !paste_verified {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe("Paste did not restore text".into(), cx);
                });
                return;
            }
            println!("gpui-settings-spike: native menu edit actions verified");
        })
        .detach();
    }

    fn start_tooltip_probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui_probe.tooltip_seen.store(false, Ordering::Release);
        self.ui_probe
            .tooltip_hovered
            .store(false, Ordering::Release);
        let window_handle = window.window_handle();

        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(100)).await;
            let mut hovered_y = None;
            for y in (360..=440).step_by(2) {
                let posted = window_handle.update(cx, |_, window, cx| {
                    let _ = cx;
                    platform_ui_probe::post_mouse_move(window, 580, y, 520)
                });
                if !matches!(posted, Ok(Ok(()))) {
                    let _ = this.update(cx, |settings, cx| {
                        settings.fail_ui_probe(
                            "settings window closed before tooltip hover".into(),
                            cx,
                        );
                    });
                    return;
                }
                Timer::after(Duration::from_millis(10)).await;
                let hovered = this
                    .update(cx, |settings, _| {
                        settings.ui_probe.tooltip_hovered.load(Ordering::Acquire)
                    })
                    .unwrap_or(false);
                if hovered {
                    hovered_y = Some(y);
                    break;
                }
            }
            if hovered_y.is_none() {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe("native mouse moves did not hit Reset".into(), cx);
                });
                return;
            }
            println!("gpui-settings-spike: native tooltip hover entered");

            // GPUI 0.2.2 intentionally waits 500 ms before constructing a tooltip.
            Timer::after(Duration::from_millis(650)).await;
            let visible = this
                .update(cx, |settings, _| {
                    settings.ui_probe.tooltip_seen.load(Ordering::Acquire)
                        && settings.ui_probe.tooltip_hovered.load(Ordering::Acquire)
                })
                .unwrap_or(false);
            if !visible {
                let _ = this.update(cx, |settings, cx| {
                    settings
                        .fail_ui_probe("tooltip was not built after the hover delay".into(), cx);
                });
                return;
            }

            let left = window_handle.update(cx, |_, window, cx| {
                let _ = cx;
                platform_ui_probe::post_mouse_move(window, 20, 20, 520)
            });
            if !matches!(left, Ok(Ok(()))) {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe(
                        "settings window closed before tooltip hover exit".into(),
                        cx,
                    );
                });
                return;
            }
            Timer::after(Duration::from_millis(100)).await;
            let hover_cleared = this
                .update(cx, |settings, _| {
                    !settings.ui_probe.tooltip_hovered.load(Ordering::Acquire)
                })
                .unwrap_or(false);
            if !hover_cleared {
                let _ = this.update(cx, |settings, cx| {
                    settings.fail_ui_probe("tooltip hover state did not clear".into(), cx);
                });
                return;
            }
            println!("gpui-settings-spike: tooltip hover delay and exit verified");
        })
        .detach();
    }

    fn fail_ui_probe(&mut self, error: String, cx: &mut Context<Self>) {
        self.ui_probe.failed.store(true, Ordering::Release);
        eprintln!("gpui-settings-spike: UI probe failed: {error}");
        cx.quit();
    }

    fn apply_accessibility_action(
        &mut self,
        action: AccessibilityAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            AccessibilityAction::SelectTheme(theme) => {
                let mode = ThemeMode::from(theme);
                let focus = self.theme_focus[mode as usize].clone();
                window.focus(&focus);
                self.set_theme_mode(mode, window, cx);
            }
            AccessibilityAction::FocusTheme(theme) => {
                window.focus(&self.theme_focus[ThemeMode::from(theme) as usize]);
                cx.notify();
            }
            AccessibilityAction::FocusModelName => {
                window.focus(&self.model_name.focus_handle(cx));
                cx.notify();
            }
            AccessibilityAction::SetModelName(value) => {
                self.model_name.update(cx, |input, cx| {
                    input.set_content(&value, window, cx);
                });
            }
            AccessibilityAction::FocusRefresh => {
                window.focus(&self.refresh_focus);
                cx.notify();
            }
            AccessibilityAction::RefreshRuntime => self.request_runtime_snapshot(cx),
            AccessibilityAction::FocusReset => {
                window.focus(&self.reset_focus);
                cx.notify();
            }
            AccessibilityAction::OpenResetDialog => self.open_reset_dialog(window, cx),
            AccessibilityAction::FocusDialogCancel => {
                if self.reset_dialog_open {
                    window.focus(&self.dialog_cancel_focus);
                    cx.notify();
                }
            }
            AccessibilityAction::FocusDialogConfirm => {
                if self.reset_dialog_open {
                    window.focus(&self.dialog_confirm_focus);
                    cx.notify();
                }
            }
            AccessibilityAction::CancelReset => {
                if self.reset_dialog_open {
                    self.close_reset_dialog(window, cx);
                }
            }
            AccessibilityAction::ConfirmReset => {
                if self.reset_dialog_open {
                    self.confirm_reset(window, cx);
                }
            }
        }
        println!("gpui-settings-spike: accessibility action applied");
    }

    fn request_runtime_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.runtime_request_in_flight {
            return;
        }
        self.runtime_request_in_flight = true;
        self.runtime_error = None;
        cx.notify();
        let bridge = self.runtime_bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.read_snapshot().await;
            let _ = this.update(cx, |settings, cx| {
                settings.runtime_request_in_flight = false;
                match result {
                    Ok(snapshot) => {
                        if settings
                            .runtime_snapshot
                            .is_none_or(|current| snapshot.revision > current.revision)
                        {
                            settings.runtime_snapshot = Some(snapshot);
                        }
                        println!(
                            "gpui-settings-spike: runtime snapshot revision={}",
                            snapshot.revision
                        );
                    }
                    Err(error) => settings.runtime_error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl From<AccessibilityTheme> for ThemeMode {
    fn from(value: AccessibilityTheme) -> Self {
        match value {
            AccessibilityTheme::System => Self::System,
            AccessibilityTheme::Light => Self::Light,
            AccessibilityTheme::Dark => Self::Dark,
        }
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

fn runtime_probe_mode() -> RuntimeProbeMode {
    if std::env::args().any(|argument| argument == "--runtime-error-probe") {
        RuntimeProbeMode::DelayedErrorRecovery
    } else {
        RuntimeProbeMode::Normal
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
        let runtime_status = if let Some(error) = &self.runtime_error {
            error.clone()
        } else if self.runtime_request_in_flight {
            "Refreshing...".into()
        } else if let Some(snapshot) = self.runtime_snapshot {
            let health = match snapshot.health {
                runtime_bridge::RuntimeHealth::Ready => "Ready",
            };
            format!("Runtime {health} · revision {}", snapshot.revision).into()
        } else {
            "Runtime unavailable".into()
        };
        self.accessibility.update(AccessibilitySnapshot {
            selected_theme: self.theme_mode.label(),
            model_name: self.model_name.read(cx).content().to_string(),
            runtime_status: runtime_status.to_string(),
            runtime_busy: self.runtime_request_in_flight,
            runtime_error: self.runtime_error.is_some(),
            focus: if self.model_name.focus_handle(cx).is_focused(window) {
                AccessibilityFocus::ModelName
            } else if let Some((mode, _)) = ThemeMode::ALL
                .into_iter()
                .zip(&self.theme_focus)
                .find(|(_, focus)| focus.is_focused(window))
            {
                AccessibilityFocus::Theme(match mode {
                    ThemeMode::System => AccessibilityTheme::System,
                    ThemeMode::Light => AccessibilityTheme::Light,
                    ThemeMode::Dark => AccessibilityTheme::Dark,
                })
            } else if self.refresh_focus.is_focused(window) {
                AccessibilityFocus::Refresh
            } else if self.reset_focus.is_focused(window) {
                AccessibilityFocus::Reset
            } else if self.dialog_cancel_focus.is_focused(window) {
                AccessibilityFocus::DialogCancel
            } else if self.dialog_confirm_focus.is_focused(window) {
                AccessibilityFocus::DialogConfirm
            } else {
                AccessibilityFocus::Root
            },
            reset_dialog_open: self.reset_dialog_open,
        });

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
                        .key_context("SettingsButton")
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
                            this.set_theme_mode(mode, window, cx);
                        }))
                        .child(mode.label())
                }),
        );

        let reset_tooltip_tokens = tokens;
        let tooltip_probe_enabled = self.tooltip_probe_enabled;
        let tooltip_probe_seen = Arc::clone(&self.ui_probe.tooltip_seen);
        let tooltip_probe_hovered = Arc::clone(&self.ui_probe.tooltip_hovered);
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .p_6()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            .child(div().text_2xl().child("Appearance"))
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
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_1()
                    .border_color(tokens.border)
                    .rounded_md()
                    .p_4()
                    .bg(tokens.surface)
                    .child(div().min_w_0().flex_1().child(runtime_status))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap_2()
                            .child(
                                div()
                                    .id("reset-settings")
                                    .key_context("SettingsButton")
                                    .track_focus(&self.reset_focus)
                                    .tab_index(6)
                                    .w(px(88.0))
                                    .h(px(32.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if self.reset_focus.is_focused(window) {
                                        tokens.accent
                                    } else {
                                        tokens.border
                                    })
                                    .hover(|style| {
                                        style.bg(tokens.surface_selected).cursor_pointer()
                                    })
                                    .on_hover(move |hovered, _, _| {
                                        if tooltip_probe_enabled {
                                            tooltip_probe_hovered
                                                .store(*hovered, Ordering::Release);
                                        }
                                    })
                                    .tooltip(move |_, cx| {
                                        if tooltip_probe_enabled
                                            && !tooltip_probe_seen.swap(true, Ordering::AcqRel)
                                        {
                                            println!(
                                                "gpui-settings-spike: tooltip built after hover delay"
                                            );
                                        }
                                        cx.new(|_| SettingsTooltip {
                                            text: "Restore the settings shown in this spike".into(),
                                            tokens: reset_tooltip_tokens,
                                        })
                                        .into()
                                    })
                                    .on_click(cx.listener(|settings, _, window, cx| {
                                        settings.open_reset_dialog(window, cx);
                                    }))
                                    .child("Reset..."),
                            )
                            .child(
                                div()
                                    .id("refresh-runtime")
                                    .key_context("SettingsButton")
                                    .track_focus(&self.refresh_focus)
                                    .tab_index(5)
                                    .w(px(88.0))
                                    .h(px(32.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if self.refresh_focus.is_focused(window) {
                                        tokens.accent
                                    } else {
                                        tokens.border
                                    })
                                    .hover(|style| {
                                        style.bg(tokens.surface_selected).cursor_pointer()
                                    })
                                    .on_click(cx.listener(|settings, _, _, cx| {
                                        settings.request_runtime_snapshot(cx);
                                    }))
                                    .child("Refresh"),
                            ),
                    ),
            );

        let reset_dialog = self.reset_dialog_open.then(|| {
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000088))
                .child(
                    div()
                        .w(px(360.0))
                        .p_5()
                        .rounded_md()
                        .border_1()
                        .border_color(tokens.border)
                        .bg(tokens.surface)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().text_xl().child("Reset settings?"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(tokens.text_muted)
                                .child("Theme and model display name will return to defaults."),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .id("cancel-reset")
                                        .key_context("SettingsButton")
                                        .track_focus(&self.dialog_cancel_focus)
                                        .tab_index(7)
                                        .w(px(88.0))
                                        .h(px(32.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(
                                            if self.dialog_cancel_focus.is_focused(window) {
                                                tokens.accent
                                            } else {
                                                tokens.border
                                            },
                                        )
                                        .hover(|style| {
                                            style.bg(tokens.surface_selected).cursor_pointer()
                                        })
                                        .on_click(cx.listener(|settings, _, window, cx| {
                                            settings.close_reset_dialog(window, cx);
                                        }))
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("confirm-reset")
                                        .key_context("SettingsButton")
                                        .track_focus(&self.dialog_confirm_focus)
                                        .tab_index(8)
                                        .w(px(88.0))
                                        .h(px(32.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(tokens.accent)
                                        .bg(tokens.surface_selected)
                                        .text_color(tokens.accent)
                                        .hover(|style| style.cursor_pointer())
                                        .on_click(cx.listener(|settings, _, window, cx| {
                                            settings.confirm_reset(window, cx);
                                        }))
                                        .child("Reset"),
                                ),
                        ),
                )
        });

        div()
            .id("settings-root")
            .track_focus(&self.root_focus)
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_tab_previous))
            .on_action(cx.listener(Self::on_activate_focused))
            .on_action(cx.listener(Self::on_dismiss_dialog))
            .on_action(cx.listener(Self::on_minimize_window))
            .on_action(cx.listener(Self::on_zoom_window))
            .relative()
            .flex()
            .size_full()
            .child(sidebar)
            .child(content)
            .children(reset_dialog)
    }
}

struct SettingsTooltip {
    text: SharedString,
    tokens: Tokens,
}

impl Render for SettingsTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(220.0))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(self.tokens.border)
            .bg(self.tokens.surface)
            .text_sm()
            .text_color(self.tokens.text)
            .child(self.text.clone())
    }
}

fn open_settings_window(
    cx: &mut App,
    runtime_bridge: RuntimeBridge,
    startup_started_at: Option<Instant>,
    probes: UiProbeOptions,
    ui_probe: UiProbeState,
) -> bool {
    #[cfg(not(target_os = "macos"))]
    let _ = probes.menu;

    let bounds = Bounds::centered(None, size(px(760.0), px(520.0)), cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("BongoCat Settings".into()),
                ..Default::default()
            }),
            focus: false,
            show: false,
            ..Default::default()
        },
        move |window, cx| {
            let (accessibility, accessibility_actions) = AccessibilityBridge::attach(window)
                .expect("attach AccessKit before the GPUI window is shown");
            let settings = cx.new(|cx| {
                SettingsWindow::new(
                    window,
                    runtime_bridge,
                    accessibility,
                    accessibility_actions,
                    ui_probe,
                    probes.tooltip,
                    cx,
                )
            });
            window.activate_window();
            settings
        },
    );

    match result {
        Ok(window) => {
            let setup = window
                .update(cx, |settings, window, cx| {
                    settings.start_accessibility_action_task(window, cx);
                    #[cfg(target_os = "macos")]
                    if probes.menu {
                        settings.start_menu_probe(window, cx);
                    }
                    if probes.tooltip {
                        settings.start_tooltip_probe(window, cx);
                    }
                    window.focus(&settings.model_name.focus_handle(cx));
                    settings.request_runtime_snapshot(cx);
                    if let Some(started_at) = startup_started_at {
                        window.on_next_frame(move |window, _| {
                            println!(
                                "gpui-settings-spike: first frame elapsed_ms={:.3} scale_factor={:.3}",
                                started_at.elapsed().as_secs_f64() * 1_000.0,
                                window.scale_factor(),
                            );
                        });
                    }
                    settings.accessibility.verify_platform_tree()
                })
                .map_err(|error| error.to_string())
                .and_then(|result| result);
            if let Err(error) = setup {
                eprintln!("gpui-settings-spike: accessibility setup failed: {error}");
                return false;
            }
            println!("gpui-settings-spike: window opened");
            true
        }
        Err(error) => {
            eprintln!("gpui-settings-spike: failed to open window: {error:#}");
            false
        }
    }
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn hide_application(_: &HideApplication, cx: &mut App) {
    cx.hide();
}

fn hide_other_applications(_: &HideOtherApplications, cx: &mut App) {
    cx.hide_other_apps();
}

fn show_all_applications(_: &ShowAllApplications, cx: &mut App) {
    cx.unhide_other_apps();
}

fn menu_probe_enabled() -> bool {
    std::env::args().any(|argument| argument == "--menu-probe")
}

fn tooltip_probe_enabled() -> bool {
    std::env::args().any(|argument| argument == "--tooltip-probe")
}

fn main() {
    let startup_started_at = Instant::now();
    let auto_quit_delay = auto_quit_delay();
    let runtime_probe_mode = runtime_probe_mode();
    let menu_probe_enabled = menu_probe_enabled();
    let tooltip_probe_enabled = tooltip_probe_enabled();
    let application = Application::new();
    let (runtime_bridge, runtime_commands) = RuntimeBridge::new();
    let reopen_bridge = runtime_bridge.clone();
    let window_open_failed = Arc::new(AtomicBool::new(false));
    let reopen_window_failed = Arc::clone(&window_open_failed);
    let startup_window_failed = Arc::clone(&window_open_failed);
    let quit_window_failed = Arc::clone(&window_open_failed);
    let ui_probe = UiProbeState::default();
    let reopen_ui_probe = ui_probe.clone();
    let startup_ui_probe = ui_probe.clone();
    let quit_ui_probe = ui_probe.clone();
    let after_run_ui_probe = ui_probe.clone();

    application.on_reopen(move |cx| {
        if cx.windows().is_empty()
            && !open_settings_window(
                cx,
                reopen_bridge.clone(),
                None,
                UiProbeOptions::default(),
                reopen_ui_probe.clone(),
            )
        {
            reopen_window_failed.store(true, Ordering::Release);
            cx.quit();
            return;
        }
        cx.activate(true);
    });

    application.run(move |cx: &mut App| {
        let executor = cx.background_executor().clone();
        let delay_executor = executor.clone();
        executor
            .spawn(run_runtime(
                runtime_commands,
                runtime_probe_mode,
                move |delay| delay_executor.timer(delay),
            ))
            .detach();
        let quit_bridge = runtime_bridge.clone();
        cx.on_app_quit(move |_| {
            let bridge = quit_bridge.clone();
            let window_failed = Arc::clone(&quit_window_failed);
            let ui_probe = quit_ui_probe.clone();
            async move {
                if let Err(error) = bridge.shutdown().await {
                    eprintln!("gpui-settings-spike: runtime shutdown failed: {error}");
                }
                println!("gpui-settings-spike: runtime stopped");
                println!("gpui-settings-spike: stopped");
                if window_failed.load(Ordering::Acquire) {
                    std::process::exit(1);
                }
                if ui_probe.failed.load(Ordering::Acquire) {
                    std::process::exit(1);
                }
            }
        })
        .detach();

        cx.on_action(quit);
        cx.on_action(hide_application);
        cx.on_action(hide_other_applications);
        cx.on_action(show_all_applications);
        cx.bind_keys([
            KeyBinding::new("tab", Tab, None),
            KeyBinding::new("shift-tab", TabPrevious, None),
            KeyBinding::new("enter", ActivateFocused, Some("SettingsButton")),
            KeyBinding::new("space", ActivateFocused, Some("SettingsButton")),
            KeyBinding::new("escape", DismissDialog, None),
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
            KeyBinding::new("cmd-h", HideApplication, None),
            KeyBinding::new("cmd-alt-h", HideOtherApplications, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
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
        cx.set_menus(vec![
            Menu {
                name: "BongoCat GPUI Spike".into(),
                items: vec![
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Hide BongoCat GPUI Spike", HideApplication),
                    MenuItem::action("Hide Others", HideOtherApplications),
                    MenuItem::action("Show All", ShowAllApplications),
                    MenuItem::separator(),
                    MenuItem::action("Quit BongoCat GPUI Spike", Quit),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Cut", Cut),
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Paste", Paste),
                    MenuItem::action("Select All", SelectAll),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("Minimize", MinimizeWindow),
                    MenuItem::action("Zoom", ZoomWindow),
                ],
            },
        ]);

        if !open_settings_window(
            cx,
            runtime_bridge.clone(),
            Some(startup_started_at),
            UiProbeOptions {
                menu: menu_probe_enabled,
                tooltip: tooltip_probe_enabled,
            },
            startup_ui_probe.clone(),
        ) {
            startup_window_failed.store(true, Ordering::Release);
            cx.quit();
            return;
        }
        cx.activate(true);

        if let Some(delay) = auto_quit_delay {
            cx.spawn(async move |cx| {
                Timer::after(delay).await;
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });

    if window_open_failed.load(Ordering::Acquire)
        || after_run_ui_probe.failed.load(Ordering::Acquire)
    {
        std::process::exit(1);
    }
}
