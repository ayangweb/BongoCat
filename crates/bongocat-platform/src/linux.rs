use crate::{
    DisplayBounds, InputPermission, NativeWindowError, PlatformInputDiagnostics,
    PlatformInputError, PlatformInputServiceStatus, gilrs_gamepad::GilrsGamepad,
};
use bongocat_config::Language;
use bongocat_input::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorViewport,
    GamepadAxisProducer, InputControl, InputEdge, InputEvent, InputProducer, InputPublishError,
    InputResetReason, InputSource, MonotonicMillis, MouseButton, PhysicalKey,
    PlatformInputDiagnosticsProducer,
};
use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode, SynchronizationCode};
use raw_window_handle::HasWindowHandle;
use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    mem::MaybeUninit,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const INPUT_DIRECTORY: &str = "/dev/input";
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const RECONCILIATION_INTERVAL: Duration = Duration::from_millis(250);
const DEVICE_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const SUSPEND_DETECTION_THRESHOLD: Duration = Duration::from_secs(1);
const SERVICE_TIMEOUT: Duration = Duration::from_secs(2);
// evdev reports relative device counts, not compositor cursor coordinates.
// This fixed calibration space controls model-input sensitivity and is not a
// detected display resolution.
const VIRTUAL_CURSOR_CALIBRATION_WIDTH: f64 = 1_920.0;
const VIRTUAL_CURSOR_CALIBRATION_HEIGHT: f64 = 1_080.0;

pub fn system_language() -> Language {
    sys_locale::get_locale().map_or_else(Language::default, |locale| {
        Language::from_system_locale(&locale)
    })
}

pub fn current_display_bounds() -> Option<DisplayBounds> {
    None
}

pub fn display_bounds_for_window(x: f32, y: f32, width: f32, height: f32) -> Option<DisplayBounds> {
    current_display_bounds().filter(|display| display.intersects_window(x, y, width, height))
}

pub const fn local_window_origin(_display: DisplayBounds, x: f32, y: f32) -> (f32, f32) {
    (x, y)
}

pub const fn global_window_origin(_display_id: Option<u32>, x: f32, y: f32) -> (f32, f32) {
    (x, y)
}

pub fn hide_native_window(_window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    Err(NativeWindowError::UnsupportedHandle)
}

pub fn show_native_window(_window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    Err(NativeWindowError::UnsupportedHandle)
}

struct InputDevice {
    path: PathBuf,
    device: Device,
    relative_motion: RelativeMotion,
}

#[derive(Default)]
struct RelativeMotion {
    x: i64,
    y: i64,
}

impl RelativeMotion {
    fn observe(&mut self, axis: RelativeAxisCode, value: i32) {
        match axis {
            RelativeAxisCode::REL_X => self.x = self.x.saturating_add(i64::from(value)),
            RelativeAxisCode::REL_Y => self.y = self.y.saturating_add(i64::from(value)),
            _ => {}
        }
    }

    fn finish_report(&mut self) -> Option<(i64, i64)> {
        let motion = (self.x != 0 || self.y != 0).then_some((self.x, self.y));
        self.x = 0;
        self.y = 0;
        motion
    }

    fn discard_report(&mut self) {
        self.x = 0;
        self.y = 0;
    }
}

struct VirtualCursor {
    position: CursorPosition,
}

impl Default for VirtualCursor {
    fn default() -> Self {
        Self {
            position: CursorPosition {
                x: VIRTUAL_CURSOR_CALIBRATION_WIDTH / 2.0,
                y: VIRTUAL_CURSOR_CALIBRATION_HEIGHT / 2.0,
            },
        }
    }
}

impl VirtualCursor {
    fn apply(&mut self, x: i64, y: i64) -> bool {
        let previous = self.position;
        self.position.x = (self.position.x + x as f64).clamp(0.0, VIRTUAL_CURSOR_CALIBRATION_WIDTH);
        self.position.y =
            (self.position.y + y as f64).clamp(0.0, VIRTUAL_CURSOR_CALIBRATION_HEIGHT);
        self.position != previous
    }

