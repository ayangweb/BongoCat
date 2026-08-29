use bongocat_input_windows_spike::{
    CaptureResetReason, KeyStateSnapshot, KeyboardEdge, MouseButton, MouseButtonStateSnapshot,
    PhysicalKey, PressedKeyCandidates, PressedMouseCandidates, RawInputDeviceChange,
    RawInputHeader, RawMousePacket, collect_key_state_snapshot_with,
    collect_mouse_button_state_snapshot_with, decode_keyboard_packet, decode_mouse_button_edges,
    decode_raw_keyboard_bytes, decode_raw_mouse_bytes,
};
use std::{
    collections::{BTreeSet, VecDeque},
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
        System::RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
            WTSUnRegisterSessionNotification,
        },
        System::StationsAndDesktops::{
            CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, OpenInputDesktop,
        },
        UI::{
            Input::{
                GetRawInputData, HRAWINPUT,
                KeyboardAndMouse::{
                    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_MOVE,
                    MOUSEEVENTF_MOVE_NOCOALESCE, MOUSEINPUT, SendInput, VIRTUAL_KEY,
                },
                RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK,
                RIDEV_REMOVE, RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GIDC_ARRIVAL, GIDC_REMOVAL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
                KillTimer, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
                PBT_APMRESUMESTANDBY, PBT_APMRESUMESUSPEND, PBT_APMSTANDBY, PBT_APMSUSPEND,
                PostQuitMessage, RegisterClassW, SendMessageW, SetTimer, SetWindowLongPtrW,
                TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY,
                WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_NCCREATE, WM_NCDESTROY, WM_POWERBROADCAST,
                WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_CONSOLE_CONNECT,
                WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
                WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
            },
        },
    },
    core::{Error, Result as WindowsResult, w},
};

const STOP_TIMER_ID: usize = 1;
const RECONCILIATION_TIMER_ID: usize = 2;
const RECONCILIATION_INTERVAL_MS: u32 = 250;
const REQUIRED_MISSING_CONFIRMATIONS: u8 = 2;
const SYNTHETIC_INPUT_BATCH_SIZE: usize = 256;
pub const SYNTHETIC_POINTER_MOVES_PER_KEY_PAIR: usize = 4;
pub const SYNTHETIC_PRESSURE_KEY_COUNT: usize = 6;
pub const MAX_SYNTHETIC_PRESSURE_CYCLES: usize = 256;

const SYNTHETIC_PRESSURE_KEYS: [SyntheticKey; SYNTHETIC_PRESSURE_KEY_COUNT] = [
    SyntheticKey::new(0x1e, false, PhysicalKey::A),
    SyntheticKey::new(0x1f, false, PhysicalKey::S),
    SyntheticKey::new(0x39, false, PhysicalKey::Space),
    SyntheticKey::new(0x2a, false, PhysicalKey::ShiftLeft),
    SyntheticKey::new(0x1d, false, PhysicalKey::ControlLeft),
    SyntheticKey::new(0x1d, true, PhysicalKey::ControlRight),
];

#[derive(Clone, Copy)]
struct SyntheticKey {
    scan_code: u16,
    extended: bool,
    physical_key: PhysicalKey,
}

impl SyntheticKey {
    const fn new(scan_code: u16, extended: bool, physical_key: PhysicalKey) -> Self {
        Self {
            scan_code,
            extended,
            physical_key,
        }
    }
}

