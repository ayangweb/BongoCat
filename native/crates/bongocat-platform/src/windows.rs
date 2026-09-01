use crate::{PlatformInputDiagnostics, PlatformInputError, PlatformInputServiceStatus};
use bongocat_runtime::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorViewport, GamepadAxis,
    GamepadAxisKey, GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample, GamepadButton,
    GamepadButtonKey, GamepadConnection, GamepadConnectionError, InputControl, InputEdge,
    InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource, MonotonicMillis,
    MouseButton, PhysicalKey, PlatformInputDiagnosticsProducer,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
        System::{
            LibraryLoader::{
                GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
            },
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, OpenInputDesktop,
            },
        },
        UI::{
            Input::{
                GetRawInputData, HRAWINPUT, KeyboardAndMouse::GetAsyncKeyState, RAWINPUTDEVICE,
                RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDEV_REMOVE,
                RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GIDC_REMOVAL, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW,
                KillTimer, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
                PBT_APMRESUMESTANDBY, PBT_APMRESUMESUSPEND, PBT_APMSTANDBY, PBT_APMSUSPEND,
                PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SetTimer,
                SetWindowLongPtrW, ShowWindow, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_NCCREATE,
                WM_NCDESTROY, WM_POWERBROADCAST, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW,
                WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT,
                WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
            },
        },
    },
    core::{PCSTR, w},
};

const WINDOW_CLASS: windows::core::PCWSTR = w!("BongoCatProductRawInputWindow");
const CAPTURE_QUEUE_CAPACITY: usize = 256;
const SERVICE_TICK_TIMER: usize = 1;
const RECONCILIATION_TIMER: usize = 2;
const SERVICE_TICK_MS: u32 = 16;
const RECONCILIATION_INTERVAL_MS: u32 = 250;
const REQUIRED_MISSING_CONFIRMATIONS: u8 = 2;
const SERVICE_TIMEOUT: Duration = Duration::from_secs(2);
const FINAL_RESET_ATTEMPTS: usize = 20;
const FINAL_RESET_RETRY: Duration = Duration::from_millis(5);

