use gilrs::{EventType, Gilrs};
use serde::Serialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Runtime, command};

static IS_LISTENING: AtomicBool = AtomicBool::new(false);
const POLL_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, Clone, Serialize)]
pub enum GamepadEventKind {
    ButtonChanged,
    AxisChanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct GamepadEvent {
    pub(crate) kind: GamepadEventKind,
    pub(crate) name: String,
    pub(crate) value: f32,
}

#[command]
pub async fn start_gamepad_listing<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    if IS_LISTENING.load(Ordering::SeqCst) {
        return Ok(());
    }

    IS_LISTENING.store(true, Ordering::SeqCst);

    thread::spawn(move || {
        run_gamepad_listener(app_handle);
        IS_LISTENING.store(false, Ordering::SeqCst);
    });

    Ok(())
}

fn run_gamepad_listener<R: Runtime>(app_handle: AppHandle<R>) {
    #[cfg(target_os = "windows")]
    {
        match crate::core::dualsense::run_usb_listener(&app_handle, &IS_LISTENING) {
            crate::core::dualsense::RunResult::Stopped => return,
            crate::core::dualsense::RunResult::Unavailable
            | crate::core::dualsense::RunResult::Disconnected => {}
        }
    }

    let _ = run_gilrs_listener(app_handle);
}

fn run_gilrs_listener<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    let mut gilrs = Gilrs::new().map_err(|err| err.to_string())?;

    while IS_LISTENING.load(Ordering::SeqCst) {
        while let Some(event) = gilrs.next_event() {
            let gamepad_event = match event.event {
                EventType::ButtonChanged(button, value, ..) => GamepadEvent {
                    kind: GamepadEventKind::ButtonChanged,
                    name: format!("{:?}", button),
                    value,
                },
                EventType::AxisChanged(axis, value, ..) => GamepadEvent {
                    kind: GamepadEventKind::AxisChanged,
                    name: format!("{:?}", axis),
                    value,
                },
                _ => continue,
            };

            let _ = app_handle.emit("gamepad-changed", gamepad_event);
        }

        thread::sleep(POLL_INTERVAL);
    }

    Ok(())
}

#[command]
pub async fn stop_gamepad_listing() {
    if !IS_LISTENING.load(Ordering::SeqCst) {
        return;
    }

    IS_LISTENING.store(false, Ordering::SeqCst);
}
