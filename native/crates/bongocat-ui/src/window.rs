use crate::{
    RuntimeHealth, SettingsClient, SettingsError, SettingsErrorCode, SettingsModelAvailability,
    SettingsModelImportMonitor, SettingsModelImportOperation, SettingsModelImportRequest,
    SettingsModelImportStage, SettingsModelOrigin, SettingsOperationId, SettingsSnapshot,
};
use bongocat_platform::{DirectoryPickerError, DirectoryPickerOutcome, pick_model_directory};
use gpui::{
    App, Bounds, Context, FocusHandle, KeyDownEvent, Render, SharedString, Timer, TitlebarOptions,
    Window, WindowAppearance, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb,
    size,
};
use std::{path::Path, path::PathBuf, rc::Rc, time::Duration};

const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsPage {
    #[default]
    General,
    Models,
}

enum ModelImportState {
    Empty,
    Ready,
    PickerCancelled,
    PickerFailed(DirectoryPickerError),
    Starting { cancel_requested: bool },
    Running(SettingsModelImportMonitor),
    Succeeded,
    Failed(SettingsError),
    Cancelled,
}

struct ModelImportDraft {
    id: String,
    source_root: Option<PathBuf>,
    state: ModelImportState,
}

impl Default for ModelImportDraft {
    fn default() -> Self {
        Self {
            id: String::new(),
            source_root: None,
            state: ModelImportState::Empty,
        }
    }
}

impl ModelImportDraft {
    fn is_running(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. } | ModelImportState::Running(_)
        )
    }

    fn can_import(&self) -> bool {
        self.source_root.is_some() && !self.id.is_empty() && !self.is_running()
    }

    fn running_operation_id(&self) -> Option<SettingsOperationId> {
        match &self.state {
            ModelImportState::Running(monitor) => Some(monitor.operation_id()),
            _ => None,
        }
    }

    fn apply_starting_cancellation(&self, operation: &SettingsModelImportOperation) {
        if matches!(
            self.state,
            ModelImportState::Starting {
                cancel_requested: true
            }
        ) {
            operation.cancel();
        }
    }

    fn edit_id(&mut self, event: &KeyDownEvent) -> bool {
        if self.is_running()
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.platform
            || event.keystroke.modifiers.alt
        {
            return false;
        }
        if event.keystroke.key == "backspace" {
            let changed = self.id.pop().is_some();
            if changed {
                self.reset_result_state();
            }
            return changed;
        }
        let Some(character) = event.keystroke.key_char.as_deref() else {
            return false;
        };
        if self.id.len() >= 64
            || character.len() != 1
            || !character
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return false;
        }
        self.id.push_str(character);
        self.reset_result_state();
        true
    }

    fn reset_result_state(&mut self) {
        self.state = if self.source_root.is_some() {
            ModelImportState::Ready
        } else {
            ModelImportState::Empty
        };
    }
}

pub struct SettingsView {
    client: SettingsClient,
    snapshot: Option<SettingsSnapshot>,
    pending: Option<PendingOperation>,
    error: Option<SettingsError>,
    page: SettingsPage,
    model_import: ModelImportDraft,
    window_hidden: bool,
    request_quit: Rc<dyn Fn(&mut App)>,
    general_focus: FocusHandle,
    models_focus: FocusHandle,
    overlay_focus: FocusHandle,
    audio_focus: FocusHandle,
    model_id_focus: FocusHandle,
    choose_model_focus: FocusHandle,
    import_model_focus: FocusHandle,
    refresh_focus: FocusHandle,
    quit_focus: FocusHandle,
}