#[derive(Clone, Copy, Default)]
enum SmokeMode {
    #[default]
    Registration,
    Lifecycle,
    ReleaseRecovery,
    EdgePressure {
        cycles: usize,
    },
    PointerFlood {
        cycles: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegistrationReport {
    pub registered: bool,
    pub session_notifications_registered: bool,
    pub session_notifications_unregistered: bool,
    pub clean_shutdown: bool,
    pub raw_messages: u64,
    pub keyboard_edges: u64,
    pub mouse_messages: u64,
    pub mouse_button_edges: u64,
    pub mouse_captured_down: u64,
    pub mouse_captured_up: u64,
    pub mouse_duplicate_down: u64,
    pub mouse_unmatched_up: u64,
    pub mouse_resets: u64,
    pub mouse_reset_releases: u64,
    pub mouse_reconciled_releases: u64,
    pub mouse_candidates_remaining: usize,
    pub device_arrivals: u64,
    pub device_removals: u64,
    pub resets: u64,
    pub reset_releases: u64,
    pub device_removed_resets: u64,
    pub session_change_resets: u64,
    pub power_change_resets: u64,
    pub service_stopped_resets: u64,
    pub unqueryable_key_resets: u64,
    pub state_query_unavailable_resets: u64,
    pub reconciliation_runs: u64,
    pub reconciled_releases: u64,
    pub reconciliation_query_errors: u64,
    pub decode_errors: u64,
    pub callback_panics: u64,
    pub synthetic_inputs_sent: u64,
    pub synthetic_pointer_inputs_requested: u64,
    pub synthetic_expected_edges: u64,
    pub synthetic_edges_seen: u64,
    pub synthetic_down_edges: u64,
    pub synthetic_up_edges: u64,
    pub synthetic_order_errors: u64,
    pub synthetic_expected_edges_remaining: usize,
    pub intentionally_dropped_releases: u64,
    pub captured_down: u64,
    pub captured_up: u64,
    pub duplicate_down: u64,
    pub unmatched_up: u64,
    pub pressed_candidates_remaining: usize,
}

#[derive(Default)]
struct WindowState {
    session_notifications_registered: bool,
    session_notifications_unregistered: bool,
    raw_messages: u64,
    keyboard_edges: u64,
    mouse_messages: u64,
    mouse_button_edges: u64,
    device_arrivals: u64,
    device_removals: u64,
    pressed_candidates: PressedKeyCandidates,
    pressed_mouse_candidates: PressedMouseCandidates,
    reconciliation_runs: u64,
    reconciliation_query_errors: u64,
    decode_errors: u64,
    callback_panics: u64,
    synthetic_inputs_sent: u64,
    synthetic_pointer_inputs_requested: u64,
    synthetic_expected_edge_count: u64,
    synthetic_expected_edges: VecDeque<KeyboardEdge>,
    synthetic_edges_seen: u64,
    synthetic_down_edges: u64,
    synthetic_up_edges: u64,
    synthetic_order_errors: u64,
    intentionally_dropped_releases: u64,
    drop_next_release: bool,
}

impl WindowState {
    fn report(&self, registered: bool, clean_shutdown: bool) -> RegistrationReport {
        let candidate_counters = self.pressed_candidates.counters();
        let mouse_candidate_counters = self.pressed_mouse_candidates.counters();
        RegistrationReport {
            registered,
            session_notifications_registered: self.session_notifications_registered,
            session_notifications_unregistered: self.session_notifications_unregistered,
            clean_shutdown,
            raw_messages: self.raw_messages,
            keyboard_edges: self.keyboard_edges,
            mouse_messages: self.mouse_messages,
            mouse_button_edges: self.mouse_button_edges,
            mouse_captured_down: mouse_candidate_counters.captured_down,
            mouse_captured_up: mouse_candidate_counters.captured_up,
            mouse_duplicate_down: mouse_candidate_counters.duplicate_down,
            mouse_unmatched_up: mouse_candidate_counters.unmatched_up,
            mouse_resets: mouse_candidate_counters.resets,
            mouse_reset_releases: mouse_candidate_counters.reset_releases,
            mouse_reconciled_releases: mouse_candidate_counters.reconciled_releases,
            mouse_candidates_remaining: self.pressed_mouse_candidates.buttons().len(),
            device_arrivals: self.device_arrivals,
            device_removals: self.device_removals,
            resets: candidate_counters.resets,
            reset_releases: candidate_counters.reset_releases,
            device_removed_resets: candidate_counters.device_removed_resets,
            session_change_resets: candidate_counters.session_change_resets,
            power_change_resets: candidate_counters.power_change_resets,
            service_stopped_resets: candidate_counters.service_stopped_resets,
            unqueryable_key_resets: candidate_counters.unqueryable_key_resets,
            state_query_unavailable_resets: candidate_counters.state_query_unavailable_resets,
            reconciliation_runs: self.reconciliation_runs,
            reconciled_releases: candidate_counters.reconciled_releases,
            reconciliation_query_errors: self.reconciliation_query_errors,
            decode_errors: self.decode_errors,
            callback_panics: self.callback_panics,
            synthetic_inputs_sent: self.synthetic_inputs_sent,
            synthetic_pointer_inputs_requested: self.synthetic_pointer_inputs_requested,
            synthetic_expected_edges: self.synthetic_expected_edge_count,
            synthetic_edges_seen: self.synthetic_edges_seen,
            synthetic_down_edges: self.synthetic_down_edges,
            synthetic_up_edges: self.synthetic_up_edges,
            synthetic_order_errors: self.synthetic_order_errors,
            synthetic_expected_edges_remaining: self.synthetic_expected_edges.len(),
            intentionally_dropped_releases: self.intentionally_dropped_releases,
            captured_down: candidate_counters.captured_down,
            captured_up: candidate_counters.captured_up,
            duplicate_down: candidate_counters.duplicate_down,
            unmatched_up: candidate_counters.unmatched_up,
            pressed_candidates_remaining: self.pressed_candidates.keys().len(),
        }
    }

    fn reconcile_pressed_candidates(&mut self) {
        self.reconciliation_runs += 1;
        let candidates = self.pressed_candidates.keys().clone();
        if !candidates.is_empty() {
            match query_pressed_keys(&candidates) {
                Ok(snapshot) => {
                    self.pressed_candidates
                        .reconcile(&snapshot, REQUIRED_MISSING_CONFIRMATIONS)
                        .expect("non-zero reconciliation confirmation threshold");
                }
                Err(_) => {
                    self.reconciliation_query_errors += 1;
                    self.pressed_candidates
                        .reset(CaptureResetReason::StateQueryUnavailable);
                }
            }
        }
        let mouse_candidates = self.pressed_mouse_candidates.buttons().clone();
        if !mouse_candidates.is_empty() {
            match query_pressed_mouse_buttons(&mouse_candidates) {
                Ok(snapshot) => {
                    self.pressed_mouse_candidates
                        .reconcile(&snapshot, REQUIRED_MISSING_CONFIRMATIONS)
                        .expect("non-zero reconciliation confirmation threshold");
                }
                Err(_) => {
                    self.reconciliation_query_errors += 1;
                    self.pressed_mouse_candidates
                        .reset(CaptureResetReason::StateQueryUnavailable);
                }
            }
        }
    }

    fn reset_candidates(&mut self, reason: CaptureResetReason) {
        self.pressed_candidates.reset(reason);
        self.pressed_mouse_candidates.reset(reason);
    }

    fn observe_synthetic_edge(&mut self, edge: KeyboardEdge) {
        let expected = self.synthetic_expected_edges.pop_front();
        self.synthetic_edges_seen += 1;
        if edge.pressed {
            self.synthetic_down_edges += 1;
        } else {
            self.synthetic_up_edges += 1;
        }
        if expected != Some(edge) {
            self.synthetic_order_errors += 1;
        }
    }
}

pub fn run_registration_smoke(duration: Duration) -> WindowsResult<RegistrationReport> {
    // SAFETY: all Win32 window operations remain on this thread. The boxed
    // state outlives the HWND and is released only after WM_NCDESTROY clears
    // GWLP_USERDATA and the message loop exits.
    unsafe { run_registration_smoke_inner(duration, SmokeMode::Registration) }
}

pub fn run_lifecycle_smoke(duration: Duration) -> WindowsResult<RegistrationReport> {
    // SAFETY: this uses the same thread-confined window contract and sends only
    // synchronous lifecycle messages to its own hidden HWND.
    unsafe { run_registration_smoke_inner(duration, SmokeMode::Lifecycle) }
}

pub fn run_synthetic_release_recovery_smoke(
    duration: Duration,
) -> WindowsResult<RegistrationReport> {
    // SAFETY: SendInput is issued only after the thread-confined Raw Input
    // window is registered. The synthetic key is always paired with KeyUp;
    // only this spike's consumer intentionally ignores that captured release.
    unsafe { run_registration_smoke_inner(duration, SmokeMode::ReleaseRecovery) }
}

pub fn run_synthetic_edge_pressure_smoke(
    duration: Duration,
    cycles: usize,
) -> WindowsResult<RegistrationReport> {
    assert!(cycles > 0, "synthetic pressure cycles must be non-zero");
    assert!(
        cycles <= MAX_SYNTHETIC_PRESSURE_CYCLES,
        "synthetic pressure cycles exceed the bounded smoke limit"
    );
    // SAFETY: every injected scan-code down has a paired up, including the
    // cleanup path after partial SendInput failure. Expected edge storage and
    // the Raw Input HWND remain confined to this thread.
    unsafe { run_registration_smoke_inner(duration, SmokeMode::EdgePressure { cycles }) }
}

pub fn run_synthetic_pointer_flood_smoke(
    duration: Duration,
    cycles: usize,
) -> WindowsResult<RegistrationReport> {
    assert!(
        cycles > 0,
        "synthetic pointer flood cycles must be non-zero"
    );
    assert!(
        cycles <= MAX_SYNTHETIC_PRESSURE_CYCLES,
        "synthetic pointer flood cycles exceed the bounded smoke limit"
    );
    // SAFETY: relative pointer moves are paired to return the cursor to its
    // starting position. Keyboard down/up cleanup and owner confinement match
    // the edge-pressure smoke contract.
    unsafe { run_registration_smoke_inner(duration, SmokeMode::PointerFlood { cycles }) }
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

pub fn query_pressed_mouse_buttons(
    candidates: &BTreeSet<MouseButton>,
) -> WindowsResult<MouseButtonStateSnapshot> {
    // SAFETY: the input desktop guard and validated virtual-key contract match
    // query_pressed_keys; all five project mouse buttons have stable VK codes.
    unsafe {
        let input_desktop =
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS::default(), false, DESKTOP_READOBJECTS)?;
        let report = collect_mouse_button_state_snapshot_with(candidates, |virtual_key| {
            GetAsyncKeyState(virtual_key.as_i32()) as u16 & 0x8000 != 0
        });
        CloseDesktop(input_desktop)?;
        Ok(report)
    }
}

unsafe fn run_registration_smoke_inner(
    duration: Duration,
    mode: SmokeMode,
) -> WindowsResult<RegistrationReport> {
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
        return Err(Error::from_thread());
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
            None,
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

    if let Err(error) = unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } {
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
        return Err(error);
    }
    state.session_notifications_registered = true;

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

    if matches!(mode, SmokeMode::Lifecycle) {
        unsafe { inject_lifecycle_messages(window, state_ptr) };
    }

    let synthetic_input = match mode {
        SmokeMode::ReleaseRecovery => {
            state.drop_next_release = true;
            build_synthetic_sequence(&[SYNTHETIC_PRESSURE_KEYS[0]], 1)
        }
        SmokeMode::EdgePressure { cycles } => {
            build_synthetic_sequence(&SYNTHETIC_PRESSURE_KEYS, cycles)
        }
        SmokeMode::PointerFlood { cycles } => {
            build_pointer_flood_sequence(&SYNTHETIC_PRESSURE_KEYS, cycles)
        }
        SmokeMode::Registration | SmokeMode::Lifecycle => Vec::new(),
    };
    if !synthetic_input.is_empty() {
        state.synthetic_expected_edges = synthetic_input
            .iter()
            .filter_map(|input| input.expected_edge)
            .collect();
        state.synthetic_expected_edge_count = synthetic_input.len() as u64;
        state.synthetic_pointer_inputs_requested = synthetic_input
            .iter()
            .filter(|input| input.expected_edge.is_none())
            .count() as u64;
        state.synthetic_expected_edge_count -= state.synthetic_pointer_inputs_requested;
        match unsafe { send_synthetic_sequence(&synthetic_input) } {
            Ok(sent) => state.synthetic_inputs_sent = u64::from(sent),
            Err(error) => {
                let _ = unsafe { DestroyWindow(window) };
                let _ = unsafe { unregister_raw_input_devices() };
                let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
                return Err(error);
            }
        }
    }

    if unsafe {
        SetTimer(
            Some(window),
            RECONCILIATION_TIMER_ID,
            RECONCILIATION_INTERVAL_MS,
            None,
        )
    } == 0
    {
        let error = Error::from_thread();
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { unregister_raw_input_devices() };
        let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
        return Err(error);
    }
    let timeout_ms = duration.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
    if unsafe { SetTimer(Some(window), STOP_TIMER_ID, timeout_ms, None) } == 0 {
        let error = Error::from_thread();
        let _ = unsafe { KillTimer(Some(window), RECONCILIATION_TIMER_ID) };
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { unregister_raw_input_devices() };
        let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
        return Err(error);
    }

    let mut message = MSG::default();
    let message_error = loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            break Some(Error::from_thread());
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
    Ok(state.report(true, state.session_notifications_unregistered))
}

#[derive(Clone, Copy)]
struct SyntheticInput {
    input: INPUT,
    expected_edge: Option<KeyboardEdge>,
}

fn build_synthetic_sequence(keys: &[SyntheticKey], cycles: usize) -> Vec<SyntheticInput> {
    let mut sequence = Vec::with_capacity(keys.len() * cycles * 2);
    for _ in 0..cycles {
        for key in keys {
            sequence.push(SyntheticInput {
                input: synthetic_input(*key, false),
                expected_edge: Some(KeyboardEdge {
                    key: key.physical_key,
                    pressed: true,
                }),
            });
            sequence.push(SyntheticInput {
                input: synthetic_input(*key, true),
                expected_edge: Some(KeyboardEdge {
                    key: key.physical_key,
                    pressed: false,
                }),
            });
        }
    }
    sequence
}

fn build_pointer_flood_sequence(keys: &[SyntheticKey], cycles: usize) -> Vec<SyntheticInput> {
    let inputs_per_key = 2 + SYNTHETIC_POINTER_MOVES_PER_KEY_PAIR;
    let mut sequence = Vec::with_capacity(keys.len() * cycles * inputs_per_key);
    for _ in 0..cycles {
        for key in keys {
            sequence.push(pointer_move_input(1, 0));
            sequence.push(SyntheticInput {
                input: synthetic_input(*key, false),
                expected_edge: Some(KeyboardEdge {
                    key: key.physical_key,
                    pressed: true,
                }),
            });
            sequence.push(pointer_move_input(0, 1));
            sequence.push(pointer_move_input(-1, 0));
            sequence.push(SyntheticInput {
                input: synthetic_input(*key, true),
                expected_edge: Some(KeyboardEdge {
                    key: key.physical_key,
                    pressed: false,
                }),
            });
            sequence.push(pointer_move_input(0, -1));
        }
    }
    sequence
}

unsafe fn send_synthetic_sequence(sequence: &[SyntheticInput]) -> WindowsResult<u32> {
    let inputs = sequence.iter().map(|item| item.input).collect::<Vec<_>>();
    let mut total_sent = 0u32;
    for batch in inputs.chunks(SYNTHETIC_INPUT_BATCH_SIZE) {
        let sent = unsafe { SendInput(batch, size_of::<INPUT>() as i32) };
        total_sent += sent;
        if sent != batch.len() as u32 {
            // Best-effort cleanup prevents a partial injection from leaving the
            // runner's input state pressed even when the smoke must fail.
            let releases = SYNTHETIC_PRESSURE_KEYS
                .iter()
                .map(|key| synthetic_input(*key, true))
                .collect::<Vec<_>>();
            let _ = unsafe { SendInput(&releases, size_of::<INPUT>() as i32) };
            return Err(Error::from_thread());
        }
    }
    Ok(total_sent)
}

fn synthetic_input(key: SyntheticKey, released: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if released {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: key.scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn pointer_move_input(dx: i32, dy: i32) -> SyntheticInput {
    SyntheticInput {
        input: INPUT {
            r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        expected_edge: None,
    }
}

unsafe fn inject_lifecycle_messages(window: HWND, state: *mut WindowState) {
    for (message, event) in [
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK),
        (WM_POWERBROADCAST, PBT_APMSUSPEND),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
    ] {
        // SAFETY: state is the live Box installed in this window's user data.
        unsafe {
            (*state).pressed_candidates.apply_edge(KeyboardEdge {
                key: PhysicalKey::A,
                pressed: true,
            })
        };
        unsafe {
            SendMessageW(
                window,
                message,
                Some(WPARAM(event as usize)),
                Some(LPARAM(0)),
            )
        };
    }
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
            match unsafe { read_raw_input(lparam) } {
                Ok(Some(CapturedRawInput::Keyboard(edge))) => {
                    state.keyboard_edges += 1;
                    if !state.synthetic_expected_edges.is_empty() {
                        state.observe_synthetic_edge(edge);
                    } else if state.synthetic_edges_seen > 0 {
                        state.synthetic_edges_seen += 1;
                        state.synthetic_order_errors += 1;
                    }
                    if !edge.pressed && state.drop_next_release {
                        state.drop_next_release = false;
                        state.intentionally_dropped_releases += 1;
                    } else {
                        state.pressed_candidates.apply_edge(edge);
                    }
                }
                Ok(Some(CapturedRawInput::Mouse(packet))) => {
                    state.mouse_messages += 1;
                    for edge in decode_mouse_button_edges(packet) {
                        state.mouse_button_edges += 1;
                        state.pressed_mouse_candidates.apply_edge(edge);
                    }
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
            if state.pressed_candidates.apply_device_change(change) {
                state
                    .pressed_mouse_candidates
                    .reset(CaptureResetReason::DeviceRemoved);
            }
            Some(LRESULT(0))
        }
        WM_WTSSESSION_CHANGE if !state.is_null() => {
            let reset = matches!(
                wparam.0 as u32,
                WTS_SESSION_LOCK
                    | WTS_SESSION_UNLOCK
                    | WTS_CONSOLE_CONNECT
                    | WTS_CONSOLE_DISCONNECT
                    | WTS_REMOTE_CONNECT
                    | WTS_REMOTE_DISCONNECT
            );
            if reset {
                // SAFETY: GWLP_USERDATA points to the live WindowState until
                // WM_NCDESTROY clears it.
                unsafe { (*state).reset_candidates(CaptureResetReason::SessionChanged) };
            }
            Some(LRESULT(0))
        }
        WM_POWERBROADCAST if !state.is_null() => {
            let reset = matches!(
                wparam.0 as u32,
                PBT_APMSUSPEND
                    | PBT_APMSTANDBY
                    | PBT_APMRESUMEAUTOMATIC
                    | PBT_APMRESUMECRITICAL
                    | PBT_APMRESUMESTANDBY
                    | PBT_APMRESUMESUSPEND
            );
            if reset {
                // SAFETY: GWLP_USERDATA points to the live WindowState until
                // WM_NCDESTROY clears it.
                unsafe { (*state).reset_candidates(CaptureResetReason::PowerChanged) };
            }
            Some(LRESULT(1))
        }
        WM_TIMER if wparam.0 == RECONCILIATION_TIMER_ID && !state.is_null() => {
            // SAFETY: GWLP_USERDATA points to the live WindowState until
            // WM_NCDESTROY clears it.
            unsafe { (*state).reconcile_pressed_candidates() };
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == STOP_TIMER_ID => {
            let _ = unsafe { KillTimer(Some(window), STOP_TIMER_ID) };
            let _ = unsafe { KillTimer(Some(window), RECONCILIATION_TIMER_ID) };
            let _ = unsafe { DestroyWindow(window) };
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            if !state.is_null() {
                // SAFETY: WM_DESTROY runs before WM_NCDESTROY clears the
                // WindowState pointer, so every normal destruction path can
                // reset its platform-local candidates exactly once.
                unsafe { (*state).reset_candidates(CaptureResetReason::ServiceStopped) };
                if unsafe { (*state).session_notifications_registered }
                    && !unsafe { (*state).session_notifications_unregistered }
                {
                    let unregistered = unsafe { WTSUnRegisterSessionNotification(window) }.is_ok();
                    unsafe { (*state).session_notifications_unregistered = unregistered };
                }
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

enum CapturedRawInput {
    Keyboard(KeyboardEdge),
    Mouse(RawMousePacket),
}

unsafe fn read_raw_input(lparam: LPARAM) -> Result<Option<CapturedRawInput>, ()> {
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
    if native_header.dwType == 0 {
        let packet = decode_raw_mouse_bytes(
            RawInputHeader {
                declared_size: native_header.dwSize as usize,
                input_type: native_header.dwType,
            },
            bytes,
            header_size as usize,
        )
        .map_err(|_| ())?;
        return Ok(Some(CapturedRawInput::Mouse(packet)));
    }
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
    Ok(Some(CapturedRawInput::Keyboard(decode_keyboard_packet(
        packet,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_sequence_pairs_every_key_in_stable_order() {
        let sequence = build_synthetic_sequence(&SYNTHETIC_PRESSURE_KEYS, 2);
        assert_eq!(sequence.len(), SYNTHETIC_PRESSURE_KEY_COUNT * 4);

        let expected_cycle = SYNTHETIC_PRESSURE_KEYS
            .iter()
            .flat_map(|key| {
                [
                    KeyboardEdge {
                        key: key.physical_key,
                        pressed: true,
                    },
                    KeyboardEdge {
                        key: key.physical_key,
                        pressed: false,
                    },
                ]
            })
            .collect::<Vec<_>>();
        let actual = sequence
            .iter()
            .filter_map(|input| input.expected_edge)
            .collect::<Vec<_>>();
        assert_eq!(actual[..expected_cycle.len()], expected_cycle);
        assert_eq!(actual[expected_cycle.len()..], expected_cycle);
    }

    #[test]
    fn pointer_flood_pairs_motion_and_keeps_keyboard_expectations() {
        let sequence = build_pointer_flood_sequence(&SYNTHETIC_PRESSURE_KEYS, 1);
        assert_eq!(
            sequence.len(),
            SYNTHETIC_PRESSURE_KEY_COUNT * (2 + SYNTHETIC_POINTER_MOVES_PER_KEY_PAIR)
        );
        assert_eq!(
            sequence
                .iter()
                .filter(|input| input.expected_edge.is_none())
                .count(),
            SYNTHETIC_PRESSURE_KEY_COUNT * SYNTHETIC_POINTER_MOVES_PER_KEY_PAIR
        );
        assert_eq!(
            sequence
                .iter()
                .filter(|input| input.expected_edge.is_some())
                .count(),
            SYNTHETIC_PRESSURE_KEY_COUNT * 2
        );
    }
}