const XINPUT_ERROR_SUCCESS: u32 = 0;
const XINPUT_ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;
const XINPUT_GAMEPAD_DPAD_UP: u16 = 0x0001;
const XINPUT_GAMEPAD_DPAD_DOWN: u16 = 0x0002;
const XINPUT_GAMEPAD_DPAD_LEFT: u16 = 0x0004;
const XINPUT_GAMEPAD_DPAD_RIGHT: u16 = 0x0008;
const XINPUT_GAMEPAD_START: u16 = 0x0010;
const XINPUT_GAMEPAD_BACK: u16 = 0x0020;
const XINPUT_GAMEPAD_LEFT_THUMB: u16 = 0x0040;
const XINPUT_GAMEPAD_RIGHT_THUMB: u16 = 0x0080;
const XINPUT_GAMEPAD_LEFT_SHOULDER: u16 = 0x0100;
const XINPUT_GAMEPAD_RIGHT_SHOULDER: u16 = 0x0200;
const XINPUT_GAMEPAD_A: u16 = 0x1000;
const XINPUT_GAMEPAD_B: u16 = 0x2000;
const XINPUT_GAMEPAD_X: u16 = 0x4000;
const XINPUT_GAMEPAD_Y: u16 = 0x8000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputGamepad {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    left_thumb_x: i16,
    left_thumb_y: i16,
    right_thumb_x: i16,
    right_thumb_y: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputState {
    packet_number: u32,
    gamepad: XInputGamepad,
}

type XInputGetState = unsafe extern "system" fn(user_index: u32, state: *mut XInputState) -> u32;

struct XInputApi {
    module: HMODULE,
    get_state: XInputGetState,
}

impl XInputApi {
    fn load() -> Option<Self> {
        // SAFETY: the DLL name is a fixed system component and the restricted
        // search flag prevents loading an untrusted copy from the current path.
        let module = unsafe {
            LoadLibraryExW(w!("xinput1_4.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?
        };
        // SAFETY: `XInputGetState` is the documented export with the ABI used
        // by `XInputState`; the module is retained for the poller's lifetime.
        let get_state = unsafe {
            GetProcAddress(module, PCSTR(c"XInputGetState".as_ptr().cast()))
                .map(|function| std::mem::transmute::<_, XInputGetState>(function))?
        };
        Some(Self { module, get_state })
    }
}

impl Drop for XInputApi {
    fn drop(&mut self) {
        // SAFETY: this adapter owns the matching LoadLibraryExW reference and
        // the poller cannot invoke `get_state` after dropping this field.
        unsafe { FreeLibrary(self.module) }.ok();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWindowError {
    HandleUnavailable,
    UnsupportedHandle,
    CloseRequestFailed,
}

impl std::fmt::Display for NativeWindowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::HandleUnavailable => "the native window handle is unavailable",
            Self::UnsupportedHandle => "the native window handle is not a Win32 HWND",
            Self::CloseRequestFailed => "the native window rejected the close request",
        })
    }
}

impl std::error::Error for NativeWindowError {}

pub fn hide_native_window(window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    let hwnd = native_hwnd(window)?;
    // SAFETY: raw-window-handle guarantees that the HWND remains valid while
    // `window` is borrowed, and the GPUI callback runs on the window owner thread.
    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    Ok(())
}

pub fn show_native_window(window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    let hwnd = native_hwnd(window)?;
    // SAFETY: raw-window-handle guarantees that the HWND remains valid while
    // `window` is borrowed, and the GPUI callback runs on the window owner thread.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    Ok(())
}

pub fn request_native_window_close(window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    let hwnd = native_hwnd(window)?;
    // SAFETY: raw-window-handle guarantees that the HWND remains valid while
    // `window` is borrowed. Posting defers WM_CLOSE to the owning message loop.
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
        .map_err(|_| NativeWindowError::CloseRequestFailed)
}

/// Ends the Windows process after every BongoCat-owned worker and native
/// resource has shut down. GPUI 0.2.2 synchronously re-enters its `AsyncApp`
/// from `WM_DESTROY`, so its retained settings window cannot be safely dropped.
pub fn terminate_after_product_shutdown(exit_code: i32) -> ! {
    std::process::exit(exit_code)
}

fn native_hwnd(window: &impl HasWindowHandle) -> Result<HWND, NativeWindowError> {
    let handle = window
        .window_handle()
        .map_err(|_| NativeWindowError::HandleUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(NativeWindowError::UnsupportedHandle);
    };
    Ok(HWND(handle.hwnd.get() as *mut c_void))
}

const RI_KEY_BREAK: u16 = 0x0001;
const RI_KEY_E0: u16 = 0x0002;
const RI_KEY_E1: u16 = 0x0004;
const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
const RI_MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x0004;
const RI_MOUSE_RIGHT_BUTTON_UP: u16 = 0x0008;
const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;
const RI_MOUSE_BACK_BUTTON_DOWN: u16 = 0x0040;
const RI_MOUSE_BACK_BUTTON_UP: u16 = 0x0080;
const RI_MOUSE_FORWARD_BUTTON_DOWN: u16 = 0x0100;
const RI_MOUSE_FORWARD_BUTTON_UP: u16 = 0x0200;

#[derive(Clone, Copy, Debug)]
struct SystemControl(i32);

#[derive(Clone, Copy, Debug)]
enum CapturedEvent {
    Edge {
        control: InputControl,
        system: SystemControl,
        edge: InputEdge,
    },
    Reset(InputResetReason),
    Reconcile,
}

#[derive(Clone, Copy, Debug)]
struct RawKeyboardPacket {
    make_code: u16,
    flags: u16,
    virtual_key: u16,
}

#[derive(Clone, Copy, Debug)]
struct RawMousePacket {
    button_flags: u16,
    moved: bool,
}

#[derive(Clone, Copy, Debug)]
enum RawInputPacket {
    Keyboard(RawKeyboardPacket),
    Mouse(RawMousePacket),
}

#[derive(Clone, Copy, Debug, Default)]
struct WorkerOptions {
    drop_next_key_release: bool,
}

#[derive(Clone, Copy)]
struct XInputSlot {
    connection: GamepadConnection,
    buttons: u32,
}

struct XInputPoller {
    slots: [Option<XInputSlot>; 4],
    api: Option<XInputApi>,
}

impl Default for XInputPoller {
    fn default() -> Self {
        Self {
            slots: [None; 4],
            api: XInputApi::load(),
        }
    }
}

impl XInputPoller {
    fn disconnect_all(&mut self, axis_producer: &GamepadAxisProducer) -> u64 {
        let mut disconnected = 0_u64;
        for slot in &mut self.slots {
            if let Some(previous) = slot.take() {
                axis_producer.disconnect(previous.connection);
                disconnected = disconnected.saturating_add(1);
            }
        }
        disconnected
    }

    fn poll(
        &mut self,
        producer: &InputProducer,
        axis_producer: &GamepadAxisProducer,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(api) = self.api.as_ref() else {
            diagnostics.gamepad_backend_unavailable =
                diagnostics.gamepad_backend_unavailable.saturating_add(1);
            return Ok(());
        };
        let get_state = api.get_state;
        self.poll_with(producer, axis_producer, at, diagnostics, |slot, state| {
            // SAFETY: `state` is a valid writable pointer and slot is
            // constrained by `poll_with` to the documented 0..4 range.
            unsafe { get_state(slot, state) }
        })
    }

    fn poll_with(
        &mut self,
        producer: &InputProducer,
        axis_producer: &GamepadAxisProducer,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
        mut query: impl FnMut(u32, *mut XInputState) -> u32,
    ) -> Result<(), InputPublishError> {
        for slot in 0..4_usize {
            let mut state = XInputState::default();
            let result = query(slot as u32, &mut state);
            diagnostics.gamepad_polls = diagnostics.gamepad_polls.saturating_add(1);
            if result == XINPUT_ERROR_DEVICE_NOT_CONNECTED {
                if let Some(previous) = self.slots[slot].take() {
                    axis_producer.disconnect(previous.connection);
                    producer.publish(InputEvent::GamepadDisconnected {
                        connection: previous.connection,
                        at,
                    })?;
                    diagnostics.gamepad_disconnections =
                        diagnostics.gamepad_disconnections.saturating_add(1);
                }
                continue;
            }
            if result != XINPUT_ERROR_SUCCESS {
                diagnostics.gamepad_query_errors =
                    diagnostics.gamepad_query_errors.saturating_add(1);
                continue;
            }
            let connection = if let Some(previous) = self.slots[slot] {
                previous.connection
            } else {
                let connection = match axis_producer.connect(slot as u8) {
                    Ok(connection) => connection,
                    Err(GamepadConnectionError::RuntimeStopped) => {
                        return Err(InputPublishError::RuntimeStopped(InputEvent::Reset {
                            reason: InputResetReason::ServiceRestart,
                            at,
                        }));
                    }
                    Err(GamepadConnectionError::GenerationExhausted) => {
                        diagnostics.gamepad_axis_publish_rejections = diagnostics
                            .gamepad_axis_publish_rejections
                            .saturating_add(1);
                        continue;
                    }
                };
                producer.publish(InputEvent::GamepadConnected { connection, at })?;
                diagnostics.gamepad_connections = diagnostics.gamepad_connections.saturating_add(1);
                connection
            };
            let old_buttons = self.slots[slot].map_or(0, |value| value.buttons);
            let new_buttons = u32::from(state.gamepad.buttons)
                | (u32::from(state.gamepad.left_trigger >= 128) << 16)
                | (u32::from(state.gamepad.right_trigger >= 128) << 17);
            for (button, bit) in XINPUT_BUTTON_BITS {
                let was_pressed = old_buttons & bit != 0;
                let is_pressed = new_buttons & bit != 0;
                if was_pressed == is_pressed {
                    continue;
                }
                producer.publish(InputEvent::Edge {
                    control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                    edge: if is_pressed {
                        InputEdge::Down
                    } else {
                        InputEdge::Up
                    },
                    source: InputSource::Capture,
                    at,
                })?;
            }
            for (axis, value) in [
                (
                    GamepadAxis::LeftStickX,
                    normalize_thumb(state.gamepad.left_thumb_x),
                ),
                (
                    GamepadAxis::LeftStickY,
                    normalize_thumb(state.gamepad.left_thumb_y),
                ),
                (
                    GamepadAxis::RightStickX,
                    normalize_thumb(state.gamepad.right_thumb_x),
                ),
                (
                    GamepadAxis::RightStickY,
                    normalize_thumb(state.gamepad.right_thumb_y),
                ),
                (
                    GamepadAxis::LeftTrigger,
                    normalize_trigger(state.gamepad.left_trigger),
                ),
                (
                    GamepadAxis::RightTrigger,
                    normalize_trigger(state.gamepad.right_trigger),
                ),
            ] {
                match axis_producer.publish(GamepadAxisSample {
                    key: GamepadAxisKey { connection, axis },
                    value,
                    at,
                }) {
                    Ok(()) => {
                        diagnostics.gamepad_axis_samples =
                            diagnostics.gamepad_axis_samples.saturating_add(1);
                    }
                    Err(GamepadAxisPublishError::RuntimeStopped(_)) => {
                        return Err(InputPublishError::RuntimeStopped(InputEvent::Reset {
                            reason: InputResetReason::ServiceRestart,
                            at,
                        }));
                    }
                    Err(_) => {
                        diagnostics.gamepad_axis_publish_rejections = diagnostics
                            .gamepad_axis_publish_rejections
                            .saturating_add(1);
                    }
                }
            }
            self.slots[slot] = Some(XInputSlot {
                connection,
                buttons: new_buttons,
            });
        }
        Ok(())
    }
}

const XINPUT_BUTTON_BITS: [(GamepadButton, u32); 16] = [
    (GamepadButton::DpadUp, XINPUT_GAMEPAD_DPAD_UP as u32),
    (GamepadButton::DpadDown, XINPUT_GAMEPAD_DPAD_DOWN as u32),
    (GamepadButton::DpadLeft, XINPUT_GAMEPAD_DPAD_LEFT as u32),
    (GamepadButton::DpadRight, XINPUT_GAMEPAD_DPAD_RIGHT as u32),
    (GamepadButton::Start, XINPUT_GAMEPAD_START as u32),
    (GamepadButton::Select, XINPUT_GAMEPAD_BACK as u32),
    (GamepadButton::LeftStick, XINPUT_GAMEPAD_LEFT_THUMB as u32),
    (GamepadButton::RightStick, XINPUT_GAMEPAD_RIGHT_THUMB as u32),
    (
        GamepadButton::LeftShoulder,
        XINPUT_GAMEPAD_LEFT_SHOULDER as u32,
    ),
    (
        GamepadButton::RightShoulder,
        XINPUT_GAMEPAD_RIGHT_SHOULDER as u32,
    ),
    (GamepadButton::South, XINPUT_GAMEPAD_A as u32),
    (GamepadButton::East, XINPUT_GAMEPAD_B as u32),
    (GamepadButton::West, XINPUT_GAMEPAD_X as u32),
    (GamepadButton::North, XINPUT_GAMEPAD_Y as u32),
    (GamepadButton::LeftTrigger, 1 << 16),
    (GamepadButton::RightTrigger, 1 << 17),
];

fn normalize_thumb(value: i16) -> f32 {
    if value < 0 {
        f32::from(value) / 32_768.0
    } else {
        f32::from(value) / 32_767.0
    }
}

fn normalize_trigger(value: u8) -> f32 {
    f32::from(value) / 255.0
}

struct WindowState {
    producer: InputProducer,
    cursor_producer: CursorProducer,
    _gamepad_axis_producer: GamepadAxisProducer,
    gamepad_poller: XInputPoller,
    stop: Arc<AtomicBool>,
    started: Instant,
    queue: VecDeque<CapturedEvent>,
    candidates: BTreeMap<InputControl, SystemControl>,
    missing_confirmations: BTreeMap<InputControl, u8>,
    accepting: bool,
    recovery_pending: bool,
    pointer_dirty: bool,
    terminal_error: Option<PlatformInputError>,
    session_notifications_registered: bool,
    session_notifications_unregistered: bool,
    final_reset_published: bool,
    drop_next_key_release: bool,
    diagnostics: PlatformInputDiagnostics,
    diagnostics_producer: PlatformInputDiagnosticsProducer,
}

impl WindowState {
    fn new(
        producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        diagnostics_producer: PlatformInputDiagnosticsProducer,
        stop: Arc<AtomicBool>,
        options: WorkerOptions,
    ) -> Self {
        Self {
            producer,
            cursor_producer,
            _gamepad_axis_producer: gamepad_axis_producer,
            gamepad_poller: XInputPoller::default(),
            stop,
            started: Instant::now(),
            queue: VecDeque::with_capacity(CAPTURE_QUEUE_CAPACITY),
            candidates: BTreeMap::new(),
            missing_confirmations: BTreeMap::new(),
            accepting: true,
            recovery_pending: false,
            pointer_dirty: true,
            terminal_error: None,
            session_notifications_registered: false,
            session_notifications_unregistered: false,
            final_reset_published: false,
            drop_next_key_release: options.drop_next_key_release,
            diagnostics: PlatformInputDiagnostics {
                service_status: PlatformInputServiceStatus::Running,
                service_start_attempts: 1,
                cursor_captured: 1,
                ..PlatformInputDiagnostics::default()
            },
            diagnostics_producer,
        }
    }

    fn enqueue(&mut self, event: CapturedEvent) {
        if !self.accepting {
            self.diagnostics.rejected_after_stop =
                self.diagnostics.rejected_after_stop.saturating_add(1);
            return;
        }
        if self.queue.len() == CAPTURE_QUEUE_CAPACITY {
            self.diagnostics.capture_queue_overflows =
                self.diagnostics.capture_queue_overflows.saturating_add(1);
            self.diagnostics.capture_queue_discarded = self
                .diagnostics
                .capture_queue_discarded
                .saturating_add(self.queue.len() as u64)
                .saturating_add(1);
            self.queue.clear();
            self.accepting = false;
            self.recovery_pending = true;
            return;
        }
        self.queue.push_back(event);
        self.diagnostics.queued_edges = self.diagnostics.queued_edges.saturating_add(1);
    }

    fn capture_raw_input(&mut self, packet: RawInputPacket) {
        match packet {
            RawInputPacket::Keyboard(packet) => {
                let Some(key) = map_scan_code(packet.make_code, packet.flags) else {
                    self.diagnostics.unmapped_keys =
                        self.diagnostics.unmapped_keys.saturating_add(1);
                    return;
                };
                let edge = if packet.flags & RI_KEY_BREAK == 0 {
                    InputEdge::Down
                } else {
                    InputEdge::Up
                };
                self.diagnostics.captured_edges = self.diagnostics.captured_edges.saturating_add(1);
                self.enqueue(CapturedEvent::Edge {
                    control: InputControl::Key(key),
                    system: SystemControl(normalize_virtual_key(packet)),
                    edge,
                });
            }
            RawInputPacket::Mouse(packet) => {
                if packet.moved {
                    self.diagnostics.cursor_captured =
                        self.diagnostics.cursor_captured.saturating_add(1);
                    if self.pointer_dirty {
                        self.diagnostics.cursor_coalesced =
                            self.diagnostics.cursor_coalesced.saturating_add(1);
                    }
                    self.pointer_dirty = true;
                }
                for (button, virtual_key, down, up) in [
                    (
                        MouseButton::Left,
                        0x01,
                        RI_MOUSE_LEFT_BUTTON_DOWN,
                        RI_MOUSE_LEFT_BUTTON_UP,
                    ),
                    (
                        MouseButton::Right,
                        0x02,
                        RI_MOUSE_RIGHT_BUTTON_DOWN,
                        RI_MOUSE_RIGHT_BUTTON_UP,
                    ),
                    (
                        MouseButton::Middle,
                        0x04,
                        RI_MOUSE_MIDDLE_BUTTON_DOWN,
                        RI_MOUSE_MIDDLE_BUTTON_UP,
                    ),
                    (
                        MouseButton::Back,
                        0x05,
                        RI_MOUSE_BACK_BUTTON_DOWN,
                        RI_MOUSE_BACK_BUTTON_UP,
                    ),
                    (
                        MouseButton::Forward,
                        0x06,
                        RI_MOUSE_FORWARD_BUTTON_DOWN,
                        RI_MOUSE_FORWARD_BUTTON_UP,
                    ),
                ] {
                    for edge in [
                        (packet.button_flags & down != 0).then_some(InputEdge::Down),
                        (packet.button_flags & up != 0).then_some(InputEdge::Up),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        self.diagnostics.captured_edges =
                            self.diagnostics.captured_edges.saturating_add(1);
                        self.enqueue(CapturedEvent::Edge {
                            control: InputControl::Mouse(button),
                            system: SystemControl(virtual_key),
                            edge,
                        });
                    }
                }
            }
        }
    }

    fn request_recovery(&mut self) {
        self.accepting = false;
        self.recovery_pending = true;
        self.diagnostics.capture_queue_discarded = self
            .diagnostics
            .capture_queue_discarded
            .saturating_add(self.queue.len() as u64);
        self.queue.clear();
    }

    fn service_tick(&mut self) {
        self.service_tick_inner();
        self.publish_diagnostics();
    }

    fn service_tick_inner(&mut self) {
        if self.terminal_error.is_some() {
            return;
        }
        if self.recovery_pending {
            match self
                .producer
                .recover(InputResetReason::QueueOverflow, self.monotonic())
            {
                Ok(_) => {
                    self.candidates.clear();
                    self.missing_confirmations.clear();
                    self.recovery_pending = false;
                    self.accepting = true;
                    self.diagnostics.recovery_resets =
                        self.diagnostics.recovery_resets.saturating_add(1);
                }
                Err(InputPublishError::QueueFull(_)) => {
                    self.diagnostics.runtime_queue_overflows =
                        self.diagnostics.runtime_queue_overflows.saturating_add(1);
                    return;
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    self.terminal_error = Some(PlatformInputError::RuntimeStopped);
                    return;
                }
            }
        }

        if let Err(error) = self.gamepad_poller.poll(
            &self.producer,
            &self._gamepad_axis_producer,
            self.monotonic(),
            &mut self.diagnostics,
        ) {
            match error {
                InputPublishError::QueueFull(_) => self.request_recovery(),
                InputPublishError::RuntimeStopped(_) => {
                    self.terminal_error = Some(PlatformInputError::RuntimeStopped)
                }
            }
            return;
        }

        while let Some(event) = self.queue.pop_front() {
            if let Err(error) = self.publish(event) {
                match error {
                    InputPublishError::QueueFull(_) => {
                        self.diagnostics.runtime_queue_overflows =
                            self.diagnostics.runtime_queue_overflows.saturating_add(1);
                        self.request_recovery();
                    }
                    InputPublishError::RuntimeStopped(_) => {
                        self.terminal_error = Some(PlatformInputError::RuntimeStopped);
                    }
                }
                break;
            }
        }
        self.forward_cursor();
    }

    fn publish_diagnostics(&self) {
        let _ = self.diagnostics_producer.publish(self.diagnostics);
    }

    fn publish(&mut self, event: CapturedEvent) -> Result<(), InputPublishError> {
        match event {
            CapturedEvent::Edge {
                control,
                system,
                edge,
            } => {
                if self.drop_next_key_release
                    && edge == InputEdge::Up
                    && matches!(control, InputControl::Key(_))
                {
                    self.drop_next_key_release = false;
                    return Ok(());
                }
                self.producer.publish(InputEvent::Edge {
                    control,
                    edge,
                    source: InputSource::Capture,
                    at: self.monotonic(),
                })?;
                match edge {
                    InputEdge::Down => {
                        self.candidates.insert(control, system);
                        self.missing_confirmations.remove(&control);
                    }
                    InputEdge::Up => {
                        self.candidates.remove(&control);
                        self.missing_confirmations.remove(&control);
                    }
                }
                self.diagnostics.consumed_edges = self.diagnostics.consumed_edges.saturating_add(1);
            }
            CapturedEvent::Reset(reason) => {
                self.producer.recover(reason, self.monotonic())?;
                self.candidates.clear();
                self.missing_confirmations.clear();
                self.diagnostics.recovery_resets =
                    self.diagnostics.recovery_resets.saturating_add(1);
            }
            CapturedEvent::Reconcile => {
                if self.candidates.is_empty() {
                    return Ok(());
                }
                let pressed = match query_pressed_controls(&self.candidates) {
                    Ok(pressed) => pressed,
                    Err(_) => {
                        self.producer
                            .recover(InputResetReason::ServiceRestart, self.monotonic())?;
                        self.candidates.clear();
                        self.missing_confirmations.clear();
                        self.diagnostics.recovery_resets =
                            self.diagnostics.recovery_resets.saturating_add(1);
                        return Ok(());
                    }
                };
                self.producer.publish(InputEvent::Reconcile {
                    pressed: pressed.clone(),
                    at: self.monotonic(),
                })?;
                self.diagnostics.reconciliation_runs =
                    self.diagnostics.reconciliation_runs.saturating_add(1);
                let controls = self.candidates.keys().copied().collect::<Vec<_>>();
                for control in controls {
                    if pressed.contains(&control) {
                        self.missing_confirmations.remove(&control);
                        continue;
                    }
                    let confirmations = self.missing_confirmations.entry(control).or_insert(0);
                    *confirmations = confirmations.saturating_add(1);
                    if *confirmations >= REQUIRED_MISSING_CONFIRMATIONS {
                        self.candidates.remove(&control);
                        self.missing_confirmations.remove(&control);
                        self.diagnostics.reconciled_releases =
                            self.diagnostics.reconciled_releases.saturating_add(1);
                    }
                }
            }
        }
        Ok(())
    }

    fn forward_cursor(&mut self) {
        if !self.pointer_dirty || self.terminal_error.is_some() {
            return;
        }
        self.pointer_dirty = false;
        let Some(sample) = cursor_sample(self.monotonic()) else {
            self.diagnostics.cursor_display_lookup_failures = self
                .diagnostics
                .cursor_display_lookup_failures
                .saturating_add(1);
            return;
        };
        match self.cursor_producer.publish(sample) {
            Ok(()) => {
                self.diagnostics.cursor_consumed =
                    self.diagnostics.cursor_consumed.saturating_add(1);
            }
            Err(CursorPublishError::NonMonotonic(_)) => {
                self.diagnostics.cursor_publish_rejections =
                    self.diagnostics.cursor_publish_rejections.saturating_add(1);
            }
            Err(CursorPublishError::RuntimeStopped(_)) => {
                self.terminal_error = Some(PlatformInputError::RuntimeStopped);
            }
        }
    }

    fn publish_final_reset(&mut self) -> bool {
        if self.final_reset_published {
            return true;
        }
        for _ in 0..FINAL_RESET_ATTEMPTS {
            match self
                .producer
                .recover(InputResetReason::ServiceRestart, self.monotonic())
            {
                Ok(_) => {
                    self.final_reset_published = true;
                    self.candidates.clear();
                    self.missing_confirmations.clear();
                    self.diagnostics.recovery_resets =
                        self.diagnostics.recovery_resets.saturating_add(1);
                    return true;
                }
                Err(InputPublishError::QueueFull(_)) => {
                    self.diagnostics.runtime_queue_overflows =
                        self.diagnostics.runtime_queue_overflows.saturating_add(1);
                    thread::sleep(FINAL_RESET_RETRY);
                }
                Err(InputPublishError::RuntimeStopped(_)) => return false,
            }
        }
        false
    }

    fn monotonic(&self) -> MonotonicMillis {
        MonotonicMillis::new(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX))
    }
}

