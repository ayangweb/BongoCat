use crate::{
    InputPermission, PlatformInputDiagnostics, PlatformInputError, PlatformInputServiceStatus,
    ShortcutDispatcher,
};
use block2::RcBlock;
use bongocat_runtime::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorViewport, GamepadAxis,
    GamepadAxisKey, GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample, GamepadButton,
    GamepadButtonKey, GamepadConnection, GamepadConnectionError, InputControl, InputEdge,
    InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource, MonotonicMillis,
    MouseButton, PhysicalKey, PlatformInputDiagnosticsProducer,
};
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, CGPoint, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGError, CGEvent, CGEventField, CGEventFlags, CGEventMask,
    CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGGetDisplaysWithPoint, CGMouseButton,
};
use objc2_game_controller::{GCController, GCControllerElement, GCExtendedGamepad};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WORKSPACE_WILL_SLEEP: u8 = 1 << 0;
const WORKSPACE_DID_WAKE: u8 = 1 << 1;
const WORKSPACE_SESSION_RESIGNED: u8 = 1 << 2;
const WORKSPACE_SESSION_ACTIVE: u8 = 1 << 3;

#[derive(Default)]
struct WorkspaceLifecycleSignals(AtomicU16);

impl WorkspaceLifecycleSignals {
    fn signal(&self, bit: u8) {
        self.0.fetch_or(u16::from(bit), Ordering::Release);
    }

    fn take(&self) -> u16 {
        self.0.swap(0, Ordering::Acquire)
    }
}

struct WorkspaceLifecycleObserver {
    center: Retained<objc2_foundation::NSNotificationCenter>,
    tokens: Vec<Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>>,
    accepting: Arc<AtomicBool>,
}

impl WorkspaceLifecycleObserver {
    fn register(
        signals: Arc<WorkspaceLifecycleSignals>,
        accepting: Arc<AtomicBool>,
        recovery_requested: Arc<AtomicBool>,
        counters: Arc<CallbackCounters>,
    ) -> Self {
        use objc2_app_kit::{
            NSWorkspace, NSWorkspaceDidWakeNotification,
            NSWorkspaceSessionDidBecomeActiveNotification,
            NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
        };
        use objc2_foundation::NSNotification;

        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: these are immutable notification-name constants exported by AppKit.
        let registrations = unsafe {
            [
                (NSWorkspaceWillSleepNotification, WORKSPACE_WILL_SLEEP),
                (NSWorkspaceDidWakeNotification, WORKSPACE_DID_WAKE),
                (
                    NSWorkspaceSessionDidResignActiveNotification,
                    WORKSPACE_SESSION_RESIGNED,
                ),
                (
                    NSWorkspaceSessionDidBecomeActiveNotification,
                    WORKSPACE_SESSION_ACTIVE,
                ),
            ]
        };
        let mut tokens = Vec::with_capacity(registrations.len());
        for (name, bit) in registrations {
            let callback_signals = Arc::clone(&signals);
            let callback_accepting = Arc::clone(&accepting);
            let callback_recovery = Arc::clone(&recovery_requested);
            let callback_counters = Arc::clone(&counters);
            let block: RcBlock<dyn Fn(NonNull<NSNotification>)> =
                RcBlock::new(move |_notification: NonNull<NSNotification>| {
                    callback_boundary(
                        &callback_accepting,
                        &callback_recovery,
                        &callback_counters,
                        || {
                            if callback_accepting.load(Ordering::Acquire) {
                                callback_signals.signal(bit);
                            }
                        },
                    );
                });
            // SAFETY: no object filter is used and the block captures only thread-safe state.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            tokens.push(token);
        }
        Self {
            center,
            tokens,
            accepting,
        }
    }

    fn close_sink(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    #[cfg(test)]
    fn post_for_test(&self, bit: u8) {
        use objc2_app_kit::{
            NSWorkspaceDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
            NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
        };
        // SAFETY: these are immutable notification-name constants exported by AppKit.
        let name = unsafe {
            match bit {
                WORKSPACE_WILL_SLEEP => NSWorkspaceWillSleepNotification,
                WORKSPACE_DID_WAKE => NSWorkspaceDidWakeNotification,
                WORKSPACE_SESSION_RESIGNED => NSWorkspaceSessionDidResignActiveNotification,
                WORKSPACE_SESSION_ACTIVE => NSWorkspaceSessionDidBecomeActiveNotification,
                _ => panic!("unknown workspace lifecycle bit"),
            }
        };
        // SAFETY: the test posts a public workspace notification without object or user info.
        unsafe { self.center.postNotificationName_object(name, None) };
    }
}

impl Drop for WorkspaceLifecycleObserver {
    fn drop(&mut self) {
        self.close_sink();
        use objc2::runtime::AnyObject;
        for token in self.tokens.drain(..) {
            let token_ref: &objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol> =
                &token;
            let observer: &AnyObject = token_ref.as_ref();
            // SAFETY: each token came from this center and is removed once.
            unsafe { self.center.removeObserver(observer) };
        }
    }
}

const CAPTURE_QUEUE_CAPACITY: usize = 256;
const RUN_LOOP_SLICE: Duration = Duration::from_millis(10);
const RECONCILIATION_INTERVAL: Duration = Duration::from_millis(250);
const REQUIRED_MISSING_CONFIRMATIONS: u8 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const SERVICE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_GAMEPADS: usize = 4;
const GAMEPAD_BUTTON_THRESHOLD: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
enum SystemControl {
    Key(u16),
    Mouse(u8),
}

#[derive(Clone, Copy, Debug)]
enum CapturedEvent {
    Edge {
        control: InputControl,
        system: SystemControl,
        edge: InputEdge,
    },
    GamepadEdge {
        connection: GamepadConnection,
        button: GamepadButton,
        edge: InputEdge,
    },
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MacCursorPoint {
    x: f64,
    y: f64,
}

#[derive(Default)]
struct LatestCursorState {
    pending: Option<MacCursorPoint>,
    closed: bool,
    captured: u64,
    coalesced: u64,
    consumed: u64,
    rejected_after_close: u64,
}

#[derive(Clone, Default)]
struct LatestCursor {
    state: Arc<Mutex<LatestCursorState>>,
}

impl LatestCursor {
    fn publish(&self, point: MacCursorPoint) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.rejected_after_close = state.rejected_after_close.saturating_add(1);
            return;
        }
        state.captured = state.captured.saturating_add(1);
        if state.pending.replace(point).is_some() {
            state.coalesced = state.coalesced.saturating_add(1);
        }
    }

    fn take(&self) -> Option<MacCursorPoint> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let point = state.pending.take();
        if point.is_some() {
            state.consumed = state.consumed.saturating_add(1);
        }
        point
    }

    fn close(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
    }

    fn merge_diagnostics(&self, diagnostics: &mut PlatformInputDiagnostics) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        diagnostics.cursor_captured = state.captured;
        diagnostics.cursor_coalesced = state.coalesced;
        diagnostics.cursor_consumed = state.consumed;
        diagnostics.cursor_rejected_after_stop = state.rejected_after_close;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MacGamepadSnapshot {
    buttons: u16,
    axes: [f32; 6],
    invalid_values: u64,
}

struct LatestGamepadAxes {
    values: [std::sync::atomic::AtomicU32; 6],
    version: AtomicU64,
    dirty: AtomicBool,
    closed: AtomicBool,
}

impl Default for LatestGamepadAxes {
    fn default() -> Self {
        Self {
            values: std::array::from_fn(|_| std::sync::atomic::AtomicU32::new(0.0_f32.to_bits())),
            version: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }
}

impl LatestGamepadAxes {
    fn publish(&self, values: [f32; 6]) {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return;
            }
            let version = self.version.load(Ordering::Acquire);
            if version & 1 != 0 {
                thread::yield_now();
                continue;
            }
            if self
                .version
                .compare_exchange(
                    version,
                    version.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                continue;
            }
            if self.closed.load(Ordering::Acquire) {
                self.version.fetch_add(1, Ordering::Release);
                return;
            }
            for (slot, value) in self.values.iter().zip(values) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
            self.version.fetch_add(1, Ordering::Release);
            self.dirty.store(true, Ordering::Release);
            return;
        }
    }

    fn take(&self) -> Option<[f32; 6]> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return None;
        }
        for _ in 0..8 {
            let before = self.version.load(Ordering::Acquire);
            if before & 1 != 0 {
                thread::yield_now();
                continue;
            }
            let values = self
                .values
                .each_ref()
                .map(|value| f32::from_bits(value.load(Ordering::Relaxed)));
            if self.version.load(Ordering::Acquire) == before {
                return Some(values);
            }
            thread::yield_now();
        }
        self.dirty.store(true, Ordering::Release);
        None
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        loop {
            if self.version.load(Ordering::Acquire) & 1 == 0 {
                self.dirty.store(false, Ordering::Release);
                return;
            }
            thread::yield_now();
        }
    }
}