    fn sample(&self, at: MonotonicMillis) -> CursorSample {
        CursorSample::new(
            self.position,
            CursorViewport {
                origin: CursorPosition { x: 0.0, y: 0.0 },
                width: VIRTUAL_CURSOR_CALIBRATION_WIDTH,
                height: VIRTUAL_CURSOR_CALIBRATION_HEIGHT,
            },
            at,
        )
        .expect("the fixed Linux virtual cursor viewport is valid")
    }
}

struct DeviceDiscovery {
    devices: Vec<InputDevice>,
    event_nodes: bool,
    permission_denied: bool,
    observed_paths: Option<HashSet<PathBuf>>,
    unsupported_paths: HashSet<PathBuf>,
}

struct SuspendDetector {
    boot_time_offset: Option<Duration>,
}

impl SuspendDetector {
    fn new() -> Self {
        Self {
            boot_time_offset: boot_time_offset(),
        }
    }

    fn resumed(&mut self) -> bool {
        let Some(current) = boot_time_offset() else {
            return false;
        };
        let resumed = self
            .boot_time_offset
            .is_some_and(|previous| resumed_since(previous, current, SUSPEND_DETECTION_THRESHOLD));
        self.boot_time_offset = Some(current);
        resumed
    }
}

pub struct LinuxInputService {
    stop: Arc<AtomicBool>,
    completion: Receiver<Result<PlatformInputDiagnostics, PlatformInputError>>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxInputService {
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
        let discovery = discover_devices(&HashSet::new(), &HashSet::new());

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bongocat-linux-input".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_input_worker(
                        discovery,
                        producer,
                        cursor_producer,
                        gamepad_axis_producer,
                        diagnostics_producer,
                        worker_stop,
                    )
                }))
                .unwrap_or(Err(PlatformInputError::WorkerPanicked));
                let _ = completion_sender.send(result);
            })
            .map_err(|_| PlatformInputError::WorkerPanicked)?;
        Ok(Self {
            stop,
            completion: completion_receiver,
            worker: Some(worker),
        })
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

pub(crate) fn evdev_input_permission() -> InputPermission {
    let Ok(entries) = fs::read_dir(INPUT_DIRECTORY) else {
        return InputPermission::Denied;
    };
    if entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_event_node(path))
        .any(|path| fs::File::open(path).is_ok())
    {
        InputPermission::Granted
    } else {
        InputPermission::Denied
    }
}

impl Drop for LinuxInputService {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish(SERVICE_TIMEOUT);
        }
    }
}

fn discover_devices(
    existing: &HashSet<PathBuf>,
    known_unsupported: &HashSet<PathBuf>,
) -> DeviceDiscovery {
    let entries = match fs::read_dir(INPUT_DIRECTORY) {
        Ok(entries) => entries,
        Err(error) => {
            return DeviceDiscovery {
                devices: Vec::new(),
                event_nodes: error.kind() == io::ErrorKind::PermissionDenied,
                permission_denied: error.kind() == io::ErrorKind::PermissionDenied,
                observed_paths: None,
                unsupported_paths: HashSet::new(),
            };
        }
    };
    let mut observed_paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_event_node(path))
        .collect::<Vec<_>>();
    observed_paths.sort();
    let event_nodes = !observed_paths.is_empty();
    let mut permission_denied = false;
    let mut devices = Vec::new();
    let mut unsupported_paths = HashSet::new();
    for path in observed_paths
        .iter()
        .filter(|path| !existing.contains(*path) && !known_unsupported.contains(*path))
        .cloned()
    {
        match Device::open(&path) {
            Ok(device) if device_has_supported_controls(&device) => {
                if device.set_nonblocking(true).is_ok() {
                    devices.push(InputDevice {
                        path,
                        device,
                        relative_motion: RelativeMotion::default(),
                    });
                }
            }
            Ok(_) => {
                unsupported_paths.insert(path);
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                permission_denied = true;
            }
            Err(_) => {}
        }
    }
    DeviceDiscovery {
        devices,
        event_nodes,
        permission_denied,
        observed_paths: Some(observed_paths.into_iter().collect()),
        unsupported_paths,
    }
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("event"))
}