pub struct WindowsInputService {
    stop: Arc<AtomicBool>,
    completion: Receiver<Result<PlatformInputDiagnostics, PlatformInputError>>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsInputService {
    pub fn start(
        producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
    ) -> Result<Self, PlatformInputError> {
        Self::start_with_diagnostics(
            producer,
            cursor_producer,
            gamepad_axis_producer,
            PlatformInputDiagnosticsProducer::default(),
        )
    }

    pub fn start_with_diagnostics(
        producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        diagnostics_producer: PlatformInputDiagnosticsProducer,
    ) -> Result<Self, PlatformInputError> {
        Self::start_with_diagnostics_and_options(
            producer,
            cursor_producer,
            gamepad_axis_producer,
            diagnostics_producer,
            WorkerOptions::default(),
        )
    }

    fn start_with_diagnostics_and_options(
        producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        diagnostics_producer: PlatformInputDiagnosticsProducer,
        options: WorkerOptions,
    ) -> Result<Self, PlatformInputError> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bongocat-windows-input".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_input_worker(
                        producer,
                        cursor_producer,
                        gamepad_axis_producer,
                        diagnostics_producer,
                        worker_stop,
                        options,
                        startup_sender,
                    )
                }))
                .unwrap_or(Err(PlatformInputError::WorkerPanicked));
                let _ = completion_sender.send(result);
            })
            .map_err(|_| PlatformInputError::WorkerPanicked)?;
        match startup_receiver.recv_timeout(SERVICE_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stop,
                completion: completion_receiver,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(PlatformInputError::StartupTimedOut)
            }
        }
    }

    pub fn stop(mut self) -> Result<PlatformInputDiagnostics, PlatformInputError> {
        self.finish(SERVICE_TIMEOUT)
    }

    fn finish(
        &mut self,
        timeout: Duration,
    ) -> Result<PlatformInputDiagnostics, PlatformInputError> {
        self.stop.store(true, Ordering::Release);
        let result = self
            .completion
            .recv_timeout(timeout)
            .map_err(|_| PlatformInputError::ShutdownTimedOut)?;
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| PlatformInputError::WorkerPanicked)?;
        }
        result
    }
}