struct AttachedGamepad {
    connection: GamepadConnection,
    profile: Retained<GCExtendedGamepad>,
    pressed: Arc<AtomicU16>,
    axes: Arc<LatestGamepadAxes>,
}

impl AttachedGamepad {
    fn clear_handler(&self) {
        // SAFETY: this owner retains the profile and removes its copied block
        // before releasing any Rust state captured by the callback.
        unsafe { self.profile.setValueChangedHandler(std::ptr::null_mut()) };
    }
}

struct MacGamepadOwner {
    producer: InputProducer,
    axis_producer: GamepadAxisProducer,
    capture_sender: SyncSender<CapturedEvent>,
    started: Instant,
    accepting: Arc<AtomicBool>,
    recovery_requested: Arc<AtomicBool>,
    counters: Arc<CallbackCounters>,
    attached: BTreeMap<usize, AttachedGamepad>,
    unsupported: BTreeSet<usize>,
    free_slots: BTreeSet<u8>,
    original_background_monitoring: bool,
    background_monitoring_enabled: bool,
    background_monitoring_restored: bool,
    shutdown: bool,
}

impl MacGamepadOwner {
    fn new(
        producer: InputProducer,
        axis_producer: GamepadAxisProducer,
        capture_sender: SyncSender<CapturedEvent>,
        started: Instant,
        accepting: Arc<AtomicBool>,
        recovery_requested: Arc<AtomicBool>,
        counters: Arc<CallbackCounters>,
    ) -> Self {
        // SAFETY: this owner serializes access to GameController's process-wide
        // background policy and restores the prior value during shutdown.
        let original_background_monitoring =
            unsafe { GCController::shouldMonitorBackgroundEvents() };
        unsafe { GCController::setShouldMonitorBackgroundEvents(true) };
        let background_monitoring_enabled =
            unsafe { GCController::shouldMonitorBackgroundEvents() };
        Self {
            producer,
            axis_producer,
            capture_sender,
            started,
            accepting,
            recovery_requested,
            counters,
            attached: BTreeMap::new(),
            unsupported: BTreeSet::new(),
            free_slots: (0..MAX_GAMEPADS as u8).collect(),
            original_background_monitoring,
            background_monitoring_enabled,
            background_monitoring_restored: false,
            shutdown: false,
        }
    }