fn discovery_error(discovery: &DeviceDiscovery) -> PlatformInputError {
    if discovery.event_nodes && discovery.permission_denied {
        PlatformInputError::PermissionDenied
    } else {
        PlatformInputError::BackendUnavailable
    }
}

fn device_has_supported_controls(device: &Device) -> bool {
    let has_buttons_or_keys = device
        .supported_keys()
        .is_some_and(|keys| keys.iter().any(|key| linux_control(key).is_some()));
    let has_pointer_motion = device.supported_relative_axes().is_some_and(|axes| {
        axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y)
    });
    has_buttons_or_keys || has_pointer_motion
}

fn run_input_worker(
    discovery: DeviceDiscovery,
    producer: InputProducer,
    cursor_producer: CursorProducer,
    gamepad_axis_producer: GamepadAxisProducer,
    diagnostics_producer: PlatformInputDiagnosticsProducer,
    stop: Arc<AtomicBool>,
) -> Result<PlatformInputDiagnostics, PlatformInputError> {
    let initial_evdev_error = discovery
        .devices
        .is_empty()
        .then(|| discovery_error(&discovery));
    let mut devices = discovery.devices;
    let mut known_unsupported = discovery.unsupported_paths;
    let started = Instant::now();
    let mut diagnostics = PlatformInputDiagnostics {
        service_start_attempts: 1,
        ..PlatformInputDiagnostics::default()
    };
    set_evdev_diagnostics(&mut diagnostics, initial_evdev_error);
    let _ = diagnostics_producer.publish(diagnostics);
    let mut next_reconciliation = Instant::now() + RECONCILIATION_INTERVAL;
    let mut next_device_scan = Instant::now() + DEVICE_SCAN_INTERVAL;
    let mut next_poll = Instant::now() + POLL_INTERVAL;
    let mut gamepad = GilrsGamepad::new(producer.clone(), gamepad_axis_producer);
    let mut recovery_pending = None;
    let mut suspend_detector = SuspendDetector::new();
    let mut virtual_cursor = VirtualCursor::default();
    publish_cursor(
        &cursor_producer,
        virtual_cursor.sample(monotonic(started)),
        &mut diagnostics,
    )?;

    while !stop.load(Ordering::Acquire) {
        if let Some(reason) = recovery_pending {
            if recover_input_state(&producer, &mut gamepad, reason, &mut diagnostics, started)? {
                recovery_pending = None;
            } else {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        }
        if suspend_detector.resumed() {
            devices.clear();
            known_unsupported.clear();
            next_device_scan = Instant::now();
            recovery_pending = Some(InputResetReason::Sleep);
            continue;
        }
        match gamepad.drain(monotonic(started), &mut diagnostics) {
            Ok(()) => {}
            Err(InputPublishError::QueueFull(_)) => {
                diagnostics.runtime_queue_overflows =
                    diagnostics.runtime_queue_overflows.saturating_add(1);
                recovery_pending = Some(InputResetReason::QueueOverflow);
                continue;
            }
            Err(InputPublishError::RuntimeStopped(_)) => {
                return Err(PlatformInputError::RuntimeStopped);
            }
        }
        let mut removed = Vec::new();
        let mut cursor_changed = false;
        'devices: for (index, input_device) in devices.iter_mut().enumerate() {
            let events = match input_device.device.fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(_) => {
                    removed.push(index);
                    continue;
                }
            };
            for event in events {
                match event.destructure() {
                    EventSummary::Key(_, key, value) => {
                        let Some(control) = linux_control(key) else {
                            diagnostics.unmapped_keys = diagnostics.unmapped_keys.saturating_add(1);
                            continue;
                        };
                        let edge = match value {
                            0 => InputEdge::Up,
                            1 => InputEdge::Down,
                            _ => continue,
                        };
                        diagnostics.captured_edges = diagnostics.captured_edges.saturating_add(1);
                        match producer.publish(InputEvent::Edge {
                            control,
                            edge,
                            source: InputSource::Capture,
                            at: monotonic(started),
                        }) {
                            Ok(_) => {
                                diagnostics.queued_edges =
                                    diagnostics.queued_edges.saturating_add(1);
                            }
                            Err(InputPublishError::QueueFull(_)) => {
                                diagnostics.runtime_queue_overflows =
                                    diagnostics.runtime_queue_overflows.saturating_add(1);
                                recovery_pending = Some(InputResetReason::QueueOverflow);
                                break 'devices;
                            }
                            Err(InputPublishError::RuntimeStopped(_)) => {
                                return Err(PlatformInputError::RuntimeStopped);
                            }
                        }
                    }
                    EventSummary::RelativeAxis(_, axis, value) => {
                        input_device.relative_motion.observe(axis, value);
                    }
                    EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                        if let Some((x, y)) = input_device.relative_motion.finish_report() {
                            cursor_changed |= virtual_cursor.apply(x, y);
                        }
                    }
                    EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                        input_device.relative_motion.discard_report()
                    }
                    _ => {}
                }
            }
        }
        if recovery_pending.is_some() {
            continue;
        }
        if cursor_changed {
            publish_cursor(
                &cursor_producer,
                virtual_cursor.sample(monotonic(started)),
                &mut diagnostics,
            )?;
        }

        if !removed.is_empty() {
            for index in removed.into_iter().rev() {
                devices.swap_remove(index);
            }
            recovery_pending = Some(InputResetReason::DeviceRemoved);
            continue;
        }

        let now = Instant::now();
        if now >= next_reconciliation {
            match reconcile_devices(&devices, &producer, &mut diagnostics, started) {
                Ok(()) => {}
                Err(InputPublishError::QueueFull(_)) => {
                    diagnostics.runtime_queue_overflows =
                        diagnostics.runtime_queue_overflows.saturating_add(1);
                    recovery_pending = Some(InputResetReason::QueueOverflow);
                    continue;
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    return Err(PlatformInputError::RuntimeStopped);
                }
            }
            let _ = diagnostics_producer.publish(diagnostics);
            next_reconciliation = now + RECONCILIATION_INTERVAL;
        }
        if now >= next_device_scan {
            let open_paths = devices
                .iter()
                .map(|device| device.path.clone())
                .collect::<HashSet<_>>();
            let discovery = discover_devices(&open_paths, &known_unsupported);
            let evdev_error = (devices.is_empty() && discovery.devices.is_empty())
                .then(|| discovery_error(&discovery));
            devices.extend(discovery.devices);
            if let Some(current_paths) = discovery.observed_paths {
                known_unsupported.retain(|path| current_paths.contains(path));
            }
            known_unsupported.extend(discovery.unsupported_paths);
            set_evdev_diagnostics(&mut diagnostics, evdev_error);
            next_device_scan = now + DEVICE_SCAN_INTERVAL;
        }
        // Keep polling on a fixed cadence instead of adding the time spent
        // draining devices and scanning /dev/input to every interval.
        thread::sleep(next_poll.saturating_duration_since(Instant::now()));
        let now = Instant::now();
        next_poll += POLL_INTERVAL;
        if next_poll <= now {
            next_poll = now + POLL_INTERVAL;
        }
    }

    let gamepad_shutdown = gamepad.shutdown();
    diagnostics.gamepad_disconnections = diagnostics
        .gamepad_disconnections
        .saturating_add(gamepad_shutdown.disconnected);
    let _ = producer.recover(InputResetReason::ServiceRestart, monotonic(started));
    cursor_producer.stop();
    diagnostics.service_status = PlatformInputServiceStatus::Stopped;
    diagnostics.clean_shutdown = gamepad_shutdown.backend_clean;
    let _ = diagnostics_producer.publish(diagnostics);
    Ok(diagnostics)
}

