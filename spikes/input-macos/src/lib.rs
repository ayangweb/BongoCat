#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

#[cfg(target_os = "macos")]
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Unknown,
    Denied,
    Granted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapState {
    Stopped,
    Running,
    Disabled,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAction {
    RequestPermission,
    StartTap,
    RestartTap,
    ResetPressedState,
    ShowPermissionGuidance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureEvent {
    PermissionChecked(PermissionState),
    PermissionChanged(PermissionState),
    TapStarted,
    TapDisabled,
    TapTimedOut,
    TapStopped,
    SessionReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureDecision {
    pub permission: PermissionState,
    pub tap: TapState,
    pub reset_pressed_state: bool,
    pub action: Option<CaptureAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacCaptureLifecycle {
    permission: PermissionState,
    tap: TapState,
    reset_pressed_state: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturedInputEvent {
    KeyDown { key_code: u16, repeat: bool },
    KeyUp { key_code: u16 },
    FlagsChanged { key_code: u16 },
    MouseDown,
    MouseUp,
    Reset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyCandidateCounters {
    pub duplicate_down: u64,
    pub unmatched_up: u64,
    pub resets: u64,
    pub reconciled_releases: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CandidateReconciliation {
    pub checked: usize,
    pub released: usize,
    pub still_pressed: usize,
    pub pending_confirmations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMissingConfirmations;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacPressedKeyCandidates {
    keys: BTreeSet<u16>,
    missing_confirmations: BTreeMap<u16, u8>,
    counters: KeyCandidateCounters,
}

impl MacPressedKeyCandidates {
    pub fn apply_event_with(
        &mut self,
        event: CapturedInputEvent,
        mut is_pressed: impl FnMut(u16) -> bool,
    ) {
        match event {
            CapturedInputEvent::KeyDown { key_code, .. } => {
                self.missing_confirmations.remove(&key_code);
                if !self.keys.insert(key_code) {
                    self.counters.duplicate_down += 1;
                }
            }
            CapturedInputEvent::KeyUp { key_code } => {
                self.missing_confirmations.remove(&key_code);
                if !self.keys.remove(&key_code) {
                    self.counters.unmatched_up += 1;
                }
            }
            CapturedInputEvent::FlagsChanged { key_code } => {
                self.missing_confirmations.remove(&key_code);
                if is_pressed(key_code) {
                    if !self.keys.insert(key_code) {
                        self.counters.duplicate_down += 1;
                    }
                } else if !self.keys.remove(&key_code) {
                    self.counters.unmatched_up += 1;
                }
            }
            CapturedInputEvent::Reset => self.reset(),
            CapturedInputEvent::MouseDown | CapturedInputEvent::MouseUp => {}
        }
    }

    pub fn reconcile(
        &mut self,
        snapshot: &KeyReconciliation,
        required_missing_confirmations: u8,
    ) -> Result<CandidateReconciliation, InvalidMissingConfirmations> {
        if required_missing_confirmations == 0 {
            return Err(InvalidMissingConfirmations);
        }
        let checked = self.keys.len();
        let mut released = 0usize;
        let candidates = self.keys.iter().copied().collect::<Vec<_>>();
        for key_code in candidates {
            if snapshot.still_pressed.contains(&key_code) {
                self.missing_confirmations.remove(&key_code);
                continue;
            }
            let confirmations = self.missing_confirmations.entry(key_code).or_insert(0);
            *confirmations = confirmations.saturating_add(1);
            if *confirmations >= required_missing_confirmations {
                self.keys.remove(&key_code);
                self.missing_confirmations.remove(&key_code);
                released += 1;
            }
        }
        self.counters.reconciled_releases += released as u64;
        Ok(CandidateReconciliation {
            checked,
            released,
            still_pressed: self.keys.len(),
            pending_confirmations: self.missing_confirmations.len(),
        })
    }

    pub fn reset(&mut self) {
        self.keys.clear();
        self.missing_confirmations.clear();
        self.counters.resets += 1;
    }

    pub fn keys(&self) -> &BTreeSet<u16> {
        &self.keys
    }

    pub fn counters(&self) -> KeyCandidateCounters {
        self.counters
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureQueueErrorKind {
    Full,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CaptureQueueError {
    pub kind: CaptureQueueErrorKind,
    pub event: CapturedInputEvent,
    pub discarded: usize,
}

#[derive(Debug)]
pub struct CaptureQueue {
    capacity: usize,
    events: VecDeque<CapturedInputEvent>,
    closed: bool,
    overflow_count: u64,
    recovery_reset_count: u64,
    recovery_discard_count: u64,
}

impl CaptureQueue {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "capture queue capacity must be positive");
        Self {
            capacity,
            events: VecDeque::with_capacity(capacity),
            closed: false,
            overflow_count: 0,
            recovery_reset_count: 0,
            recovery_discard_count: 0,
        }
    }

    pub fn push(&mut self, event: CapturedInputEvent) -> Result<(), CaptureQueueError> {
        if self.closed {
            return Err(CaptureQueueError {
                kind: CaptureQueueErrorKind::Closed,
                event,
                discarded: 0,
            });
        }
        if self.events.len() == self.capacity {
            return Err(CaptureQueueError {
                kind: CaptureQueueErrorKind::Full,
                event,
                discarded: 0,
            });
        }
        self.events.push_back(event);
        Ok(())
    }

    pub fn push_with_overflow_reset(
        &mut self,
        event: CapturedInputEvent,
    ) -> Result<(), CaptureQueueError> {
        if self.closed {
            return Err(CaptureQueueError {
                kind: CaptureQueueErrorKind::Closed,
                event,
                discarded: 0,
            });
        }
        if self.events.len() == self.capacity {
            let discarded = self.events.len();
            self.overflow_count += 1;
            self.recovery_reset_count += 1;
            self.recovery_discard_count += discarded as u64;
            self.events.clear();
            self.events.push_back(CapturedInputEvent::Reset);
            return Err(CaptureQueueError {
                kind: CaptureQueueErrorKind::Full,
                event,
                discarded,
            });
        }
        self.events.push_back(event);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<CapturedInputEvent> {
        self.events.pop_front()
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
    }

    pub fn recovery_reset_count(&self) -> u64 {
        self.recovery_reset_count
    }

    pub fn recovery_discard_count(&self) -> u64 {
        self.recovery_discard_count
    }
}

#[derive(Clone, Debug)]
struct SharedCaptureQueue(Arc<Mutex<CaptureQueue>>);

impl SharedCaptureQueue {
    fn new(capacity: usize) -> Self {
        Self(Arc::new(Mutex::new(CaptureQueue::with_capacity(capacity))))
    }

    fn push_with_overflow_reset(&self, event: CapturedInputEvent) -> Result<(), CaptureQueueError> {
        let mut queue = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.push_with_overflow_reset(event)
    }

    fn drain(&self) -> Vec<CapturedInputEvent> {
        let mut queue = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut drained = Vec::with_capacity(queue.len());
        while let Some(event) = queue.pop() {
            drained.push(event);
        }
        drained
    }

    fn close(&self) {
        let mut queue = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.close();
    }

    fn diagnostics(&self) -> (u64, u64, u64) {
        let queue = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            queue.overflow_count(),
            queue.recovery_reset_count(),
            queue.recovery_discard_count(),
        )
    }
}

impl Default for MacCaptureLifecycle {
    fn default() -> Self {
        Self {
            permission: PermissionState::Unknown,
            tap: TapState::Stopped,
            reset_pressed_state: false,
        }
    }
}

impl MacCaptureLifecycle {
    pub fn permission(&self) -> PermissionState {
        self.permission
    }

    pub fn tap(&self) -> TapState {
        self.tap
    }

    pub fn take_reset_signal(&mut self) -> bool {
        std::mem::take(&mut self.reset_pressed_state)
    }

    pub fn apply(&mut self, event: CaptureEvent) -> CaptureDecision {
        let mut action = None;
        match event {
            CaptureEvent::PermissionChecked(permission)
            | CaptureEvent::PermissionChanged(permission) => {
                let was_granted = self.permission == PermissionState::Granted;
                self.permission = permission;
                if permission != PermissionState::Granted {
                    self.tap = TapState::Stopped;
                    self.reset_pressed_state = true;
                    action = Some(CaptureAction::ShowPermissionGuidance);
                } else if !was_granted {
                    action = Some(CaptureAction::StartTap);
                }
            }
            CaptureEvent::TapStarted => {
                self.tap = TapState::Running;
            }
            CaptureEvent::TapDisabled | CaptureEvent::TapTimedOut => {
                self.tap = match event {
                    CaptureEvent::TapDisabled => TapState::Disabled,
                    CaptureEvent::TapTimedOut => TapState::TimedOut,
                    _ => unreachable!(),
                };
                self.reset_pressed_state = true;
                action = (self.permission == PermissionState::Granted)
                    .then_some(CaptureAction::RestartTap);
            }
            CaptureEvent::TapStopped => {
                self.tap = TapState::Stopped;
                self.reset_pressed_state = true;
            }
            CaptureEvent::SessionReset => {
                self.tap = TapState::Stopped;
                self.reset_pressed_state = true;
                action = (self.permission == PermissionState::Granted)
                    .then_some(CaptureAction::RestartTap);
            }
        }
        CaptureDecision {
            permission: self.permission,
            tap: self.tap,
            reset_pressed_state: self.reset_pressed_state,
            action,
        }
    }
}

#[cfg(target_os = "macos")]
pub fn input_monitoring_preflight() -> bool {
    // The CoreGraphics API is the narrow platform boundary. It does not
    // request access, so normal startup remains free of permission prompts.
    unsafe { core_graphics2::event::CGPreflightListenEventAccess() }
}

#[cfg(target_os = "macos")]
pub fn request_input_monitoring_access() -> bool {
    // Callers must only invoke this from an explicit user action.
    unsafe { core_graphics2::event::CGRequestListenEventAccess() }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TapProbeReport {
    pub started: bool,
    pub finished_enabled: bool,
    pub key_down: u64,
    pub key_up: u64,
    pub flags_changed: u64,
    pub mouse_down: u64,
    pub mouse_up: u64,
    pub disabled_by_timeout: u64,
    pub disabled_by_user: u64,
    pub reenabled: u64,
    pub callback_panics: u64,
    pub queued_events: u64,
    pub consumed_events: u64,
    pub queue_overflows: u64,
    pub queue_recovery_resets: u64,
    pub queue_discarded_events: u64,
    pub queue_closed_events: u64,
    pub reconciliation_runs: u64,
    pub reconciled_releases: u64,
    pub candidate_resets: u64,
    pub duplicate_down: u64,
    pub unmatched_up: u64,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub enum TapProbeError {
    PermissionDenied,
    TapCreateFailed,
    RunLoopSourceFailed,
    ThreadPanicked,
}

#[cfg(target_os = "macos")]
struct TapCounters {
    key_down: AtomicU64,
    key_up: AtomicU64,
    flags_changed: AtomicU64,
    mouse_down: AtomicU64,
    mouse_up: AtomicU64,
    disabled_by_timeout: AtomicU64,
    disabled_by_user: AtomicU64,
    reenabled: AtomicU64,
    callback_panics: AtomicU64,
    queued_events: AtomicU64,
    consumed_events: AtomicU64,
    queue_overflows: AtomicU64,
    queue_recovery_resets: AtomicU64,
    queue_discarded_events: AtomicU64,
    queue_closed_events: AtomicU64,
    reconciliation_runs: AtomicU64,
    reconciled_releases: AtomicU64,
    candidate_resets: AtomicU64,
    duplicate_down: AtomicU64,
    unmatched_up: AtomicU64,
}

#[cfg(target_os = "macos")]
impl TapCounters {
    fn snapshot(&self, started: bool, finished_enabled: bool) -> TapProbeReport {
        TapProbeReport {
            started,
            finished_enabled,
            key_down: self.key_down.load(Ordering::Relaxed),
            key_up: self.key_up.load(Ordering::Relaxed),
            flags_changed: self.flags_changed.load(Ordering::Relaxed),
            mouse_down: self.mouse_down.load(Ordering::Relaxed),
            mouse_up: self.mouse_up.load(Ordering::Relaxed),
            disabled_by_timeout: self.disabled_by_timeout.load(Ordering::Relaxed),
            disabled_by_user: self.disabled_by_user.load(Ordering::Relaxed),
            reenabled: self.reenabled.load(Ordering::Relaxed),
            callback_panics: self.callback_panics.load(Ordering::Relaxed),
            queued_events: self.queued_events.load(Ordering::Relaxed),
            consumed_events: self.consumed_events.load(Ordering::Relaxed),
            queue_overflows: self.queue_overflows.load(Ordering::Relaxed),
            queue_recovery_resets: self.queue_recovery_resets.load(Ordering::Relaxed),
            queue_discarded_events: self.queue_discarded_events.load(Ordering::Relaxed),
            queue_closed_events: self.queue_closed_events.load(Ordering::Relaxed),
            reconciliation_runs: self.reconciliation_runs.load(Ordering::Relaxed),
            reconciled_releases: self.reconciled_releases.load(Ordering::Relaxed),
            candidate_resets: self.candidate_resets.load(Ordering::Relaxed),
            duplicate_down: self.duplicate_down.load(Ordering::Relaxed),
            unmatched_up: self.unmatched_up.load(Ordering::Relaxed),
        }
    }
}

#[cfg(target_os = "macos")]
fn run_tap_on_run_loop(duration: Duration) -> Result<TapProbeReport, TapProbeError> {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use core_graphics2::event::{
        CGEventField, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType,
    };

    let counters = Arc::new(TapCounters {
        key_down: AtomicU64::new(0),
        key_up: AtomicU64::new(0),
        flags_changed: AtomicU64::new(0),
        mouse_down: AtomicU64::new(0),
        mouse_up: AtomicU64::new(0),
        disabled_by_timeout: AtomicU64::new(0),
        disabled_by_user: AtomicU64::new(0),
        reenabled: AtomicU64::new(0),
        callback_panics: AtomicU64::new(0),
        queued_events: AtomicU64::new(0),
        consumed_events: AtomicU64::new(0),
        queue_overflows: AtomicU64::new(0),
        queue_recovery_resets: AtomicU64::new(0),
        queue_discarded_events: AtomicU64::new(0),
        queue_closed_events: AtomicU64::new(0),
        reconciliation_runs: AtomicU64::new(0),
        reconciled_releases: AtomicU64::new(0),
        candidate_resets: AtomicU64::new(0),
        duplicate_down: AtomicU64::new(0),
        unmatched_up: AtomicU64::new(0),
    });
    let (disabled_tx, disabled_rx) = mpsc::sync_channel::<()>(8);
    let event_queue = SharedCaptureQueue::new(256);
    let callback_queue = event_queue.clone();
    let callback_counters = Arc::clone(&counters);
    let callback = move |_proxy: core_graphics2::event::CGEventTapProxy,
                         event_type: CGEventType,
                         event: &core_graphics2::event::CGEvent| {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let captured_event = match event_type {
                CGEventType::KeyDown => {
                    callback_counters.key_down.fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::KeyDown {
                        key_code: event
                            .get_integer_value_field(CGEventField::KeyboardEventKeycode)
                            .clamp(0, i64::from(u16::MAX)) as u16,
                        repeat: event
                            .get_integer_value_field(CGEventField::KeyboardEventAutorepeat)
                            != 0,
                    })
                }
                CGEventType::KeyUp => {
                    callback_counters.key_up.fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::KeyUp {
                        key_code: event
                            .get_integer_value_field(CGEventField::KeyboardEventKeycode)
                            .clamp(0, i64::from(u16::MAX)) as u16,
                    })
                }
                CGEventType::FlagsChanged => {
                    callback_counters
                        .flags_changed
                        .fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::FlagsChanged {
                        key_code: event
                            .get_integer_value_field(CGEventField::KeyboardEventKeycode)
                            .clamp(0, i64::from(u16::MAX)) as u16,
                    })
                }
                CGEventType::LeftMouseDown
                | CGEventType::RightMouseDown
                | CGEventType::OtherMouseDown => {
                    callback_counters.mouse_down.fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::MouseDown)
                }
                CGEventType::LeftMouseUp
                | CGEventType::RightMouseUp
                | CGEventType::OtherMouseUp => {
                    callback_counters.mouse_up.fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::MouseUp)
                }
                CGEventType::TapDisabledByTimeout => {
                    let _ = disabled_tx.try_send(());
                    callback_counters
                        .disabled_by_timeout
                        .fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::Reset)
                }
                CGEventType::TapDisabledByUserInput => {
                    let _ = disabled_tx.try_send(());
                    callback_counters
                        .disabled_by_user
                        .fetch_add(1, Ordering::Relaxed);
                    Some(CapturedInputEvent::Reset)
                }
                _ => None,
            };
            if let Some(captured_event) = captured_event {
                match callback_queue.push_with_overflow_reset(captured_event) {
                    Ok(()) => {
                        callback_counters
                            .queued_events
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    Err(CaptureQueueError {
                        kind: CaptureQueueErrorKind::Full,
                        discarded,
                        ..
                    }) => {
                        callback_counters
                            .queue_overflows
                            .fetch_add(1, Ordering::Relaxed);
                        callback_counters
                            .queue_recovery_resets
                            .fetch_add(1, Ordering::Relaxed);
                        callback_counters
                            .queue_discarded_events
                            .fetch_add(discarded as u64, Ordering::Relaxed);
                    }
                    Err(CaptureQueueError {
                        kind: CaptureQueueErrorKind::Closed,
                        ..
                    }) => {
                        callback_counters
                            .queue_closed_events
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
        if outcome.is_err() {
            callback_counters
                .callback_panics
                .fetch_add(1, Ordering::Relaxed);
        }
        None
    };

    let tap_result = catch_unwind(AssertUnwindSafe(|| {
        CGEventTap::new(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::TailAppendEventTap,
            CGEventTapOptions::ListenOnly,
            vec![
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ],
            callback,
        )
    }));
    let tap = match tap_result {
        Ok(result) => result.map_err(|_| TapProbeError::TapCreateFailed)?,
        Err(_) => return Err(TapProbeError::TapCreateFailed),
    };
    let source = tap
        .mach_port
        .create_runloop_source(0)
        .map_err(|_| TapProbeError::RunLoopSourceFailed)?;
    let run_loop = CFRunLoop::get_current();
    // SAFETY: the run-loop mode is the process-owned constant supplied by CoreFoundation.
    run_loop.add_source(&source, unsafe { kCFRunLoopDefaultMode });
    tap.enable(true);
    let deadline = Instant::now() + duration;
    let reconciliation_interval = Duration::from_millis(250);
    let mut next_reconciliation = Instant::now() + reconciliation_interval;
    let mut pressed_candidates = MacPressedKeyCandidates::default();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = remaining.min(Duration::from_millis(20));
        // SAFETY: the run-loop mode remains valid for the duration of this loop.
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, slice, true);
        let drained = event_queue.drain();
        counters
            .consumed_events
            .fetch_add(drained.len() as u64, Ordering::Relaxed);
        for event in drained {
            pressed_candidates.apply_event_with(event, reconcile_key_state);
        }
        while let Ok(()) = disabled_rx.try_recv() {
            tap.enable(true);
            counters.reenabled.fetch_add(1, Ordering::Relaxed);
        }
        if Instant::now() >= next_reconciliation {
            let snapshot = reconcile_pressed_key_codes(pressed_candidates.keys());
            pressed_candidates
                .reconcile(&snapshot, 2)
                .expect("non-zero reconciliation confirmation threshold");
            counters.reconciliation_runs.fetch_add(1, Ordering::Relaxed);
            next_reconciliation = Instant::now() + reconciliation_interval;
        }
    }
    let finished_enabled = tap.is_enabled();
    tap.enable(false);
    run_loop.remove_source(&source, unsafe { kCFRunLoopDefaultMode });
    event_queue.close();
    let drained = event_queue.drain();
    counters
        .consumed_events
        .fetch_add(drained.len() as u64, Ordering::Relaxed);
    for event in drained {
        pressed_candidates.apply_event_with(event, reconcile_key_state);
    }
    pressed_candidates.reset();
    let candidate_counters = pressed_candidates.counters();
    counters
        .reconciled_releases
        .store(candidate_counters.reconciled_releases, Ordering::Relaxed);
    counters
        .candidate_resets
        .store(candidate_counters.resets, Ordering::Relaxed);
    counters
        .duplicate_down
        .store(candidate_counters.duplicate_down, Ordering::Relaxed);
    counters
        .unmatched_up
        .store(candidate_counters.unmatched_up, Ordering::Relaxed);
    let (queue_overflows, queue_recovery_resets, queue_discarded_events) =
        event_queue.diagnostics();
    counters
        .queue_overflows
        .store(queue_overflows, Ordering::Relaxed);
    counters
        .queue_recovery_resets
        .store(queue_recovery_resets, Ordering::Relaxed);
    counters
        .queue_discarded_events
        .store(queue_discarded_events, Ordering::Relaxed);
    Ok(counters.snapshot(true, finished_enabled))
}

#[cfg(target_os = "macos")]
pub fn run_listen_only_tap(duration: Duration) -> Result<TapProbeReport, TapProbeError> {
    if !input_monitoring_preflight() {
        return Err(TapProbeError::PermissionDenied);
    }
    thread::Builder::new()
        .name("bongocat-macos-input-tap".to_string())
        .spawn(move || run_tap_on_run_loop(duration))
        .map_err(|_| TapProbeError::ThreadPanicked)?
        .join()
        .map_err(|_| TapProbeError::ThreadPanicked)?
}

#[cfg(target_os = "macos")]
pub fn reconcile_key_state(key_code: u16) -> bool {
    use core_graphics2::event::{CGEventSource, CGEventSourceStateID};
    CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, key_code)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyReconciliation {
    pub still_pressed: BTreeSet<u16>,
    pub released_count: usize,
}

fn reconcile_pressed_key_codes_with(
    candidates: &BTreeSet<u16>,
    mut is_pressed: impl FnMut(u16) -> bool,
) -> KeyReconciliation {
    let still_pressed = candidates
        .iter()
        .copied()
        .filter(|key_code| is_pressed(*key_code))
        .collect::<BTreeSet<_>>();
    KeyReconciliation {
        released_count: candidates.len().saturating_sub(still_pressed.len()),
        still_pressed,
    }
}

#[cfg(target_os = "macos")]
pub fn reconcile_pressed_key_codes(candidates: &BTreeSet<u16>) -> KeyReconciliation {
    reconcile_pressed_key_codes_with(candidates, reconcile_key_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_permission_stops_tap_resets_pressed_state_and_guides_user() {
        let mut lifecycle = MacCaptureLifecycle::default();
        let decision = lifecycle.apply(CaptureEvent::PermissionChecked(PermissionState::Denied));
        assert_eq!(decision.tap, TapState::Stopped);
        assert!(decision.reset_pressed_state);
        assert_eq!(decision.action, Some(CaptureAction::ShowPermissionGuidance));
        assert!(lifecycle.take_reset_signal());
    }

    #[test]
    fn grant_starts_tap_and_timeout_restarts_after_reset() {
        let mut lifecycle = MacCaptureLifecycle::default();
        assert_eq!(
            lifecycle
                .apply(CaptureEvent::PermissionChanged(PermissionState::Granted))
                .action,
            Some(CaptureAction::StartTap)
        );
        lifecycle.apply(CaptureEvent::TapStarted);
        let decision = lifecycle.apply(CaptureEvent::TapTimedOut);
        assert_eq!(decision.tap, TapState::TimedOut);
        assert_eq!(decision.action, Some(CaptureAction::RestartTap));
        assert!(lifecycle.take_reset_signal());
        assert!(!lifecycle.take_reset_signal());
    }

    #[test]
    fn permission_revocation_does_not_restart_tap_until_granted_again() {
        let mut lifecycle = MacCaptureLifecycle::default();
        lifecycle.apply(CaptureEvent::PermissionChanged(PermissionState::Granted));
        lifecycle.apply(CaptureEvent::TapStarted);
        let denied = lifecycle.apply(CaptureEvent::PermissionChanged(PermissionState::Denied));
        assert_eq!(denied.action, Some(CaptureAction::ShowPermissionGuidance));
        assert_eq!(lifecycle.tap(), TapState::Stopped);
        assert_eq!(
            lifecycle
                .apply(CaptureEvent::PermissionChanged(PermissionState::Granted))
                .action,
            Some(CaptureAction::StartTap)
        );
    }

    #[test]
    fn session_reset_restarts_only_when_permission_is_available() {
        let mut lifecycle = MacCaptureLifecycle::default();
        assert_eq!(lifecycle.apply(CaptureEvent::SessionReset).action, None);
        lifecycle.apply(CaptureEvent::PermissionChanged(PermissionState::Granted));
        assert_eq!(
            lifecycle.apply(CaptureEvent::SessionReset).action,
            Some(CaptureAction::RestartTap)
        );
    }

    #[test]
    fn reconciliation_keeps_confirmed_keys_and_counts_releases() {
        let candidates = BTreeSet::from([0, 1, 2, 3]);
        let report = reconcile_pressed_key_codes_with(&candidates, |key_code| key_code % 2 == 0);
        assert_eq!(report.still_pressed, BTreeSet::from([0, 2]));
        assert_eq!(report.released_count, 2);
    }

    #[test]
    fn reconciliation_does_not_query_keys_outside_the_pressed_set() {
        let candidates = BTreeSet::from([12, 13]);
        let mut queried = Vec::new();
        let report = reconcile_pressed_key_codes_with(&candidates, |key_code| {
            queried.push(key_code);
            false
        });
        assert_eq!(queried, vec![12, 13]);
        assert!(report.still_pressed.is_empty());
        assert_eq!(report.released_count, 2);
    }

    #[test]
    fn candidate_state_tracks_key_and_modifier_edges() {
        let mut candidates = MacPressedKeyCandidates::default();
        candidates.apply_event_with(
            CapturedInputEvent::KeyDown {
                key_code: 0,
                repeat: false,
            },
            |_| false,
        );
        candidates.apply_event_with(CapturedInputEvent::FlagsChanged { key_code: 55 }, |_| true);
        assert_eq!(candidates.keys(), &BTreeSet::from([0, 55]));
        candidates.apply_event_with(CapturedInputEvent::KeyUp { key_code: 0 }, |_| false);
        candidates.apply_event_with(CapturedInputEvent::FlagsChanged { key_code: 55 }, |_| false);
        assert!(candidates.keys().is_empty());
    }

    #[test]
    fn candidate_state_requires_two_missing_snapshots_before_release() {
        let mut candidates = MacPressedKeyCandidates::default();
        candidates.apply_event_with(
            CapturedInputEvent::KeyDown {
                key_code: 0,
                repeat: false,
            },
            |_| false,
        );
        let empty = KeyReconciliation {
            still_pressed: BTreeSet::new(),
            released_count: 1,
        };
        let first = candidates.reconcile(&empty, 2).unwrap();
        assert_eq!(first.released, 0);
        assert_eq!(first.pending_confirmations, 1);
        let second = candidates.reconcile(&empty, 2).unwrap();
        assert_eq!(second.released, 1);
        assert!(candidates.keys().is_empty());
        assert_eq!(candidates.counters().reconciled_releases, 1);
    }

    #[test]
    fn confirmed_key_cancels_pending_release_and_reset_clears_state() {
        let mut candidates = MacPressedKeyCandidates::default();
        candidates.apply_event_with(
            CapturedInputEvent::KeyDown {
                key_code: 12,
                repeat: false,
            },
            |_| false,
        );
        candidates
            .reconcile(&KeyReconciliation::default(), 2)
            .unwrap();
        let held = KeyReconciliation {
            still_pressed: BTreeSet::from([12]),
            released_count: 0,
        };
        let report = candidates.reconcile(&held, 2).unwrap();
        assert_eq!(report.pending_confirmations, 0);
        candidates.apply_event_with(CapturedInputEvent::Reset, |_| false);
        assert!(candidates.keys().is_empty());
        assert_eq!(candidates.counters().resets, 1);
    }

    #[test]
    fn candidate_reconciliation_rejects_zero_confirmation_threshold() {
        let mut candidates = MacPressedKeyCandidates::default();
        assert_eq!(
            candidates.reconcile(&KeyReconciliation::default(), 0),
            Err(InvalidMissingConfirmations)
        );
    }

    #[test]
    fn capture_queue_keeps_edge_order_and_releases_items_after_close() {
        let mut queue = CaptureQueue::with_capacity(3);
        queue
            .push(CapturedInputEvent::KeyDown {
                key_code: 30,
                repeat: false,
            })
            .unwrap();
        queue
            .push(CapturedInputEvent::KeyUp { key_code: 30 })
            .unwrap();
        queue.close();
        assert_eq!(
            queue.pop(),
            Some(CapturedInputEvent::KeyDown {
                key_code: 30,
                repeat: false,
            })
        );
        assert_eq!(
            queue.pop(),
            Some(CapturedInputEvent::KeyUp { key_code: 30 })
        );
        assert!(queue.is_empty());
        let error = queue
            .push(CapturedInputEvent::FlagsChanged { key_code: 55 })
            .unwrap_err();
        assert_eq!(error.kind, CaptureQueueErrorKind::Closed);
        assert_eq!(error.discarded, 0);
    }

    #[test]
    fn capture_queue_overflow_injects_reset_and_reports_discarded_edges() {
        let mut queue = CaptureQueue::with_capacity(2);
        queue
            .push(CapturedInputEvent::KeyDown {
                key_code: 30,
                repeat: false,
            })
            .unwrap();
        queue
            .push(CapturedInputEvent::KeyDown {
                key_code: 31,
                repeat: true,
            })
            .unwrap();
        let error = queue
            .push_with_overflow_reset(CapturedInputEvent::KeyUp { key_code: 30 })
            .unwrap_err();
        assert_eq!(error.kind, CaptureQueueErrorKind::Full);
        assert_eq!(error.discarded, 2);
        assert_eq!(queue.pop(), Some(CapturedInputEvent::Reset));
        assert!(queue.is_empty());
        assert_eq!(queue.overflow_count(), 1);
        assert_eq!(queue.recovery_reset_count(), 1);
        assert_eq!(queue.recovery_discard_count(), 2);
    }

    #[test]
    fn shared_capture_queue_closes_callback_sink_without_poisoning_consumer() {
        let queue = SharedCaptureQueue::new(2);
        queue
            .push_with_overflow_reset(CapturedInputEvent::MouseDown)
            .unwrap();
        queue.close();
        let error = queue
            .push_with_overflow_reset(CapturedInputEvent::MouseUp)
            .unwrap_err();
        assert_eq!(error.kind, CaptureQueueErrorKind::Closed);
        assert_eq!(queue.drain(), vec![CapturedInputEvent::MouseDown]);
        assert_eq!(queue.diagnostics(), (0, 0, 0));
    }
}