impl Drop for WindowsInputService {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish(SERVICE_TIMEOUT);
        }
    }
}

fn run_input_worker(
    producer: InputProducer,
    cursor_producer: CursorProducer,
    gamepad_axis_producer: GamepadAxisProducer,
    diagnostics_producer: PlatformInputDiagnosticsProducer,
    stop: Arc<AtomicBool>,
    options: WorkerOptions,
    startup: SyncSender<Result<(), PlatformInputError>>,
) -> Result<PlatformInputDiagnostics, PlatformInputError> {
    // SAFETY: every Win32 object and callback state is confined to this worker
    // thread. The boxed state outlives the HWND and is dropped only after
    // WM_NCDESTROY clears GWLP_USERDATA and the message loop terminates.
    unsafe {
        run_input_worker_inner(
            producer,
            cursor_producer,
            gamepad_axis_producer,
            diagnostics_producer,
            stop,
            options,
            startup,
        )
    }
}

unsafe fn run_input_worker_inner(
    producer: InputProducer,
    cursor_producer: CursorProducer,
    gamepad_axis_producer: GamepadAxisProducer,
    diagnostics_producer: PlatformInputDiagnosticsProducer,
    stop: Arc<AtomicBool>,
    options: WorkerOptions,
    startup: SyncSender<Result<(), PlatformInputError>>,
) -> Result<PlatformInputDiagnostics, PlatformInputError> {
    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(_) => {
            let _ = startup.send(Err(PlatformInputError::WindowClassRegistrationFailed));
            return Err(PlatformInputError::WindowClassRegistrationFailed);
        }
    };
    let instance = HINSTANCE(module.0);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = startup.send(Err(PlatformInputError::WindowClassRegistrationFailed));
        return Err(PlatformInputError::WindowClassRegistrationFailed);
    }

    let mut state = Box::new(WindowState::new(
        producer,
        cursor_producer,
        gamepad_axis_producer,
        diagnostics_producer,
        stop,
        options,
    ));
    let state_ptr = (&mut *state) as *mut WindowState;
    let window = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            w!("BongoCat Raw Input"),
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
    } {
        Ok(window) => window,
        Err(_) => {
            let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
            let _ = startup.send(Err(PlatformInputError::WindowCreateFailed));
            return Err(PlatformInputError::WindowCreateFailed);
        }
    };

    if unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) }.is_err() {
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
        let _ = startup.send(Err(PlatformInputError::SessionNotificationFailed));
        return Err(PlatformInputError::SessionNotificationFailed);
    }
    state.session_notifications_registered = true;
    let devices = [
        raw_input_device(0x02, window),
        raw_input_device(0x06, window),
    ];
    if unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) }.is_err() {
        let _ = unsafe { WTSUnRegisterSessionNotification(window) };
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
        let _ = startup.send(Err(PlatformInputError::RawInputRegistrationFailed));
        return Err(PlatformInputError::RawInputRegistrationFailed);
    }
    if unsafe { SetTimer(Some(window), SERVICE_TICK_TIMER, SERVICE_TICK_MS, None) } == 0
        || unsafe {
            SetTimer(
                Some(window),
                RECONCILIATION_TIMER,
                RECONCILIATION_INTERVAL_MS,
                None,
            )
        } == 0
    {
        let _ = unsafe { KillTimer(Some(window), SERVICE_TICK_TIMER) };
        let _ = unsafe { KillTimer(Some(window), RECONCILIATION_TIMER) };
        let _ = unsafe { unregister_raw_input_devices() };
        let _ = unsafe { WTSUnRegisterSessionNotification(window) };
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
        let _ = startup.send(Err(PlatformInputError::TimerCreateFailed));
        return Err(PlatformInputError::TimerCreateFailed);
    }
    state.publish_diagnostics();
    let _ = startup.send(Ok(()));

    let mut message = MSG::default();
    let mut message_failed = false;
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            message_failed = true;
            break;
        }
        if !result.as_bool() {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        state.service_tick();
        if state.terminal_error.is_some() {
            let _ = unsafe { DestroyWindow(window) };
        }
    }

    let _ = unsafe { KillTimer(Some(window), SERVICE_TICK_TIMER) };
    let _ = unsafe { KillTimer(Some(window), RECONCILIATION_TIMER) };
    let raw_unregistered = unsafe { unregister_raw_input_devices() }.is_ok();
    if state.session_notifications_registered && !state.session_notifications_unregistered {
        state.session_notifications_unregistered =
            unsafe { WTSUnRegisterSessionNotification(window) }.is_ok();
    }
    state.diagnostics.gamepad_disconnections =
        state.diagnostics.gamepad_disconnections.saturating_add(
            state
                .gamepad_poller
                .disconnect_all(&state._gamepad_axis_producer),
        );
    let final_reset = state.publish_final_reset();
    state.diagnostics.clean_shutdown = !message_failed
        && raw_unregistered
        && state.session_notifications_unregistered
        && final_reset;
    state.diagnostics.service_status =
        if !message_failed && state.terminal_error.is_none() && state.diagnostics.clean_shutdown {
            PlatformInputServiceStatus::Stopped
        } else {
            PlatformInputServiceStatus::Failed
        };
    state.publish_diagnostics();
    let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
    if message_failed {
        return Err(PlatformInputError::WorkerPanicked);
    }
    if let Some(error) = state.terminal_error {
        return Err(error);
    }
    Ok(state.diagnostics)
}