fn set_evdev_diagnostics(
    diagnostics: &mut PlatformInputDiagnostics,
    error: Option<PlatformInputError>,
) {
    match error {
        None => {
            diagnostics.service_status = PlatformInputServiceStatus::Running;
            diagnostics.service_error_code = None;
        }
        Some(PlatformInputError::PermissionDenied) => {
            diagnostics.service_status = PlatformInputServiceStatus::PermissionDenied;
            diagnostics.service_error_code = Some(PlatformInputError::PermissionDenied.as_str());
        }
        Some(error) => {
            diagnostics.service_status = PlatformInputServiceStatus::BackendUnavailable;
            diagnostics.service_error_code = Some(error.as_str());
        }
    }
}

fn reconcile_devices(
    devices: &[InputDevice],
    producer: &InputProducer,
    diagnostics: &mut PlatformInputDiagnostics,
    started: Instant,
) -> Result<(), InputPublishError> {
    let mut pressed = BTreeSet::new();
    for input_device in devices {
        let Ok(keys) = input_device.device.get_key_state() else {
            continue;
        };
        pressed.extend(keys.iter().filter_map(linux_control));
    }
    diagnostics.reconciliation_runs = diagnostics.reconciliation_runs.saturating_add(1);
    producer
        .publish(InputEvent::Reconcile {
            pressed,
            at: monotonic(started),
        })
        .map(|_| ())
}

