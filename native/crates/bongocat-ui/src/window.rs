use crate::{
    RuntimeHealth, SettingsClient, SettingsError, SettingsErrorCode, SettingsModelAvailability,
    SettingsModelDiagnostic, SettingsModelEntry, SettingsModelImportMonitor,
    SettingsModelImportOperation, SettingsModelImportRequest, SettingsModelImportStage,
    SettingsModelKey, SettingsModelOrigin, SettingsOperationId, SettingsSnapshot,
};
use bongocat_platform::{DirectoryPickerError, DirectoryPickerOutcome, pick_model_directory};
use gpui::{
    App, Bounds, Context, FocusHandle, KeyDownEvent, Render, SharedString, Timer, TitlebarOptions,
    Window, WindowAppearance, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb,
    size,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

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
    ModelSelection,
    ModelDeletion,
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
    Picking,
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModelRowKey {
    origin_rank: u8,
    id: String,
}

impl ModelRowKey {
    fn new(origin: SettingsModelOrigin, id: &str) -> Self {
        Self {
            origin_rank: match origin {
                SettingsModelOrigin::Preset => 0,
                SettingsModelOrigin::Installed => 1,
            },
            id: id.to_owned(),
        }
    }
}

#[derive(Clone)]
struct ModelRowFocus {
    activate: FocusHandle,
    delete: FocusHandle,
    cancel_delete: FocusHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActions {
    active: bool,
    can_activate: bool,
    can_delete: bool,
}

#[derive(Clone, Copy)]
enum ModelRowAction {
    Activate,
    Delete,
    CancelDelete,
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
        self.source_root.is_some()
            && !self.id.is_empty()
            && !self.is_running()
            && !self.is_picker_open()
    }

    fn is_picker_open(&self) -> bool {
        matches!(self.state, ModelImportState::Picking)
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
    model_delete_confirmation: Option<SettingsModelKey>,
    model_row_focus: BTreeMap<ModelRowKey, ModelRowFocus>,
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
            model_delete_confirmation: None,
            model_row_focus: BTreeMap::new(),
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

    pub fn show_models_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::Models;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "models page has not received a runtime snapshot".to_owned())?;
        if snapshot.model_catalog.error.is_some() {
            return Err("models page received a catalog error".to_owned());
        }
        if snapshot.model_catalog.entries.is_empty() {
            return Err("models page catalog is empty".to_owned());
        }
        let active_model = snapshot
            .active_model
            .as_ref()
            .ok_or_else(|| "models page has no active model identity".to_owned())?;
        let active_entry = snapshot
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == active_model.origin && entry.id == active_model.id)
            .ok_or_else(|| "models page active model is absent from the catalog".to_owned())?;
        let active_actions = model_row_actions(active_entry, Some(active_model), false);
        if !active_actions.active || active_actions.can_activate || active_actions.can_delete {
            return Err("models page did not protect the active model row".to_owned());
        }

        let mut has_activation_target = false;
        for entry in &snapshot.model_catalog.entries {
            let actions = model_row_actions(entry, Some(active_model), false);
            if entry.origin == SettingsModelOrigin::Preset && actions.can_delete {
                return Err("models page exposed deletion for a preset model".to_owned());
            }
            if matches!(
                entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) && actions.can_activate
            {
                return Err("models page exposed activation for an invalid model".to_owned());
            }
            has_activation_target |= actions.can_activate;
        }
        if !has_activation_target {
            return Err("models page has no ready inactive activation target".to_owned());
        }
        Ok(())
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
        if self.model_import.is_running() || self.model_import.is_picker_open() {
            return;
        }
        self.model_import.state = ModelImportState::Picking;
        cx.notify();

        let (sender, receiver) = async_channel::bounded(1);
        if let Err(error) = pick_model_directory(move |result| {
            let _ = sender.try_send(result);
        }) {
            self.apply_model_directory_result(Err(error));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or(Err(DirectoryPickerError::BackendUnavailable));
            let _ = this.update(cx, |view, cx| {
                view.apply_model_directory_result(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_model_directory_result(
        &mut self,
        result: Result<DirectoryPickerOutcome, DirectoryPickerError>,
    ) {
        match result {
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

    fn sync_model_row_focus(
        &mut self,
        entries: &[SettingsModelEntry],
        active_model: Option<&SettingsModelKey>,
        commands_blocked: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .model_delete_confirmation
            .as_ref()
            .is_some_and(|model| !model_delete_confirmation_is_valid(entries, active_model, model))
        {
            self.model_delete_confirmation = None;
        }
        let keys = entries
            .iter()
            .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
            .collect::<BTreeSet<_>>();
        self.model_row_focus.retain(|key, _| keys.contains(key));
        for (index, entry) in entries.iter().enumerate() {
            let key = ModelRowKey::new(entry.origin, &entry.id);
            let model = SettingsModelKey {
                id: entry.id.clone(),
                origin: entry.origin,
            };
            let actions = model_row_actions(entry, active_model, commands_blocked);
            let confirming_delete = self.model_delete_confirmation.as_ref() == Some(&model);
            let offset = isize::try_from(index)
                .unwrap_or(isize::MAX / 3)
                .saturating_mul(3);
            let tab_index = 40_isize.saturating_add(offset);
            let action_tabs = model_row_action_tab_indices(tab_index, confirming_delete);
            let focus = self
                .model_row_focus
                .entry(key)
                .or_insert_with(|| ModelRowFocus {
                    activate: cx.focus_handle(),
                    delete: cx.focus_handle(),
                    cancel_delete: cx.focus_handle(),
                });
            focus.activate = focus
                .activate
                .clone()
                .tab_index(action_tabs.activate)
                .tab_stop(actions.can_activate);
            focus.delete = focus
                .delete
                .clone()
                .tab_index(action_tabs.delete)
                .tab_stop(actions.can_delete);
            focus.cancel_delete = focus
                .cancel_delete
                .clone()
                .tab_index(action_tabs.cancel_delete)
                .tab_stop(actions.can_delete && confirming_delete);
        }
    }

    fn select_model(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        self.pending = Some(PendingOperation::ModelSelection);
        self.error = None;
        self.model_delete_confirmation = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.select_model(model).await;
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

    fn request_model_delete(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        if self.model_delete_confirmation.as_ref() == Some(&model) {
            self.delete_model(model, cx);
        } else {
            self.model_delete_confirmation = Some(model);
            self.error = None;
            cx.notify();
        }
    }

    fn cancel_model_delete(&mut self, model: &SettingsModelKey, cx: &mut Context<Self>) {
        if self.model_delete_confirmation.as_ref() == Some(model) {
            self.model_delete_confirmation = None;
            cx.notify();
        }
    }

    fn delete_model(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        self.pending = Some(PendingOperation::ModelDeletion);
        self.error = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.delete_model(model).await;
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
                        view.model_delete_confirmation = None;
                    }
                    Ok(_) => view.model_delete_confirmation = None,
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn run_model_row_action(
        &mut self,
        action: ModelRowAction,
        model: SettingsModelKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.origin == model.origin && entry.id == model.id)
        }) else {
            return;
        };
        let actions = model_row_actions(
            entry,
            self.snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.active_model.as_ref()),
            self.pending.is_some() || self.model_import.is_running(),
        );
        let Some(focus) = self
            .model_row_focus
            .get(&ModelRowKey::new(model.origin, &model.id))
            .cloned()
        else {
            return;
        };
        match action {
            ModelRowAction::Activate if actions.can_activate => {
                window.focus(&focus.activate);
                self.select_model(model, cx);
            }
            ModelRowAction::Delete if actions.can_delete => {
                window.focus(&focus.delete);
                self.request_model_delete(model, cx);
            }
            ModelRowAction::CancelDelete
                if self.model_delete_confirmation.as_ref() == Some(&model) =>
            {
                window.focus(&focus.cancel_delete);
                self.cancel_model_delete(&model, cx);
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
            (_, Some(PendingOperation::Refresh), _) => "Refreshing...".into(),
            (_, Some(PendingOperation::OverlayVisibility | PendingOperation::MotionAudio), _) => {
                "Saving...".into()
            }
            (_, Some(PendingOperation::ModelSelection), _) => "Activating model...".into(),
            (_, Some(PendingOperation::ModelDeletion), _) => "Deleting model...".into(),
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
        let import_running = self.model_import.is_running();
        let picker_open = self.model_import.is_picker_open();
        let model_id_disabled = import_running || picker_open;
        let model_commands_blocked = import_running || picker_open || self.pending.is_some();
        let model_entries = snapshot
            .as_ref()
            .map(|snapshot| snapshot.model_catalog.entries.clone())
            .unwrap_or_default();
        let active_model = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.active_model.as_ref());
        self.sync_model_row_focus(&model_entries, active_model, model_commands_blocked, cx);
        let mut model_rows = model_entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let model = SettingsModelKey {
                    id: entry.id.clone(),
                    origin: entry.origin,
                };
                let actions = model_row_actions(&entry, active_model, model_commands_blocked);
                let confirming_delete = self.model_delete_confirmation.as_ref() == Some(&model);
                let focus = self
                    .model_row_focus
                    .get(&ModelRowKey::new(entry.origin, &entry.id))
                    .expect("model row focus is synchronized")
                    .clone();
                let tab_index = 40_isize.saturating_add(
                    isize::try_from(index)
                        .unwrap_or(isize::MAX / 3)
                        .saturating_mul(3),
                );
                let action_tabs = model_row_action_tab_indices(tab_index, confirming_delete);
                let availability = if confirming_delete {
                    format!(
                        "{} · Confirm deletion",
                        model_availability_status(&entry, actions.active)
                    )
                    .into()
                } else {
                    model_availability_status(&entry, actions.active)
                };
                let activate_label = if actions.active {
                    "Active"
                } else if matches!(
                    entry.availability,
                    SettingsModelAvailability::Invalid { .. }
                ) {
                    "Unavailable"
                } else {
                    "Activate"
                };
                let activate_model = model.clone();
                let activate_key_model = model.clone();
                let mut actions_row = div().flex().items_center().gap_2().child(
                    command_button(
                        activate_label,
                        &focus.activate,
                        action_tabs.activate,
                        window,
                        tokens,
                        !actions.can_activate,
                    )
                    .w(px(92.0))
                    .id(("activate-model", index))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.run_model_row_action(
                            ModelRowAction::Activate,
                            activate_model.clone(),
                            window,
                            cx,
                        );
                    }))
                    .on_key_down(cx.listener(
                        move |view, event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                view.run_model_row_action(
                                    ModelRowAction::Activate,
                                    activate_key_model.clone(),
                                    window,
                                    cx,
                                );
                            }
                        },
                    )),
                );
                if actions.can_delete {
                    if confirming_delete {
                        let cancel_model = model.clone();
                        let cancel_key_model = model.clone();
                        actions_row = actions_row.child(
                            command_button(
                                "Cancel",
                                &focus.cancel_delete,
                                action_tabs.cancel_delete,
                                window,
                                tokens,
                                false,
                            )
                            .id(("cancel-delete-model", index))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.run_model_row_action(
                                    ModelRowAction::CancelDelete,
                                    cancel_model.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_key_down(cx.listener(
                                move |view, event, window, cx| {
                                    if is_activation_key(event) {
                                        cx.stop_propagation();
                                        view.run_model_row_action(
                                            ModelRowAction::CancelDelete,
                                            cancel_key_model.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                },
                            )),
                        );
                    }
                    let delete_model = model.clone();
                    let delete_key_model = model.clone();
                    actions_row = actions_row.child(
                        command_button(
                            if confirming_delete {
                                "Confirm"
                            } else {
                                "Delete"
                            },
                            &focus.delete,
                            action_tabs.delete,
                            window,
                            tokens,
                            false,
                        )
                        .id(("delete-model", index))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.run_model_row_action(
                                ModelRowAction::Delete,
                                delete_model.clone(),
                                window,
                                cx,
                            );
                        }))
                        .on_key_down(cx.listener(
                            move |view, event, window, cx| {
                                if is_activation_key(event) {
                                    cx.stop_propagation();
                                    view.run_model_row_action(
                                        ModelRowAction::Delete,
                                        delete_key_model.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            },
                        )),
                    );
                }
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .py_3()
                    .border_b_1()
                    .border_color(tokens.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_color(tokens.text)
                                    .child(entry.id),
                            )
                            .child(actions_row),
                    )
                    .child(div().text_sm().text_color(tokens.muted).child(availability))
            })
            .collect::<Vec<_>>();
        let catalog_error = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.model_catalog.error.is_some());
        if model_rows.is_empty() {
            let empty_status = if snapshot.is_none() {
                "Loading models..."
            } else if catalog_error {
                "Model catalog is unavailable"
            } else {
                "No models available"
            };
            model_rows.push(
                div()
                    .py_4()
                    .text_sm()
                    .text_color(if catalog_error {
                        tokens.danger
                    } else {
                        tokens.muted
                    })
                    .child(empty_status),
            );
        }
        let (management_status, management_failed): (SharedString, bool) =
            match (&self.error, self.pending, catalog_error) {
                (Some(error), _, _) => (error.to_string().into(), true),
                (_, Some(PendingOperation::ModelSelection), _) => {
                    ("Activating model...".into(), false)
                }
                (_, Some(PendingOperation::ModelDeletion), _) => {
                    ("Deleting model...".into(), false)
                }
                (_, Some(PendingOperation::Refresh), _) => ("Refreshing models...".into(), false),
                (_, _, true) => ("Catalog unavailable".into(), true),
                _ => ("".into(), false),
            };
        let picker_disabled = import_running || picker_open || self.pending.is_some();
        let picker_button_label = if picker_open {
            "Choosing..."
        } else {
            "Choose folder"
        };
        let import_button_label = if import_running { "Cancel" } else { "Import" };
        let import_disabled =
            !import_running && (!self.model_import.can_import() || self.pending.is_some());
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
                                    picker_button_label,
                                    &self.choose_model_focus,
                                    21,
                                    window,
                                    tokens,
                                    picker_disabled,
                                )
                                .w(px(112.0))
                                .id("choose-model-directory")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    if !view.model_import.is_running()
                                        && !view.model_import.is_picker_open()
                                        && view.pending.is_none()
                                    {
                                        window.focus(&view.choose_model_focus);
                                        view.choose_model_directory(cx);
                                    }
                                }))
                                .on_key_down(cx.listener(
                                    |view, event, window, cx| {
                                        if !view.model_import.is_running()
                                            && !view.model_import.is_picker_open()
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
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .text_sm()
                    .child(div().text_color(tokens.muted).child("Available models"))
                    .child(
                        div()
                            .min_w_0()
                            .text_color(if management_failed {
                                tokens.danger
                            } else {
                                tokens.muted
                            })
                            .child(management_status),
                    ),
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
                        self.pending.is_some()
                            || self.model_import.is_running()
                            || self.model_import.is_picker_open(),
                    )
                    .id("refresh-settings")
                    .on_click(cx.listener(|view, _, window, cx| {
                        if view.pending.is_none()
                            && !view.model_import.is_running()
                            && !view.model_import.is_picker_open()
                        {
                            window.focus(&view.refresh_focus);
                            view.refresh(cx);
                        }
                    }))
                    .on_key_down(cx.listener(|view, event, window, cx| {
                        if view.pending.is_none()
                            && !view.model_import.is_running()
                            && !view.model_import.is_picker_open()
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

fn model_row_actions(
    entry: &SettingsModelEntry,
    active_model: Option<&SettingsModelKey>,
    commands_blocked: bool,
) -> ModelRowActions {
    let model = SettingsModelKey {
        id: entry.id.clone(),
        origin: entry.origin,
    };
    let active = active_model == Some(&model);
    let ready = matches!(entry.availability, SettingsModelAvailability::Ready { .. });
    ModelRowActions {
        active,
        can_activate: ready && !active && !commands_blocked,
        can_delete: entry.origin == SettingsModelOrigin::Installed && !active && !commands_blocked,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActionTabIndices {
    activate: isize,
    delete: isize,
    cancel_delete: isize,
}

fn model_row_action_tab_indices(
    first_tab_index: isize,
    confirming_delete: bool,
) -> ModelRowActionTabIndices {
    let second = first_tab_index.saturating_add(1);
    let third = first_tab_index.saturating_add(2);
    ModelRowActionTabIndices {
        activate: first_tab_index,
        delete: if confirming_delete { third } else { second },
        cancel_delete: if confirming_delete { second } else { third },
    }
}

fn model_delete_confirmation_is_valid(
    entries: &[SettingsModelEntry],
    active_model: Option<&SettingsModelKey>,
    model: &SettingsModelKey,
) -> bool {
    model.origin == SettingsModelOrigin::Installed
        && active_model != Some(model)
        && entries
            .iter()
            .any(|entry| entry.origin == model.origin && entry.id == model.id)
}

fn model_availability_status(entry: &SettingsModelEntry, active: bool) -> SharedString {
    let origin = match entry.origin {
        SettingsModelOrigin::Preset => "Preset",
        SettingsModelOrigin::Installed => "Installed",
    };
    let active = if active { " · Active" } else { "" };
    match entry.availability {
        SettingsModelAvailability::Ready {
            texture_count,
            expression_count,
            motion_count,
        } => format!(
            "{origin}{active} · {texture_count} textures · {expression_count} expressions · {motion_count} motions"
        )
        .into(),
        SettingsModelAvailability::Invalid { diagnostic } => {
            let diagnostic = match diagnostic {
                SettingsModelDiagnostic::InvalidModelId
                | SettingsModelDiagnostic::ModelEntryAmbiguous
                | SettingsModelDiagnostic::ModelEntryMissing
                | SettingsModelDiagnostic::ModelReferenceEscapesRoot
                | SettingsModelDiagnostic::ModelReferenceInvalid
                | SettingsModelDiagnostic::ModelReferenceSymlinkEscape
                | SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
                    "Package layout is invalid"
                }
                SettingsModelDiagnostic::ModelFileCountExceeded
                | SettingsModelDiagnostic::ModelFileTooLarge
                | SettingsModelDiagnostic::ModelJsonTooLarge
                | SettingsModelDiagnostic::ModelPackageDepthExceeded
                | SettingsModelDiagnostic::ModelPackageSizeExceeded
                | SettingsModelDiagnostic::ModelTextureDimensionExceeded => {
                    "Package exceeds safety limits"
                }
                SettingsModelDiagnostic::ModelJsonInvalid
                | SettingsModelDiagnostic::ModelUnsupportedVersion => {
                    "Model definition is unsupported"
                }
                SettingsModelDiagnostic::ModelTextureInvalidPng
                | SettingsModelDiagnostic::ModelTextureMissing => "Texture is invalid",
                SettingsModelDiagnostic::ModelIoError => "Model files are unavailable",
                SettingsModelDiagnostic::ModelMocMissing
                | SettingsModelDiagnostic::ModelResourceInvalid
                | SettingsModelDiagnostic::ModelResourceMissing
                | SettingsModelDiagnostic::ModelResourceNotFile => "Model resource is invalid",
            };
            format!("{origin} · {diagnostic}").into()
        }
    }
}

fn model_import_status(draft: &ModelImportDraft) -> (SharedString, bool) {
    match &draft.state {
        ModelImportState::Empty => ("No folder selected".into(), false),
        ModelImportState::Ready => ("Folder selected".into(), false),
        ModelImportState::Picking => ("Choosing folder...".into(), false),
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

    #[test]
    fn picker_open_state_blocks_conflicting_import_actions() {
        let draft = ModelImportDraft {
            id: "custom-model".to_owned(),
            source_root: Some(PathBuf::from("/private/source")),
            state: ModelImportState::Picking,
        };

        assert!(draft.is_picker_open());
        assert!(!draft.can_import());
        let (status, failed) = model_import_status(&draft);
        assert!(!failed);
        assert_eq!(status, "Choosing folder...");
    }

    #[test]
    fn model_row_actions_preserve_origin_availability_and_active_identity() {
        let ready = SettingsModelAvailability::Ready {
            texture_count: 1,
            expression_count: 0,
            motion_count: 0,
        };
        let preset = SettingsModelEntry {
            id: "duplicate".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: ready,
        };
        let installed = SettingsModelEntry {
            id: "duplicate".to_owned(),
            origin: SettingsModelOrigin::Installed,
            availability: ready,
        };
        let active_preset = SettingsModelKey {
            id: "duplicate".to_owned(),
            origin: SettingsModelOrigin::Preset,
        };

        assert_eq!(
            model_row_actions(&preset, Some(&active_preset), false),
            ModelRowActions {
                active: true,
                can_activate: false,
                can_delete: false,
            }
        );
        assert_eq!(
            model_row_actions(&installed, Some(&active_preset), false),
            ModelRowActions {
                active: false,
                can_activate: true,
                can_delete: true,
            }
        );

        let invalid = SettingsModelEntry {
            availability: SettingsModelAvailability::Invalid {
                diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
            },
            ..installed.clone()
        };
        assert_eq!(
            model_row_actions(&invalid, Some(&active_preset), false),
            ModelRowActions {
                active: false,
                can_activate: false,
                can_delete: true,
            }
        );
        assert_eq!(
            model_row_actions(&installed, Some(&active_preset), true),
            ModelRowActions {
                active: false,
                can_activate: false,
                can_delete: false,
            }
        );
        assert!(model_delete_confirmation_is_valid(
            &[preset.clone(), installed.clone()],
            Some(&active_preset),
            &SettingsModelKey {
                id: "duplicate".to_owned(),
                origin: SettingsModelOrigin::Installed,
            },
        ));
        assert!(!model_delete_confirmation_is_valid(
            &[preset, installed],
            Some(&active_preset),
            &active_preset,
        ));
    }

    #[test]
    fn invalid_model_status_is_stable_and_path_free() {
        let entry = SettingsModelEntry {
            id: "private-model".to_owned(),
            origin: SettingsModelOrigin::Installed,
            availability: SettingsModelAvailability::Invalid {
                diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
            },
        };
        let status = model_availability_status(&entry, false);
        assert_eq!(status, "Installed · Package layout is invalid");
        assert!(!status.contains("private-model"));
        assert!(!status.contains('/'));
    }

    #[test]
    fn model_delete_confirmation_tab_order_matches_visual_order() {
        assert_eq!(
            model_row_action_tab_indices(40, false),
            ModelRowActionTabIndices {
                activate: 40,
                delete: 41,
                cancel_delete: 42,
            }
        );
        assert_eq!(
            model_row_action_tab_indices(40, true),
            ModelRowActionTabIndices {
                activate: 40,
                cancel_delete: 41,
                delete: 42,
            }
        );
    }
}