fn raw_input_device(usage: u16, window: HWND) -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: usage,
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: window,
    }
}

unsafe fn unregister_raw_input_devices() -> windows::core::Result<()> {
    let devices = [
        raw_input_device(0x02, HWND::default()),
        raw_input_device(0x06, HWND::default()),
    ]
    .map(|mut device| {
        device.dwFlags = RIDEV_REMOVE;
        device
    });
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
            // SAFETY: CreateWindowExW receives the live boxed WindowState and
            // WM_NCCREATE provides that same pointer in CREATESTRUCTW.
            let state = unsafe { (*create).lpCreateParams as *mut WindowState };
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, state as isize) };
        }
    }
    let state = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowState };
    let outcome = catch_unwind(AssertUnwindSafe(|| match message {
        WM_INPUT if !state.is_null() => {
            let state = unsafe { &mut *state };
            match unsafe { read_raw_input(lparam) } {
                Ok(Some(packet)) => state.capture_raw_input(packet),
                Ok(None) => {}
                Err(()) => {
                    state.diagnostics.decode_errors =
                        state.diagnostics.decode_errors.saturating_add(1);
                }
            }
            Some(LRESULT(0))
        }
        WM_INPUT_DEVICE_CHANGE if !state.is_null() && wparam.0 as u32 == GIDC_REMOVAL => {
            unsafe { &mut *state }.enqueue(CapturedEvent::Reset(InputResetReason::DeviceRemoved));
            Some(LRESULT(0))
        }
        WM_WTSSESSION_CHANGE
            if !state.is_null()
                && matches!(
                    wparam.0 as u32,
                    WTS_SESSION_LOCK
                        | WTS_SESSION_UNLOCK
                        | WTS_CONSOLE_CONNECT
                        | WTS_CONSOLE_DISCONNECT
                        | WTS_REMOTE_CONNECT
                        | WTS_REMOTE_DISCONNECT
                ) =>
        {
            unsafe { &mut *state }.enqueue(CapturedEvent::Reset(InputResetReason::SessionLock));
            Some(LRESULT(0))
        }
        WM_POWERBROADCAST
            if !state.is_null()
                && matches!(
                    wparam.0 as u32,
                    PBT_APMSUSPEND
                        | PBT_APMSTANDBY
                        | PBT_APMRESUMEAUTOMATIC
                        | PBT_APMRESUMECRITICAL
                        | PBT_APMRESUMESTANDBY
                        | PBT_APMRESUMESUSPEND
                ) =>
        {
            unsafe { &mut *state }.enqueue(CapturedEvent::Reset(InputResetReason::Sleep));
            Some(LRESULT(1))
        }
        WM_TIMER if !state.is_null() && wparam.0 == RECONCILIATION_TIMER => {
            unsafe { &mut *state }.enqueue(CapturedEvent::Reconcile);
            Some(LRESULT(0))
        }
        WM_TIMER if !state.is_null() && wparam.0 == SERVICE_TICK_TIMER => {
            let state = unsafe { &mut *state };
            if state.stop.load(Ordering::Acquire) {
                let _ = unsafe { DestroyWindow(window) };
            }
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            if !state.is_null() {
                let state = unsafe { &mut *state };
                state.accepting = false;
                state.diagnostics.capture_queue_discarded = state
                    .diagnostics
                    .capture_queue_discarded
                    .saturating_add(state.queue.len() as u64);
                state.queue.clear();
                if state.session_notifications_registered
                    && !state.session_notifications_unregistered
                {
                    state.session_notifications_unregistered =
                        unsafe { WTSUnRegisterSessionNotification(window) }.is_ok();
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
                let state = unsafe { &mut *state };
                state.diagnostics.callback_panics =
                    state.diagnostics.callback_panics.saturating_add(1);
                state.request_recovery();
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}

unsafe fn read_raw_input(lparam: LPARAM) -> Result<Option<RawInputPacket>, ()> {
    let raw_input = HRAWINPUT(lparam.0 as *mut c_void);
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let mut byte_count = 0_u32;
    let query =
        unsafe { GetRawInputData(raw_input, RID_INPUT, None, &mut byte_count, header_size) };
    if query == u32::MAX || byte_count < header_size {
        return Err(());
    }
    let words = (byte_count as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; words];
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
    let bytes = unsafe { slice::from_raw_parts(storage.as_ptr().cast::<u8>(), read as usize) };
    decode_raw_input_bytes(bytes, header_size as usize)
}

fn decode_raw_input_bytes(bytes: &[u8], header_size: usize) -> Result<Option<RawInputPacket>, ()> {
    if bytes.len() < header_size || header_size < 8 {
        return Err(());
    }
    let input_type = u32::from_le_bytes(bytes[0..4].try_into().map_err(|_| ())?);
    let declared_size = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| ())?) as usize;
    if declared_size > bytes.len() || declared_size < header_size {
        return Err(());
    }
    match input_type {
        0 => {
            let end = header_size.checked_add(20).ok_or(())?;
            if declared_size < end {
                return Err(());
            }
            let button_flags = u16::from_le_bytes(
                bytes[header_size + 4..header_size + 6]
                    .try_into()
                    .map_err(|_| ())?,
            );
            let x = i32::from_le_bytes(
                bytes[header_size + 12..header_size + 16]
                    .try_into()
                    .map_err(|_| ())?,
            );
            let y = i32::from_le_bytes(
                bytes[header_size + 16..header_size + 20]
                    .try_into()
                    .map_err(|_| ())?,
            );
            Ok(Some(RawInputPacket::Mouse(RawMousePacket {
                button_flags,
                moved: x != 0 || y != 0,
            })))
        }
        1 => {
            let end = header_size.checked_add(8).ok_or(())?;
            if declared_size < end {
                return Err(());
            }
            Ok(Some(RawInputPacket::Keyboard(RawKeyboardPacket {
                make_code: u16::from_le_bytes(
                    bytes[header_size..header_size + 2]
                        .try_into()
                        .map_err(|_| ())?,
                ),
                flags: u16::from_le_bytes(
                    bytes[header_size + 2..header_size + 4]
                        .try_into()
                        .map_err(|_| ())?,
                ),
                virtual_key: u16::from_le_bytes(
                    bytes[header_size + 6..header_size + 8]
                        .try_into()
                        .map_err(|_| ())?,
                ),
            })))
        }
        _ => Ok(None),
    }
}

fn map_scan_code(make_code: u16, flags: u16) -> Option<PhysicalKey> {
    let extended = flags & RI_KEY_E0 != 0;
    let e1 = flags & RI_KEY_E1 != 0;
    let usage = if e1 && make_code == 0x1d {
        0x48
    } else if extended {
        match make_code {
            0x1c => 0x58,
            0x1d => 0xe4,
            0x35 => 0x54,
            0x37 => 0x46,
            0x38 => 0xe6,
            0x47 => 0x4a,
            0x48 => 0x52,
            0x49 => 0x4b,
            0x4b => 0x50,
            0x4d => 0x4f,
            0x4f => 0x4d,
            0x50 => 0x51,
            0x51 => 0x4e,
            0x52 => 0x49,
            0x53 => 0x4c,
            0x5b => 0xe3,
            0x5c => 0xe7,
            _ => return None,
        }
    } else {
        match make_code {
            0x01 => 0x29,
            0x02..=0x0a => 0x1e + make_code - 0x02,
            0x0b => 0x27,
            0x0c => 0x2d,
            0x0d => 0x2e,
            0x0e => 0x2a,
            0x0f => 0x2b,
            0x10..=0x19 => [0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13]
                [(make_code - 0x10) as usize],
            0x1a => 0x2f,
            0x1b => 0x30,
            0x1c => 0x28,
            0x1d => 0xe0,
            0x1e..=0x26 => {
                [0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f][(make_code - 0x1e) as usize]
            }
            0x27 => 0x33,
            0x28 => 0x34,
            0x29 => 0x35,
            0x2a => 0xe1,
            0x2b => 0x31,
            0x2c..=0x32 => [0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10][(make_code - 0x2c) as usize],
            0x33 => 0x36,
            0x34 => 0x37,
            0x35 => 0x38,
            0x36 => 0xe5,
            0x37 => 0x55,
            0x38 => 0xe2,
            0x39 => 0x2c,
            0x3a => 0x39,
            0x3b..=0x44 => 0x3a + make_code - 0x3b,
            0x45 => 0x53,
            0x46 => 0x47,
            0x47 => 0x5f,
            0x48 => 0x60,
            0x49 => 0x61,
            0x4a => 0x56,
            0x4b => 0x5c,
            0x4c => 0x5d,
            0x4d => 0x5e,
            0x4e => 0x57,
            0x4f => 0x59,
            0x50 => 0x5a,
            0x51 => 0x5b,
            0x52 => 0x62,
            0x53 => 0x63,
            0x56 => 0x64,
            0x57 => 0x44,
            0x58 => 0x45,
            _ => return None,
        }
    };
    Some(PhysicalKey::from_hid_usage(usage))
}

fn normalize_virtual_key(packet: RawKeyboardPacket) -> i32 {
    match (packet.make_code, packet.flags & RI_KEY_E0 != 0) {
        (0x2a, _) => 0xa0,
        (0x36, _) => 0xa1,
        (0x1d, false) => 0xa2,
        (0x1d, true) => 0xa3,
        (0x38, false) => 0xa4,
        (0x38, true) => 0xa5,
        _ => i32::from(packet.virtual_key),
    }
}

fn query_pressed_controls(
    candidates: &BTreeMap<InputControl, SystemControl>,
) -> windows::core::Result<BTreeSet<InputControl>> {
    // SAFETY: OpenInputDesktop is an availability guard and every successful
    // open is closed on this thread. GetAsyncKeyState receives only captured
    // Win32 virtual-key values and does not retain references.
    unsafe {
        let desktop =
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS::default(), false, DESKTOP_READOBJECTS)?;
        let pressed = candidates
            .iter()
            .filter_map(|(control, key)| {
                (GetAsyncKeyState(key.0) as u16 & 0x8000 != 0).then_some(*control)
            })
            .collect();
        CloseDesktop(desktop)?;
        Ok(pressed)
    }
}

fn cursor_sample(at: MonotonicMillis) -> Option<CursorSample> {
    // SAFETY: each query receives a valid initialized stack pointer; the
    // monitor handle is used only for the immediately following bounds query.
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).ok()?;
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        CursorSample::new(
            CursorPosition {
                x: f64::from(point.x),
                y: f64::from(point.y),
            },
            CursorViewport {
                origin: CursorPosition {
                    x: f64::from(info.rcMonitor.left),
                    y: f64::from(info.rcMonitor.top),
                },
                width: f64::from(info.rcMonitor.right - info.rcMonitor.left),
                height: f64::from(info.rcMonitor.bottom - info.rcMonitor.top),
            },
            at,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_runtime::{HandSide, InputBindings, RuntimeCommand, RuntimeOwner};
    use std::sync::Arc;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput,
        VIRTUAL_KEY,
    };

    #[test]
    fn window_state_publishes_live_platform_diagnostics() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 8);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let diagnostics_producer = runtime.platform_input_diagnostics_producer();
        let mut state = WindowState::new(
            runtime.input_producer(),
            runtime.cursor_producer(),
            runtime.gamepad_axis_producer(),
            diagnostics_producer,
            Arc::new(AtomicBool::new(false)),
            WorkerOptions::default(),
        );
        state.diagnostics.runtime_queue_overflows = 2;
        state.diagnostics.gamepad_connections = 3;
        state.diagnostics.gamepad_disconnections = 1;
        state.diagnostics.gamepad_axis_publish_rejections = 4;
        state.publish_diagnostics();

        let live = client.snapshot().platform_input;
        assert_eq!(live.runtime_queue_overflows, 2);
        assert_eq!(live.gamepad_connections, 3);
        assert_eq!(live.gamepad_disconnections, 1);
        assert_eq!(live.gamepad_axis_publish_rejections, 4);
        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[test]
    fn scan_codes_preserve_hid_identity_and_modifier_side() {
        assert_eq!(map_scan_code(0x1e, 0), Some(PhysicalKey::KEY_A));
        assert_eq!(map_scan_code(0x1d, 0), Some(PhysicalKey::LEFT_CONTROL));
        assert_eq!(
            map_scan_code(0x1d, RI_KEY_E0),
            Some(PhysicalKey::from_hid_usage(0xe4))
        );
        assert_eq!(
            map_scan_code(0x37, RI_KEY_E0),
            Some(PhysicalKey::from_hid_usage(0x46))
        );
        assert_eq!(map_scan_code(0x7f, 0), None);

        let shortcuts = bongocat_config::ShortcutConfig {
            commands: vec![bongocat_config::ShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+A".to_owned(),
            }],
            ..bongocat_config::ShortcutConfig::default()
        }
        .compile()
        .expect("compiled shortcut");
        let modifiers = bongocat_config::ShortcutModifiers::from_bits(
            bongocat_config::ShortcutModifiers::CONTROL,
        )
        .expect("control modifier");
        let mapped = map_scan_code(0x1e, 0).expect("mapped A");
        assert!(
            shortcuts
                .resolve_hid_usage(modifiers, mapped.hid_usage())
                .is_some()
        );
    }

    #[test]
    fn raw_keyboard_decoder_rejects_truncation_and_reads_release() {
        let header_size = size_of::<RAWINPUTHEADER>();
        let mut bytes = vec![0_u8; header_size + 16];
        bytes[0..4].copy_from_slice(&1_u32.to_le_bytes());
        let declared = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&declared.to_le_bytes());
        bytes[header_size..header_size + 2].copy_from_slice(&0x1e_u16.to_le_bytes());
        bytes[header_size + 2..header_size + 4].copy_from_slice(&RI_KEY_BREAK.to_le_bytes());
        bytes[header_size + 6..header_size + 8].copy_from_slice(&0x41_u16.to_le_bytes());
        let Some(RawInputPacket::Keyboard(packet)) =
            decode_raw_input_bytes(&bytes, header_size).expect("keyboard packet")
        else {
            panic!("expected keyboard packet");
        };
        assert_eq!(packet.make_code, 0x1e);
        assert_eq!(packet.flags, RI_KEY_BREAK);
        assert_eq!(packet.virtual_key, 0x41);
        assert!(decode_raw_input_bytes(&bytes[..header_size + 3], header_size).is_err());
    }

    #[test]
    fn xinput_loader_resolves_the_system_api_without_an_import_library() {
        assert!(XInputApi::load().is_some());
    }

    #[test]
    fn xinput_poller_maps_initial_edges_multiple_slots_and_reconnect_generation() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 64);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let producer = runtime.input_producer();
        let axis_producer = runtime.gamepad_axis_producer();
        let mut poller = XInputPoller::default();
        let mut diagnostics = PlatformInputDiagnostics::default();
        let mut states = [None; 4];
        states[0] = Some(XInputState {
            gamepad: XInputGamepad {
                buttons: XINPUT_GAMEPAD_A,
                left_trigger: 128,
                ..XInputGamepad::default()
            },
            ..XInputState::default()
        });
        states[2] = Some(XInputState {
            gamepad: XInputGamepad {
                buttons: XINPUT_GAMEPAD_B,
                ..XInputGamepad::default()
            },
            ..XInputState::default()
        });

        poll_synthetic(
            &mut poller,
            &producer,
            &axis_producer,
            MonotonicMillis::new(1),
            &mut diagnostics,
            states,
        )
        .expect("initial poll");
        wait_for_input_sequence(&client, 4, TIMEOUT);
        let first = client.snapshot();
        assert_eq!(first.input.pressed_gamepad_button_count, 3);
        assert_eq!(diagnostics.gamepad_connections, 2);
        assert_eq!(diagnostics.gamepad_axis_samples, 12);
        let first_generation = poller.slots[0]
            .expect("slot 0 connected")
            .connection
            .generation;
        assert_eq!(
            poller.slots[2]
                .expect("slot 2 connected")
                .connection
                .generation,
            1
        );

        states[0] = None;
        poll_synthetic(
            &mut poller,
            &producer,
            &axis_producer,
            MonotonicMillis::new(2),
            &mut diagnostics,
            states,
        )
        .expect("disconnect poll");
        states[0] = Some(XInputState {
            gamepad: XInputGamepad {
                left_trigger: 127,
                ..XInputGamepad::default()
            },
            ..XInputState::default()
        });
        poll_synthetic(
            &mut poller,
            &producer,
            &axis_producer,
            MonotonicMillis::new(3),
            &mut diagnostics,
            states,
        )
        .expect("reconnect poll");
        let reconnected = poller.slots[0].expect("slot 0 reconnected");
        assert!(reconnected.connection.generation > first_generation);
        assert_eq!(reconnected.buttons & (1 << 16), 0);
        assert_eq!(diagnostics.gamepad_disconnections, 1);
        assert_eq!(diagnostics.gamepad_connections, 3);
        assert_eq!(diagnostics.gamepad_axis_publish_rejections, 0);

        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[test]
    fn stopped_axis_transport_is_not_reported_as_reliable_queue_overflow() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 64);
        let producer = runtime.input_producer();
        let axis_producer = runtime.gamepad_axis_producer();
        runtime.shutdown(TIMEOUT).expect("runtime stop");
        let mut poller = XInputPoller::default();
        let mut diagnostics = PlatformInputDiagnostics::default();
        let mut states = [None; 4];
        states[0] = Some(XInputState::default());

        let error = poll_synthetic(
            &mut poller,
            &producer,
            &axis_producer,
            MonotonicMillis::new(1),
            &mut diagnostics,
            states,
        )
        .expect_err("stopped axis transport must fail");
        assert!(matches!(error, InputPublishError::RuntimeStopped(_)));
        assert_eq!(diagnostics.gamepad_axis_publish_rejections, 0);
    }

    fn poll_synthetic(
        poller: &mut XInputPoller,
        producer: &InputProducer,
        axis_producer: &GamepadAxisProducer,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
        states: [Option<XInputState>; 4],
    ) -> Result<(), InputPublishError> {
        poller.poll_with(
            producer,
            axis_producer,
            at,
            diagnostics,
            |slot, output| match states[slot as usize] {
                Some(state) => {
                    // SAFETY: `poll_with` supplies a valid pointer to its
                    // initialized stack state for the duration of this call.
                    unsafe { output.write(state) };
                    XINPUT_ERROR_SUCCESS
                }
                None => XINPUT_ERROR_DEVICE_NOT_CONNECTED,
            },
        )
    }

    fn wait_for_input_sequence(
        client: &bongocat_runtime::RuntimeClient,
        expected: u64,
        timeout: Duration,
    ) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if client
                .snapshot()
                .input
                .last_input_sequence
                .is_some_and(|sequence| sequence >= expected)
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("input sequence {expected} did not reach runtime");
    }

    #[test]
    #[ignore = "requires a Windows interactive input desktop"]
    fn synthetic_missing_release_is_reconciled_in_formal_runtime() {
        const TIMEOUT: Duration = Duration::from_secs(3);
        let runtime = RuntimeOwner::start(true, 64);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let bindings = InputBindings::new(BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]));
        let sequence = client
            .send(RuntimeCommand::SetInputBindings(Arc::new(bindings)))
            .expect("bindings");
        client
            .wait_for_command(sequence, TIMEOUT)
            .expect("bindings applied");
        let service = WindowsInputService::start_with_diagnostics_and_options(
            runtime.input_producer(),
            runtime.cursor_producer(),
            runtime.gamepad_axis_producer(),
            runtime.platform_input_diagnostics_producer(),
            WorkerOptions {
                drop_next_key_release: true,
            },
        )
        .expect("formal Windows input service");

        let inputs = [keyboard_input(false), keyboard_input(true)];
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        assert_eq!(sent, inputs.len() as u32);
        let deadline = Instant::now() + TIMEOUT;
        let mut observed_pressed = false;
        let mut released = false;
        while Instant::now() < deadline {
            let snapshot = client.snapshot();
            observed_pressed |= snapshot.model_input.left_hand_down;
            if observed_pressed
                && !snapshot.model_input.left_hand_down
                && snapshot.input.diagnostics.reconciled_release > 0
            {
                released = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(observed_pressed, "synthetic key down did not reach runtime");
        assert!(released, "missing release left a pressed key in runtime");

        let diagnostics = service.stop().expect("input service stop");
        assert!(diagnostics.captured_edges >= 2);
        assert_eq!(diagnostics.callback_panics, 0);
        assert_eq!(diagnostics.capture_queue_overflows, 0);
        assert_eq!(diagnostics.runtime_queue_overflows, 0);
        assert!(diagnostics.reconciliation_runs >= 2);
        assert!(diagnostics.clean_shutdown);
        assert_eq!(client.snapshot().platform_input, diagnostics);
        let stopped = runtime.shutdown(TIMEOUT).expect("runtime stop");
        assert_eq!(stopped.input.pressed_key_count, 0);
    }

    fn keyboard_input(released: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: 0x1e,
                    dwFlags: if released {
                        KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_SCANCODE
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}
