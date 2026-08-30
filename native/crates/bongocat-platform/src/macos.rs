use crate::{InputPermission, PlatformInputDiagnostics, PlatformInputError};
use bongocat_runtime::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorViewport, InputControl,
    InputEdge, InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource,
    MonotonicMillis, MouseButton, PhysicalKey,
};
use objc2_core_foundation::{CFMachPort, CFRunLoop, CGPoint, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGError, CGEvent, CGEventField, CGEventFlags, CGEventMask,
    CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGGetDisplaysWithPoint, CGMouseButton,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPTURE_QUEUE_CAPACITY: usize = 256;
const RUN_LOOP_SLICE: Duration = Duration::from_millis(10);
const RECONCILIATION_INTERVAL: Duration = Duration::from_millis(250);
const REQUIRED_MISSING_CONFIRMATIONS: u8 = 2;
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
    // SAFETY: `run_input_worker` passes a pointer to a pinned Box that outlives
    // the enabled tap. The worker disables the tap and removes its run-loop
    // source before dropping the Box, so callbacks cannot observe freed state.
    let context = unsafe { &*user_info.cast::<TapCallbackContext>() };
    // SAFETY: CoreGraphics guarantees that the callback event is non-null and
    // valid for the duration of this callback.
    let event_ref = unsafe { event.as_ref() };
    if catch_unwind(AssertUnwindSafe(|| context.capture(event_type, event_ref))).is_err() {
        context
            .counters
            .callback_panics
            .fetch_add(1, Ordering::Relaxed);
        context.accepting.store(false, Ordering::Release);
        context.recovery_requested.store(true, Ordering::Release);
    }
    event.as_ptr()
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
    completion: Receiver<Result<PlatformInputDiagnostics, PlatformInputError>>,
    worker: Option<JoinHandle<()>>,
}

impl MacInputService {
    pub fn start(
        producer: InputProducer,
        cursor_producer: CursorProducer,
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
                    run_input_worker(producer, cursor_producer, worker_stop, startup_sender)
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
    stop: Arc<AtomicBool>,
    startup: SyncSender<Result<(), PlatformInputError>>,
) -> Result<PlatformInputDiagnostics, PlatformInputError> {
    let started = Instant::now();
    let counters = Arc::new(CallbackCounters::default());
    let accepting = Arc::new(AtomicBool::new(true));
    let recovery_requested = Arc::new(AtomicBool::new(false));
    let tap_disabled = Arc::new(AtomicBool::new(false));
    let modifier_keys = Arc::new(Mutex::new(BTreeSet::<u16>::new()));
    let latest_cursor = LatestCursor::default();
    let (capture_sender, capture_receiver) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);

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
    // SAFETY: `callback_context_ptr` points to the stable Box above. It remains
    // alive until after the tap is disabled and detached from this run loop.
    let tap = match unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::TailAppendEventTap,
            CGEventTapOptions::ListenOnly,
            input_event_mask(),
            Some(event_tap_callback),
            callback_context_ptr,
        )
    } {
        Some(tap) => tap,
        None => {
            let _ = startup.send(Err(PlatformInputError::TapCreateFailed));
            return Err(PlatformInputError::TapCreateFailed);
        }
    };
    let source = match CFMachPort::new_run_loop_source(None, Some(&tap), 0) {
        Some(source) => source,
        None => {
            let _ = startup.send(Err(PlatformInputError::RunLoopSourceFailed));
            return Err(PlatformInputError::RunLoopSourceFailed);
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
    let _ = startup.send(Ok(()));

    let mut diagnostics = counters.snapshot();
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

        if recovery_requested.swap(false, Ordering::AcqRel) {
            recovery_pending = true;
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
                        CGEvent::tap_enable(&tap, true);
                        diagnostics.tap_restarts = diagnostics.tap_restarts.saturating_add(1);
                        tap_restart_pending = false;
                    }
                    accepting.store(true, Ordering::Release);
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
            modifier_keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            recovery_pending = true;
            tap_restart_pending = true;
            continue;
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
    CGEvent::tap_enable(&tap, false);
    run_loop.remove_source(Some(&source), Some(mode));
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
    diagnostics.clean_shutdown = publish_final_reset(&producer, started, &mut diagnostics);
    merge_callback_diagnostics(&mut diagnostics, &counters);
    latest_cursor.merge_diagnostics(&mut diagnostics);
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

    #[test]
    fn mac_key_codes_map_to_usb_hid_usages() {
        assert_eq!(map_key_code(0), Some(PhysicalKey::KEY_A));
        assert_eq!(map_key_code(55).map(PhysicalKey::hid_usage), Some(0xe3));
        assert_eq!(map_key_code(60).map(PhysicalKey::hid_usage), Some(0xe5));
        assert_eq!(map_key_code(123).map(PhysicalKey::hid_usage), Some(0x50));
        assert_eq!(map_key_code(124).map(PhysicalKey::hid_usage), Some(0x4f));
        assert_eq!(map_key_code(u16::MAX), None);
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
}
