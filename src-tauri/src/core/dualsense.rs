use crate::core::gamepad::{GamepadEvent, GamepadEventKind};
use std::{
    mem::{offset_of, size_of},
    slice,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, Runtime};
use windows::{
    Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
                SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
                SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
                SetupDiGetDeviceInterfaceDetailW,
            },
            HumanInterfaceDevice::{
                HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FlushQueue,
                HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
                HidP_GetCaps, PHIDP_PREPARSED_DATA,
            },
        },
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadFile,
        },
    },
    core::PCWSTR,
};

const SONY_VENDOR_ID: u16 = 0x054C;
const DUALSENSE_PRODUCT_ID: u16 = 0x0CE6;
const USB_REPORT_ID: u8 = 0x01;
const USB_INPUT_REPORT_LEN: usize = 64;

const STICK_DEADZONE: f32 = 0.08;
const STICK_EPSILON: f32 = 0.01;
const TRIGGER_THRESHOLD: u8 = 15;

const BTN_SQUARE: u8 = 0x10;
const BTN_CROSS: u8 = 0x20;
const BTN_CIRCLE: u8 = 0x40;
const BTN_TRIANGLE: u8 = 0x80;
const DPAD_LEFT: u8 = 0x01;
const DPAD_DOWN: u8 = 0x02;
const DPAD_RIGHT: u8 = 0x04;
const DPAD_UP: u8 = 0x08;

const BTN_LEFT_BUMPER: u8 = 0x01;
const BTN_RIGHT_BUMPER: u8 = 0x02;
const BTN_LEFT_TRIGGER: u8 = 0x04;
const BTN_RIGHT_TRIGGER: u8 = 0x08;
const BTN_LEFT_STICK: u8 = 0x40;
const BTN_RIGHT_STICK: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Unavailable,
    Stopped,
    Disconnected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DualSenseState {
    left_stick_x: i8,
    left_stick_y: i8,
    right_stick_x: i8,
    right_stick_y: i8,
    left_trigger: u8,
    right_trigger: u8,
    buttons_and_dpad: u8,
    buttons_a: u8,
}

struct HidDeviceInfoSet(HDEVINFO);

impl Drop for HidDeviceInfoSet {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

struct PreparsedData(PHIDP_PREPARSED_DATA);

impl Drop for PreparsedData {
    fn drop(&mut self) {
        unsafe {
            let _ = HidD_FreePreparsedData(self.0);
        }
    }
}

struct HidDevice(HANDLE);

impl HidDevice {
    fn read_state(&self) -> Option<DualSenseState> {
        let mut buffer = [0; USB_INPUT_REPORT_LEN];
        let mut bytes_read = 0;

        unsafe {
            let _ = HidD_FlushQueue(self.0);

            ReadFile(self.0, Some(&mut buffer), Some(&mut bytes_read), None).ok()?;
        }

        if bytes_read as usize != USB_INPUT_REPORT_LEN || buffer[0] != USB_REPORT_ID {
            return None;
        }

        Some(parse_usb_input_report(&buffer))
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn run_usb_listener<R: Runtime>(
    app_handle: &AppHandle<R>,
    is_listening: &AtomicBool,
) -> RunResult {
    let Some(device) = open_first_usb_dualsense() else {
        return RunResult::Unavailable;
    };

    let mut previous = DualSenseState::default();
    let mut has_previous = false;

    while is_listening.load(Ordering::SeqCst) {
        let Some(current) = device.read_state() else {
            if has_previous {
                emit_state_changes(app_handle, Some(previous), DualSenseState::default());
            }

            return RunResult::Disconnected;
        };

        emit_state_changes(app_handle, has_previous.then_some(previous), current);
        previous = current;
        has_previous = true;
    }

    if has_previous {
        emit_state_changes(app_handle, Some(previous), DualSenseState::default());
    }

    RunResult::Stopped
}

fn open_first_usb_dualsense() -> Option<HidDevice> {
    let hid_guid = unsafe { HidD_GetHidGuid() };
    let device_info_set = unsafe {
        SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            None,
            DIGCF_DEVICEINTERFACE | DIGCF_PRESENT,
        )
        .ok()?
    };
    let device_info_set = HidDeviceInfoSet(device_info_set);

    let mut index = 0;
    loop {
        let mut interface_data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };

        let enumerated = unsafe {
            SetupDiEnumDeviceInterfaces(
                device_info_set.0,
                None,
                &hid_guid,
                index,
                &mut interface_data,
            )
            .is_ok()
        };

        if !enumerated {
            break;
        }

        if let Some(path) = get_device_path(device_info_set.0, &interface_data) {
            if let Some(device) = open_hid_device(&path) {
                if is_usb_dualsense(&device) {
                    return Some(device);
                }
            }
        }

        index += 1;
    }

