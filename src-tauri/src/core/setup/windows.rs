use crate::core::device::{DeviceEventKind, emit_device_event};
use std::mem::{size_of, zeroed};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use tauri::{AppHandle, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAW_INPUT_DATA_COMMAND_FLAGS, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEMOUSE,
    RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_INPUT, WNDPROC,
};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static ORIGINAL_WND_PROC: AtomicIsize = AtomicIsize::new(0);
static RAW_INPUT_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn platform(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    _preference_window: WebviewWindow,
) {
    if RAW_INPUT_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    let _ = APP_HANDLE.set(app_handle.clone());

    let Ok(hwnd) = main_window.hwnd() else {
        RAW_INPUT_INSTALLED.store(false, Ordering::SeqCst);
        return;
    };

    unsafe {
        if register_raw_mouse(hwnd).is_err() {
            RAW_INPUT_INSTALLED.store(false, Ordering::SeqCst);
            return;
        }

        let previous =
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, raw_input_wnd_proc as *const () as isize);

        if previous == 0 {
            RAW_INPUT_INSTALLED.store(false, Ordering::SeqCst);
            return;
        }

        ORIGINAL_WND_PROC.store(previous, Ordering::SeqCst);
    }
}

unsafe fn register_raw_mouse(hwnd: HWND) -> windows::core::Result<()> {
    let devices = [RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    }];

    unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) }
}

unsafe extern "system" fn raw_input_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        if let Some((x, y)) = unsafe { read_mouse_delta(lparam) } {
            if let Some(app_handle) = APP_HANDLE.get() {
                emit_device_event(
                    app_handle,
                    DeviceEventKind::MouseMoveDelta,
                    serde_json::json!({ "x": x, "y": y }),
                );
            }
        }

        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    unsafe { call_original_wnd_proc(hwnd, msg, wparam, lparam) }
}

unsafe fn call_original_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let previous = ORIGINAL_WND_PROC.load(Ordering::SeqCst);

    if previous == 0 {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    let previous_proc: WNDPROC = Some(unsafe { std::mem::transmute(previous) });

    unsafe { CallWindowProcW(previous_proc, hwnd, msg, wparam, lparam) }
}

unsafe fn read_mouse_delta(lparam: LPARAM) -> Option<(i32, i32)> {
    let mut raw_input: RAWINPUT = unsafe { zeroed() };
    let mut raw_input_size = size_of::<RAWINPUT>() as u32;
    let header_size = size_of::<RAWINPUTHEADER>() as u32;

    let status = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as *mut _),
            RAW_INPUT_DATA_COMMAND_FLAGS(RID_INPUT.0),
            Some(&mut raw_input as *mut _ as *mut _),
            &mut raw_input_size,
            header_size,
        )
    };

    if status == u32::MAX || status == 0 || raw_input.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }

    let mouse = unsafe { raw_input.data.mouse };

    if mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0 != 0 {
        return None;
    }

    if mouse.lLastX == 0 && mouse.lLastY == 0 {
        return None;
    }

    Some((mouse.lLastX, mouse.lLastY))
}
