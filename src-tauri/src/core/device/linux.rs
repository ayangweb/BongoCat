use input::{
    event::{
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::ButtonState,
        PointerEvent,
    },
    Event, Libinput, LibinputInterface,
};
use nix::{
    libc::{O_RDONLY, O_RDWR, O_WRONLY},
    poll::{poll, PollFd, PollFlags, PollTimeout},
};
use std::{
    fs::{File, OpenOptions}, os::{fd::{AsFd, OwnedFd}, unix::prelude::OpenOptionsExt}, path::Path
};

use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Runtime, command};

use crate::core::{device::{DeviceEvent, DeviceKind}, setup::key_from_code};

static IS_RUNNING: AtomicBool = AtomicBool::new(false);

pub struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .custom_flags(flags)
            .read((flags & O_RDONLY != 0) | (flags & O_RDWR != 0))
            .write((flags & O_WRONLY != 0) | (flags & O_RDWR != 0))
            .open(path)
            .map(|file| file.into())
            .map_err(|err| err.raw_os_error().unwrap())
    }

    #[allow(unused_must_use)]
    fn close_restricted(&mut self, fd: OwnedFd) {
        File::from(fd);
    }
}

fn build_device_event(event: &Event) -> Option<DeviceEvent> {
    match event {
        Event::Keyboard(ev) => {
            let key_code = ev.key();
            let key_name = match key_from_code(key_code) {
                Some(name) => name.to_string(),
                None => format!("Unknown({})", key_code),
            };
            match ev.key_state() {
                KeyState::Pressed => Some(DeviceEvent {
                    kind: DeviceKind::KeyboardPress,
                    value: json!(key_name), 
                }),
                KeyState::Released => Some(DeviceEvent{
                    kind: DeviceKind::KeyboardRelease,
                    value: json!(key_name),
                }) 
            }
        },
        Event::Pointer(ev) => {
            match ev {
                PointerEvent::Button(e) => {
                    let btn_code = e.button();
                    let btn_name = match btn_code {
                        0x110 => String::from("Left"),
                        0x111 => String::from("Right"),
                        0x112 => String::from("Middle"),
                        _ => format!("Unknown({})", btn_code as u8),
                    };
                    match e.button_state() {
                        ButtonState::Pressed => Some(DeviceEvent {
                            kind: DeviceKind::MousePress,
                            value: json!(btn_name), 
                        }),
                        ButtonState::Released => Some(DeviceEvent {
                            kind: DeviceKind::MouseRelease,
                            value: json!(btn_name), 
                        })
                    }
                },
                PointerEvent::Motion(e) => {
                    Some(DeviceEvent {
                        kind: DeviceKind::MouseMove,
                        value: json!({
                            "x": e.dx_unaccelerated(),
                            "y": e.dy_unaccelerated()
                        }),
                    })
                },
                _ => None,
            }
        },
        _ => None,
    }
}


#[command]
pub async fn start_device_listening<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    if IS_RUNNING.load(Ordering::SeqCst) {
        return Err("Device is already listening".to_string());
    }

    IS_RUNNING.store(true, Ordering::SeqCst);

    let mut input = Libinput::new_with_udev(Interface);
    match input.udev_assign_seat("seat0") {
        Ok(_) => {
            let input_clone = &input.clone();
            let mut pollfds = [PollFd::new(input_clone.as_fd(), PollFlags::POLLIN)];
            while poll(&mut pollfds, PollTimeout::NONE).is_ok() {
                input.dispatch().unwrap();
                for event in &mut input {
                    let device_event = build_device_event(&event);
                    if let Some(e) = device_event {
                        app_handle.emit("device-changed", e).unwrap();
                    }
                }
            }
        },
        Err(_) => return Err("Failed to assign seat".to_string()),
    }

    Ok(())
}