    fn reconcile(&mut self) -> Result<(), PlatformInputError> {
        // SAFETY: GameController returns an owned immutable NSArray and each
        // retained controller remains valid through this reconciliation pass.
        let controllers = unsafe { GCController::controllers() }.to_vec();
        let mut present = BTreeSet::new();
        for controller in controllers {
            let identity = Retained::as_ptr(&controller) as usize;
            present.insert(identity);
            if self.attached.contains_key(&identity) || self.unsupported.contains(&identity) {
                continue;
            }
            // SAFETY: the retained controller is valid and the returned
            // extended profile, when present, is independently retained.
            let Some(profile) = (unsafe { controller.extendedGamepad() }) else {
                self.unsupported.insert(identity);
                self.counters
                    .gamepad_unsupported_profiles
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            };
            let Some(device_id) = self.free_slots.pop_first() else {
                continue;
            };
            let connection = match self.axis_producer.connect(device_id) {
                Ok(connection) => connection,
                Err(GamepadConnectionError::RuntimeStopped) => {
                    self.free_slots.insert(device_id);
                    return Err(PlatformInputError::RuntimeStopped);
                }
                Err(GamepadConnectionError::GenerationExhausted) => {
                    self.free_slots.insert(device_id);
                    self.counters
                        .gamepad_axis_publish_rejections
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            if let Err(error) = self.producer.publish(InputEvent::GamepadConnected {
                connection,
                at: monotonic(self.started),
            }) {
                self.axis_producer.disconnect(connection);
                self.free_slots.insert(device_id);
                self.handle_input_error(error)?;
                continue;
            }
            self.counters
                .gamepad_connections
                .fetch_add(1, Ordering::Relaxed);
            let pressed = Arc::new(AtomicU16::new(0));
            let axes = Arc::new(LatestGamepadAxes::default());
            let callback_sender = self.capture_sender.clone();
            let callback_pressed = Arc::clone(&pressed);
            let callback_axes = Arc::clone(&axes);
            let callback_accepting = Arc::clone(&self.accepting);
            let callback_recovery = Arc::clone(&self.recovery_requested);
            let callback_counters = Arc::clone(&self.counters);
            let block: RcBlock<dyn Fn(NonNull<GCExtendedGamepad>, NonNull<GCControllerElement>)> =
                RcBlock::new(
                    move |profile: NonNull<GCExtendedGamepad>,
                          _element: NonNull<GCControllerElement>| {
                        if !callback_accepting.load(Ordering::Acquire) {
                            callback_counters
                                .gamepad_rejected_after_stop
                                .fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                        callback_boundary(
                            &callback_accepting,
                            &callback_recovery,
                            &callback_counters,
                            || {
                                // SAFETY: GameController supplies a valid retained
                                // profile for the duration of this copied block call.
                                let snapshot =
                                    unsafe { snapshot_extended_gamepad(profile.as_ref()) };
                                capture_gamepad_snapshot(
                                    snapshot,
                                    connection,
                                    &callback_sender,
                                    &callback_pressed,
                                    &callback_axes,
                                    &callback_accepting,
                                    &callback_recovery,
                                    &callback_counters,
                                );
                            },
                        );
                    },
                );
            // SAFETY: the retained profile copies the block. The closure owns
            // only Send + Sync Rust producers, atomics and value identifiers.
            unsafe { profile.setValueChangedHandler(RcBlock::as_ptr(&block)) };
            // SAFETY: all profile elements are retained for each immediate
            // scalar read and the profile remains owned by this handler.
            let snapshot = unsafe { snapshot_extended_gamepad(&profile) };
            capture_gamepad_snapshot(
                snapshot,
                connection,
                &self.capture_sender,
                &pressed,
                &axes,
                &self.accepting,
                &self.recovery_requested,
                &self.counters,
            );
            self.attached.insert(
                identity,
                AttachedGamepad {
                    connection,
                    profile,
                    pressed,
                    axes,
                },
            );
        }

        let removed = self
            .attached
            .keys()
            .copied()
            .filter(|identity| !present.contains(identity))
            .collect::<Vec<_>>();
        for identity in removed {
            let Some(handler) = self.attached.remove(&identity) else {
                continue;
            };
            handler.clear_handler();
            handler.axes.close();
            self.axis_producer.disconnect(handler.connection);
            self.free_slots.insert(handler.connection.device_id);
            self.counters
                .gamepad_disconnections
                .fetch_add(1, Ordering::Relaxed);
            if let Err(error) = self.producer.publish(InputEvent::GamepadDisconnected {
                connection: handler.connection,
                at: monotonic(self.started),
            }) {
                self.handle_input_error(error)?;
            }
        }
        self.unsupported
            .retain(|identity| present.contains(identity));
        Ok(())
    }

    fn reseed(&self) {
        for handler in self.attached.values() {
            handler.pressed.store(0, Ordering::Release);
            // SAFETY: the owner retains each attached profile through this
            // synchronous state read.
            let snapshot = unsafe { snapshot_extended_gamepad(&handler.profile) };
            capture_gamepad_snapshot(
                snapshot,
                handler.connection,
                &self.capture_sender,
                &handler.pressed,
                &handler.axes,
                &self.accepting,
                &self.recovery_requested,
                &self.counters,
            );
        }
    }

    fn forward_axes(&self) -> Result<(), PlatformInputError> {
        let at = monotonic(self.started);
        for handler in self.attached.values() {
            let Some(values) = handler.axes.take() else {
                continue;
            };
            for axis in GamepadAxis::ALL {
                match self.axis_producer.publish(GamepadAxisSample {
                    key: GamepadAxisKey {
                        connection: handler.connection,
                        axis,
                    },
                    value: values[axis as usize],
                    at,
                }) {
                    Ok(()) => {
                        self.counters
                            .gamepad_axis_samples
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    Err(GamepadAxisPublishError::RuntimeStopped(_)) => {
                        return Err(PlatformInputError::RuntimeStopped);
                    }
                    Err(_) => {
                        self.counters
                            .gamepad_axis_publish_rejections
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_input_error(&self, error: InputPublishError) -> Result<(), PlatformInputError> {
        match error {
            InputPublishError::QueueFull(_) => {
                self.counters
                    .gamepad_runtime_queue_overflows
                    .fetch_add(1, Ordering::Relaxed);
                self.accepting.store(false, Ordering::Release);
                self.recovery_requested.store(true, Ordering::Release);
                Ok(())
            }
            InputPublishError::RuntimeStopped(_) => Err(PlatformInputError::RuntimeStopped),
        }
    }

    fn shutdown(&mut self) -> bool {
        if self.shutdown {
            return self.background_monitoring_restored;
        }
        self.accepting.store(false, Ordering::Release);
        for (_, handler) in std::mem::take(&mut self.attached) {
            handler.clear_handler();
            handler.axes.close();
            self.axis_producer.disconnect(handler.connection);
            self.counters
                .gamepad_disconnections
                .fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: all handlers are removed before restoring the process-wide
        // delivery policy, so no callback can retain this owner's Rust state.
        unsafe {
            GCController::setShouldMonitorBackgroundEvents(self.original_background_monitoring)
        };
        self.background_monitoring_restored =
            unsafe { GCController::shouldMonitorBackgroundEvents() }
                == self.original_background_monitoring;
        self.shutdown = true;
        self.background_monitoring_restored
    }
}

impl Drop for MacGamepadOwner {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_gamepad_snapshot(
    snapshot: MacGamepadSnapshot,
    connection: GamepadConnection,
    sender: &SyncSender<CapturedEvent>,
    pressed: &AtomicU16,
    axes: &LatestGamepadAxes,
    accepting: &AtomicBool,
    recovery_requested: &AtomicBool,
    counters: &CallbackCounters,
) {
    counters
        .gamepad_invalid_values
        .fetch_add(snapshot.invalid_values, Ordering::Relaxed);
    let previous = pressed.load(Ordering::Acquire);
    let changed = previous ^ snapshot.buttons;
    for button in GamepadButton::ALL {
        let bit = 1_u16 << button as u16;
        if changed & bit == 0 {
            continue;
        }
        let event = CapturedEvent::GamepadEdge {
            connection,
            button,
            edge: if snapshot.buttons & bit != 0 {
                InputEdge::Down
            } else {
                InputEdge::Up
            },
        };
        match sender.try_send(event) {
            Ok(()) => {
                counters
                    .gamepad_button_edges
                    .fetch_add(1, Ordering::Relaxed);
                counters.queued_edges.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_)) => {
                counters
                    .capture_queue_overflows
                    .fetch_add(1, Ordering::Relaxed);
                accepting.store(false, Ordering::Release);
                recovery_requested.store(true, Ordering::Release);
                return;
            }
            Err(TrySendError::Disconnected(_)) => {
                counters
                    .gamepad_rejected_after_stop
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
    pressed.store(snapshot.buttons, Ordering::Release);
    axes.publish(snapshot.axes);
}

unsafe fn snapshot_extended_gamepad(profile: &GCExtendedGamepad) -> MacGamepadSnapshot {
    use objc2_game_controller::GCControllerButtonInput;

    unsafe fn button(
        profile: &GCExtendedGamepad,
        getter: unsafe fn(&GCExtendedGamepad) -> Retained<GCControllerButtonInput>,
    ) -> f32 {
        // SAFETY: the caller supplies a getter for this retained profile.
        unsafe { getter(profile).value() }
    }

    fn normalized(value: f32, trigger: bool, invalid_values: &mut u64) -> f32 {
        if !value.is_finite() {
            *invalid_values = invalid_values.saturating_add(1);
            return 0.0;
        }
        if trigger {
            value.clamp(0.0, 1.0)
        } else {
            value.clamp(-1.0, 1.0)
        }
    }

    fn set_button(buttons: &mut u16, button: GamepadButton, value: f32, invalid_values: &mut u64) {
        if normalized(value, true, invalid_values) >= GAMEPAD_BUTTON_THRESHOLD {
            *buttons |= 1_u16 << button as u16;
        }
    }

    // SAFETY: all getters belong to a retained extended profile and returned
    // elements are retained for each immediate scalar read.
    unsafe {
        let left = profile.leftThumbstick();
        let right = profile.rightThumbstick();
        let dpad = profile.dpad();
        let left_trigger = profile.leftTrigger().value();
        let right_trigger = profile.rightTrigger().value();
        let mut snapshot = MacGamepadSnapshot::default();
        for (button_id, value) in [
            (
                GamepadButton::South,
                button(profile, GCExtendedGamepad::buttonA),
            ),
            (
                GamepadButton::East,
                button(profile, GCExtendedGamepad::buttonB),
            ),
            (
                GamepadButton::West,
                button(profile, GCExtendedGamepad::buttonX),
            ),
            (
                GamepadButton::North,
                button(profile, GCExtendedGamepad::buttonY),
            ),
            (
                GamepadButton::LeftShoulder,
                button(profile, GCExtendedGamepad::leftShoulder),
            ),
            (
                GamepadButton::RightShoulder,
                button(profile, GCExtendedGamepad::rightShoulder),
            ),
            (GamepadButton::LeftTrigger, left_trigger),
            (GamepadButton::RightTrigger, right_trigger),
            (GamepadButton::Start, profile.buttonMenu().value()),
            (GamepadButton::DpadUp, dpad.up().value()),
            (GamepadButton::DpadDown, dpad.down().value()),
            (GamepadButton::DpadLeft, dpad.left().value()),
            (GamepadButton::DpadRight, dpad.right().value()),
        ] {
            set_button(
                &mut snapshot.buttons,
                button_id,
                value,
                &mut snapshot.invalid_values,
            );
        }
        for (button_id, input) in [
            (GamepadButton::Select, profile.buttonOptions()),
            (GamepadButton::LeftStick, profile.leftThumbstickButton()),
            (GamepadButton::RightStick, profile.rightThumbstickButton()),
        ] {
            if let Some(input) = input {
                set_button(
                    &mut snapshot.buttons,
                    button_id,
                    input.value(),
                    &mut snapshot.invalid_values,
                );
            }
        }
        for (axis, value) in [
            (GamepadAxis::LeftStickX, left.xAxis().value()),
            (GamepadAxis::LeftStickY, left.yAxis().value()),
            (GamepadAxis::RightStickX, right.xAxis().value()),
            (GamepadAxis::RightStickY, right.yAxis().value()),
            (GamepadAxis::LeftTrigger, left_trigger),
            (GamepadAxis::RightTrigger, right_trigger),
        ] {
            snapshot.axes[axis as usize] =
                normalized(value, axis.is_trigger(), &mut snapshot.invalid_values);
        }
        snapshot
    }
}

#[derive(Default)]
struct CallbackCounters {
    captured_edges: AtomicU64,
    queued_edges: AtomicU64,
    unmapped_keys: AtomicU64,
    unsupported_buttons: AtomicU64,
    callback_panics: AtomicU64,
    capture_queue_overflows: AtomicU64,
    rejected_after_stop: AtomicU64,
    gamepad_connections: AtomicU64,
    gamepad_disconnections: AtomicU64,
    gamepad_button_edges: AtomicU64,
    gamepad_axis_samples: AtomicU64,
    gamepad_axis_publish_rejections: AtomicU64,
    gamepad_runtime_queue_overflows: AtomicU64,
    gamepad_unsupported_profiles: AtomicU64,
    gamepad_rejected_after_stop: AtomicU64,
    gamepad_invalid_values: AtomicU64,
}

struct TapCallbackContext {
    sender: SyncSender<CapturedEvent>,
    accepting: Arc<AtomicBool>,
    recovery_requested: Arc<AtomicBool>,
    tap_disabled: Arc<AtomicBool>,
    modifier_keys: Arc<Mutex<BTreeSet<u16>>>,
    cursor: LatestCursor,
    counters: Arc<CallbackCounters>,
}

impl TapCallbackContext {
    fn capture(&self, event_type: CGEventType, event: &CGEvent) {
        capture_callback_event(
            event_type,
            event,
            &self.sender,
            &self.accepting,
            &self.recovery_requested,
            &self.tap_disabled,
            &self.modifier_keys,
            &self.cursor,
            &self.counters,
        );
    }
}

unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `run_input_worker` passes a pointer to a stable Box allocation
    // that outlives every enabled tap. The worker disables the tap and removes
    // its run-loop source before dropping the Box, so callbacks cannot observe
    // freed state.
    let context = unsafe { &*user_info.cast::<TapCallbackContext>() };
    // SAFETY: CoreGraphics guarantees that the callback event is non-null and
    // valid for the duration of this callback.
    let event_ref = unsafe { event.as_ref() };
    callback_boundary(
        &context.accepting,
        &context.recovery_requested,
        &context.counters,
        || context.capture(event_type, event_ref),
    );
    event.as_ptr()
}

fn callback_boundary(
    accepting: &AtomicBool,
    recovery_requested: &AtomicBool,
    counters: &CallbackCounters,
    callback: impl FnOnce(),
) {
    autoreleasepool(|_| {
        if catch_unwind(AssertUnwindSafe(callback)).is_err() {
            counters.callback_panics.fetch_add(1, Ordering::Relaxed);
            accepting.store(false, Ordering::Release);
            recovery_requested.store(true, Ordering::Release);
        }
    });
}

fn create_event_tap(
    callback_context: *mut c_void,
) -> Result<(CFRetained<CFMachPort>, CFRetained<CFRunLoopSource>), PlatformInputError> {
    // SAFETY: the caller keeps `callback_context` alive until the returned tap
    // is disabled and detached from its run loop.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::TailAppendEventTap,
            CGEventTapOptions::ListenOnly,
            input_event_mask(),
            Some(event_tap_callback),
            callback_context,
        )
    }
    .ok_or(PlatformInputError::TapCreateFailed)?;
    let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
        .ok_or(PlatformInputError::RunLoopSourceFailed)?;
    Ok((tap, source))
}

impl CallbackCounters {
    fn snapshot(&self) -> PlatformInputDiagnostics {
        PlatformInputDiagnostics {
            captured_edges: self.captured_edges.load(Ordering::Relaxed),
            queued_edges: self.queued_edges.load(Ordering::Relaxed),
            unmapped_keys: self.unmapped_keys.load(Ordering::Relaxed),
            unsupported_buttons: self.unsupported_buttons.load(Ordering::Relaxed),
            callback_panics: self.callback_panics.load(Ordering::Relaxed),
            capture_queue_overflows: self.capture_queue_overflows.load(Ordering::Relaxed),
            rejected_after_stop: self.rejected_after_stop.load(Ordering::Relaxed),
            gamepad_connections: self.gamepad_connections.load(Ordering::Relaxed),
            gamepad_disconnections: self.gamepad_disconnections.load(Ordering::Relaxed),
            gamepad_button_edges: self.gamepad_button_edges.load(Ordering::Relaxed),
            gamepad_axis_samples: self.gamepad_axis_samples.load(Ordering::Relaxed),
            gamepad_axis_publish_rejections: self
                .gamepad_axis_publish_rejections
                .load(Ordering::Relaxed),
            runtime_queue_overflows: self.gamepad_runtime_queue_overflows.load(Ordering::Relaxed),
            gamepad_unsupported_profiles: self.gamepad_unsupported_profiles.load(Ordering::Relaxed),
            gamepad_rejected_after_stop: self.gamepad_rejected_after_stop.load(Ordering::Relaxed),
            gamepad_invalid_values: self.gamepad_invalid_values.load(Ordering::Relaxed),
            ..PlatformInputDiagnostics::default()
        }
    }
}

pub struct MacInputService {
    stop: Arc<AtomicBool>,
    completion: Receiver<Result<PlatformInputDiagnostics, PlatformInputError>>,
    worker: Option<JoinHandle<()>>,
}

impl MacInputService {
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
        Self::start_with_diagnostics_and_shortcuts(
            producer,
            cursor_producer,
            gamepad_axis_producer,
            diagnostics_producer,
            None,
        )
    }

    pub fn start_with_diagnostics_and_shortcuts(
        producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        diagnostics_producer: PlatformInputDiagnosticsProducer,
        shortcut_dispatcher: Option<ShortcutDispatcher>,
    ) -> Result<Self, PlatformInputError> {
        if input_monitoring_permission() != InputPermission::Granted {
            return Err(PlatformInputError::PermissionDenied);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bongocat-macos-input".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_input_worker(
                        producer,
                        cursor_producer,
                        gamepad_axis_producer,
                        diagnostics_producer,
                        shortcut_dispatcher,
                        worker_stop,
                        startup_sender,
                    )
                }))
                .unwrap_or(Err(PlatformInputError::WorkerPanicked));
                let _ = completion_sender.send(result);
            })
            .map_err(|_| PlatformInputError::WorkerPanicked)?;
        match startup_receiver.recv_timeout(STARTUP_TIMEOUT) {
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

impl Drop for MacInputService {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish(SERVICE_TIMEOUT);
        }
    }
}

pub fn input_monitoring_permission() -> InputPermission {
    if objc2_core_graphics::CGPreflightListenEventAccess() {
        InputPermission::Granted
    } else {
        InputPermission::Denied
    }
}

pub fn request_input_monitoring_permission() -> InputPermission {
    if objc2_core_graphics::CGRequestListenEventAccess() {
        InputPermission::Granted
    } else {
        InputPermission::Denied
    }
}

fn run_input_worker(
    producer: InputProducer,
    cursor_producer: CursorProducer,
    gamepad_axis_producer: GamepadAxisProducer,
    diagnostics_producer: PlatformInputDiagnosticsProducer,
    shortcut_dispatcher: Option<ShortcutDispatcher>,
    stop: Arc<AtomicBool>,
    startup: SyncSender<Result<(), PlatformInputError>>,
) -> Result<PlatformInputDiagnostics, PlatformInputError> {
    let started = Instant::now();
    let counters = Arc::new(CallbackCounters::default());
    let accepting = Arc::new(AtomicBool::new(true));
    let recovery_requested = Arc::new(AtomicBool::new(false));
    let workspace_signals = Arc::new(WorkspaceLifecycleSignals::default());
    let workspace_observer = WorkspaceLifecycleObserver::register(
        Arc::clone(&workspace_signals),
        Arc::clone(&accepting),
        Arc::clone(&recovery_requested),
        Arc::clone(&counters),
    );
    let tap_disabled = Arc::new(AtomicBool::new(false));
    let modifier_keys = Arc::new(Mutex::new(BTreeSet::<u16>::new()));
    let latest_cursor = LatestCursor::default();
    let (capture_sender, capture_receiver) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
    let mut gamepad_owner = MacGamepadOwner::new(
        producer.clone(),
        gamepad_axis_producer,
        capture_sender.clone(),
        started,
        Arc::clone(&accepting),
        Arc::clone(&recovery_requested),
        Arc::clone(&counters),
    );
    let mut shortcut_dispatcher = shortcut_dispatcher;

    let mut callback_context = Box::new(TapCallbackContext {
        sender: capture_sender,
        accepting: Arc::clone(&accepting),
        recovery_requested: Arc::clone(&recovery_requested),
        tap_disabled: Arc::clone(&tap_disabled),
        modifier_keys: Arc::clone(&modifier_keys),
        cursor: latest_cursor.clone(),
        counters: Arc::clone(&counters),
    });
    let callback_context_ptr = (&mut *callback_context as *mut TapCallbackContext).cast();
    let (mut tap, mut source) = match create_event_tap(callback_context_ptr) {
        Ok(tap) => tap,
        Err(error) => {
            let _ = startup.send(Err(error));
            return Err(error);
        }
    };
    let run_loop = CFRunLoop::current().ok_or(PlatformInputError::RunLoopSourceFailed)?;
    // SAFETY: CoreFoundation owns this immutable process-lifetime constant.
    let mode = unsafe { kCFRunLoopDefaultMode }.ok_or(PlatformInputError::RunLoopSourceFailed)?;
    run_loop.add_source(Some(&source), Some(mode));
    CGEvent::tap_enable(&tap, true);
    if !CGEvent::tap_is_enabled(&tap) {
        run_loop.remove_source(Some(&source), Some(mode));
        let _ = startup.send(Err(PlatformInputError::TapCreateFailed));
        return Err(PlatformInputError::TapCreateFailed);
    }
    let mut diagnostics = PlatformInputDiagnostics {
        service_status: PlatformInputServiceStatus::Running,
        service_start_attempts: 1,
        gamepad_background_monitoring_enabled: gamepad_owner.background_monitoring_enabled,
        ..PlatformInputDiagnostics::default()
    };
    publish_live_diagnostics(
        &diagnostics_producer,
        diagnostics,
        &counters,
        &latest_cursor,
    );
    let _ = startup.send(Ok(()));
    if let Some(event) = CGEvent::new(None) {
        let location = CGEvent::location(Some(&event));
        latest_cursor.publish(MacCursorPoint {
            x: location.x,
            y: location.y,
        });
    }
    let mut candidates = BTreeMap::<InputControl, SystemControl>::new();
    let mut missing_confirmations = BTreeMap::<InputControl, u8>::new();
    let mut next_reconciliation = Instant::now() + RECONCILIATION_INTERVAL;
    let mut recovery_pending = false;
    let mut tap_restart_pending = false;

    let mut service_result = 'service: loop {
        if stop.load(Ordering::Acquire) {
            break Ok(());
        }
        CFRunLoop::run_in_mode(Some(mode), RUN_LOOP_SLICE.as_secs_f64(), true);
        publish_live_diagnostics(
            &diagnostics_producer,
            diagnostics,
            &counters,
            &latest_cursor,
        );

        if recovery_requested.swap(false, Ordering::AcqRel) {
            recovery_pending = true;
        }
        if workspace_signals.take() != 0 {
            accepting.store(false, Ordering::Release);
            recovery_pending = true;
            tap_restart_pending = true;
        }
        if recovery_pending {
            diagnostics.capture_queue_discarded = diagnostics
                .capture_queue_discarded
                .saturating_add(drain_capture_queue(&capture_receiver));
            let reason = if tap_restart_pending {
                InputResetReason::ServiceRestart
            } else {
                InputResetReason::QueueOverflow
            };
            match producer.recover(reason, monotonic(started)) {
                Ok(_) => {
                    candidates.clear();
                    missing_confirmations.clear();
                    if let Some(dispatcher) = shortcut_dispatcher.as_mut() {
                        dispatcher.reset();
                    }
                    modifier_keys
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clear();
                    diagnostics.recovery_resets = diagnostics.recovery_resets.saturating_add(1);
                    recovery_pending = false;
                    if tap_restart_pending {
                        if input_monitoring_permission() != InputPermission::Granted {
                            break 'service Err(PlatformInputError::PermissionDenied);
                        }
                        CGEvent::tap_enable(&tap, false);
                        run_loop.remove_source(Some(&source), Some(mode));
                        let (replacement_tap, replacement_source) =
                            match create_event_tap(callback_context_ptr) {
                                Ok(replacement) => replacement,
                                Err(error) => break 'service Err(error),
                            };
                        run_loop.add_source(Some(&replacement_source), Some(mode));
                        CGEvent::tap_enable(&replacement_tap, true);
                        if !CGEvent::tap_is_enabled(&replacement_tap) {
                            run_loop.remove_source(Some(&replacement_source), Some(mode));
                            break 'service Err(PlatformInputError::TapCreateFailed);
                        }
                        tap = replacement_tap;
                        source = replacement_source;
                        diagnostics.tap_restarts = diagnostics.tap_restarts.saturating_add(1);
                        tap_restart_pending = false;
                    }
                    accepting.store(true, Ordering::Release);
                    gamepad_owner.reseed();
                }
                Err(InputPublishError::QueueFull(_)) => {
                    diagnostics.runtime_queue_overflows =
                        diagnostics.runtime_queue_overflows.saturating_add(1);
                    continue;
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    break 'service Err(PlatformInputError::RuntimeStopped);
                }
            }
        }

        if tap_disabled.swap(false, Ordering::AcqRel) {
            accepting.store(false, Ordering::Release);
            candidates.clear();
            missing_confirmations.clear();
            if let Some(dispatcher) = shortcut_dispatcher.as_mut() {
                dispatcher.reset();
            }
            modifier_keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            recovery_pending = true;
            tap_restart_pending = true;
            continue;
        }

        if let Err(error) = gamepad_owner.reconcile() {
            break 'service Err(error);
        }

        loop {
            let event = match capture_receiver.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            };
            if let Err(error) = publish_captured(
                &producer,
                event,
                monotonic(started),
                &mut candidates,
                &mut missing_confirmations,
                &mut diagnostics,
                &mut shortcut_dispatcher,
            ) {
                match error {
                    InputPublishError::QueueFull(_) => {
                        diagnostics.runtime_queue_overflows =
                            diagnostics.runtime_queue_overflows.saturating_add(1);
                        accepting.store(false, Ordering::Release);
                        recovery_pending = true;
                        break;
                    }
                    InputPublishError::RuntimeStopped(_) => {
                        break 'service Err(PlatformInputError::RuntimeStopped);
                    }
                }
            }
        }

        if let Err(error) = gamepad_owner.forward_axes() {
            break 'service Err(error);
        }

        if let Err(error) =
            forward_latest_cursor(&latest_cursor, &cursor_producer, started, &mut diagnostics)
        {
            break 'service Err(error);
        }

        if Instant::now() >= next_reconciliation && !candidates.is_empty() {
            let pressed = candidates
                .iter()
                .filter_map(|(control, system)| system_pressed(*system).then_some(*control))
                .collect::<BTreeSet<_>>();
            match producer.publish(InputEvent::Reconcile {
                pressed: pressed.clone(),
                at: monotonic(started),
            }) {
                Ok(_) => {
                    diagnostics.reconciliation_runs =
                        diagnostics.reconciliation_runs.saturating_add(1);
                    let controls = candidates.keys().copied().collect::<Vec<_>>();
                    for control in controls {
                        if pressed.contains(&control) {
                            missing_confirmations.remove(&control);
                        } else {
                            let confirmations = missing_confirmations.entry(control).or_insert(0);
                            *confirmations = confirmations.saturating_add(1);
                            if *confirmations >= REQUIRED_MISSING_CONFIRMATIONS {
                                candidates.remove(&control);
                                missing_confirmations.remove(&control);
                            }
                        }
                    }
                }
                Err(InputPublishError::QueueFull(_)) => {
                    diagnostics.runtime_queue_overflows =
                        diagnostics.runtime_queue_overflows.saturating_add(1);
                    accepting.store(false, Ordering::Release);
                    recovery_pending = true;
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    break 'service Err(PlatformInputError::RuntimeStopped);
                }
            }
            next_reconciliation = Instant::now() + RECONCILIATION_INTERVAL;
        }
    };

    accepting.store(false, Ordering::Release);
    workspace_observer.close_sink();
    CGEvent::tap_enable(&tap, false);
    run_loop.remove_source(Some(&source), Some(mode));
    diagnostics.gamepad_background_monitoring_enabled = gamepad_owner.background_monitoring_enabled;
    diagnostics.gamepad_background_monitoring_restored = gamepad_owner.shutdown();
    latest_cursor.close();
    if service_result.is_ok()
        && let Err(error) =
            forward_latest_cursor(&latest_cursor, &cursor_producer, started, &mut diagnostics)
    {
        service_result = Err(error);
    }
    diagnostics.capture_queue_discarded = diagnostics
        .capture_queue_discarded
        .saturating_add(drain_capture_queue(&capture_receiver));
    diagnostics.clean_shutdown = diagnostics.gamepad_background_monitoring_restored
        && publish_final_reset(&producer, started, &mut diagnostics);
    diagnostics.service_status = if service_result.is_ok() && diagnostics.clean_shutdown {
        PlatformInputServiceStatus::Stopped
    } else {
        PlatformInputServiceStatus::Failed
    };
    merge_callback_diagnostics(&mut diagnostics, &counters);
    latest_cursor.merge_diagnostics(&mut diagnostics);
    let _ = diagnostics_producer.publish(diagnostics);
    service_result?;
    Ok(diagnostics)
}

fn forward_latest_cursor(
    latest: &LatestCursor,
    producer: &CursorProducer,
    started: Instant,
    diagnostics: &mut PlatformInputDiagnostics,
) -> Result<(), PlatformInputError> {
    let Some(point) = latest.take() else {
        return Ok(());
    };
    let Some(viewport) = display_viewport(point) else {
        diagnostics.cursor_display_lookup_failures =
            diagnostics.cursor_display_lookup_failures.saturating_add(1);
        return Ok(());
    };
    let sample = match CursorSample::new(
        CursorPosition {
            x: point.x,
            y: point.y,
        },
        viewport,
        monotonic(started),
    ) {
        Ok(sample) => sample,
        Err(_) => {
            diagnostics.cursor_publish_rejections =
                diagnostics.cursor_publish_rejections.saturating_add(1);
            return Ok(());
        }
    };
    match producer.publish(sample) {
        Ok(()) => Ok(()),
        Err(CursorPublishError::NonMonotonic(_)) => {
            diagnostics.cursor_publish_rejections =
                diagnostics.cursor_publish_rejections.saturating_add(1);
            Ok(())
        }
        Err(CursorPublishError::RuntimeStopped(_)) => Err(PlatformInputError::RuntimeStopped),
    }
}

fn display_viewport(point: MacCursorPoint) -> Option<CursorViewport> {
    let mut display: CGDirectDisplayID = 0;
    let mut display_count = 0_u32;
    // SAFETY: both output pointers refer to initialized stack values and
    // max_displays limits CoreGraphics to the single display slot supplied.
    let result = unsafe {
        CGGetDisplaysWithPoint(
            CGPoint {
                x: point.x,
                y: point.y,
            },
            1,
            &mut display,
            &mut display_count,
        )
    };
    if result != CGError::Success || display_count == 0 {
        return None;
    }
    let bounds = CGDisplayBounds(display);
    Some(CursorViewport {
        origin: CursorPosition {
            x: bounds.origin.x,
            y: bounds.origin.y,
        },
        width: bounds.size.width,
        height: bounds.size.height,
    })
}

fn input_event_mask() -> CGEventMask {
    [
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
    ]
    .into_iter()
    .fold(0, |mask, event_type| mask | (1_u64 << event_type.0))
}

#[allow(clippy::too_many_arguments)]
fn capture_callback_event(
    event_type: CGEventType,
    event: &CGEvent,
    sender: &SyncSender<CapturedEvent>,
    accepting: &AtomicBool,
    recovery_requested: &AtomicBool,
    tap_disabled: &AtomicBool,
    modifier_keys: &Mutex<BTreeSet<u16>>,
    cursor: &LatestCursor,
    counters: &CallbackCounters,
) {
    if matches!(
        event_type,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        modifier_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        accepting.store(false, Ordering::Release);
        tap_disabled.store(true, Ordering::Release);
        return;
    }
    if !accepting.load(Ordering::Acquire) {
        counters.rejected_after_stop.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let captured = match event_type {
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let key_code = event_key_code(event);
            let Some(key) = map_key_code(key_code) else {
                counters.unmapped_keys.fetch_add(1, Ordering::Relaxed);
                return;
            };
            Some(CapturedEvent::Edge {
                control: InputControl::Key(key),
                system: SystemControl::Key(key_code),
                edge: if event_type == CGEventType::KeyDown {
                    InputEdge::Down
                } else {
                    InputEdge::Up
                },
            })
        }
        CGEventType::FlagsChanged => {
            let key_code = event_key_code(event);
            let Some(key) = map_key_code(key_code) else {
                counters.unmapped_keys.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let mut pressed = modifier_keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was_pressed = pressed.contains(&key_code);
            let Some(is_pressed) = modifier_pressed(event, key_code, was_pressed) else {
                pressed.clear();
                enqueue_event(
                    CapturedEvent::Reset,
                    sender,
                    accepting,
                    recovery_requested,
                    counters,
                );
                return;
            };
            if is_pressed {
                pressed.insert(key_code);
            } else {
                pressed.remove(&key_code);
            }
            Some(CapturedEvent::Edge {
                control: InputControl::Key(key),
                system: SystemControl::Key(key_code),
                edge: if is_pressed {
                    InputEdge::Down
                } else {
                    InputEdge::Up
                },
            })
        }
        CGEventType::LeftMouseDown
        | CGEventType::LeftMouseUp
        | CGEventType::RightMouseDown
        | CGEventType::RightMouseUp
        | CGEventType::OtherMouseDown
        | CGEventType::OtherMouseUp => {
            let button =
                CGEvent::integer_value_field(Some(event), CGEventField::MouseEventButtonNumber);
            let Ok(button) = u8::try_from(button) else {
                counters.unsupported_buttons.fetch_add(1, Ordering::Relaxed);
                return;
            };
            if button > 31 {
                counters.unsupported_buttons.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Some(CapturedEvent::Edge {
                control: InputControl::Mouse(map_mouse_button(button)),
                system: SystemControl::Mouse(button),
                edge: if matches!(
                    event_type,
                    CGEventType::LeftMouseDown
                        | CGEventType::RightMouseDown
                        | CGEventType::OtherMouseDown
                ) {
                    InputEdge::Down
                } else {
                    InputEdge::Up
                },
            })
        }
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            let location = CGEvent::location(Some(event));
            cursor.publish(MacCursorPoint {
                x: location.x,
                y: location.y,
            });
            None
        }
        _ => None,
    };
    if let Some(captured) = captured {
        counters.captured_edges.fetch_add(1, Ordering::Relaxed);
        enqueue_event(captured, sender, accepting, recovery_requested, counters);
    }
}

fn enqueue_event(
    event: CapturedEvent,
    sender: &SyncSender<CapturedEvent>,
    accepting: &AtomicBool,
    recovery_requested: &AtomicBool,
    counters: &CallbackCounters,
) {
    match sender.try_send(event) {
        Ok(()) => {
            counters.queued_edges.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Full(_)) => {
            counters
                .capture_queue_overflows
                .fetch_add(1, Ordering::Relaxed);
            accepting.store(false, Ordering::Release);
            recovery_requested.store(true, Ordering::Release);
        }
        Err(TrySendError::Disconnected(_)) => {
            counters.rejected_after_stop.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn publish_captured(
    producer: &InputProducer,
    captured: CapturedEvent,
    at: MonotonicMillis,
    candidates: &mut BTreeMap<InputControl, SystemControl>,
    missing_confirmations: &mut BTreeMap<InputControl, u8>,
    diagnostics: &mut PlatformInputDiagnostics,
    shortcut_dispatcher: &mut Option<ShortcutDispatcher>,
) -> Result<(), InputPublishError> {
    match captured {
        CapturedEvent::Edge {
            control,
            system,
            edge,
        } => {
            producer.publish(InputEvent::Edge {
                control,
                edge,
                source: InputSource::Capture,
                at,
            })?;
            if let InputControl::Key(key) = control
                && let Some(dispatcher) = shortcut_dispatcher.as_mut()
            {
                match dispatcher.apply(key, edge) {
                    Ok(_) => {}
                    Err(crate::ShortcutDispatchError::Runtime(
                        bongocat_runtime::SendError::QueueFull(_),
                    ))
                    | Err(crate::ShortcutDispatchError::ApplicationQueueFull) => {
                        diagnostics.runtime_queue_overflows =
                            diagnostics.runtime_queue_overflows.saturating_add(1);
                    }
                    Err(crate::ShortcutDispatchError::Runtime(
                        bongocat_runtime::SendError::RuntimeStopped(_),
                    )) => {
                        return Err(InputPublishError::RuntimeStopped(InputEvent::Reset {
                            reason: InputResetReason::ServiceRestart,
                            at,
                        }));
                    }
                }
            }
            match edge {
                InputEdge::Down => {
                    candidates.insert(control, system);
                    missing_confirmations.remove(&control);
                }
                InputEdge::Up => {
                    candidates.remove(&control);
                    missing_confirmations.remove(&control);
                }
            }
            diagnostics.consumed_edges = diagnostics.consumed_edges.saturating_add(1);
        }
        CapturedEvent::GamepadEdge {
            connection,
            button,
            edge,
        } => {
            producer.publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge,
                source: InputSource::Capture,
                at,
            })?;
            diagnostics.consumed_edges = diagnostics.consumed_edges.saturating_add(1);
        }
        CapturedEvent::Reset => {
            producer.recover(InputResetReason::ServiceRestart, at)?;
            candidates.clear();
            missing_confirmations.clear();
            if let Some(dispatcher) = shortcut_dispatcher.as_mut() {
                dispatcher.reset();
            }
            diagnostics.recovery_resets = diagnostics.recovery_resets.saturating_add(1);
        }
    }
    Ok(())
}

fn publish_final_reset(
    producer: &InputProducer,
    started: Instant,
    diagnostics: &mut PlatformInputDiagnostics,
) -> bool {
    for _ in 0..20 {
        match producer.recover(InputResetReason::ServiceRestart, monotonic(started)) {
            Ok(_) => {
                diagnostics.recovery_resets = diagnostics.recovery_resets.saturating_add(1);
                return true;
            }
            Err(InputPublishError::QueueFull(_)) => {
                diagnostics.runtime_queue_overflows =
                    diagnostics.runtime_queue_overflows.saturating_add(1);
                thread::sleep(Duration::from_millis(5));
            }
            Err(InputPublishError::RuntimeStopped(_)) => return false,
        }
    }
    false
}

fn drain_capture_queue(receiver: &Receiver<CapturedEvent>) -> u64 {
    let mut discarded = 0_u64;
    loop {
        match receiver.try_recv() {
            Ok(_) => discarded = discarded.saturating_add(1),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return discarded,
        }
    }
}

fn merge_callback_diagnostics(
    diagnostics: &mut PlatformInputDiagnostics,
    counters: &CallbackCounters,
) {
    let callback = counters.snapshot();
    diagnostics.captured_edges = callback.captured_edges;
    diagnostics.queued_edges = callback.queued_edges;
    diagnostics.unmapped_keys = callback.unmapped_keys;
    diagnostics.unsupported_buttons = callback.unsupported_buttons;
    diagnostics.callback_panics = callback.callback_panics;
    diagnostics.capture_queue_overflows = callback.capture_queue_overflows;
    diagnostics.rejected_after_stop = callback.rejected_after_stop;
    diagnostics.runtime_queue_overflows = diagnostics
        .runtime_queue_overflows
        .saturating_add(callback.runtime_queue_overflows);
    diagnostics.gamepad_connections = callback.gamepad_connections;
    diagnostics.gamepad_disconnections = callback.gamepad_disconnections;
    diagnostics.gamepad_button_edges = callback.gamepad_button_edges;
    diagnostics.gamepad_axis_samples = callback.gamepad_axis_samples;
    diagnostics.gamepad_axis_publish_rejections = callback.gamepad_axis_publish_rejections;
    diagnostics.gamepad_unsupported_profiles = callback.gamepad_unsupported_profiles;
    diagnostics.gamepad_rejected_after_stop = callback.gamepad_rejected_after_stop;
    diagnostics.gamepad_invalid_values = callback.gamepad_invalid_values;
}

fn publish_live_diagnostics(
    producer: &PlatformInputDiagnosticsProducer,
    diagnostics: PlatformInputDiagnostics,
    counters: &CallbackCounters,
    cursor: &LatestCursor,
) {
    let mut snapshot = diagnostics;
    merge_callback_diagnostics(&mut snapshot, counters);
    cursor.merge_diagnostics(&mut snapshot);
    let _ = producer.publish(snapshot);
}

fn monotonic(started: Instant) -> MonotonicMillis {
    MonotonicMillis::new(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
}

fn event_key_code(event: &CGEvent) -> u16 {
    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode)
        .clamp(0, i64::from(u16::MAX)) as u16
}

fn modifier_pressed(event: &CGEvent, key_code: u16, was_pressed: bool) -> Option<bool> {
    let mask = match key_code {
        54 | 55 => CGEventFlags::MaskCommand,
        56 | 60 => CGEventFlags::MaskShift,
        57 => CGEventFlags::MaskAlphaShift,
        58 | 61 => CGEventFlags::MaskAlternate,
        59 | 62 => CGEventFlags::MaskControl,
        63 => CGEventFlags::MaskSecondaryFn,
        _ => return None,
    };
    Some(CGEvent::flags(Some(event)).contains(mask) && !was_pressed)
}

fn map_mouse_button(button: u8) -> MouseButton {
    match button {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        other => MouseButton::Other(other),
    }
}

fn map_key_code(key_code: u16) -> Option<PhysicalKey> {
    let usage = match key_code {
        0 => 0x04,
        1 => 0x16,
        2 => 0x07,
        3 => 0x09,
        4 => 0x0b,
        5 => 0x0a,
        6 => 0x1d,
        7 => 0x1b,
        8 => 0x06,
        9 => 0x19,
        11 => 0x05,
        12 => 0x14,
        13 => 0x1a,
        14 => 0x08,
        15 => 0x15,
        16 => 0x1c,
        17 => 0x17,
        18 => 0x1e,
        19 => 0x1f,
        20 => 0x20,
        21 => 0x21,
        22 => 0x23,
        23 => 0x22,
        24 => 0x2e,
        25 => 0x26,
        26 => 0x24,
        27 => 0x2d,
        28 => 0x25,
        29 => 0x27,
        30 => 0x30,
        31 => 0x12,
        32 => 0x18,
        33 => 0x2f,
        34 => 0x0c,
        35 => 0x13,
        36 => 0x28,
        37 => 0x0f,
        38 => 0x0d,
        39 => 0x34,
        40 => 0x0e,
        41 => 0x33,
        42 => 0x31,
        43 => 0x36,
        44 => 0x38,
        45 => 0x11,
        46 => 0x10,
        47 => 0x37,
        48 => 0x2b,
        49 => 0x2c,
        50 => 0x35,
        51 => 0x2a,
        53 => 0x29,
        54 => 0xe7,
        55 => 0xe3,
        56 => 0xe1,
        57 => 0x39,
        58 => 0xe2,
        59 => 0xe0,
        60 => 0xe5,
        61 => 0xe6,
        62 => 0xe4,
        64 => 0x6c,
        65 => 0x63,
        67 => 0x55,
        69 => 0x57,
        71 => 0x53,
        75 => 0x54,
        76 => 0x58,
        78 => 0x56,
        79 => 0x6d,
        80 => 0x6e,
        81 => 0x67,
        82 => 0x62,
        83 => 0x59,
        84 => 0x5a,
        85 => 0x5b,
        86 => 0x5c,
        87 => 0x5d,
        88 => 0x5e,
        89 => 0x5f,
        90 => 0x6f,
        91 => 0x60,
        92 => 0x61,
        96 => 0x3e,
        97 => 0x3f,
        98 => 0x40,
        99 => 0x3c,
        100 => 0x41,
        101 => 0x42,
        103 => 0x44,
        105 => 0x68,
        106 => 0x6b,
        107 => 0x69,
        109 => 0x43,
        111 => 0x45,
        113 => 0x6a,
        115 => 0x4a,
        116 => 0x4b,
        117 => 0x4c,
        119 => 0x4d,
        121 => 0x4e,
        122 => 0x3a,
        120 => 0x3b,
        118 => 0x3d,
        123 => 0x50,
        124 => 0x4f,
        125 => 0x51,
        126 => 0x52,
        _ => return None,
    };
    Some(PhysicalKey::from_hid_usage(usage))
}

fn system_pressed(system: SystemControl) -> bool {
    match system {
        SystemControl::Key(key_code) => {
            CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, key_code)
        }
        SystemControl::Mouse(button) => CGEventSource::button_state(
            CGEventSourceStateID::CombinedSessionState,
            CGMouseButton(u32::from(button)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_runtime::RuntimeOwner;

    #[test]
    fn workspace_lifecycle_signals_merge_and_clear_atomically() {
        let signals = WorkspaceLifecycleSignals::default();
        signals.signal(WORKSPACE_WILL_SLEEP);
        signals.signal(WORKSPACE_SESSION_RESIGNED);
        assert_eq!(
            signals.take(),
            u16::from(WORKSPACE_WILL_SLEEP | WORKSPACE_SESSION_RESIGNED)
        );
        assert_eq!(signals.take(), 0);
    }

    #[test]
    fn workspace_observer_receives_all_notifications_and_closes_its_sink() {
        let signals = Arc::new(WorkspaceLifecycleSignals::default());
        let accepting = Arc::new(AtomicBool::new(true));
        let recovery = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(CallbackCounters::default());
        let observer = WorkspaceLifecycleObserver::register(
            Arc::clone(&signals),
            accepting,
            recovery,
            Arc::clone(&counters),
        );

        for bit in [
            WORKSPACE_WILL_SLEEP,
            WORKSPACE_DID_WAKE,
            WORKSPACE_SESSION_RESIGNED,
            WORKSPACE_SESSION_ACTIVE,
        ] {
            observer.post_for_test(bit);
        }
        assert_eq!(signals.take(), 0b1111);
        assert_eq!(counters.callback_panics.load(Ordering::Relaxed), 0);

        observer.close_sink();
        observer.post_for_test(WORKSPACE_DID_WAKE);
        assert_eq!(signals.take(), 0);
    }

    #[test]
    fn callback_boundary_contains_panics_and_requests_recovery() {
        let accepting = AtomicBool::new(true);
        let recovery = AtomicBool::new(false);
        let counters = CallbackCounters::default();

        callback_boundary(&accepting, &recovery, &counters, || {
            panic!("controlled callback panic");
        });

        assert!(!accepting.load(Ordering::Acquire));
        assert!(recovery.load(Ordering::Acquire));
        assert_eq!(counters.callback_panics.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn disabled_tap_signals_stop_capture_and_clear_modifier_decoder() {
        for event_type in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
        ] {
            let (sender, receiver) = mpsc::sync_channel(1);
            let accepting = Arc::new(AtomicBool::new(true));
            let recovery = Arc::new(AtomicBool::new(false));
            let tap_disabled = Arc::new(AtomicBool::new(false));
            let modifier_keys = Arc::new(Mutex::new(BTreeSet::from([56])));
            let counters = Arc::new(CallbackCounters::default());
            let context = TapCallbackContext {
                sender,
                accepting: Arc::clone(&accepting),
                recovery_requested: recovery,
                tap_disabled: Arc::clone(&tap_disabled),
                modifier_keys: Arc::clone(&modifier_keys),
                cursor: LatestCursor::default(),
                counters,
            };
            let event = CGEvent::new(None).expect("event");

            context.capture(event_type, &event);

            assert!(!accepting.load(Ordering::Acquire));
            assert!(tap_disabled.load(Ordering::Acquire));
            assert!(modifier_keys.lock().expect("modifier keys").is_empty());
            assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        }
    }

    #[test]
    fn mac_key_codes_map_to_usb_hid_usages() {
        assert_eq!(map_key_code(0), Some(PhysicalKey::KEY_A));
        assert_eq!(map_key_code(55).map(PhysicalKey::hid_usage), Some(0xe3));
        assert_eq!(map_key_code(60).map(PhysicalKey::hid_usage), Some(0xe5));
        assert_eq!(map_key_code(123).map(PhysicalKey::hid_usage), Some(0x50));
        assert_eq!(map_key_code(124).map(PhysicalKey::hid_usage), Some(0x4f));
        assert_eq!(map_key_code(u16::MAX), None);

        let shortcuts = bongocat_config::ShortcutConfig {
            commands: vec![bongocat_config::ShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Meta+A".to_owned(),
            }],
            ..bongocat_config::ShortcutConfig::default()
        }
        .compile()
        .expect("compiled shortcut");
        let modifiers =
            bongocat_config::ShortcutModifiers::from_bits(bongocat_config::ShortcutModifiers::META)
                .expect("meta modifier");
        let mapped = map_key_code(0).expect("mapped A");
        assert!(
            shortcuts
                .resolve_hid_usage(modifiers, mapped.hid_usage())
                .is_some()
        );
    }

    #[test]
    fn all_reconcilable_mouse_buttons_keep_identity() {
        assert_eq!(map_mouse_button(0), MouseButton::Left);
        assert_eq!(map_mouse_button(4), MouseButton::Forward);
        assert_eq!(map_mouse_button(31), MouseButton::Other(31));
    }

    #[test]
    fn cursor_callback_slot_coalesces_without_touching_the_edge_queue() {
        let cursor = LatestCursor::default();
        for index in 0_u32..10_000 {
            cursor.publish(MacCursorPoint {
                x: f64::from(index),
                y: 1.0,
            });
        }
        assert_eq!(cursor.take(), Some(MacCursorPoint { x: 9_999.0, y: 1.0 }));
        cursor.close();
        cursor.publish(MacCursorPoint { x: 0.0, y: 0.0 });
        let mut diagnostics = PlatformInputDiagnostics::default();
        cursor.merge_diagnostics(&mut diagnostics);
        assert_eq!(diagnostics.cursor_captured, 10_000);
        assert_eq!(diagnostics.cursor_coalesced, 9_999);
        assert_eq!(diagnostics.cursor_consumed, 1);
        assert_eq!(diagnostics.cursor_rejected_after_stop, 1);
        assert_eq!(diagnostics.captured_edges, 0);
    }

    #[test]
    fn live_diagnostics_merge_worker_callback_and_cursor_without_accumulating() {
        let producer = PlatformInputDiagnosticsProducer::default();
        let counters = CallbackCounters::default();
        counters
            .gamepad_runtime_queue_overflows
            .store(3, Ordering::Relaxed);
        counters.gamepad_connections.store(4, Ordering::Relaxed);
        counters
            .gamepad_axis_publish_rejections
            .store(5, Ordering::Relaxed);
        let cursor = LatestCursor::default();
        cursor.publish(MacCursorPoint { x: 1.0, y: 2.0 });
        cursor.publish(MacCursorPoint { x: 3.0, y: 4.0 });
        assert_eq!(cursor.take(), Some(MacCursorPoint { x: 3.0, y: 4.0 }));
        let worker = PlatformInputDiagnostics {
            runtime_queue_overflows: 2,
            recovery_resets: 6,
            ..PlatformInputDiagnostics::default()
        };

        publish_live_diagnostics(&producer, worker, &counters, &cursor);
        publish_live_diagnostics(&producer, worker, &counters, &cursor);
        let live = producer.diagnostics();
        assert_eq!(live.runtime_queue_overflows, 5);
        assert_eq!(live.gamepad_connections, 4);
        assert_eq!(live.gamepad_axis_publish_rejections, 5);
        assert_eq!(live.recovery_resets, 6);
        assert_eq!(live.cursor_captured, 2);
        assert_eq!(live.cursor_coalesced, 1);
        assert_eq!(live.cursor_consumed, 1);
    }

    #[test]
    fn gamepad_callback_queues_reliable_edges_and_atomic_latest_axes() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 64);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let producer = runtime.input_producer();
        let axis_producer = runtime.gamepad_axis_producer();
        let connection = axis_producer.connect(0).expect("connection");
        producer
            .publish(InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            })
            .expect("connected event");
        let pressed = AtomicU16::new(0);
        let accepting = AtomicBool::new(true);
        let recovery = AtomicBool::new(false);
        let counters = CallbackCounters::default();
        let axes = LatestGamepadAxes::default();
        let (sender, receiver) = mpsc::sync_channel(8);
        let mut down = MacGamepadSnapshot {
            buttons: 1_u16 << GamepadButton::South as u16,
            ..MacGamepadSnapshot::default()
        };
        down.axes[GamepadAxis::LeftStickX as usize] = 0.75;
        down.axes[GamepadAxis::RightTrigger as usize] = 0.5;
        capture_gamepad_snapshot(
            down, connection, &sender, &pressed, &axes, &accepting, &recovery, &counters,
        );
        let mut candidates = BTreeMap::new();
        let mut missing = BTreeMap::new();
        let mut diagnostics = PlatformInputDiagnostics::default();
        let values = axes.take().expect("axis snapshot");
        for axis in GamepadAxis::ALL {
            axis_producer
                .publish(GamepadAxisSample {
                    key: GamepadAxisKey { connection, axis },
                    value: values[axis as usize],
                    at: MonotonicMillis::new(1),
                })
                .expect("axis published");
        }
        publish_captured(
            &producer,
            receiver.recv().expect("button edge"),
            MonotonicMillis::new(1),
            &mut candidates,
            &mut missing,
            &mut diagnostics,
            &mut None,
        )
        .expect("button down published");
        let down_snapshot = client
            .wait_for_input_sequence(1, TIMEOUT)
            .expect("button down consumed");
        assert_eq!(down_snapshot.input.connected_gamepad_count, 1);
        assert_eq!(down_snapshot.input.pressed_gamepad_button_count, 1);
        assert!(down_snapshot.model_input.stick_left_x > 0.7);
        assert_eq!(down_snapshot.model_input.right_trigger, 0.5);

        capture_gamepad_snapshot(
            MacGamepadSnapshot::default(),
            connection,
            &sender,
            &pressed,
            &axes,
            &accepting,
            &recovery,
            &counters,
        );
        publish_captured(
            &producer,
            receiver.recv().expect("button release"),
            MonotonicMillis::new(2),
            &mut candidates,
            &mut missing,
            &mut diagnostics,
            &mut None,
        )
        .expect("button up published");
        let released = client
            .wait_for_input_sequence(2, TIMEOUT)
            .expect("button up consumed");
        assert_eq!(released.input.pressed_gamepad_button_count, 0);
        assert_eq!(counters.gamepad_button_edges.load(Ordering::Relaxed), 2);
        assert!(!recovery.load(Ordering::Acquire));

        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[test]
    fn latest_gamepad_axes_never_exposes_a_mixed_snapshot() {
        let axes = Arc::new(LatestGamepadAxes::default());
        let writer_axes = Arc::clone(&axes);
        let writer = thread::spawn(move || {
            for value in 1..=50_000_u32 {
                let value = value as f32;
                writer_axes.publish([value; 6]);
            }
        });
        while !writer.is_finished() {
            if let Some(values) = axes.take() {
                assert!(values.iter().all(|value| *value == values[0]));
            }
            thread::yield_now();
        }
        writer.join().expect("axis writer");
        while let Some(values) = axes.take() {
            assert!(values.iter().all(|value| *value == values[0]));
        }
        axes.close();
    }

    #[test]
    fn gamecontroller_owner_restores_background_policy_and_handlers() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 64);
        let counters = Arc::new(CallbackCounters::default());
        let accepting = Arc::new(AtomicBool::new(true));
        let recovery = Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = mpsc::sync_channel(8);
        let mut owner = MacGamepadOwner::new(
            runtime.input_producer(),
            runtime.gamepad_axis_producer(),
            sender,
            Instant::now(),
            Arc::clone(&accepting),
            Arc::clone(&recovery),
            Arc::clone(&counters),
        );
        assert!(owner.background_monitoring_enabled);
        owner.reconcile().expect("controller enumeration");
        assert!(owner.shutdown());
        assert!(owner.attached.is_empty());
        assert!(!accepting.load(Ordering::Acquire));
        assert_eq!(counters.callback_panics.load(Ordering::Relaxed), 0);
        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }
}
