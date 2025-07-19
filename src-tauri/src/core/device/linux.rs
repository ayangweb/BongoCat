use crate::core::device::{DeviceEvent, DeviceEventKind};
use input::{
    Event, Libinput, LibinputInterface,
    event::{
        PointerEvent,
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::ButtonState,
    },
};
use nix::{
    libc::{O_RDONLY, O_RDWR, O_WRONLY},
    poll::{PollFd, PollFlags, PollTimeout, poll},
};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsFd, OwnedFd},
        unix::prelude::OpenOptionsExt,
    },
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, Runtime, command};

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

pub fn keyname_from_code(code: u32) -> Option<&'static str> {
    match code {
        // Function key
        1 => Some("Escape"),
        28 => Some("Return"),
        14 => Some("Backspace"),
        15 => Some("Tab"),
        57 => Some("Space"),
        58 => Some("CapsLock"),
        99 => Some("PrintScreen"),
        70 => Some("ScrollLock"),
        119 => Some("Pause"),
        69 => Some("NumLock"),
        110 => Some("Insert"),
        102 => Some("Home"),
        107 => Some("End"),
        104 => Some("PageUp"),
        109 => Some("PageDown"),
        111 => Some("Delete"),

        // Arrow key
        103 => Some("UpArrow"),
        108 => Some("DownArrow"),
        105 => Some("LeftArrow"),
        106 => Some("RightArrow"),

        // F key
        59 => Some("F1"),
        60 => Some("F2"),
        61 => Some("F3"),
        62 => Some("F4"),
        63 => Some("F5"),
        64 => Some("F6"),
        65 => Some("F7"),
        66 => Some("F8"),
        67 => Some("F9"),
        68 => Some("F10"),
        87 => Some("F11"),
        88 => Some("F12"),

        // Numeric
        2 => Some("Num1"),
        3 => Some("Num2"),
        4 => Some("Num3"),
        5 => Some("Num4"),
        6 => Some("Num5"),
        7 => Some("Num6"),
        8 => Some("Num7"),
        9 => Some("Num8"),
        10 => Some("Num9"),
        11 => Some("Num0"),

        // Alphabetic
        16 => Some("KeyQ"),
        17 => Some("KeyW"),
        18 => Some("KeyE"),
        19 => Some("KeyR"),
        20 => Some("KeyT"),
        21 => Some("KeyY"),
        22 => Some("KeyU"),
        23 => Some("KeyI"),
        24 => Some("KeyO"),
        25 => Some("KeyP"),
        30 => Some("KeyA"),
        31 => Some("KeyS"),
        32 => Some("KeyD"),
        33 => Some("KeyF"),
        34 => Some("KeyG"),
        35 => Some("KeyH"),
        36 => Some("KeyJ"),
        37 => Some("KeyK"),
        38 => Some("KeyL"),
        44 => Some("KeyZ"),
        45 => Some("KeyX"),
        46 => Some("KeyC"),
        47 => Some("KeyV"),
        48 => Some("KeyB"),
        49 => Some("KeyN"),
        50 => Some("KeyM"),

        // Symbolic
        41 => Some("BackQuote"),
        12 => Some("Minus"),
        13 => Some("Equal"),
        26 => Some("LeftBracket"),
        27 => Some("RightBracket"),
        39 => Some("SemiColon"),
        40 => Some("Quote"),
        43 => Some("BackSlash"),
        86 => Some("IntlBackslash"),
        89 => Some("IntlRo"),
        124 => Some("IntlYen"),
        101 => Some("KanaMode"),
        51 => Some("Comma"),
        52 => Some("Dot"),
        53 => Some("Slash"),

        // Control key
        29 => Some("ControlLeft"),
        97 => Some("ControlRight"),
        42 => Some("ShiftLeft"),
        54 => Some("ShiftRight"),
        56 => Some("Alt"),
        100 => Some("AltGr"),
        125 => Some("MetaLeft"),
        126 => Some("MetaRight"),
        127 => Some("Apps"),

        // NumPad
        55 => Some("KpMultiply"),
        78 => Some("KpMinus"),
        74 => Some("KpPlus"),
        98 => Some("KpDivide"),
        117 => Some("KpEqual"),
        121 => Some("KpComma"),
        96 => Some("KpReturn"),
        83 => Some("KpDecimal"),
        79 => Some("Kp1"),
        80 => Some("Kp2"),
        81 => Some("Kp3"),
        75 => Some("Kp4"),
        76 => Some("Kp5"),
        77 => Some("Kp6"),
        71 => Some("Kp7"),
        72 => Some("Kp8"),
        73 => Some("Kp9"),
        82 => Some("Kp0"),

        // Media key
        115 => Some("VolumeUp"),
        114 => Some("VolumeDown"),
        113 => Some("VolumeMute"),

        // Language key
        90 => Some("Lang1"),
        91 => Some("Lang2"),
        92 => Some("Lang3"),
        93 => Some("Lang4"),
        94 => Some("Lang5"),

        // Other
        _ => None,
    }
}

fn build_device_event(event: &Event) -> Option<DeviceEvent> {
    match event {
        Event::Keyboard(ev) => {
            let key_code = ev.key();
            let key_name = match keyname_from_code(key_code) {
                Some(name) => name.to_string(),
                None => format!("Unknown({})", key_code),
            };
            match ev.key_state() {
                KeyState::Pressed => Some(DeviceEvent {
                    kind: DeviceEventKind::KeyboardPress,
                    value: json!(key_name),
                }),
                KeyState::Released => Some(DeviceEvent {
                    kind: DeviceEventKind::KeyboardRelease,
                    value: json!(key_name),
                }),
            }
        }
        Event::Pointer(ev) => match ev {
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
                        kind: DeviceEventKind::MousePress,
                        value: json!(btn_name),
                    }),
                    ButtonState::Released => Some(DeviceEvent {
                        kind: DeviceEventKind::MouseRelease,
                        value: json!(btn_name),
                    }),
                }
            }
            PointerEvent::Motion(e) => Some(DeviceEvent {
                kind: DeviceEventKind::MouseMove,
                value: json!({
                    "x": e.dx_unaccelerated(),
                    "y": e.dy_unaccelerated()
                }),
            }),
            _ => None,
        },
        _ => None,
    }
}

#[command]
pub async fn start_device_listening<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    if IS_RUNNING.load(Ordering::SeqCst) {
        return Ok(());
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
        }
        Err(err) => return Err(format!("Failed to assign seat: {:?}", err)),
    }

    Ok(())
}