    None
}

fn get_device_path(
    device_info_set: HDEVINFO,
    interface_data: &SP_DEVICE_INTERFACE_DATA,
) -> Option<Vec<u16>> {
    let mut required_size = 0;

    unsafe {
        let _ = SetupDiGetDeviceInterfaceDetailW(
            device_info_set,
            interface_data,
            None,
            0,
            Some(&mut required_size),
            None,
        );
    }

    if required_size == 0 {
        return None;
    }

    let mut detail_buffer = vec![0u8; required_size as usize];
    let detail_data = detail_buffer
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();

    unsafe {
        (*detail_data).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

        SetupDiGetDeviceInterfaceDetailW(
            device_info_set,
            interface_data,
            Some(detail_data),
            required_size,
            None,
            None,
        )
        .ok()?;
    }

    let path_offset = offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
    let path_len = (required_size as usize - path_offset) / size_of::<u16>();
    let path = unsafe {
        slice::from_raw_parts(
            detail_buffer.as_ptr().add(path_offset).cast::<u16>(),
            path_len,
        )
    };
    let nul_index = path.iter().position(|char| *char == 0)?;

    Some(path[..=nul_index].to_vec())
}

fn open_hid_device(path: &[u16]) -> Option<HidDevice> {
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .ok()?
    };

    Some(HidDevice(handle))
}

fn is_usb_dualsense(device: &HidDevice) -> bool {
    let mut attributes = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };

    let matches_ids = unsafe { HidD_GetAttributes(device.0, &mut attributes) }
        && attributes.VendorID == SONY_VENDOR_ID
        && attributes.ProductID == DUALSENSE_PRODUCT_ID;

    if !matches_ids {
        return false;
    }

    let mut preparsed_data = PHIDP_PREPARSED_DATA::default();
    if !unsafe { HidD_GetPreparsedData(device.0, &mut preparsed_data) } {
        return false;
    }

    let preparsed_data = PreparsedData(preparsed_data);
    let mut caps = HIDP_CAPS::default();
    let status = unsafe { HidP_GetCaps(preparsed_data.0, &mut caps) };

    status == HIDP_STATUS_SUCCESS && caps.InputReportByteLength as usize == USB_INPUT_REPORT_LEN
}

fn parse_usb_input_report(buffer: &[u8; USB_INPUT_REPORT_LEN]) -> DualSenseState {
    let raw_buttons_and_dpad = buffer[8];

    DualSenseState {
        left_stick_x: normalize_raw_stick_x(buffer[1]),
        left_stick_y: normalize_raw_stick_y(buffer[2]),
        right_stick_x: normalize_raw_stick_x(buffer[3]),
        right_stick_y: normalize_raw_stick_y(buffer[4]),
        left_trigger: buffer[5],
        right_trigger: buffer[6],
        buttons_and_dpad: (raw_buttons_and_dpad & 0xF0) | dpad_mask(raw_buttons_and_dpad & 0x0F),
        buttons_a: buffer[9],
    }
}

fn normalize_raw_stick_x(value: u8) -> i8 {
    (value as i16 - 128) as i8
}

fn normalize_raw_stick_y(value: u8) -> i8 {
    ((value as i16 - 127) * -1) as i8
}

fn dpad_mask(value: u8) -> u8 {
    match value {
        0x0 => DPAD_UP,
        0x1 => DPAD_RIGHT | DPAD_UP,
        0x2 => DPAD_RIGHT,
        0x3 => DPAD_RIGHT | DPAD_DOWN,
        0x4 => DPAD_DOWN,
        0x5 => DPAD_LEFT | DPAD_DOWN,
        0x6 => DPAD_LEFT,
        0x7 => DPAD_LEFT | DPAD_UP,
        _ => 0,
    }
}