fn recover_input_state(
    producer: &InputProducer,
    gamepad: &mut GilrsGamepad,
    reason: InputResetReason,
    diagnostics: &mut PlatformInputDiagnostics,
    started: Instant,
) -> Result<bool, PlatformInputError> {
    match producer.recover(reason, monotonic(started)) {
        Ok(_) => {
            diagnostics.recovery_resets = diagnostics.recovery_resets.saturating_add(1);
            match gamepad.reseed(monotonic(started), diagnostics) {
                Ok(()) => Ok(true),
                Err(InputPublishError::QueueFull(_)) => {
                    diagnostics.runtime_queue_overflows =
                        diagnostics.runtime_queue_overflows.saturating_add(1);
                    Ok(false)
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    Err(PlatformInputError::RuntimeStopped)
                }
            }
        }
        Err(InputPublishError::QueueFull(_)) => {
            diagnostics.runtime_queue_overflows =
                diagnostics.runtime_queue_overflows.saturating_add(1);
            Ok(false)
        }
        Err(InputPublishError::RuntimeStopped(_)) => Err(PlatformInputError::RuntimeStopped),
    }
}

fn publish_cursor(
    producer: &CursorProducer,
    sample: CursorSample,
    diagnostics: &mut PlatformInputDiagnostics,
) -> Result<(), PlatformInputError> {
    diagnostics.cursor_captured = diagnostics.cursor_captured.saturating_add(1);
    match producer.publish(sample) {
        Ok(()) => Ok(()),
        Err(CursorPublishError::NonMonotonic(_)) => {
            diagnostics.cursor_publish_rejections =
                diagnostics.cursor_publish_rejections.saturating_add(1);
            Ok(())
        }
        Err(CursorPublishError::RuntimeStopped(_)) => {
            diagnostics.cursor_publish_rejections =
                diagnostics.cursor_publish_rejections.saturating_add(1);
            diagnostics.cursor_rejected_after_stop =
                diagnostics.cursor_rejected_after_stop.saturating_add(1);
            Err(PlatformInputError::RuntimeStopped)
        }
    }
}

