//! GPUI integration and presentation differences for the active desktop backend.
use super::*;

pub(super) const GLOBAL_WINDOW_CONTROL: bool = !cfg!(target_os = "linux");
pub(super) const RELATIVE_POINTER_SENSITIVITY: bool = cfg!(target_os = "linux");
pub(super) const FORCE_POINTER_MOVEMENT: bool = !cfg!(target_os = "linux");
const INPUT_STATUS_NOTIFICATIONS: bool = cfg!(target_os = "linux");
pub(super) const AUTOMATIC_UPDATES: bool = !cfg!(target_os = "linux");
pub(super) const SHOW_ON_CREATE: bool = cfg!(target_os = "linux");

pub(super) fn window_decorations() -> Option<gpui_kit::WindowDecorations> {
    #[cfg(target_os = "linux")]
    {
        Some(gpui_kit::WindowDecorations::Client)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub(super) fn copy_text(text: String, cx: &mut App) -> bool {
    #[cfg(target_os = "linux")]
    {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
        true
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cx;
        bongocat_platform::write_clipboard_text(&text).is_ok()
    }
}

pub(super) fn decorate_content(
    content: Div,
    language: SettingsLanguage,
    window: &Window,
    cx: &mut Context<SettingsView>,
) -> Div {
    #[cfg(target_os = "linux")]
    {
        let title_bar = matches!(
            window.window_decorations(),
            gpui_kit::Decorations::Client { .. }
        )
        .then(|| {
            gpui_kit::component::TitleBar::new()
                .flex_shrink_0()
                .child(bongocat_i18n::text(
                    language.catalog_locale(),
                    "navigation.settings.title",
                ))
                .on_close_window(cx.listener(|view, _, window, cx| {
                    let _ = view.close(window, cx);
                }))
        });
        div()
            .flex()
            .flex_col()
            .size_full()
            .children(title_bar)
            .child(content)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (language, window, cx);
        content
    }
}

#[derive(Default)]
pub(super) struct InputNotifications {
    previous: Option<bongocat_ui_protocol::SettingsInputServiceStatus>,
}
struct InputNotification;
impl InputNotifications {
    pub(super) fn observe(
        &mut self,
        snapshot: Option<&SettingsSnapshot>,
        language: SettingsLanguage,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) {
        if !INPUT_STATUS_NOTIFICATIONS {
            return;
        }
        let Some(snapshot) = snapshot else {
            return;
        };
        use bongocat_ui_protocol::SettingsInputServiceStatus as Status;
        let status = snapshot.input_diagnostics.service_status;
        if self.previous == Some(status) {
            return;
        }
        self.previous = Some(status);
        let key = match status {
            Status::BackendUnavailable => Some("linux_input.unavailable"),
            Status::PermissionDenied => Some("linux_input.denied"),
            Status::Failed | Status::Stopped => Some("linux_input.stopped"),
            Status::NotStarted | Status::Running => None,
        };
        if let Some(key) = key {
            window.push_notification(
                Notification::new()
                    .id::<InputNotification>()
                    .message(bongocat_i18n::text(language.catalog_locale(), key))
                    .with_type(NotificationType::Error),
                cx,
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn placement_from_window(
    _window: &Window,
    _cx: &App,
) -> Option<SettingsWindowPlacement> {
    None
}

#[cfg(target_os = "linux")]
pub(super) fn initial_window_bounds(
    _state: &SettingsWindowState,
    cx: &App,
) -> (WindowBounds, Option<DisplayId>) {
    (
        WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
            cx,
        )),
        None,
    )
}
