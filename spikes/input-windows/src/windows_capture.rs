use bongocat_input_windows_spike::{
    CaptureResetReason, KeyStateSnapshot, PhysicalKey, PressedKeyCandidates, RawInputDeviceChange,
    RawInputHeader, collect_key_state_snapshot_with, decode_keyboard_packet,
    decode_raw_keyboard_bytes,
};
use std::{
    collections::BTreeSet,
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        System::StationsAndDesktops::{
            CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, OpenInputDesktop,
        },
        UI::{
            Input::{
                GetRawInputData, HRAWINPUT, KeyboardAndMouse::GetAsyncKeyState, RAWINPUTDEVICE,
                RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDEV_REMOVE,
                RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GIDC_ARRIVAL, GIDC_REMOVAL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
                HWND_MESSAGE, KillTimer, MSG, PostQuitMessage, RegisterClassW, SetTimer,
                SetWindowLongPtrW, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_DESTROY, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_NCCREATE,
                WM_NCDESTROY, WM_TIMER, WNDCLASSW,
            },
        },
    },
    core::{Error, Result as WindowsResult, w},
};

const TIMER_ID: usize = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegistrationReport {
    pub registered: bool,
    pub clean_shutdown: bool,
    pub raw_messages: u64,
    pub keyboard_edges: u64,
    pub device_arrivals: u64,
    pub device_removals: u64,
    pub resets: u64,
    pub device_removed_resets: u64,
    pub service_stopped_resets: u64,
    pub decode_errors: u64,
    pub callback_panics: u64,
}

#[derive(Default)]
struct WindowState {
    raw_messages: u64,
    keyboard_edges: u64,
    device_arrivals: u64,
    device_removals: u64,
    pressed_candidates: PressedKeyCandidates,
    decode_errors: u64,
    callback_panics: u64,
}

impl WindowState {
    fn report(&self, registered: bool, clean_shutdown: bool) -> RegistrationReport {
        let candidate_counters = self.pressed_candidates.counters();
        RegistrationReport {
            registered,
            clean_shutdown,
            raw_messages: self.raw_messages,
            keyboard_edges: self.keyboard_edges,
            device_arrivals: self.device_arrivals,
            device_removals: self.device_removals,
            resets: candidate_counters.resets,
            device_removed_resets: candidate_counters.device_removed_resets,
            service_stopped_resets: candidate_counters.service_stopped_resets,
            decode_errors: self.decode_errors,
            callback_panics: self.callback_panics,
        }
    }
}

pub fn run_registration_smoke(duration: Duration) -> WindowsResult<RegistrationReport> {
    // SAFETY: all Win32 window operations remain on this thread. The boxed
    // state outlives the HWND and is released only after WM_NCDESTROY clears
    // GWLP_USERDATA and the message loop exits.
    unsafe { run_registration_smoke_inner(duration) }
}

pub fn query_pressed_keys(candidates: &BTreeSet<PhysicalKey>) -> WindowsResult<KeyStateSnapshot> {
    // SAFETY: OpenInputDesktop yields an owned HDESK used only as an
    // availability guard. Every successful open is paired with CloseDesktop,
    // and GetAsyncKeyState receives only validated virtual-key integers.
    unsafe {
        let input_desktop =
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS::default(), false, DESKTOP_READOBJECTS)?;
        let report = collect_key_state_snapshot_with(candidates, |virtual_key| {
            GetAsyncKeyState(virtual_key.as_i32()) as u16 & 0x8000 != 0
        });
        CloseDesktop(input_desktop)?;
        Ok(report)
    }
}

unsafe fn run_registration_smoke_inner(duration: Duration) -> WindowsResult<RegistrationReport> {
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    let class_name = w!("BongoCatInputSpikeWindow");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(Error::from_win32());
    }

    let mut state = Box::<WindowState>::default();
    let state_ptr = (&mut *state) as *mut WindowState;
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("BongoCat Raw Input Spike"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            Some(state_ptr.cast()),
        )
    };
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
            return Err(error);
        }
    };

    let devices = [
        raw_input_device(0x02, window),
        raw_input_device(0x06, window),
    ];
    if let Err(error) =
        unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) }
    {
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
        return Err(error);
    }

    let timeout_ms = duration.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
    if unsafe { SetTimer(Some(window), TIMER_ID, timeout_ms, None) } == 0 {
        let error = Error::from_win32();
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
        return Err(error);
    }

    let mut message = MSG::default();
    let message_error = loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            break Some(Error::from_win32());
        }
        if !result.as_bool() {
            break None;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    let removal_result = unsafe { unregister_raw_input_devices() };
    let class_result = unsafe { UnregisterClassW(class_name, Some(instance)) };
    if let Some(error) = message_error {
        return Err(error);
    }
    removal_result?;
    class_result?;
    Ok(state.report(true, true))
}