fn emit_state_changes<R: Runtime>(
    app_handle: &AppHandle<R>,
    previous: Option<DualSenseState>,
    current: DualSenseState,
) {
    emit_axis_if_changed(
        app_handle,
        previous.map(|state| normalize_stick(state.left_stick_x)),
        "LeftStickX",
        normalize_stick(current.left_stick_x),
    );
    emit_axis_if_changed(
        app_handle,
        previous.map(|state| normalize_stick(state.left_stick_y)),
        "LeftStickY",
        normalize_stick(current.left_stick_y),
    );
    emit_axis_if_changed(
        app_handle,
        previous.map(|state| normalize_stick(state.right_stick_x)),
        "RightStickX",
        normalize_stick(current.right_stick_x),
    );
    emit_axis_if_changed(
        app_handle,
        previous.map(|state| normalize_stick(state.right_stick_y)),
        "RightStickY",
        normalize_stick(current.right_stick_y),
    );

    for (mask, name) in [
        (BTN_CROSS, "South"),
        (BTN_CIRCLE, "East"),
        (BTN_SQUARE, "West"),
        (BTN_TRIANGLE, "North"),
        (DPAD_UP, "DPadUp"),
        (DPAD_DOWN, "DPadDown"),
        (DPAD_LEFT, "DPadLeft"),
        (DPAD_RIGHT, "DPadRight"),
    ] {
        emit_button_if_changed(
            app_handle,
            previous.map(|state| state.buttons_and_dpad & mask != 0),
            name,
            current.buttons_and_dpad & mask != 0,
        );
    }

    for (mask, name) in [
        (BTN_LEFT_BUMPER, "LeftTrigger"),
        (BTN_RIGHT_BUMPER, "RightTrigger"),
        (BTN_LEFT_STICK, "LeftThumb"),
        (BTN_RIGHT_STICK, "RightThumb"),
    ] {
        emit_button_if_changed(
            app_handle,
            previous.map(|state| state.buttons_a & mask != 0),
            name,
            current.buttons_a & mask != 0,
        );
    }

    emit_button_if_changed(
        app_handle,
        previous.map(is_left_trigger_pressed),
        "LeftTrigger2",
        is_left_trigger_pressed(current),
    );
    emit_button_if_changed(
        app_handle,
        previous.map(is_right_trigger_pressed),
        "RightTrigger2",
        is_right_trigger_pressed(current),
    );
}

fn emit_axis_if_changed<R: Runtime>(
    app_handle: &AppHandle<R>,
    previous: Option<f32>,
    name: &str,
    value: f32,
) {
    if previous.is_some_and(|previous| (previous - value).abs() < STICK_EPSILON) {
        return;
    }

    emit_gamepad_event(app_handle, GamepadEventKind::AxisChanged, name, value);
}

fn emit_button_if_changed<R: Runtime>(
    app_handle: &AppHandle<R>,
    previous: Option<bool>,
    name: &str,
    pressed: bool,
) {
    if previous.is_some_and(|previous| previous == pressed) {
        return;
    }

    emit_gamepad_event(
        app_handle,
        GamepadEventKind::ButtonChanged,
        name,
        if pressed { 1.0 } else { 0.0 },
    );
}

fn emit_gamepad_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    kind: GamepadEventKind,
    name: &str,
    value: f32,
) {
    let _ = app_handle.emit(
        "gamepad-changed",
        GamepadEvent {
            kind,
            name: name.to_owned(),
            value,
        },
    );
}

fn normalize_stick(value: i8) -> f32 {
    let value = if value >= 0 {
        value as f32 / 127.0
    } else {
        value as f32 / 128.0
    };

    if value.abs() < STICK_DEADZONE {
        0.0
    } else {
        value.clamp(-1.0, 1.0)
    }
}

fn is_left_trigger_pressed(state: DualSenseState) -> bool {
    state.left_trigger > TRIGGER_THRESHOLD || state.buttons_a & BTN_LEFT_TRIGGER != 0
}

fn is_right_trigger_pressed(state: DualSenseState) -> bool {
    state.right_trigger > TRIGGER_THRESHOLD || state.buttons_a & BTN_RIGHT_TRIGGER != 0
}
