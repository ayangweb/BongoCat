use rdev::{Event, EventType, listen};
#[cfg(target_os = "windows")]
use rdev::{Keyboard, KeyboardState};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Runtime, command};

#[derive(Debug, Clone, Serialize)]
pub enum DeviceEventKind {
    MousePress,
    MouseRelease,
    MouseMove,
    KeyboardPress,
    KeyboardRelease,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEvent {
    kind: DeviceEventKind,
    value: Value,
}

static IS_LISTENING: AtomicBool = AtomicBool::new(false);

struct ListeningGuard;

impl ListeningGuard {
    fn acquire() -> Option<Self> {
        IS_LISTENING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for ListeningGuard {
    fn drop(&mut self) {
        IS_LISTENING.store(false, Ordering::Release);
    }
}

#[command]
pub async fn start_device_listening<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    let Some(_listening_guard) = ListeningGuard::acquire() else {
        return Ok(());
    };

    #[cfg(target_os = "windows")]
    let mut keyboard = Keyboard::new();

    let callback = move |event: Event| {
        let label = event
            .unicode
            .as_ref()
            .and_then(|unicode| unicode.name.clone());
        #[cfg(target_os = "windows")]
        let label = label.or_else(|| {
            keyboard
                .add(&event.event_type)
                .and_then(|unicode| unicode.name)
        });

        let device_event = match event.event_type {
            EventType::ButtonPress(button) => DeviceEvent {
                kind: DeviceEventKind::MousePress,
                value: json!(format!("{:?}", button)),
            },
            EventType::ButtonRelease(button) => DeviceEvent {
                kind: DeviceEventKind::MouseRelease,
                value: json!(format!("{:?}", button)),
            },
            EventType::MouseMove { x, y } => DeviceEvent {
                kind: DeviceEventKind::MouseMove,
                value: json!({ "x": x, "y": y }),
            },
            EventType::KeyPress(key) => DeviceEvent {
                kind: DeviceEventKind::KeyboardPress,
                value: json!({
                    "code": format!("{:?}", key),
                    "label": label,
                }),
            },
            EventType::KeyRelease(key) => DeviceEvent {
                kind: DeviceEventKind::KeyboardRelease,
                value: json!({
                    "code": format!("{:?}", key),
                    "label": label,
                }),
            },
            _ => return,
        };

        let _ = app_handle.emit("device-changed", device_event);
    };

    listen(callback).map_err(|err| format!("Failed to listen device: {:?}", err))?;

    Ok(())
}