fn monotonic(started: Instant) -> MonotonicMillis {
    MonotonicMillis::new(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
}

fn boot_time_offset() -> Option<Duration> {
    let monotonic = linux_clock(libc::CLOCK_MONOTONIC)?;
    linux_clock(libc::CLOCK_BOOTTIME)?.checked_sub(monotonic)
}

fn resumed_since(previous: Duration, current: Duration, threshold: Duration) -> bool {
    current.saturating_sub(previous) >= threshold
}

fn linux_clock(clock: libc::clockid_t) -> Option<Duration> {
    let mut time = MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime initializes the provided timespec on success, and
    // the pointer remains valid for the duration of the call.
    if unsafe { libc::clock_gettime(clock, time.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the successful clock_gettime call above initialized the value.
    let time = unsafe { time.assume_init() };
    let seconds = u64::try_from(time.tv_sec).ok()?;
    let nanoseconds = u32::try_from(time.tv_nsec).ok()?;
    (nanoseconds < 1_000_000_000).then(|| Duration::new(seconds, nanoseconds))
}

fn linux_control(key: KeyCode) -> Option<InputControl> {
    let mouse = match key {
        KeyCode::BTN_LEFT => Some(MouseButton::Left),
        KeyCode::BTN_RIGHT => Some(MouseButton::Right),
        KeyCode::BTN_MIDDLE => Some(MouseButton::Middle),
        KeyCode::BTN_SIDE => Some(MouseButton::Back),
        KeyCode::BTN_EXTRA => Some(MouseButton::Forward),
        _ => None,
    };
    if let Some(button) = mouse {
        return Some(InputControl::Mouse(button));
    }
    linux_key_hid_usage(key).map(|usage| InputControl::Key(PhysicalKey::from_hid_usage(usage)))
}

fn linux_key_hid_usage(key: KeyCode) -> Option<u16> {
    if (KeyCode::KEY_F1.0..=KeyCode::KEY_F10.0).contains(&key.0) {
        return Some(0x3a + key.0 - KeyCode::KEY_F1.0);
    }
    if (KeyCode::KEY_F13.0..=KeyCode::KEY_F24.0).contains(&key.0) {
        return Some(0x68 + key.0 - KeyCode::KEY_F13.0);
    }
    let usage = match key {
        KeyCode::KEY_A => 0x04,
        KeyCode::KEY_B => 0x05,
        KeyCode::KEY_C => 0x06,
        KeyCode::KEY_D => 0x07,
        KeyCode::KEY_E => 0x08,
        KeyCode::KEY_F => 0x09,
        KeyCode::KEY_G => 0x0a,
        KeyCode::KEY_H => 0x0b,
        KeyCode::KEY_I => 0x0c,
        KeyCode::KEY_J => 0x0d,
        KeyCode::KEY_K => 0x0e,
        KeyCode::KEY_L => 0x0f,
        KeyCode::KEY_M => 0x10,
        KeyCode::KEY_N => 0x11,
        KeyCode::KEY_O => 0x12,
        KeyCode::KEY_P => 0x13,
        KeyCode::KEY_Q => 0x14,
        KeyCode::KEY_R => 0x15,
        KeyCode::KEY_S => 0x16,
        KeyCode::KEY_T => 0x17,
        KeyCode::KEY_U => 0x18,
        KeyCode::KEY_V => 0x19,
        KeyCode::KEY_W => 0x1a,
        KeyCode::KEY_X => 0x1b,
        KeyCode::KEY_Y => 0x1c,
        KeyCode::KEY_Z => 0x1d,
        KeyCode::KEY_1 => 0x1e,
        KeyCode::KEY_2 => 0x1f,
        KeyCode::KEY_3 => 0x20,
        KeyCode::KEY_4 => 0x21,
        KeyCode::KEY_5 => 0x22,
        KeyCode::KEY_6 => 0x23,
        KeyCode::KEY_7 => 0x24,
        KeyCode::KEY_8 => 0x25,
        KeyCode::KEY_9 => 0x26,
        KeyCode::KEY_0 => 0x27,
        KeyCode::KEY_ENTER => 0x28,
        KeyCode::KEY_ESC => 0x29,
        KeyCode::KEY_BACKSPACE => 0x2a,
        KeyCode::KEY_TAB => 0x2b,
        KeyCode::KEY_SPACE => 0x2c,
        KeyCode::KEY_MINUS => 0x2d,
        KeyCode::KEY_EQUAL => 0x2e,
        KeyCode::KEY_LEFTBRACE => 0x2f,
        KeyCode::KEY_RIGHTBRACE => 0x30,
        KeyCode::KEY_BACKSLASH => 0x31,
        KeyCode::KEY_SEMICOLON => 0x33,
        KeyCode::KEY_APOSTROPHE => 0x34,
        KeyCode::KEY_GRAVE => 0x35,
        KeyCode::KEY_COMMA => 0x36,
        KeyCode::KEY_DOT => 0x37,
        KeyCode::KEY_SLASH => 0x38,
        KeyCode::KEY_CAPSLOCK => 0x39,
        KeyCode::KEY_F11 => 0x44,
        KeyCode::KEY_F12 => 0x45,
        KeyCode::KEY_SYSRQ => 0x46,
        KeyCode::KEY_SCROLLLOCK => 0x47,
        KeyCode::KEY_PAUSE => 0x48,
        KeyCode::KEY_INSERT => 0x49,
        KeyCode::KEY_HOME => 0x4a,
        KeyCode::KEY_PAGEUP => 0x4b,
        KeyCode::KEY_DELETE => 0x4c,
        KeyCode::KEY_END => 0x4d,
        KeyCode::KEY_PAGEDOWN => 0x4e,
        KeyCode::KEY_RIGHT => 0x4f,
        KeyCode::KEY_LEFT => 0x50,
        KeyCode::KEY_DOWN => 0x51,
        KeyCode::KEY_UP => 0x52,
        KeyCode::KEY_NUMLOCK => 0x53,
        KeyCode::KEY_KPSLASH => 0x54,
        KeyCode::KEY_KPASTERISK => 0x55,
        KeyCode::KEY_KPMINUS => 0x56,
        KeyCode::KEY_KPPLUS => 0x57,
        KeyCode::KEY_KPENTER => 0x58,
        KeyCode::KEY_KP1 => 0x59,
        KeyCode::KEY_KP2 => 0x5a,
        KeyCode::KEY_KP3 => 0x5b,
        KeyCode::KEY_KP4 => 0x5c,
        KeyCode::KEY_KP5 => 0x5d,
        KeyCode::KEY_KP6 => 0x5e,
        KeyCode::KEY_KP7 => 0x5f,
        KeyCode::KEY_KP8 => 0x60,
        KeyCode::KEY_KP9 => 0x61,
        KeyCode::KEY_KP0 => 0x62,
        KeyCode::KEY_KPDOT => 0x63,
        KeyCode::KEY_102ND => 0x64,
        KeyCode::KEY_COMPOSE => 0x65,
        KeyCode::KEY_POWER => 0x66,
        KeyCode::KEY_KPEQUAL => 0x67,
        KeyCode::KEY_HELP => 0x75,
        KeyCode::KEY_PROPS => 0x76,
        KeyCode::KEY_AGAIN => 0x79,
        KeyCode::KEY_UNDO => 0x7a,
        KeyCode::KEY_CUT => 0x7b,
        KeyCode::KEY_COPY => 0x7c,
        KeyCode::KEY_PASTE => 0x7d,
        KeyCode::KEY_FIND => 0x7e,
        KeyCode::KEY_MUTE => 0x7f,
        KeyCode::KEY_VOLUMEUP => 0x80,
        KeyCode::KEY_VOLUMEDOWN => 0x81,
        KeyCode::KEY_LEFTCTRL => 0xe0,
        KeyCode::KEY_LEFTSHIFT => 0xe1,
        KeyCode::KEY_LEFTALT => 0xe2,
        KeyCode::KEY_LEFTMETA => 0xe3,
        KeyCode::KEY_RIGHTCTRL => 0xe4,
        KeyCode::KEY_RIGHTSHIFT => 0xe5,
        KeyCode::KEY_RIGHTALT => 0xe6,
        KeyCode::KEY_RIGHTMETA => 0xe7,
        _ => return None,
    };
    Some(usage)
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn linux_keys_map_to_usb_hid_usages() {
        assert_eq!(linux_key_hid_usage(KeyCode::KEY_A), Some(0x04));
        assert_eq!(linux_key_hid_usage(KeyCode::KEY_ENTER), Some(0x28));
        assert_eq!(linux_key_hid_usage(KeyCode::KEY_LEFTCTRL), Some(0xe0));
        assert_eq!(linux_key_hid_usage(KeyCode::KEY_RIGHTMETA), Some(0xe7));
        assert_eq!(linux_key_hid_usage(KeyCode::KEY_BRIGHTNESSUP), None);
    }

    #[test]
    fn mouse_buttons_do_not_leak_as_keyboard_keys() {
        assert_eq!(
            linux_control(KeyCode::BTN_LEFT),
            Some(InputControl::Mouse(MouseButton::Left))
        );
        assert_eq!(linux_key_hid_usage(KeyCode::BTN_LEFT), None);
    }

    #[test]
    fn virtual_cursor_dimensions_are_a_normalized_input_calibration() {
        let mut cursor = VirtualCursor::default();
        assert_eq!(cursor.sample(MonotonicMillis::new(0)).normalized().x, 0.0);
        assert!(cursor.apply(960, 540));
        let lower_right = cursor.sample(MonotonicMillis::new(1)).normalized();
        assert_eq!((lower_right.x, lower_right.y), (-1.0, -1.0));
        assert!(cursor.apply(-1_920, -1_080));
        let upper_left = cursor.sample(MonotonicMillis::new(2)).normalized();
        assert_eq!((upper_left.x, upper_left.y), (1.0, 1.0));
    }

    #[test]
    fn inaccessible_event_nodes_report_permission_denied() {
        let discovery = DeviceDiscovery {
            devices: Vec::new(),
            event_nodes: true,
            permission_denied: true,
            observed_paths: Some(HashSet::new()),
            unsupported_paths: HashSet::new(),
        };
        assert_eq!(
            discovery_error(&discovery),
            PlatformInputError::PermissionDenied
        );
    }

    #[test]
    fn suspend_detector_observes_boot_time_offset_growth() {
        assert!(!resumed_since(
            Duration::from_secs(4),
            Duration::from_millis(4_999),
            SUSPEND_DETECTION_THRESHOLD,
        ));
        assert!(resumed_since(
            Duration::from_secs(4),
            Duration::from_secs(5),
            SUSPEND_DETECTION_THRESHOLD,
        ));
    }

    #[test]
    fn evdev_degradation_keeps_the_worker_live_for_gamepads_and_recovery() {
        let mut diagnostics = PlatformInputDiagnostics::default();
        set_evdev_diagnostics(&mut diagnostics, Some(PlatformInputError::PermissionDenied));
        assert_eq!(
            diagnostics.service_status,
            PlatformInputServiceStatus::PermissionDenied
        );
        assert_eq!(
            diagnostics.service_error_code,
            Some("platform_input_permission_denied")
        );

        set_evdev_diagnostics(&mut diagnostics, None);
        assert_eq!(
            diagnostics.service_status,
            PlatformInputServiceStatus::Running
        );
        assert_eq!(diagnostics.service_error_code, None);
    }
}
