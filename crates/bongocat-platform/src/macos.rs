use crate::{
    DisplayBounds, InputPermission, NativeWindowError, PlatformInputDiagnostics,
    PlatformInputError, PlatformInputServiceStatus, gilrs_gamepad::GilrsGamepad,
};
use block2::RcBlock;
use bongocat_config::Language;
use bongocat_input::{
    CursorDelta, CursorForceMoveState, CursorPosition, CursorProducer, CursorPublishError,
    CursorSample, CursorViewport, GamepadAxisProducer, InputControl, InputEdge, InputEvent,
    InputProducer, InputPublishError, InputResetReason, InputSource, MonotonicMillis, MouseButton,
    PhysicalKey, PlatformInputDiagnosticsProducer,
};
use objc2::{
    MainThreadMarker,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, CGPoint, CGRect, CGSize,
    kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGError, CGEvent, CGEventField, CGEventFlags, CGEventMask,
    CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGGetActiveDisplayList,
    CGGetDisplaysWithPoint, CGMouseButton,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
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

pub fn system_language() -> Language {
    sys_locale::get_locale().map_or_else(Language::default, |locale| {
        Language::from_system_locale(&locale)
    })
}

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

/// How long the Caps Lock trigger stays down before the platform releases it.
///
/// Caps Lock is the one control whose physical key-up this platform cannot
/// observe. Measured on macOS 27.0 arm64 with a listen-only tap at
/// `kCGHIDEventTap` + `kCGHeadInsertEventTap`: N physical taps deliver exactly N
/// `FlagsChanged` events, and the release delivers nothing at all — the
/// `AlphaShift` bit is the *latch*, which the driver flips on the press, and
/// Apple's stateless caps masks (`0x01000000` / `0x00000080`) are never set in
/// the event flags. There is no `kCGEventKeyUp` for a latch, so no amount of
/// decoding recovers a physical release.
///
/// The product's documented behavior for this key is a brief trigger
/// (`docs/migration/legacy-behavior-inventory.md`), so the candidate is released on this
/// fixed window instead of inventing an edge the platform never sent. This is
/// deliberately not configurable and is deliberately *not* the retired
/// `input.keyboard.release_fallback_timeout_ms`: that setting timed out keys
/// the user was still holding, which is a real release question. This key has
/// no hold to preserve — its own release is unreportable — so the window is a
/// fixed part of the Caps Lock behavior, matching the pre-refactor product.
const CAPS_LOCK_AUTO_RELEASE: Duration = Duration::from_millis(100);

/// Returns the vertical chrome AppKit adds above a standard titled content rectangle.
pub fn window_content_top_inset() -> f32 {
    let Some(marker) = MainThreadMarker::new() else {
        return 0.0;
    };
    let content = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::Miniaturizable;
    let frame = NSWindow::frameRectForContentRect_styleMask(content, style, marker);
    let inset = (frame.size.height - content.size.height) as f32;
    if inset.is_finite() {
        inset.max(0.0)
    } else {
        0.0
    }
}

/// Hides the window without closing or destroying it.
///
/// The settings and update windows are pre-rendered and reused, so AppKit is asked to
/// remove the window from the screen (`orderOut:`) while it stays in the application's
/// window list. GPUI therefore keeps the same view, its subscriptions and its
/// in-flight drafts, exactly like the Windows `ShowWindow(SW_HIDE)` path.
pub fn hide_native_window(window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    native_window(window)?.orderOut(None);
    Ok(())
}

/// Shows a window that [`hide_native_window`] ordered out.
///
/// Ordering the window front *and* keying it is what makes the re-shown settings window
/// the one the user types into; `orderFront:` alone would leave the key with whichever
/// window already had it.
pub fn show_native_window(window: &impl HasWindowHandle) -> Result<(), NativeWindowError> {
    native_window(window)?.makeKeyAndOrderFront(None);
    Ok(())
}

/// Whether the process currently shows a Dock icon.
///
/// macOS owns the Dock icon as a process-wide activation policy rather than a
/// window style, so this is the readable half of the pair that
/// [`set_dock_icon_visible`] writes. `Regular` is the only policy that puts the
/// application in the Dock; `Accessory` (what the product starts as) and
/// `Prohibited` both hide it.
pub fn dock_icon_is_visible() -> Result<bool, NativeWindowError> {
    let application = shared_application()?;
    Ok(application.activationPolicy() == NSApplicationActivationPolicy::Regular)
}

/// Shows or hides the process Dock icon by moving the activation policy.
///
/// The product launches the overlay as an `Accessory` application, so a Dock
/// icon is opt-in. The policy is read back instead of trusted: `setActivationPolicy`
/// reports failure by returning `false`, and a policy that did not actually change
/// would leave the shell disagreeing with the persisted preference, which is the
/// one thing the settings worker refuses to commit.
pub fn set_dock_icon_visible(visible: bool) -> Result<(), NativeWindowError> {
    let application = shared_application()?;
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if application.activationPolicy() == policy {
        return Ok(());
    }
    if !application.setActivationPolicy(policy) {
        return Err(NativeWindowError::DockVisibilityUpdateFailed);
    }
    if dock_icon_is_visible()? != visible {
        return Err(NativeWindowError::DockVisibilityUpdateFailed);
    }
    Ok(())
}

fn shared_application() -> Result<Retained<NSApplication>, NativeWindowError> {
    // `NSApplication` is a process-wide AppKit object and its activation policy is
    // main-thread-only. Constructing the shared instance off the main thread also
    // aborts, so the marker is checked before the instance is asked for.
    let marker = MainThreadMarker::new().ok_or(NativeWindowError::WrongThread)?;
    Ok(NSApplication::sharedApplication(marker))
}

fn native_window(window: &impl HasWindowHandle) -> Result<Retained<NSWindow>, NativeWindowError> {
    // AppKit windows are main-thread objects. The callers are GPUI window callbacks, which
    // run on the window owner thread; refuse the call instead of making it off that thread.
    MainThreadMarker::new().ok_or(NativeWindowError::WrongThread)?;
    let handle = window
        .window_handle()
        .map_err(|_| NativeWindowError::HandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(NativeWindowError::UnsupportedHandle);
    };
    // SAFETY: raw-window-handle guarantees `ns_view` points to the NSView GPUI owns for the
    // complete Window lifetime, and the returned borrow does not outlive `window`.
    let view: &NSView = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    view.window().ok_or(NativeWindowError::HandleUnavailable)
}

const REQUIRED_MISSING_CONFIRMATIONS: u8 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const SERVICE_TIMEOUT: Duration = Duration::from_secs(2);

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
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MacCursorPoint {
    x: f64,
    y: f64,
    /// Relative motion the device reported with this event.
    ///
    /// The location above is where the pointer is; this is how far it moved.
    /// An application that captures the pointer keeps the location parked while
    /// this keeps arriving, which is what makes it worth carrying separately.
    delta: CursorDelta,
}

impl MacCursorPoint {
    /// A reading with no motion of its own, for a location read on its own.
    fn at(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            delta: CursorDelta::default(),
        }
    }
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
        // `MacCursorPoint` is `Copy`, so this reads the pending sample out
        // instead of borrowing the field the match is on.
        match state.pending {
            Some(mut pending) => {
                // A coalesced sample keeps only the latest location but must
                // keep every packet's motion: several events can arrive between
                // two service slices, and dropping their deltas would make a
                // fast move in a captured application travel less far than the
                // device did.
                pending.x = point.x;
                pending.y = point.y;
                pending.delta.x += point.delta.x;
                pending.delta.y += point.delta.y;
                state.pending = Some(pending);
                state.coalesced = state.coalesced.saturating_add(1);
            }
            None => state.pending = Some(point),
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

#[derive(Default)]
struct CallbackCounters {
    captured_edges: AtomicU64,
    queued_edges: AtomicU64,
    unmapped_keys: AtomicU64,
    unsupported_buttons: AtomicU64,
    callback_panics: AtomicU64,
    capture_queue_overflows: AtomicU64,
    rejected_after_stop: AtomicU64,
}

struct TapCallbackContext {
    sender: SyncSender<CapturedEvent>,
    accepting: Arc<AtomicBool>,
    recovery_requested: Arc<AtomicBool>,
    tap_disabled: Arc<AtomicBool>,
    modifier_decoder: Arc<Mutex<ModifierDecoder>>,
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
            &self.modifier_decoder,
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
    // The tap must sit at the HID layer's head, matching rdev's listen setup.
    // Measured on macOS 26.5.2: at the session tail, Right Shift release
    // `FlagsChanged` events are never delivered (and repeated presses arrive
    // with byte-identical flags), while the HID head receives complete
    // press/release pairs for every modifier. Listen-only, so events are only
    // observed, never modified or swallowed.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
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
            ..PlatformInputDiagnostics::default()
        }
    }
}