impl SettingsView {
    fn new(
        client: SettingsClient,
        request_quit: Rc<dyn Fn(&mut App)>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            client,
            snapshot: None,
            pending: None,
            error: None,
            page: SettingsPage::General,
            model_import: ModelImportDraft::default(),
            window_hidden: false,
            request_quit,
            general_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            models_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            overlay_focus: cx.focus_handle().tab_index(10).tab_stop(true),
            audio_focus: cx.focus_handle().tab_index(11).tab_stop(true),
            model_id_focus: cx.focus_handle().tab_index(20).tab_stop(true),
            choose_model_focus: cx.focus_handle().tab_index(21).tab_stop(true),
            import_model_focus: cx.focus_handle().tab_index(22).tab_stop(true),
            refresh_focus: cx.focus_handle().tab_index(30).tab_stop(true),
            quit_focus: cx.focus_handle().tab_index(31).tab_stop(true),
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

    pub fn window_hidden(&self) -> bool {
        self.window_hidden
    }

    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        bongocat_platform::show_native_window(window).map_err(|error| error.to_string())?;
        self.window_hidden = false;
        self.refresh(cx);
        window.activate_window();
        Ok(())
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

    fn choose_model_directory(&mut self, cx: &mut Context<Self>) {
        if self.model_import.is_running() {
            return;
        }
        match pick_model_directory() {
            Ok(DirectoryPickerOutcome::Selected(source_root)) => {
                self.model_import.id = suggested_model_id(&source_root);
                self.model_import.source_root = Some(source_root);
                self.model_import.state = ModelImportState::Ready;
            }
            Ok(DirectoryPickerOutcome::Cancelled) => {
                self.model_import.state = ModelImportState::PickerCancelled;
            }
            Err(error) => {
                self.model_import.state = ModelImportState::PickerFailed(error);
            }
        }
        cx.notify();
    }

    fn start_model_import(&mut self, cx: &mut Context<Self>) {
        if !self.model_import.can_import() || self.pending.is_some() {
            return;
        }
        let request = SettingsModelImportRequest {
            id: self.model_import.id.clone(),
            source_root: self
                .model_import
                .source_root
                .clone()
                .expect("importable draft has a source directory"),
        };
        self.model_import.state = ModelImportState::Starting {
            cancel_requested: false,
        };
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.start_model_import(request).await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(operation) => {
                    view.model_import.apply_starting_cancellation(&operation);
                    view.observe_model_import(operation, cx);
                }
                Err(error) => {
                    view.model_import.state = ModelImportState::Failed(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn observe_model_import(
        &mut self,
        operation: SettingsModelImportOperation,
        cx: &mut Context<Self>,
    ) {
        let monitor = operation.monitor();
        let operation_id = monitor.operation_id();
        self.model_import.state = ModelImportState::Running(monitor);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let final_result = operation.final_result().await;
            let _ = this.update(cx, |view, cx| {
                if view.model_import.running_operation_id() != Some(final_result.operation_id) {
                    return;
                }
                match final_result.result {
                    Ok(snapshot)
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision) =>
                    {
                        view.snapshot = Some(snapshot);
                        view.model_import.source_root = None;
                        view.model_import.state = ModelImportState::Succeeded;
                    }
                    Ok(_) => {
                        view.model_import.source_root = None;
                        view.model_import.state = ModelImportState::Succeeded;
                    }
                    Err(error) if error.code() == SettingsErrorCode::ModelImportCancelled => {
                        view.model_import.state = ModelImportState::Cancelled;
                    }
                    Err(error) => view.model_import.state = ModelImportState::Failed(error),
                }
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(100)).await;
                let keep_polling = this
                    .update(cx, |view, cx| {
                        let keep_polling =
                            view.model_import.running_operation_id() == Some(operation_id);
                        if keep_polling {
                            cx.notify();
                        }
                        keep_polling
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();
    }

    fn cancel_model_import(&mut self, cx: &mut Context<Self>) {
        match &mut self.model_import.state {
            ModelImportState::Starting { cancel_requested } => {
                *cancel_requested = true;
                cx.notify();
            }
            ModelImportState::Running(monitor) => {
                monitor.cancel();
                cx.notify();
            }
            _ => {}
        }
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
        let disabled =
            self.pending.is_some() || snapshot.is_none() || self.model_import.is_running();
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
            .and_then(|snapshot| snapshot.active_model.as_ref())
            .map(|model| model.id.clone())
            .unwrap_or_else(|| "No active model".to_owned())
            .into();

        let general_selected = self.page == SettingsPage::General;
        let models_selected = self.page == SettingsPage::Models;
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
                navigation_item(
                    "General",
                    general_selected,
                    &self.general_focus,
                    1,
                    window,
                    tokens,
                )
                .id("general-page")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.general_focus);
                    view.page = SettingsPage::General;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&view.general_focus);
                        view.page = SettingsPage::General;
                        cx.notify();
                    }
                })),
            )
            .child(
                navigation_item(
                    "Models",
                    models_selected,
                    &self.models_focus,
                    2,
                    window,
                    tokens,
                )
                .id("models-page")
                .on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.models_focus);
                    view.page = SettingsPage::Models;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&view.models_focus);
                        view.page = SettingsPage::Models;
                        cx.notify();
                    }
                })),
            )
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
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if !disabled && is_activation_key(event) {
                cx.stop_propagation();
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
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if !disabled && is_activation_key(event) {
                cx.stop_propagation();
                window.focus(&view.audio_focus);
                view.set_motion_audio_enabled(!motion_audio_enabled, cx);
            }
        }));

        let general_content = div()
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
            .child(div().flex_1());

        let (import_status, import_failed) = model_import_status(&self.model_import);
        let model_id: SharedString = if self.model_import.id.is_empty() {
            "Model ID".into()
        } else {
            self.model_import.id.clone().into()
        };
        let model_id_disabled = self.model_import.is_running();
        let model_rows = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .model_catalog
                    .entries
                    .iter()
                    .map(|entry| {
                        let origin = match entry.origin {
                            SettingsModelOrigin::Preset => "Preset",
                            SettingsModelOrigin::Installed => "Installed",
                        };
                        let availability: SharedString = match entry.availability {
                            SettingsModelAvailability::Ready {
                                texture_count,
                                expression_count,
                                motion_count,
                            } => format!(
                                "{origin} · {texture_count} textures · {expression_count} expressions · {motion_count} motions"
                            )
                            .into(),
                            SettingsModelAvailability::Invalid { .. } => {
                                format!("{origin} · Invalid model").into()
                            }
                        };
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .py_2()
                            .border_b_1()
                            .border_color(tokens.border)
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_color(tokens.text)
                                    .child(entry.id.clone()),
                            )
                            .child(div().text_sm().text_color(tokens.muted).child(availability))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let import_running = self.model_import.is_running();
        let picker_disabled = import_running || self.pending.is_some();
        let import_button_label = if import_running { "Cancel" } else { "Import" };
        let import_disabled = !import_running && !self.model_import.can_import();
        let models_content = div()
            .min_w_0()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            .child(div().text_2xl().child("Models"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .pb_4()
                    .border_b_1()
                    .border_color(tokens.border)
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div().text_sm().text_color(tokens.muted).child("Model ID"),
                                    )
                                    .child(
                                        div()
                                            .id("model-id-input")
                                            .key_context("SettingsModelId")
                                            .track_focus(&self.model_id_focus)
                                            .tab_index(20)
                                            .h(px(34.0))
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .px_3()
                                            .border_1()
                                            .border_color(
                                                if self.model_id_focus.is_focused(window) {
                                                    tokens.accent
                                                } else {
                                                    tokens.border
                                                },
                                            )
                                            .rounded_md()
                                            .bg(tokens.surface)
                                            .text_color(if self.model_import.id.is_empty() {
                                                tokens.muted
                                            } else {
                                                tokens.text
                                            })
                                            .when(!model_id_disabled, |input| {
                                                input.hover(|style| style.cursor_text())
                                            })
                                            .on_click(cx.listener(|view, _, window, _| {
                                                if !view.model_import.is_running() {
                                                    window.focus(&view.model_id_focus);
                                                }
                                            }))
                                            .on_key_down(cx.listener(|view, event, _, cx| {
                                                if view.model_import.edit_id(event) {
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                }
                                            }))
                                            .child(model_id),
                                    ),
                            )
                            .child(
                                command_button(
                                    "Choose folder",
                                    &self.choose_model_focus,
                                    21,
                                    window,
                                    tokens,
                                    picker_disabled,
                                )
                                .w(px(112.0))
                                .id("choose-model-directory")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    if !view.model_import.is_running() && view.pending.is_none() {
                                        window.focus(&view.choose_model_focus);
                                        view.choose_model_directory(cx);
                                    }
                                }))
                                .on_key_down(cx.listener(
                                    |view, event, window, cx| {
                                        if !view.model_import.is_running()
                                            && view.pending.is_none()
                                            && is_activation_key(event)
                                        {
                                            cx.stop_propagation();
                                            window.focus(&view.choose_model_focus);
                                            view.choose_model_directory(cx);
                                        }
                                    },
                                )),
                            )
                            .child(
                                command_button(
                                    import_button_label,
                                    &self.import_model_focus,
                                    22,
                                    window,
                                    tokens,
                                    import_disabled,
                                )
                                .id("import-model")
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    if !import_disabled {
                                        window.focus(&view.import_model_focus);
                                        if import_running {
                                            view.cancel_model_import(cx);
                                        } else {
                                            view.start_model_import(cx);
                                        }
                                    }
                                }))
                                .on_key_down(cx.listener(
                                    move |view, event, window, cx| {
                                        if !import_disabled && is_activation_key(event) {
                                            cx.stop_propagation();
                                            window.focus(&view.import_model_focus);
                                            if import_running {
                                                view.cancel_model_import(cx);
                                            } else {
                                                view.start_model_import(cx);
                                            }
                                        }
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .h(px(20.0))
                            .text_sm()
                            .text_color(if import_failed {
                                tokens.danger
                            } else {
                                tokens.muted
                            })
                            .child(import_status),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.muted)
                    .child("Available models"),
            )
            .child(
                div()
                    .id("model-catalog")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(model_rows),
            );

        let content = match self.page {
            SettingsPage::General => general_content,
            SettingsPage::Models => models_content,
        }
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
                        30,
                        window,
                        tokens,
                        self.pending.is_some() || self.model_import.is_running(),
                    )
                    .id("refresh-settings")
                    .on_click(cx.listener(|view, _, window, cx| {
                        if view.pending.is_none() && !view.model_import.is_running() {
                            window.focus(&view.refresh_focus);
                            view.refresh(cx);
                        }
                    }))
                    .on_key_down(cx.listener(|view, event, window, cx| {
                        if view.pending.is_none()
                            && !view.model_import.is_running()
                            && is_activation_key(event)
                        {
                            cx.stop_propagation();
                            window.focus(&view.refresh_focus);
                            view.refresh(cx);
                        }
                    })),
                )
                .child(
                    command_button("Quit", &self.quit_focus, 31, window, tokens, false)
                        .id("quit-application")
                        .on_click(cx.listener(|view, _, window, cx| {
                            window.focus(&view.quit_focus);
                            (view.request_quit)(cx);
                        }))
                        .on_key_down(cx.listener(|view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                window.focus(&view.quit_focus);
                                (view.request_quit)(cx);
                            }
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

fn is_activation_key(event: &KeyDownEvent) -> bool {
    !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.platform
        && !event.keystroke.modifiers.alt
        && (matches!(event.keystroke.key.as_str(), "enter" | "space")
            || event.keystroke.key_char.as_deref() == Some(" "))
}

fn suggested_model_id(source_root: &Path) -> String {
    let name = source_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut suggestion = String::with_capacity(name.len().min(64));
    let mut separator_pending = false;
    for byte in name.bytes() {
        if suggestion.len() >= 64 {
            break;
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            if separator_pending && !suggestion.is_empty() && suggestion.len() < 64 {
                suggestion.push('-');
            }
            separator_pending = false;
            suggestion.push(char::from(byte.to_ascii_lowercase()));
        } else {
            separator_pending = true;
        }
    }
    let trimmed = suggestion
        .trim_matches(|character| matches!(character, '.' | '-' | '_'))
        .to_owned();
    if trimmed.is_empty() {
        return "custom-model".to_owned();
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed.as_str());
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|value| stem.eq_ignore_ascii_case(value))
        || (stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        let maximum_tail = 64 - "model-".len();
        let tail = trimmed[..trimmed.len().min(maximum_tail)].trim_end_matches('.');
        format!("model-{tail}")
    } else {
        trimmed
    }
}

fn model_import_status(draft: &ModelImportDraft) -> (SharedString, bool) {
    match &draft.state {
        ModelImportState::Empty => ("No folder selected".into(), false),
        ModelImportState::Ready => ("Folder selected".into(), false),
        ModelImportState::PickerCancelled if draft.source_root.is_some() => (
            "Selection cancelled; previous folder retained".into(),
            false,
        ),
        ModelImportState::PickerCancelled => ("Selection cancelled".into(), false),
        ModelImportState::PickerFailed(error) => {
            let message = match error {
                DirectoryPickerError::WrongThread => "Folder picker requires the UI thread",
                DirectoryPickerError::SelectionInvalid => "Selected folder is unavailable",
                DirectoryPickerError::UnsupportedPlatform
                | DirectoryPickerError::BackendUnavailable
                | DirectoryPickerError::SelectionUnavailable => "Folder picker is unavailable",
            };
            (message.into(), true)
        }
        ModelImportState::Starting {
            cancel_requested: true,
        } => ("Cancelling import...".into(), false),
        ModelImportState::Starting {
            cancel_requested: false,
        } => ("Starting import...".into(), false),
        ModelImportState::Running(monitor) if monitor.is_cancelled() => {
            ("Cancelling import...".into(), false)
        }
        ModelImportState::Running(monitor) => {
            let progress = monitor.progress();
            let stage = match progress.stage {
                SettingsModelImportStage::Preparing => "Preparing",
                SettingsModelImportStage::Copying => "Copying",
                SettingsModelImportStage::Validating => "Validating",
                SettingsModelImportStage::Committing => "Committing",
            };
            (
                format!(
                    "{stage} · {} files · {} bytes",
                    progress.files_copied, progress.bytes_copied
                )
                .into(),
                false,
            )
        }
        ModelImportState::Succeeded => ("Import complete".into(), false),
        ModelImportState::Failed(error) => (error.to_string().into(), true),
        ModelImportState::Cancelled => ("Import cancelled".into(), false),
    }
}

fn navigation_item(
    label: &'static str,
    selected: bool,
    focus: &FocusHandle,
    tab_index: isize,
    window: &Window,
    tokens: Tokens,
) -> gpui::Div {
    div()
        .key_context("SettingsNavigation")
        .track_focus(focus)
        .tab_index(tab_index)
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(if focus.is_focused(window) {
            tokens.accent
        } else if selected {
            tokens.selected
        } else {
            tokens.sidebar
        })
        .bg(if selected {
            tokens.selected
        } else {
            tokens.sidebar
        })
        .text_color(if selected { tokens.text } else { tokens.muted })
        .hover(|style| style.bg(tokens.selected).cursor_pointer())
        .child(label)
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
    request_quit: impl Fn(&mut App) + 'static,
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
                let request_quit = Rc::new(request_quit);
                let view = cx.new(|cx| {
                    let mut view = SettingsView::new(client, request_quit, cx);
                    view.refresh(cx);
                    view
                });
                #[cfg(target_os = "windows")]
                {
                    let weak_view = view.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        let result = bongocat_platform::hide_native_window(window);
                        let _ = weak_view.update(cx, |view, cx| match result {
                            Ok(()) => {
                                view.window_hidden = true;
                                cx.notify();
                            }
                            Err(_) => view.report_service_error(
                                SettingsError::new(crate::SettingsErrorCode::WindowUnavailable),
                                cx,
                            ),
                        });
                        false
                    });
                }
                window.focus(&view.read(cx).overlay_focus);
                view
            },
        )
        .map_err(|error| error.to_string())?;
    cx.activate(true);
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};

    fn key(key: &str, key_char: Option<&str>) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: key.to_owned(),
                key_char: key_char.map(str::to_owned),
            },
            is_held: false,
        }
    }

    #[test]
    fn model_id_suggestions_are_portable_bounded_and_path_free() {
        assert_eq!(
            suggested_model_id(Path::new("/private/source/Keyboard Model 2")),
            "keyboard-model-2"
        );
        assert_eq!(
            suggested_model_id(Path::new("/private/source/模型目录")),
            "custom-model"
        );
        assert_eq!(
            suggested_model_id(Path::new("/private/source/CON.custom")),
            "model-con.custom"
        );
        let reserved = format!("CON.{}", "x".repeat(80));
        let suggestion = suggested_model_id(Path::new(&reserved));
        assert!(suggestion.starts_with("model-con."));
        assert!(suggestion.len() <= 64);
        assert!(!suggestion.ends_with('.'));
    }

    #[test]
    fn model_id_draft_accepts_only_the_product_ascii_shape() {
        let mut draft = ModelImportDraft {
            id: String::new(),
            source_root: Some(PathBuf::from("/private/source")),
            state: ModelImportState::Failed(SettingsError::new(SettingsErrorCode::InvalidModelId)),
        };
        assert!(draft.edit_id(&key("a", Some("a"))));
        assert!(draft.edit_id(&key("-", Some("-"))));
        assert!(!draft.edit_id(&key("/", Some("/"))));
        assert_eq!(draft.id, "a-");
        assert!(matches!(draft.state, ModelImportState::Ready));
        assert!(draft.edit_id(&key("backspace", None)));
        assert_eq!(draft.id, "a");

        draft.id = "x".repeat(64);
        assert!(!draft.edit_id(&key("y", Some("y"))));
        assert_eq!(draft.id.len(), 64);
    }

    #[test]
    fn commands_accept_enter_and_space_without_command_modifiers() {
        assert!(is_activation_key(&key("enter", None)));
        assert!(is_activation_key(&key("space", Some(" "))));
        assert!(!is_activation_key(&key("a", Some("a"))));
        let mut modified = key("enter", None);
        modified.keystroke.modifiers.platform = true;
        assert!(!is_activation_key(&modified));
    }

    #[test]
    fn cancellation_requested_while_starting_reaches_the_created_operation() {
        let (client, _endpoint) = SettingsClient::bounded(1);
        let (operation, _, _) = client.prepare_model_import().expect("prepared import");
        let draft = ModelImportDraft {
            id: "custom-model".to_owned(),
            source_root: Some(PathBuf::from("/private/source")),
            state: ModelImportState::Starting {
                cancel_requested: true,
            },
        };

        assert!(!operation.is_cancelled());
        draft.apply_starting_cancellation(&operation);
        assert!(operation.is_cancelled());
        let (status, failed) = model_import_status(&draft);
        assert!(!failed);
        assert_eq!(status, "Cancelling import...");
    }

    #[test]
    fn picker_status_never_contains_the_selected_path() {
        let mut draft = ModelImportDraft {
            id: "custom-model".to_owned(),
            source_root: Some(PathBuf::from("/private/secret/model")),
            state: ModelImportState::PickerCancelled,
        };
        let (status, failed) = model_import_status(&draft);
        assert!(!failed);
        assert_eq!(status, "Selection cancelled; previous folder retained");
        assert!(!status.contains("private"));

        draft.state = ModelImportState::PickerFailed(DirectoryPickerError::SelectionInvalid);
        let (status, failed) = model_import_status(&draft);
        assert!(failed);
        assert_eq!(status, "Selected folder is unavailable");
        assert!(!status.contains("secret"));
    }
}
