use crate::{RuntimeHealth, SettingsClient, SettingsError, SettingsSnapshot};
use gpui::{
    App, Bounds, Context, FocusHandle, Render, SharedString, TitlebarOptions, Window,
    WindowAppearance, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};

const WINDOW_WIDTH: f32 = 680.0;
const WINDOW_HEIGHT: f32 = 440.0;

#[derive(Clone, Copy)]
struct Tokens {
    canvas: gpui::Rgba,
    sidebar: gpui::Rgba,
    surface: gpui::Rgba,
    selected: gpui::Rgba,
    border: gpui::Rgba,
    text: gpui::Rgba,
    muted: gpui::Rgba,
    accent: gpui::Rgba,
    danger: gpui::Rgba,
}

impl Tokens {
    fn for_window(window: &Window) -> Self {
        if matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ) {
            Self {
                canvas: rgb(0x1f2227),
                sidebar: rgb(0x181a1e),
                surface: rgb(0x292d33),
                selected: rgb(0x343b45),
                border: rgb(0x414750),
                text: rgb(0xf4f5f7),
                muted: rgb(0xa8afb9),
                accent: rgb(0x55b9a6),
                danger: rgb(0xff8c82),
            }
        } else {
            Self {
                canvas: rgb(0xf4f5f7),
                sidebar: rgb(0xe7e9ed),
                surface: rgb(0xffffff),
                selected: rgb(0xdceeea),
                border: rgb(0xc8cdd4),
                text: rgb(0x20242a),
                muted: rgb(0x626b76),
                accent: rgb(0x167563),
                danger: rgb(0xb42318),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingOperation {
    Refresh,
    OverlayVisibility,
    MotionAudio,
}

pub struct SettingsView {
    client: SettingsClient,
    snapshot: Option<SettingsSnapshot>,
    pending: Option<PendingOperation>,
    error: Option<SettingsError>,
    overlay_focus: FocusHandle,
    audio_focus: FocusHandle,
    refresh_focus: FocusHandle,
    quit_focus: FocusHandle,
}

impl SettingsView {
    fn new(client: SettingsClient, cx: &mut Context<Self>) -> Self {
        Self {
            client,
            snapshot: None,
            pending: None,
            error: None,
            overlay_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            audio_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            refresh_focus: cx.focus_handle().tab_index(3).tab_stop(true),
            quit_focus: cx.focus_handle().tab_index(4).tab_stop(true),
        }
    }

    pub fn report_service_error(&mut self, error: SettingsError, cx: &mut Context<Self>) {
        self.pending = None;
        self.error = Some(error);
        cx.notify();
    }

    pub fn snapshot_revision(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.revision)
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.start_request(PendingOperation::Refresh, None, cx);
    }

    fn set_overlay_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::OverlayVisibility,
            Some(SettingValue::OverlayVisible(visible)),
            cx,
        );
    }