fn raw_input_device(usage: u16, window: HWND) -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: usage,
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: window,
    }
}

unsafe fn unregister_raw_input_devices() -> WindowsResult<()> {
    let devices = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
    ];
    unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        if !create.is_null() {
            // SAFETY: CreateWindowExW receives a pointer to the live boxed
            // WindowState and WM_NCCREATE receives the matching CREATESTRUCTW.
            let state = unsafe { (*create).lpCreateParams as *mut WindowState };
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, state as isize) };
        }
    }

    let state = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowState };
    let outcome = catch_unwind(AssertUnwindSafe(|| match message {
        WM_INPUT if !state.is_null() => {
            // SAFETY: GWLP_USERDATA points to the WindowState owned by
            // run_registration_smoke_inner until this HWND is destroyed.
            let state = unsafe { &mut *state };
            state.raw_messages += 1;
            match unsafe { read_keyboard_edge(lparam) } {
                Ok(Some(edge)) => {
                    state.keyboard_edges += 1;
                    state.pressed_candidates.apply_edge(edge);
                }
                Ok(None) => {}
                Err(()) => state.decode_errors += 1,
            }
            None
        }
        WM_INPUT_DEVICE_CHANGE if !state.is_null() => {
            // SAFETY: GWLP_USERDATA points to the live WindowState until
            // WM_NCDESTROY clears it.
            let state = unsafe { &mut *state };
            let change = match wparam.0 as u32 {
                GIDC_ARRIVAL => {
                    state.device_arrivals += 1;
                    RawInputDeviceChange::Arrival
                }
                GIDC_REMOVAL => {
                    state.device_removals += 1;
                    RawInputDeviceChange::Removal
                }
                _ => RawInputDeviceChange::Unknown,
            };
            state.pressed_candidates.apply_device_change(change);
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            let _ = unsafe { KillTimer(Some(window), TIMER_ID) };
            let _ = unsafe { DestroyWindow(window) };
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            if !state.is_null() {
                // SAFETY: WM_DESTROY runs before WM_NCDESTROY clears the
                // WindowState pointer, so every normal destruction path can
                // reset its platform-local candidates exactly once.
                unsafe {
                    (*state)
                        .pressed_candidates
                        .reset(CaptureResetReason::ServiceStopped)
                };
            }
            unsafe { PostQuitMessage(0) };
            Some(LRESULT(0))
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
            None
        }
        _ => None,
    }));

    match outcome {
        Ok(Some(result)) => result,
        Ok(None) => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        Err(_) => {
            if !state.is_null() {
                // SAFETY: the pointer is valid until WM_NCDESTROY clears it.
                unsafe { (*state).callback_panics += 1 };
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}

unsafe fn read_keyboard_edge(
    lparam: LPARAM,
) -> Result<Option<bongocat_input_windows_spike::KeyboardEdge>, ()> {
    let raw_input = HRAWINPUT(lparam.0 as *mut c_void);
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let mut byte_count = 0u32;
    let query =
        unsafe { GetRawInputData(raw_input, RID_INPUT, None, &mut byte_count, header_size) };
    if query == u32::MAX || byte_count < header_size {
        return Err(());
    }

    let word_count = (byte_count as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; word_count];
    let read = unsafe {
        GetRawInputData(
            raw_input,
            RID_INPUT,
            Some(storage.as_mut_ptr().cast()),
            &mut byte_count,
            header_size,
        )
    };
    if read == u32::MAX || read < header_size {
        return Err(());
    }

    // SAFETY: storage is usize-aligned, contains at least `read` initialized
    // bytes from GetRawInputData, and remains alive while both views are used.
    let bytes = unsafe { slice::from_raw_parts(storage.as_ptr().cast::<u8>(), read as usize) };
    let native_header = unsafe { (bytes.as_ptr() as *const RAWINPUTHEADER).read_unaligned() };
    if native_header.dwType != 1 {
        return Ok(None);
    }
    let packet = decode_raw_keyboard_bytes(
        RawInputHeader {
            declared_size: native_header.dwSize as usize,
            input_type: native_header.dwType,
        },
        bytes,
        header_size as usize,
    )
    .map_err(|_| ())?;
    Ok(Some(decode_keyboard_packet(packet)))
}