pub struct MacInputService {
    stop: Arc<AtomicBool>,
    force_move: Arc<AtomicBool>,
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
        if input_monitoring_permission() != InputPermission::Granted {
            return Err(PlatformInputError::PermissionDenied);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let force_move = Arc::new(AtomicBool::new(false));
        let worker_force_move = Arc::clone(&force_move);
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
                        worker_stop,
                        worker_force_move,
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
                force_move,
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

    /// Switch between the absolute cursor and accumulated relative motion.
    ///
    /// The value is read by the input worker on its next run-loop slice, so this
    /// is a plain store rather than a command: a stale value can only ever cost
    /// one sample, and the caller re-applies it every frame. See
    /// [`bongocat_input::CursorSettings::force_move`].
    pub fn set_force_move(&self, enabled: bool) {
        self.force_move.store(enabled, Ordering::Release);
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
    stop: Arc<AtomicBool>,
    force_move: Arc<AtomicBool>,
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
    let modifier_decoder = Arc::new(Mutex::new(ModifierDecoder::default()));
    let latest_cursor = LatestCursor::default();
    let mut cursor_force_move = CursorForceMoveState::default();
    let (capture_sender, capture_receiver) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
    let mut gamepad = GilrsGamepad::new(producer.clone(), gamepad_axis_producer);

    let mut callback_context = Box::new(TapCallbackContext {
        sender: capture_sender,
        accepting: Arc::clone(&accepting),
        recovery_requested: Arc::clone(&recovery_requested),
        tap_disabled: Arc::clone(&tap_disabled),
        modifier_decoder: Arc::clone(&modifier_decoder),
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
        latest_cursor.publish(MacCursorPoint::at(location.x, location.y));
    }
    let mut candidates = BTreeMap::<InputControl, SystemControl>::new();
    let mut missing_confirmations = BTreeMap::<InputControl, u8>::new();
    let mut next_reconciliation = Instant::now() + RECONCILIATION_INTERVAL;
    // Armed when a Caps Lock press becomes a candidate, cleared once the
    // candidate is gone for any reason.
    let mut caps_lock_trigger = CapsLockTrigger::default();
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
                    modifier_decoder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clear();
                    // A reset says the pointer state can no longer be trusted,
                    // so an accumulated position must not survive it.
                    cursor_force_move.reset();
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
                    match gamepad.reseed(monotonic(started), &mut diagnostics) {
                        Ok(()) => accepting.store(true, Ordering::Release),
                        Err(InputPublishError::QueueFull(_)) => {
                            diagnostics.runtime_queue_overflows =
                                diagnostics.runtime_queue_overflows.saturating_add(1);
                            recovery_pending = true;
                            continue;
                        }
                        Err(InputPublishError::RuntimeStopped(_)) => {
                            break 'service Err(PlatformInputError::RuntimeStopped);
                        }
                    }
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
            modifier_decoder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            cursor_force_move.reset();
            recovery_pending = true;
            tap_restart_pending = true;
            continue;
        }

        match gamepad.drain(monotonic(started), &mut diagnostics) {
            Ok(()) => {}
            Err(InputPublishError::QueueFull(_)) => {
                diagnostics.runtime_queue_overflows =
                    diagnostics.runtime_queue_overflows.saturating_add(1);
                accepting.store(false, Ordering::Release);
                recovery_pending = true;
                continue;
            }
            Err(InputPublishError::RuntimeStopped(_)) => {
                break 'service Err(PlatformInputError::RuntimeStopped);
            }
        }

        loop {
            let event = match capture_receiver.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            };
            let reset_requires_gamepad_reseed = matches!(&event, CapturedEvent::Reset);
            if let Err(error) = publish_captured(
                &producer,
                event,
                monotonic(started),
                &mut candidates,
                &mut missing_confirmations,
                &mut diagnostics,
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
            } else if reset_requires_gamepad_reseed {
                match gamepad.reseed(monotonic(started), &mut diagnostics) {
                    Ok(()) => {}
                    Err(InputPublishError::QueueFull(_)) => {
                        diagnostics.runtime_queue_overflows =
                            diagnostics.runtime_queue_overflows.saturating_add(1);
                        accepting.store(false, Ordering::Release);
                        recovery_pending = true;
                        break;
                    }
                    Err(InputPublishError::RuntimeStopped(_)) => {
                        break 'service Err(PlatformInputError::RuntimeStopped);
                    }
                }
            }
        }

        if let Err(error) = forward_latest_cursor(
            &latest_cursor,
            &cursor_producer,
            started,
            &mut diagnostics,
            force_move.load(Ordering::Acquire),
            &mut cursor_force_move,
        ) {
            break 'service Err(error);
        }

        // Arm or disarm the Caps Lock brief-trigger window from the candidate
        // set, the single source of truth for whether the key is down now.
        caps_lock_trigger.sync(&candidates, Instant::now());

        if caps_lock_trigger.take_due(Instant::now()) {
            match release_caps_lock_trigger(
                &producer,
                &mut candidates,
                &mut missing_confirmations,
                &modifier_decoder,
                monotonic(started),
            ) {
                Ok(()) => {}
                Err(InputPublishError::QueueFull(_)) => {
                    diagnostics.runtime_queue_overflows =
                        diagnostics.runtime_queue_overflows.saturating_add(1);
                    recovery_pending = true;
                }
                Err(InputPublishError::RuntimeStopped(_)) => {
                    break 'service Err(PlatformInputError::RuntimeStopped);
                }
            }
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
                    let candidates_snapshot = candidates
                        .iter()
                        .map(|(control, system)| (*control, *system))
                        .collect::<Vec<_>>();
                    for (control, system) in candidates_snapshot {
                        if pressed.contains(&control) {
                            missing_confirmations.remove(&control);
                        } else {
                            let confirmations = missing_confirmations.entry(control).or_insert(0);
                            *confirmations = confirmations.saturating_add(1);
                            if *confirmations >= REQUIRED_MISSING_CONFIRMATIONS {
                                candidates.remove(&control);
                                missing_confirmations.remove(&control);
                                if let SystemControl::Key(key_code) = system {
                                    modifier_decoder
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .set_pressed(key_code, false);
                                }
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
    let gamepad_shutdown = gamepad.shutdown();
    diagnostics.gamepad_disconnections = diagnostics
        .gamepad_disconnections
        .saturating_add(gamepad_shutdown.disconnected);
    drop(gamepad);
    latest_cursor.close();
    if service_result.is_ok()
        && let Err(error) = forward_latest_cursor(
            &latest_cursor,
            &cursor_producer,
            started,
            &mut diagnostics,
            force_move.load(Ordering::Acquire),
            &mut cursor_force_move,
        )
    {
        service_result = Err(error);
    }
    diagnostics.capture_queue_discarded = diagnostics
        .capture_queue_discarded
        .saturating_add(drain_capture_queue(&capture_receiver));
    diagnostics.clean_shutdown =
        gamepad_shutdown.backend_clean && publish_final_reset(&producer, started, &mut diagnostics);
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
    force_move: bool,
    force_move_state: &mut CursorForceMoveState,
) -> Result<(), PlatformInputError> {
    // The mode is read first so a change discards the accumulated position even
    // on a slice with nothing to publish.
    let force_move = force_move_state.sync(force_move);
    let Some(point) = latest.take() else {
        return Ok(());
    };
    let Some(viewport) = display_viewport(point) else {
        diagnostics.cursor_display_lookup_failures =
            diagnostics.cursor_display_lookup_failures.saturating_add(1);
        return Ok(());
    };
    let absolute = CursorPosition {
        x: point.x,
        y: point.y,
    };
    let position = if force_move {
        force_move_state.advance(absolute, point.delta, viewport)
    } else {
        absolute
    };
    let sample = match CursorSample::new(position, viewport, monotonic(started)) {
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
    let display = display_at_point(point)?;
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

fn display_at_point(point: MacCursorPoint) -> Option<CGDirectDisplayID> {
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
    Some(display)
}

pub fn current_display_bounds() -> Option<DisplayBounds> {
    let event = CGEvent::new(None)?;
    let location = CGEvent::location(Some(&event));
    let point = MacCursorPoint::at(location.x, location.y);
    let display = display_at_point(point)?;
    display_bounds(display)
}

pub fn display_bounds_for_window(x: f32, y: f32, width: f32, height: f32) -> Option<DisplayBounds> {
    active_display_bounds()
        .into_iter()
        .find(|display| display.intersects_window(x, y, width, height))
}

pub fn local_window_origin(display: DisplayBounds, x: f32, y: f32) -> (f32, f32) {
    (x - display.x, y - display.y)
}

pub fn global_window_origin(display_id: Option<u32>, x: f32, y: f32) -> (f32, f32) {
    display_id
        .and_then(display_bounds)
        .map_or((x, y), |display| (display.x + x, display.y + y))
}

fn active_display_bounds() -> Vec<DisplayBounds> {
    const MAX_DISPLAYS: usize = 32;
    let mut displays = [0_u32; MAX_DISPLAYS];
    let mut count = 0_u32;
    // SAFETY: CoreGraphics writes at most MAX_DISPLAYS IDs into the initialized
    // stack array and writes the actual count to a valid stack pointer.
    let result =
        unsafe { CGGetActiveDisplayList(MAX_DISPLAYS as u32, displays.as_mut_ptr(), &mut count) };
    if result != CGError::Success {
        return Vec::new();
    }
    displays[..count.min(MAX_DISPLAYS as u32) as usize]
        .iter()
        .filter_map(|display| display_bounds(*display))
        .collect()
}

fn display_bounds(display: CGDirectDisplayID) -> Option<DisplayBounds> {
    let bounds = CGDisplayBounds(display);
    if !bounds.origin.x.is_finite()
        || !bounds.origin.y.is_finite()
        || !bounds.size.width.is_finite()
        || !bounds.size.height.is_finite()
        || bounds.size.width <= 0.0
        || bounds.size.height <= 0.0
    {
        return None;
    }
    Some(DisplayBounds {
        display_id: Some(display),
        x: bounds.origin.x as f32,
        y: bounds.origin.y as f32,
        width: bounds.size.width as f32,
        height: bounds.size.height as f32,
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
    modifier_decoder: &Mutex<ModifierDecoder>,
    cursor: &LatestCursor,
    counters: &CallbackCounters,
) {
    if matches!(
        event_type,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        modifier_decoder
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
            let flags = CGEvent::flags(Some(event)).bits();
            let mut decoder = modifier_decoder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(edge) = decoder.decode(key_code, flags) else {
                enqueue_event(
                    CapturedEvent::Reset,
                    sender,
                    accepting,
                    recovery_requested,
                    counters,
                );
                return;
            };
            Some(CapturedEvent::Edge {
                control: InputControl::Key(key),
                system: SystemControl::Key(key_code),
                edge,
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
                // An application that captures the pointer keeps the location
                // above parked while these keep reporting every movement.
                delta: CursorDelta {
                    x: CGEvent::integer_value_field(Some(event), CGEventField::MouseEventDeltaX)
                        as f64,
                    y: CGEvent::integer_value_field(Some(event), CGEventField::MouseEventDeltaY)
                        as f64,
                },
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
        CapturedEvent::Reset => {
            producer.recover(InputResetReason::ServiceRestart, at)?;
            candidates.clear();
            missing_confirmations.clear();
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

/// Schedules the single release edge macOS will not send for Caps Lock.
///
/// The window is always re-derived from the candidate set, which is the one
/// source of truth for "is this key down right now". That makes two failure
/// modes unrepresentable: a deadline cannot outlive its key (so a stale timer
/// can never release something else), and a key that is down cannot go
/// unarmed (so the trigger cannot leak). `take_due` consumes the deadline, so
/// one deadline yields at most one release.
#[derive(Default)]
struct CapsLockTrigger {
    due: Option<Instant>,
}

impl CapsLockTrigger {
    /// Arms the window on the first observation of a Caps Lock candidate and
    /// disarms it the moment the candidate is gone for any reason.
    fn sync(&mut self, candidates: &BTreeMap<InputControl, SystemControl>, now: Instant) {
        let held = caps_lock_control().is_some_and(|control| candidates.contains_key(&control));
        if !held {
            self.due = None;
        } else {
            self.due.get_or_insert(now + CAPS_LOCK_AUTO_RELEASE);
        }
    }

    fn take_due(&mut self, now: Instant) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            true
        } else {
            false
        }
    }
}

/// Releases the Caps Lock candidate once its brief-trigger window has elapsed.
///
/// This is the one key-up edge the platform cannot deliver, so the adapter
/// synthesizes it here rather than leaving the candidate down forever. The
/// decoder is re-anchored to "not pressed" as part of the same step, so the
/// next latch toggle decodes as a fresh press instead of continuing the
/// alternation from a state the platform never reported.
///
/// `InputSource::Capture` is used because the edge travels the capture
/// channel, in order, immediately after the `Down` that armed it — the runtime
/// needs no new source to accept it. It does mean this one release is counted
/// as a captured release in the runtime diagnostics, which is a known and
/// accepted imprecision; the platform-side counters do not claim it.
fn release_caps_lock_trigger(
    producer: &InputProducer,
    candidates: &mut BTreeMap<InputControl, SystemControl>,
    missing_confirmations: &mut BTreeMap<InputControl, u8>,
    modifier_decoder: &Mutex<ModifierDecoder>,
    at: MonotonicMillis,
) -> Result<(), InputPublishError> {
    let Some(control) = caps_lock_control() else {
        return Ok(());
    };
    if !candidates.contains_key(&control) {
        return Ok(());
    }
    producer.publish(InputEvent::Edge {
        control,
        edge: InputEdge::Up,
        source: InputSource::Capture,
        at,
    })?;
    candidates.remove(&control);
    missing_confirmations.remove(&control);
    modifier_decoder
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .set_pressed(CAPS_LOCK_KEY_CODE, false);
    Ok(())
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

/// Device-dependent modifier bits carried in the low 8 bits of
/// `CGEventGetFlags` (NX device-dependent mask range). Each physical key owns
/// one bit, so device bits distinguish left/right and keep working while the
/// family flag stays set for a held sibling. Measured on macOS 26.5.2:
/// pressing Right Shift reports `0x00020104` (family shift + `0x04`).
mod modifier_device_flags {
    pub const LEFT_CONTROL: u64 = 0x01;
    pub const LEFT_SHIFT: u64 = 0x02;
    pub const RIGHT_SHIFT: u64 = 0x04;
    pub const LEFT_COMMAND: u64 = 0x08;
    pub const RIGHT_COMMAND: u64 = 0x10;
    pub const LEFT_ALT: u64 = 0x20;
    pub const RIGHT_ALT: u64 = 0x40;
    pub const RIGHT_CONTROL: u64 = 0x80;
}

const CAPS_LOCK_KEY_CODE: u16 = 57;

/// The `InputControl` for Caps Lock — the one control this platform must
/// release on a timer because the event stream cannot express its key-up.
/// See [`CAPS_LOCK_AUTO_RELEASE`].
fn caps_lock_control() -> Option<InputControl> {
    map_key_code(CAPS_LOCK_KEY_CODE).map(InputControl::Key)
}

fn modifier_device_bit(key_code: u16) -> u64 {
    match key_code {
        59 => modifier_device_flags::LEFT_CONTROL,
        62 => modifier_device_flags::RIGHT_CONTROL,
        56 => modifier_device_flags::LEFT_SHIFT,
        60 => modifier_device_flags::RIGHT_SHIFT,
        55 => modifier_device_flags::LEFT_COMMAND,
        54 => modifier_device_flags::RIGHT_COMMAND,
        58 => modifier_device_flags::LEFT_ALT,
        61 => modifier_device_flags::RIGHT_ALT,
        _ => 0,
    }
}

fn modifier_family_bit(key_code: u16) -> u64 {
    match key_code {
        54 | 55 => CGEventFlags::MaskCommand.bits(),
        56 | 60 => CGEventFlags::MaskShift.bits(),
        57 => CGEventFlags::MaskAlphaShift.bits(),
        58 | 61 => CGEventFlags::MaskAlternate.bits(),
        59 | 62 => CGEventFlags::MaskControl.bits(),
        63 => CGEventFlags::MaskSecondaryFn.bits(),
        _ => 0,
    }
}

/// Freezes the down/up direction of `FlagsChanged` events at callback time.
///
/// The decoder is callback-local packet state, not the runtime pressed state.
/// It must be cleared on every `Reset` and whenever the reconciliation pass
/// force-releases a modifier candidate so the alternation fallback stays in
/// sync with the runtime.
///
/// Direction rules, in priority order (measured on macOS 26.5.2):
/// 1. A device-bit transition is authoritative for left/right modifiers.
/// 2. Otherwise a family-flag transition decides — rdev's `LAST_FLAGS` diff.
/// 3. Otherwise the recorded edge alternates. Current macOS does not deliver
///    Right Shift release events and re-delivers presses with identical
///    flags, and CapsLock toggles a latch instead of reporting the physical
///    edge, so no flag transition is available for those events.
///
/// Rule 3 is sound for Right Shift because that key does deliver a release
/// event. It is *not* sound for Caps Lock: measured on macOS 27.0 arm64 at the
/// HID head, N physical taps deliver exactly N `FlagsChanged` events and the
/// release delivers nothing, so there is no second event for the alternation
/// to alternate with. The decoder is therefore only the "this latch moved"
/// detector for Caps Lock, and the platform releases that candidate on
/// [`CAPS_LOCK_AUTO_RELEASE`] and re-anchors the decoder with `set_pressed`.
#[derive(Default)]
struct ModifierDecoder {
    last_flags: u64,
    pressed: BTreeSet<u16>,
}

impl ModifierDecoder {
    fn decode(&mut self, key_code: u16, flags: u64) -> Option<InputEdge> {
        let device_bit = modifier_device_bit(key_code);
        let family_bit = modifier_family_bit(key_code);
        if device_bit == 0 && family_bit == 0 {
            self.last_flags = flags;
            return None;
        }
        let changed = flags ^ self.last_flags;
        self.last_flags = flags;
        let is_down = if device_bit != 0 && changed & device_bit != 0 {
            flags & device_bit != 0
        } else if key_code != CAPS_LOCK_KEY_CODE && family_bit != 0 && changed & family_bit != 0 {
            flags & family_bit != 0
        } else {
            !self.pressed.contains(&key_code)
        };
        if is_down {
            self.pressed.insert(key_code);
        } else {
            self.pressed.remove(&key_code);
        }
        Some(if is_down {
            InputEdge::Down
        } else {
            InputEdge::Up
        })
    }

    fn set_pressed(&mut self, key_code: u16, pressed: bool) {
        if pressed {
            self.pressed.insert(key_code);
        } else {
            self.pressed.remove(&key_code);
        }
    }

    fn clear(&mut self) {
        self.last_flags = 0;
        self.pressed.clear();
    }
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
        // `kVK_Function`: the Fn / globe key. It is the only key in this table
        // that is not on the HID Keyboard/Keypad page, so its usage is Apple's
        // vendor page folded into the same `u16` — taken from the one place
        // that defines it instead of restating the value (see
        // `bongocat_render::GLOBE_KEY_USAGE`). The key arrives as
        // `FlagsChanged` carrying `MaskSecondaryFn`, which `ModifierDecoder`
        // already decodes; the missing arm here was the only reason it never
        // reached the runtime.
        63 => PhysicalKey::GLOBE.hid_usage(),
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
        // The Menu / Application key (`kVK_ContextualMenu`). It was named and
        // bound from the start (`Apps`, HID `0x65`) but never mapped here, so no
        // model could ever draw `Apps.png`: a press the adapter cannot produce
        // never reaches the binding table at all. Apple keyboards have no such
        // key, but a third-party one sends this keycode.
        110 => 0x65,
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

/// Returns the family representative keycode to query as a fallback state
/// source for right-side modifiers. Measured on macOS 26.5.2:
/// `CGEventSourceKeyState` reports false for keycodes 54/60/61/62 even while
/// the physical key is held, while the family keycode (55/56/58/59) does
/// report the combined state.
fn family_state_keycode(key_code: u16) -> Option<u16> {
    match key_code {
        54 => Some(55),
        60 => Some(56),
        61 => Some(58),
        62 => Some(59),
        _ => None,
    }
}

fn system_pressed(system: SystemControl) -> bool {
    match system {
        SystemControl::Key(key_code) => {
            if CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, key_code) {
                return true;
            }
            match family_state_keycode(key_code) {
                Some(family) => {
                    CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, family)
                }
                None => false,
            }
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
            let mut seeded_decoder = ModifierDecoder::default();
            seeded_decoder.set_pressed(56, true);
            let modifier_decoder = Arc::new(Mutex::new(seeded_decoder));
            let counters = Arc::new(CallbackCounters::default());
            let context = TapCallbackContext {
                sender,
                accepting: Arc::clone(&accepting),
                recovery_requested: recovery,
                tap_disabled: Arc::clone(&tap_disabled),
                modifier_decoder: Arc::clone(&modifier_decoder),
                cursor: LatestCursor::default(),
                counters,
            };
            let event = CGEvent::new(None).expect("event");

            context.capture(event_type, &event);

            assert!(!accepting.load(Ordering::Acquire));
            assert!(tap_disabled.load(Ordering::Acquire));
            assert!(
                modifier_decoder
                    .lock()
                    .expect("modifier decoder")
                    .pressed
                    .is_empty()
            );
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
        // `kVK_ContextualMenu`: the Menu / Application key. Apple keyboards have
        // no such key, but a third-party one sends this keycode, and `Apps` was
        // named and bound in the vocabulary long before the adapter could
        // report it.
        assert_eq!(map_key_code(110).map(PhysicalKey::hid_usage), Some(0x65));
        // `kVK_Function`: the Fn / globe key, the one key outside the HID
        // Keyboard/Keypad page.
        assert_eq!(map_key_code(63), Some(PhysicalKey::GLOBE));
        assert_eq!(map_key_code(63).map(PhysicalKey::hid_usage), Some(0xff03));
        assert_eq!(map_key_code(u16::MAX), None);

        let shortcuts = bongocat_config::ShortcutConfig {
            commands_enabled: true,
            command_bindings: vec![bongocat_config::ShortcutBinding {
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

    /// Which function keys this adapter can actually report, so a vocabulary
    /// that claims more than the platform delivers is caught here instead of
    /// being copied from one document into the next.
    ///
    /// Carbon defines `kVK_F13` … `kVK_F20` and no keycode at all for F21 … F24,
    /// so the reachable function keys stop at HID `0x6f`. `0x70..=0x73` are
    /// named by the key vocabulary — a model may ship that artwork and it has to
    /// work — but no macOS keyboard can ever press them. The globe key is the
    /// only usage this adapter produces that is not on the Keyboard/Keypad page.
    #[test]
    fn this_adapter_reports_f1_through_f20_and_the_globe_key_only() {
        let mut function_keys = Vec::new();
        let mut outside_the_keyboard_page = Vec::new();
        for key_code in 0..=u16::MAX {
            let Some(key) = map_key_code(key_code) else {
                continue;
            };
            let usage = key.hid_usage();
            if (0x3a..=0x45).contains(&usage) || (0x68..=0x73).contains(&usage) {
                function_keys.push(usage);
            }
            if usage >= 0xff00 {
                outside_the_keyboard_page.push((key_code, usage));
            }
        }
        function_keys.sort_unstable();
        function_keys.dedup();
        let expected: Vec<u16> = (0x3a..=0x45).chain(0x68..=0x6f).collect();
        assert_eq!(function_keys, expected, "reachable function keys");
        assert_eq!(
            outside_the_keyboard_page,
            vec![(63, 0xff03)],
            "the globe key is the only vendor-page usage this adapter reports"
        );
    }

    /// The exact set of HID usages this adapter can produce, so a key that the
    /// vocabulary names and the runtime binds but no keycode reaches is caught
    /// here. That is how `Apps` (`0x65`) was found: named, bound, on the
    /// reference diagram, and unreachable on both platforms.
    ///
    /// The gaps are deliberate, and each has a reason:
    ///
    /// - `0x32` `IntlHash`: macOS gives the ISO `#` key the same keycode it
    ///   gives ANSI `\` (42), which maps to `BackSlash` (`0x31`). The two are
    ///   indistinguishable through this API.
    /// - `0x46` `PrintScreen`, `0x47` `ScrollLock`, `0x48` `Pause`, `0x49`
    ///   `Insert`: no Apple keyboard carries them. The F13/F14/F15 keycodes sit
    ///   in those physical positions and report as F13/F14/F15 above, which is
    ///   the correct identity for the key that is actually pressed.
    /// - `0x64` `IntlBackslash`: `kVK_ISO_Section` (10) is left unmapped; the ISO
    ///   extra key beside the left Shift has no HID usage macOS can distinguish
    ///   from the ANSI layout's absent key.
    /// - `0x70..=0x73` `F21` … `F24`: Carbon defines no keycode for them.
    #[test]
    fn this_adapter_reports_exactly_the_keycodes_the_platform_defines() {
        let mut reachable = Vec::new();
        for key_code in 0..=u16::MAX {
            if let Some(key) = map_key_code(key_code) {
                reachable.push(key.hid_usage());
            }
        }
        reachable.sort_unstable();
        reachable.dedup();

        let mut expected: Vec<u16> = (0x04..=0x31)
            .chain([0x33])
            .chain(0x34..=0x39)
            .chain(0x3a..=0x45)
            .chain(0x4a..=0x4e)
            .chain(0x4f..=0x52)
            .chain(0x53..=0x63)
            .chain([0x65, 0x67])
            .chain(0x68..=0x6f)
            .chain(0xe0..=0xe7)
            .chain([PhysicalKey::GLOBE.hid_usage()])
            .collect();
        expected.sort_unstable();

        assert_eq!(reachable, expected, "reachable HID usages");
    }

    #[test]
    fn right_shift_taps_alternate_despite_identical_flags() {
        // Observed on macOS 26.5.2: Right Shift taps deliver one FlagsChanged
        // per press with identical flags and no release events at all.
        let mut decoder = ModifierDecoder::default();
        assert_eq!(
            decoder.decode(60, 0x0002_0104),
            Some(InputEdge::Down),
            "first press: device bit transition"
        );
        assert_eq!(
            decoder.decode(60, 0x0002_0104),
            Some(InputEdge::Up),
            "second press arrives with unchanged flags"
        );
        assert_eq!(decoder.decode(60, 0x0002_0104), Some(InputEdge::Down));

        // Reconciliation force-releases the candidate once the family keycode
        // stops reporting the held shift; the decoder must re-align so the
        // next tap decodes as a press again.
        decoder.set_pressed(60, false);
        assert_eq!(decoder.decode(60, 0x0002_0104), Some(InputEdge::Down));
    }

    #[test]
    fn right_shift_release_pairs_arrive_at_the_hid_head_tap() {
        // Observed on macOS 26.5.2 with the tap at HID head (rdev's setup):
        // every Right Shift press and release is delivered with proper flag
        // transitions, unlike the session tail which drops the releases.
        let mut decoder = ModifierDecoder::default();
        for _ in 0..3 {
            assert_eq!(decoder.decode(60, 0x0002_0104), Some(InputEdge::Down));
            assert_eq!(decoder.decode(60, 0x0000_0100), Some(InputEdge::Up));
        }
        // Held press then release.
        assert_eq!(decoder.decode(60, 0x0002_0104), Some(InputEdge::Down));
        assert_eq!(decoder.decode(60, 0x0000_0100), Some(InputEdge::Up));
    }

    #[test]
    fn right_alt_press_release_pairs_follow_device_bit_transitions() {
        let mut decoder = ModifierDecoder::default();
        for _ in 0..3 {
            assert_eq!(decoder.decode(61, 0x0008_0140), Some(InputEdge::Down));
            assert_eq!(decoder.decode(61, 0x0000_0100), Some(InputEdge::Up));
        }
    }

    /// Caps Lock is a latch, so the measured stream is one `FlagsChanged` per
    /// physical tap and nothing at all for the release. The alternation
    /// fallback therefore decodes *every* tap as a press, which is exactly why
    /// the platform releases the candidate on [`CAPS_LOCK_AUTO_RELEASE`]
    /// instead of waiting for an edge that never arrives.
    ///
    /// The previous version of this test asserted a press/release pair per tap.
    /// That sequence does not exist on macOS 27.0, so the test passed while the
    /// product stayed stuck. Keep it aligned with the measurement.
    #[test]
    fn caps_lock_decodes_one_press_per_tap_and_no_release_event_exists() {
        let mut decoder = ModifierDecoder::default();
        // Latch off: the tap sets AlphaShift.
        assert_eq!(decoder.decode(57, 0x0001_0100), Some(InputEdge::Down));
        // Latch on: the next tap clears it. No event was delivered in between.
        assert_eq!(decoder.decode(57, 0x0000_0100), Some(InputEdge::Up));
        // Latch off again.
        assert_eq!(decoder.decode(57, 0x0001_0100), Some(InputEdge::Down));
        assert_eq!(decoder.decode(57, 0x0000_0100), Some(InputEdge::Up));

        // The platform's timed release re-anchors the decoder, so the next tap
        // is a press again even though the alternation state was left mid-pair.
        decoder.set_pressed(57, false);
        assert_eq!(decoder.decode(57, 0x0001_0100), Some(InputEdge::Down));
    }

    #[test]
    fn caps_lock_auto_release_window_is_a_brief_trigger_not_a_hold() {
        // The window is part of the Caps Lock behavior, matching the documented
        // pre-refactor product, and must stay far below any plausible hold so
        // a held Caps Lock still reads as a brief trigger. It is also not the
        // retired per-key release fallback, which had to be generous.
        assert_eq!(CAPS_LOCK_AUTO_RELEASE, Duration::from_millis(100));
        assert!(CAPS_LOCK_AUTO_RELEASE < RECONCILIATION_INTERVAL);
    }

    #[test]
    fn only_caps_lock_needs_the_timed_release() {
        // Every other modifier resolves its own direction from a device bit, so
        // the special case must not widen to the rest of the modifier family.
        let caps_lock = caps_lock_control().expect("caps lock control");
        for key_code in [54, 55, 56, 58, 59, 60, 61, 62, 63] {
            let control = map_key_code(key_code)
                .map(InputControl::Key)
                .expect("mapped modifier");
            assert_ne!(
                control, caps_lock,
                "key code {key_code} must not be special-cased"
            );
        }
        assert_eq!(caps_lock_control(), map_key_code(57).map(InputControl::Key));
    }

    #[test]
    fn the_trigger_window_arms_once_and_releases_exactly_once() {
        let control = caps_lock_control().expect("caps lock control");
        let start = Instant::now();
        let mut trigger = CapsLockTrigger::default();

        // Nothing is held, so nothing is armed and nothing can fire.
        trigger.sync(&BTreeMap::new(), start);
        assert!(!trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE + Duration::from_secs(1)));

        // The press arms the window. Repeated syncs must not extend it, or a
        // busy loop would postpone the release forever.
        let mut candidates = BTreeMap::from([(control, SystemControl::Key(CAPS_LOCK_KEY_CODE))]);
        trigger.sync(&candidates, start);
        assert!(!trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE - Duration::from_millis(1)));
        trigger.sync(&candidates, start + CAPS_LOCK_AUTO_RELEASE / 2);
        assert!(!trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE - Duration::from_millis(1)));

        // The window elapses exactly once.
        assert!(trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE));
        assert!(!trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE));

        // A second tap arms a fresh window.
        trigger.sync(&candidates, start + Duration::from_secs(1));
        assert!(trigger.take_due(start + Duration::from_secs(1) + CAPS_LOCK_AUTO_RELEASE));

        // A key released by any other route disarms the window, so a stale
        // deadline can never release a key that is no longer down.
        candidates.clear();
        trigger.sync(&candidates, start + Duration::from_secs(2));
        assert!(!trigger.take_due(start + Duration::from_secs(3)));
    }

    #[test]
    fn a_reset_clearing_the_candidate_cancels_the_pending_release() {
        // The recovery paths clear `candidates` without publishing an edge, so
        // the trigger has to disarm from the candidate set alone.
        let control = caps_lock_control().expect("caps lock control");
        let start = Instant::now();
        let mut trigger = CapsLockTrigger::default();
        let candidates = BTreeMap::from([(control, SystemControl::Key(CAPS_LOCK_KEY_CODE))]);

        trigger.sync(&candidates, start);
        trigger.sync(&BTreeMap::new(), start + Duration::from_millis(1));
        assert!(
            !trigger.take_due(start + CAPS_LOCK_AUTO_RELEASE),
            "a reset must not leave a release armed for a key it already cleared"
        );
    }

    #[test]
    fn sibling_modifiers_stay_independent_via_device_bits() {
        let mut decoder = ModifierDecoder::default();
        assert_eq!(decoder.decode(56, 0x0002_0102), Some(InputEdge::Down));
        assert_eq!(
            decoder.decode(60, 0x0002_0106),
            Some(InputEdge::Down),
            "right press while left shift keeps the family flag set"
        );
        assert_eq!(decoder.decode(56, 0x0002_0104), Some(InputEdge::Up));
        assert_eq!(decoder.decode(60, 0x0000_0100), Some(InputEdge::Up));
    }

    #[test]
    fn family_only_fallback_covers_synthetic_flags_without_device_bits() {
        let mut decoder = ModifierDecoder::default();
        assert_eq!(decoder.decode(56, 0x0002_0100), Some(InputEdge::Down));
        assert_eq!(decoder.decode(56, 0x0000_0100), Some(InputEdge::Up));
    }

    #[test]
    fn unknown_modifier_keycodes_request_reset() {
        let mut decoder = ModifierDecoder::default();
        assert_eq!(decoder.decode(130, 0x0000_0100), None);
        assert_eq!(decoder.last_flags, 0x0000_0100, "flags still recorded");
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
            cursor.publish(MacCursorPoint::at(f64::from(index), 1.0));
        }
        assert_eq!(cursor.take(), Some(MacCursorPoint::at(9_999.0, 1.0)));
        cursor.close();
        cursor.publish(MacCursorPoint::at(0.0, 0.0));
        let mut diagnostics = PlatformInputDiagnostics::default();
        cursor.merge_diagnostics(&mut diagnostics);
        assert_eq!(diagnostics.cursor_captured, 10_000);
        assert_eq!(diagnostics.cursor_coalesced, 9_999);
        assert_eq!(diagnostics.cursor_consumed, 1);
        assert_eq!(diagnostics.cursor_rejected_after_stop, 1);
        assert_eq!(diagnostics.captured_edges, 0);
    }

    /// A coalesced sample keeps the latest location but must keep every
    /// packet's motion: the accumulated position is only correct if no delta
    /// is dropped between two service slices.
    #[test]
    fn cursor_callback_slot_sums_motion_across_coalesced_samples() {
        let cursor = LatestCursor::default();
        let point = |x: f64, dx: f64| MacCursorPoint {
            x,
            y: 0.0,
            delta: CursorDelta { x: dx, y: 0.0 },
        };
        cursor.publish(point(10.0, 1.0));
        cursor.publish(point(10.0, 2.0));
        cursor.publish(point(10.0, 3.0));
        let taken = cursor.take().expect("a sample");
        assert_eq!(taken.x, 10.0, "the location is the latest reading");
        assert_eq!(
            taken.delta,
            CursorDelta { x: 6.0, y: 0.0 },
            "the motion of every packet is kept even though only the location coalesces"
        );
        assert!(cursor.take().is_none());
    }

    #[test]
    fn live_diagnostics_merge_worker_callback_and_cursor_without_accumulating() {
        let producer = PlatformInputDiagnosticsProducer::default();
        let counters = CallbackCounters::default();
        counters.captured_edges.store(3, Ordering::Relaxed);
        let cursor = LatestCursor::default();
        cursor.publish(MacCursorPoint::at(1.0, 2.0));
        cursor.publish(MacCursorPoint::at(3.0, 4.0));
        assert_eq!(cursor.take(), Some(MacCursorPoint::at(3.0, 4.0)));
        let worker = PlatformInputDiagnostics {
            runtime_queue_overflows: 2,
            recovery_resets: 6,
            ..PlatformInputDiagnostics::default()
        };

        publish_live_diagnostics(&producer, worker, &counters, &cursor);
        publish_live_diagnostics(&producer, worker, &counters, &cursor);
        let live = producer.diagnostics();
        assert_eq!(live.runtime_queue_overflows, 2);
        assert_eq!(live.captured_edges, 3);
        assert_eq!(live.recovery_resets, 6);
        assert_eq!(live.cursor_captured, 2);
        assert_eq!(live.cursor_coalesced, 1);
        assert_eq!(live.cursor_consumed, 1);
    }
}