    fn set_motion_audio_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.start_request(
            PendingOperation::MotionAudio,
            Some(SettingValue::MotionAudioEnabled(enabled)),
            cx,
        );
    }

    fn start_request(
        &mut self,
        operation: PendingOperation,
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(operation);
        self.error = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = match value {
                None => client.read_snapshot().await,
                Some(SettingValue::OverlayVisible(visible)) => {
                    client.set_overlay_visible(visible).await
                }
                Some(SettingValue::MotionAudioEnabled(enabled)) => {
                    client.set_motion_audio_enabled(enabled).await
                }
            };
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                match result {
                    Ok(snapshot)
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision) =>
                    {
                        view.snapshot = Some(snapshot);
                    }
                    Ok(_) => {}
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[derive(Clone, Copy)]
enum SettingValue {
    OverlayVisible(bool),
    MotionAudioEnabled(bool),
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::for_window(window);
        let snapshot = self.snapshot.clone();
        let overlay_visible = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.overlay_visible);
        let motion_audio_enabled = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.motion_audio_enabled);
        let disabled = self.pending.is_some() || snapshot.is_none();
        let status: SharedString = match (&self.error, self.pending, &snapshot) {
            (Some(error), _, _) => error.to_string().into(),
            (_, Some(_), _) => "Saving...".into(),
            (_, None, Some(snapshot)) => {
                let health = match snapshot.runtime_health {
                    RuntimeHealth::Starting => "Starting",
                    RuntimeHealth::Ready => "Ready",
                    RuntimeHealth::Degraded => "Degraded",
                    RuntimeHealth::Stopped => "Stopped",
                };
                format!("Runtime {health} · revision {}", snapshot.revision).into()
            }
            _ => "Connecting to runtime...".into(),
        };
        let active_model: SharedString = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.active_model_id.clone())
            .unwrap_or_else(|| "No active model".to_owned())
            .into();

        let sidebar = div()
            .w(px(164.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(tokens.sidebar)
            .child(div().text_xl().text_color(tokens.text).child("BongoCat"))
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.muted)
                    .child("Native settings"),
            )
            .child(div().h(px(12.0)))
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(tokens.selected)
                    .text_color(tokens.text)
                    .child("General"),
            )
            .child(div().p_3().text_color(tokens.muted).child("Models"))
            .child(div().p_3().text_color(tokens.muted).child("Shortcuts"))
            .child(div().p_3().text_color(tokens.muted).child("Diagnostics"));

        let overlay_row = setting_row(
            "Show desktop cat",
            "Keep the Live2D overlay visible",
            SettingRowState {
                enabled: overlay_visible,
                disabled,
                tab_index: 1,
            },
            &self.overlay_focus,
            window,
            tokens,
        )
        .id("overlay-visible")
        .on_click(cx.listener(move |view, _, window, cx| {
            if !disabled {
                window.focus(&view.overlay_focus);
                view.set_overlay_visible(!overlay_visible, cx);
            }
        }));

        let audio_row = setting_row(
            "Motion audio",
            "Play audio attached to model motions",
            SettingRowState {
                enabled: motion_audio_enabled,
                disabled,
                tab_index: 2,
            },
            &self.audio_focus,
            window,
            tokens,
        )
        .id("motion-audio")
        .on_click(cx.listener(move |view, _, window, cx| {
            if !disabled {
                window.focus(&view.audio_focus);
                view.set_motion_audio_enabled(!motion_audio_enabled, cx);
            }
        }));

        let content = div()
            .min_w_0()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            .child(div().text_2xl().child("General"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .rounded_md()
                    .border_1()
                    .border_color(tokens.border)
                    .bg(tokens.surface)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().child("Active model"))
                            .child(div().text_sm().text_color(tokens.muted).child(active_model)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(if self.error.is_some() {
                                tokens.danger
                            } else {
                                tokens.muted
                            })
                            .child(status),
                    ),
            )
            .child(overlay_row)
            .child(audio_row)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        command_button(
                            "Refresh",
                            &self.refresh_focus,
                            3,
                            window,
                            tokens,
                            self.pending.is_some(),
                        )
                        .id("refresh-settings")
                        .on_click(cx.listener(|view, _, window, cx| {
                            if view.pending.is_none() {
                                window.focus(&view.refresh_focus);
                                view.refresh(cx);
                            }
                        })),
                    )
                    .child(
                        command_button("Quit", &self.quit_focus, 4, window, tokens, false)
                            .id("quit-application")
                            .on_click(cx.listener(|view, _, window, cx| {
                                window.focus(&view.quit_focus);
                                cx.quit();
                            })),
                    ),
            );

        div()
            .id("bongocat-settings-root")
            .size_full()
            .flex()
            .child(sidebar)
            .child(content)
    }
}

fn setting_row(
    title: &'static str,
    description: &'static str,
    state: SettingRowState,
    focus: &FocusHandle,
    window: &Window,
    tokens: Tokens,
) -> gpui::Div {
    let focused = focus.is_focused(window);
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(state.tab_index)
        .flex()
        .items_center()
        .justify_between()
        .p_4()
        .rounded_md()
        .border_1()
        .border_color(if focused {
            tokens.accent
        } else {
            tokens.border
        })
        .bg(tokens.surface)
        .text_color(if state.disabled {
            tokens.muted
        } else {
            tokens.text
        })
        .when(!state.disabled, |row| {
            row.hover(|style| style.bg(tokens.selected).cursor_pointer())
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().child(title))
                .child(div().text_sm().text_color(tokens.muted).child(description)),
        )
        .child(
            div()
                .w(px(38.0))
                .h(px(22.0))
                .flex_none()
                .flex()
                .items_center()
                .p(px(3.0))
                .rounded(px(11.0))
                .bg(if state.enabled && !state.disabled {
                    tokens.accent
                } else {
                    tokens.border
                })
                .when(state.enabled, |toggle| toggle.justify_end())
                .when(!state.enabled, |toggle| toggle.justify_start())
                .child(
                    div()
                        .w(px(16.0))
                        .h(px(16.0))
                        .rounded(px(8.0))
                        .bg(tokens.surface),
                ),
        )
}

#[derive(Clone, Copy)]
struct SettingRowState {
    enabled: bool,
    disabled: bool,
    tab_index: isize,
}

fn command_button(
    label: &'static str,
    focus: &FocusHandle,
    tab_index: isize,
    window: &Window,
    tokens: Tokens,
    disabled: bool,
) -> gpui::Div {
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(tab_index)
        .w(px(86.0))
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(if focus.is_focused(window) {
            tokens.accent
        } else {
            tokens.border
        })
        .bg(tokens.surface)
        .text_color(if disabled { tokens.muted } else { tokens.text })
        .when(!disabled, |button| {
            button.hover(|style| style.bg(tokens.selected).cursor_pointer())
        })
        .child(label)
}

pub fn open_settings_window(
    client: SettingsClient,
    cx: &mut App,
) -> Result<WindowHandle<SettingsView>, String> {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("BongoCat Settings".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    let mut view = SettingsView::new(client, cx);
                    view.refresh(cx);
                    view
                });
                window.focus(&view.read(cx).overlay_focus);
                view
            },
        )
        .map_err(|error| error.to_string())?;
    cx.activate(true);
    Ok(handle)
}
