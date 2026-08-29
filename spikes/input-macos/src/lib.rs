#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

#[cfg(target_os = "macos")]
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
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
pub enum TapDisableReason {
    Timeout,
    UserInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceLifecycleEvent {
    WillSleep,
    DidWake,
    SessionResigned,
    SessionBecameActive,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceLifecycleInjection {
    Session,
    Sleep,
    Wake,
    All,
}

#[cfg(target_os = "macos")]
impl WorkspaceLifecycleInjection {
    fn events(self) -> &'static [WorkspaceLifecycleEvent] {
        use WorkspaceLifecycleEvent::{DidWake, SessionBecameActive, SessionResigned, WillSleep};

        match self {
            Self::Session => &[SessionResigned, SessionBecameActive],
            Self::Sleep => &[WillSleep],
            Self::Wake => &[DidWake],
            Self::All => &[WillSleep, DidWake, SessionResigned, SessionBecameActive],
        }
    }
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
    MouseDown { button: u8 },
    MouseUp { button: u8 },
    Reset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyCandidateCounters {
    pub duplicate_down: u64,
    pub unmatched_up: u64,
    pub resets: u64,
    pub reset_releases: u64,
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
            CapturedInputEvent::MouseDown { .. } | CapturedInputEvent::MouseUp { .. } => {}
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
        self.counters.reset_releases += self.keys.len() as u64;
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacPressedMouseCandidates {
    buttons: BTreeSet<u8>,
    missing_confirmations: BTreeMap<u8, u8>,
    counters: KeyCandidateCounters,
}

impl MacPressedMouseCandidates {
    pub fn apply_event(&mut self, event: CapturedInputEvent) {
        match event {
            CapturedInputEvent::MouseDown { button } => {
                self.missing_confirmations.remove(&button);
                if !self.buttons.insert(button) {
                    self.counters.duplicate_down += 1;
                }
            }
            CapturedInputEvent::MouseUp { button } => {
                self.missing_confirmations.remove(&button);
                if !self.buttons.remove(&button) {
                    self.counters.unmatched_up += 1;
                }
            }
            CapturedInputEvent::Reset => self.reset(),
            CapturedInputEvent::KeyDown { .. }
            | CapturedInputEvent::KeyUp { .. }
            | CapturedInputEvent::FlagsChanged { .. } => {}
        }
    }

    pub fn reconcile(
        &mut self,
        snapshot: &ButtonReconciliation,
        required_missing_confirmations: u8,
    ) -> Result<CandidateReconciliation, InvalidMissingConfirmations> {
        if required_missing_confirmations == 0 {
            return Err(InvalidMissingConfirmations);
        }
        let checked = self.buttons.len();
        let mut released = 0usize;
        let candidates = self.buttons.iter().copied().collect::<Vec<_>>();
        for button in candidates {
            if snapshot.still_pressed.contains(&button) {
                self.missing_confirmations.remove(&button);
                continue;
            }
            let confirmations = self.missing_confirmations.entry(button).or_insert(0);
            *confirmations = confirmations.saturating_add(1);
            if *confirmations >= required_missing_confirmations {
                self.buttons.remove(&button);
                self.missing_confirmations.remove(&button);
                released += 1;
            }
        }
        self.counters.reconciled_releases += released as u64;
        Ok(CandidateReconciliation {
            checked,
            released,
            still_pressed: self.buttons.len(),
            pending_confirmations: self.missing_confirmations.len(),
        })
    }

    pub fn reset(&mut self) {
        self.counters.reset_releases += self.buttons.len() as u64;
        self.buttons.clear();
        self.missing_confirmations.clear();
        self.counters.resets += 1;
    }

    pub fn buttons(&self) -> &BTreeSet<u8> {
        &self.buttons
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
    pub injected_disables: u64,
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
    pub candidate_reset_releases: u64,
    pub duplicate_down: u64,
    pub unmatched_up: u64,
    pub workspace_observers_registered: u64,
    pub workspace_observers_removed: u64,
    pub workspace_will_sleep: u64,
    pub workspace_did_wake: u64,
    pub workspace_session_resigned: u64,
    pub workspace_session_active: u64,
    pub workspace_lifecycle_resets: u64,
    pub workspace_callback_panics: u64,
    pub workspace_callbacks_ignored_after_close: u64,
    pub synthetic_events_posted: u64,
    pub intentionally_dropped_releases: u64,
    pub pressed_candidates_before_shutdown: u64,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub enum TapProbeError {
    PermissionDenied,
    TapCreateFailed,
    RunLoopSourceFailed,
    SyntheticEventCreateFailed,
    ThreadPanicked,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct TapCounters {
    key_down: AtomicU64,
    key_up: AtomicU64,
    flags_changed: AtomicU64,
    mouse_down: AtomicU64,
    mouse_up: AtomicU64,
    disabled_by_timeout: AtomicU64,
    disabled_by_user: AtomicU64,
    injected_disables: AtomicU64,
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
    candidate_reset_releases: AtomicU64,
    duplicate_down: AtomicU64,
    unmatched_up: AtomicU64,
    workspace_observers_registered: AtomicU64,
    workspace_observers_removed: AtomicU64,
    workspace_will_sleep: AtomicU64,
    workspace_did_wake: AtomicU64,
    workspace_session_resigned: AtomicU64,
    workspace_session_active: AtomicU64,
    workspace_lifecycle_resets: AtomicU64,
    workspace_callback_panics: AtomicU64,
    workspace_callbacks_ignored_after_close: AtomicU64,
    synthetic_events_posted: AtomicU64,
    intentionally_dropped_releases: AtomicU64,
    pressed_candidates_before_shutdown: AtomicU64,
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
            injected_disables: self.injected_disables.load(Ordering::Relaxed),
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
            candidate_reset_releases: self.candidate_reset_releases.load(Ordering::Relaxed),
            duplicate_down: self.duplicate_down.load(Ordering::Relaxed),
            unmatched_up: self.unmatched_up.load(Ordering::Relaxed),
            workspace_observers_registered: self
                .workspace_observers_registered
                .load(Ordering::Relaxed),
            workspace_observers_removed: self.workspace_observers_removed.load(Ordering::Relaxed),
            workspace_will_sleep: self.workspace_will_sleep.load(Ordering::Relaxed),
            workspace_did_wake: self.workspace_did_wake.load(Ordering::Relaxed),
            workspace_session_resigned: self.workspace_session_resigned.load(Ordering::Relaxed),
            workspace_session_active: self.workspace_session_active.load(Ordering::Relaxed),
            workspace_lifecycle_resets: self.workspace_lifecycle_resets.load(Ordering::Relaxed),
            workspace_callback_panics: self.workspace_callback_panics.load(Ordering::Relaxed),
            workspace_callbacks_ignored_after_close: self
                .workspace_callbacks_ignored_after_close
                .load(Ordering::Relaxed),
            synthetic_events_posted: self.synthetic_events_posted.load(Ordering::Relaxed),
            intentionally_dropped_releases: self
                .intentionally_dropped_releases
                .load(Ordering::Relaxed),
            pressed_candidates_before_shutdown: self
                .pressed_candidates_before_shutdown
                .load(Ordering::Relaxed),
        }
    }

    fn record_workspace_event(&self, event: WorkspaceLifecycleEvent) {
        let counter = match event {
            WorkspaceLifecycleEvent::WillSleep => &self.workspace_will_sleep,
            WorkspaceLifecycleEvent::DidWake => &self.workspace_did_wake,
            WorkspaceLifecycleEvent::SessionResigned => &self.workspace_session_resigned,
            WorkspaceLifecycleEvent::SessionBecameActive => &self.workspace_session_active,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(target_os = "macos")]
const TAP_TIMEOUT_PENDING: u8 = 1;
#[cfg(target_os = "macos")]
const TAP_USER_DISABLE_PENDING: u8 = 1 << 1;

#[cfg(target_os = "macos")]
#[derive(Default)]
struct TapDisableSignals(AtomicU8);

#[cfg(target_os = "macos")]
impl TapDisableSignals {
    fn signal(&self, reason: TapDisableReason) {
        let flag = match reason {
            TapDisableReason::Timeout => TAP_TIMEOUT_PENDING,
            TapDisableReason::UserInput => TAP_USER_DISABLE_PENDING,
        };
        self.0.fetch_or(flag, Ordering::Release);
    }

    fn take(&self) -> u8 {
        self.0.swap(0, Ordering::Acquire)
    }
}

#[cfg(target_os = "macos")]
const WORKSPACE_WILL_SLEEP_PENDING: u8 = 1;
#[cfg(target_os = "macos")]
const WORKSPACE_DID_WAKE_PENDING: u8 = 1 << 1;
#[cfg(target_os = "macos")]
const WORKSPACE_SESSION_RESIGNED_PENDING: u8 = 1 << 2;
#[cfg(target_os = "macos")]
const WORKSPACE_SESSION_ACTIVE_PENDING: u8 = 1 << 3;

#[cfg(target_os = "macos")]
impl WorkspaceLifecycleEvent {
    fn pending_bit(self) -> u8 {
        match self {
            Self::WillSleep => WORKSPACE_WILL_SLEEP_PENDING,
            Self::DidWake => WORKSPACE_DID_WAKE_PENDING,
            Self::SessionResigned => WORKSPACE_SESSION_RESIGNED_PENDING,
            Self::SessionBecameActive => WORKSPACE_SESSION_ACTIVE_PENDING,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct WorkspaceLifecycleSignals(AtomicU8);

#[cfg(target_os = "macos")]
impl WorkspaceLifecycleSignals {
    fn signal(&self, event: WorkspaceLifecycleEvent) {
        self.0.fetch_or(event.pending_bit(), Ordering::Release);
    }

    fn take(&self) -> u8 {
        self.0.swap(0, Ordering::Acquire)
    }
}

#[cfg(target_os = "macos")]
fn run_macos_callback_boundary(panic_counter: &AtomicU64, callback: impl FnOnce()) {
    use objc2::rc::autoreleasepool;

    let outcome = catch_unwind(AssertUnwindSafe(|| autoreleasepool(|_| callback())));
    if outcome.is_err() {
        panic_counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(target_os = "macos")]
struct WorkspaceLifecycleObserver {
    center: objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
    tokens: Vec<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
    accepting_callbacks: Arc<AtomicBool>,
    counters: Arc<TapCounters>,
}

#[cfg(target_os = "macos")]
impl WorkspaceLifecycleObserver {
    fn register(signals: Arc<WorkspaceLifecycleSignals>, counters: Arc<TapCounters>) -> Self {
        use block2::RcBlock;
        use objc2_app_kit::{
            NSWorkspace, NSWorkspaceDidWakeNotification,
            NSWorkspaceSessionDidBecomeActiveNotification,
            NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
        };
        use objc2_foundation::NSNotification;
        use std::ptr::NonNull;

        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let accepting_callbacks = Arc::new(AtomicBool::new(true));
        // SAFETY: these are immutable notification-name constants exported by AppKit.
        let registrations = unsafe {
            [
                (
                    NSWorkspaceWillSleepNotification,
                    WorkspaceLifecycleEvent::WillSleep,
                ),
                (
                    NSWorkspaceDidWakeNotification,
                    WorkspaceLifecycleEvent::DidWake,
                ),
                (
                    NSWorkspaceSessionDidResignActiveNotification,
                    WorkspaceLifecycleEvent::SessionResigned,
                ),
                (
                    NSWorkspaceSessionDidBecomeActiveNotification,
                    WorkspaceLifecycleEvent::SessionBecameActive,
                ),
            ]
        };
        let mut tokens = Vec::with_capacity(registrations.len());
        for (name, event) in registrations {
            let callback_signals = Arc::clone(&signals);
            let callback_counters = Arc::clone(&counters);
            let callback_accepting = Arc::clone(&accepting_callbacks);
            let block: RcBlock<dyn Fn(NonNull<NSNotification>)> =
                RcBlock::new(move |_notification: NonNull<NSNotification>| {
                    run_macos_callback_boundary(
                        &callback_counters.workspace_callback_panics,
                        || {
                            if !callback_accepting.load(Ordering::Acquire) {
                                callback_counters
                                    .workspace_callbacks_ignored_after_close
                                    .fetch_add(1, Ordering::Relaxed);
                                return;
                            }
                            callback_counters.record_workspace_event(event);
                            callback_signals.signal(event);
                        },
                    );
                });
            // SAFETY: no object filter is used, the public notification name matches the
            // callback signature, and the block captures only thread-safe atomics.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            tokens.push(token);
        }
        counters
            .workspace_observers_registered
            .store(tokens.len() as u64, Ordering::Relaxed);
        Self {
            center,
            tokens,
            accepting_callbacks,
            counters,
        }
    }

    fn post_for_probe(&self, event: WorkspaceLifecycleEvent) {
        use objc2_app_kit::{
            NSWorkspaceDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
            NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
        };

        // SAFETY: these are immutable notification-name constants exported by AppKit.
        let name = unsafe {
            match event {
                WorkspaceLifecycleEvent::WillSleep => NSWorkspaceWillSleepNotification,
                WorkspaceLifecycleEvent::DidWake => NSWorkspaceDidWakeNotification,
                WorkspaceLifecycleEvent::SessionResigned => {
                    NSWorkspaceSessionDidResignActiveNotification
                }
                WorkspaceLifecycleEvent::SessionBecameActive => {
                    NSWorkspaceSessionDidBecomeActiveNotification
                }
            }
        };
        // SAFETY: the probe posts a public NSWorkspace notification with no object or user info.
        unsafe { self.center.postNotificationName_object(name, None) };
    }

    fn close_sink(&self) {
        self.accepting_callbacks.store(false, Ordering::Release);
    }

    fn unregister(&mut self) {
        use objc2::runtime::AnyObject;

        for token in self.tokens.drain(..) {
            let token_ref: &objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol> =
                &token;
            let observer: &AnyObject = token_ref.as_ref();
            // SAFETY: each token was returned by this notification center and is removed once.
            unsafe { self.center.removeObserver(observer) };
            self.counters
                .workspace_observers_removed
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for WorkspaceLifecycleObserver {
    fn drop(&mut self) {
        self.close_sink();
        self.unregister();
    }
}

#[cfg(target_os = "macos")]
fn consume_workspace_lifecycle_signals(
    signals: &WorkspaceLifecycleSignals,
    pressed_key_candidates: &mut MacPressedKeyCandidates,
    pressed_mouse_candidates: &mut MacPressedMouseCandidates,
    counters: &TapCounters,
) {
    if signals.take() != 0 {
        pressed_key_candidates.reset();
        pressed_mouse_candidates.reset();
        counters
            .workspace_lifecycle_resets
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(target_os = "macos")]
const MAX_RECONCILABLE_MOUSE_BUTTON: i64 = 31;

#[cfg(target_os = "macos")]
fn captured_mouse_button(event: &core_graphics2::event::CGEvent) -> Option<u8> {
    use core_graphics2::event::CGEventField;

    let button = event.get_integer_value_field(CGEventField::MouseEventButtonNumber);
    (0..=MAX_RECONCILABLE_MOUSE_BUTTON)
        .contains(&button)
        .then_some(button as u8)
}

#[cfg(target_os = "macos")]
fn queue_captured_event(
    event: CapturedInputEvent,
    queue: &SharedCaptureQueue,
    counters: &TapCounters,
) {
    match queue.push_with_overflow_reset(event) {
        Ok(()) => {
            counters.queued_events.fetch_add(1, Ordering::Relaxed);
        }
        Err(CaptureQueueError {
            kind: CaptureQueueErrorKind::Full,
            discarded,
            ..
        }) => {
            counters.queue_overflows.fetch_add(1, Ordering::Relaxed);
            counters
                .queue_recovery_resets
                .fetch_add(1, Ordering::Relaxed);
            counters
                .queue_discarded_events
                .fetch_add(discarded as u64, Ordering::Relaxed);
        }
        Err(CaptureQueueError {
            kind: CaptureQueueErrorKind::Closed,
            ..
        }) => {
            counters.queue_closed_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "macos")]
fn record_tap_disable(
    reason: TapDisableReason,
    signals: &TapDisableSignals,
    queue: &SharedCaptureQueue,
    counters: &TapCounters,
) {
    signals.signal(reason);
    match reason {
        TapDisableReason::Timeout => {
            counters.disabled_by_timeout.fetch_add(1, Ordering::Relaxed);
        }
        TapDisableReason::UserInput => {
            counters.disabled_by_user.fetch_add(1, Ordering::Relaxed);
        }
    }
    queue_captured_event(CapturedInputEvent::Reset, queue, counters);
}

#[cfg(target_os = "macos")]
fn run_tap_on_run_loop(
    duration: Duration,
    injected_disable: Option<TapDisableReason>,
    injected_lifecycle: Option<WorkspaceLifecycleInjection>,
    inject_release_loss: bool,
) -> Result<TapProbeReport, TapProbeError> {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use core_graphics2::event::{
        CGEvent, CGEventField, CGEventSource, CGEventSourceStateID, CGEventTap, CGEventTapLocation,
        CGEventTapOptions, CGEventTapPlacement, CGEventType,
    };

    let synthetic_events = if inject_release_loss {
        let source = CGEventSource::new(CGEventSourceStateID::Private)
            .map_err(|_| TapProbeError::SyntheticEventCreateFailed)?;
        let key_down = CGEvent::new_keyboard_event(source.clone(), 0, true)
            .ok_or(TapProbeError::SyntheticEventCreateFailed)?;
        let key_up = CGEvent::new_keyboard_event(source, 0, false)
            .ok_or(TapProbeError::SyntheticEventCreateFailed)?;
        Some((key_down, key_up))
    } else {
        None
    };

    let counters = Arc::new(TapCounters::default());
    let disable_signals = Arc::new(TapDisableSignals::default());
    let workspace_signals = Arc::new(WorkspaceLifecycleSignals::default());
    let mut workspace_observer =
        WorkspaceLifecycleObserver::register(Arc::clone(&workspace_signals), Arc::clone(&counters));
    let ignore_next_user_disable = Arc::new(AtomicBool::new(false));
    let event_queue = SharedCaptureQueue::new(256);
    let callback_queue = event_queue.clone();
    let callback_counters = Arc::clone(&counters);
    let callback_disable_signals = Arc::clone(&disable_signals);
    let callback_ignore_next_user_disable = Arc::clone(&ignore_next_user_disable);
    let drop_next_key_up = Arc::new(AtomicBool::new(inject_release_loss));
    let callback_drop_next_key_up = Arc::clone(&drop_next_key_up);
    let callback = move |_proxy: core_graphics2::event::CGEventTapProxy,
                         event_type: CGEventType,
                         event: &core_graphics2::event::CGEvent| {
        run_macos_callback_boundary(&callback_counters.callback_panics, || {
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
                    if callback_drop_next_key_up.swap(false, Ordering::AcqRel) {
                        callback_counters
                            .intentionally_dropped_releases
                            .fetch_add(1, Ordering::Relaxed);
                        None
                    } else {
                        Some(CapturedInputEvent::KeyUp {
                            key_code: event
                                .get_integer_value_field(CGEventField::KeyboardEventKeycode)
                                .clamp(0, i64::from(u16::MAX))
                                as u16,
                        })
                    }
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
                    captured_mouse_button(event)
                        .map(|button| CapturedInputEvent::MouseDown { button })
                }
                CGEventType::LeftMouseUp
                | CGEventType::RightMouseUp
                | CGEventType::OtherMouseUp => {
                    callback_counters.mouse_up.fetch_add(1, Ordering::Relaxed);
                    captured_mouse_button(event)
                        .map(|button| CapturedInputEvent::MouseUp { button })
                }
                CGEventType::TapDisabledByTimeout => {
                    record_tap_disable(
                        TapDisableReason::Timeout,
                        &callback_disable_signals,
                        &callback_queue,
                        &callback_counters,
                    );
                    None
                }
                CGEventType::TapDisabledByUserInput => {
                    if !callback_ignore_next_user_disable.swap(false, Ordering::AcqRel) {
                        record_tap_disable(
                            TapDisableReason::UserInput,
                            &callback_disable_signals,
                            &callback_queue,
                            &callback_counters,
                        );
                    }
                    None
                }
                _ => None,
            };
            if let Some(captured_event) = captured_event {
                queue_captured_event(captured_event, &callback_queue, &callback_counters);
            }
        });
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
    if let Some((key_down, key_up)) = synthetic_events {
        key_down.post(CGEventTapLocation::SessionEventTap);
        key_up.post(CGEventTapLocation::SessionEventTap);
        counters.synthetic_events_posted.store(2, Ordering::Relaxed);
    }
    let deadline = Instant::now() + duration;
    let reconciliation_interval = Duration::from_millis(250);
    let mut next_reconciliation = Instant::now() + reconciliation_interval;
    let mut pressed_key_candidates = MacPressedKeyCandidates::default();
    let mut pressed_mouse_candidates = MacPressedMouseCandidates::default();
    if let Some(reason) = injected_disable {
        // Seed a missing-KeyUp case so the probe verifies that disable recovery clears state.
        queue_captured_event(
            CapturedInputEvent::KeyDown {
                key_code: 0,
                repeat: false,
            },
            &event_queue,
            &counters,
        );
        if reason == TapDisableReason::Timeout {
            // Manual disable emits a user-disable callback; timeout mode substitutes its reason.
            ignore_next_user_disable.store(true, Ordering::Release);
        }
        tap.enable(false);
        if reason == TapDisableReason::Timeout {
            record_tap_disable(reason, &disable_signals, &event_queue, &counters);
        }
        counters.injected_disables.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(injection) = injected_lifecycle {
        queue_captured_event(
            CapturedInputEvent::KeyDown {
                key_code: 0,
                repeat: false,
            },
            &event_queue,
            &counters,
        );
        for event in injection.events() {
            workspace_observer.post_for_probe(*event);
        }
    }
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
            pressed_key_candidates.apply_event_with(event, reconcile_key_state);
            pressed_mouse_candidates.apply_event(event);
        }
        consume_workspace_lifecycle_signals(
            &workspace_signals,
            &mut pressed_key_candidates,
            &mut pressed_mouse_candidates,
            &counters,
        );
        if disable_signals.take() != 0 {
            ignore_next_user_disable.store(false, Ordering::Release);
            if input_monitoring_preflight() {
                tap.enable(true);
                if tap.is_enabled() {
                    counters.reenabled.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if Instant::now() >= next_reconciliation {
            let key_snapshot = reconcile_pressed_key_codes(pressed_key_candidates.keys());
            pressed_key_candidates
                .reconcile(&key_snapshot, 2)
                .expect("non-zero reconciliation confirmation threshold");
            let button_snapshot =
                reconcile_pressed_mouse_buttons(pressed_mouse_candidates.buttons());
            pressed_mouse_candidates
                .reconcile(&button_snapshot, 2)
                .expect("non-zero reconciliation confirmation threshold");
            counters.reconciliation_runs.fetch_add(1, Ordering::Relaxed);
            next_reconciliation = Instant::now() + reconciliation_interval;
        }
    }
    let finished_enabled = tap.is_enabled();
    event_queue.close();
    tap.enable(false);
    run_loop.remove_source(&source, unsafe { kCFRunLoopDefaultMode });
    consume_workspace_lifecycle_signals(
        &workspace_signals,
        &mut pressed_key_candidates,
        &mut pressed_mouse_candidates,
        &counters,
    );
    workspace_observer.close_sink();
    if injected_lifecycle.is_some() {
        workspace_observer.post_for_probe(WorkspaceLifecycleEvent::DidWake);
    }
    workspace_observer.unregister();
    let drained = event_queue.drain();
    counters
        .consumed_events
        .fetch_add(drained.len() as u64, Ordering::Relaxed);
    for event in drained {
        pressed_key_candidates.apply_event_with(event, reconcile_key_state);
        pressed_mouse_candidates.apply_event(event);
    }
    counters.pressed_candidates_before_shutdown.store(
        (pressed_key_candidates.keys().len() + pressed_mouse_candidates.buttons().len()) as u64,
        Ordering::Relaxed,
    );
    pressed_key_candidates.reset();
    pressed_mouse_candidates.reset();
    let key_counters = pressed_key_candidates.counters();
    let mouse_counters = pressed_mouse_candidates.counters();
    counters.reconciled_releases.store(
        key_counters.reconciled_releases + mouse_counters.reconciled_releases,
        Ordering::Relaxed,
    );
    counters
        .candidate_resets
        .store(key_counters.resets, Ordering::Relaxed);
    counters.candidate_reset_releases.store(
        key_counters.reset_releases + mouse_counters.reset_releases,
        Ordering::Relaxed,
    );
    counters.duplicate_down.store(
        key_counters.duplicate_down + mouse_counters.duplicate_down,
        Ordering::Relaxed,
    );
    counters.unmatched_up.store(
        key_counters.unmatched_up + mouse_counters.unmatched_up,
        Ordering::Relaxed,
    );
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
pub fn run_listen_only_tap(
    duration: Duration,
    injected_disable: Option<TapDisableReason>,
    injected_lifecycle: Option<WorkspaceLifecycleInjection>,
    inject_release_loss: bool,
) -> Result<TapProbeReport, TapProbeError> {
    if !input_monitoring_preflight() {
        return Err(TapProbeError::PermissionDenied);
    }
    thread::Builder::new()
        .name("bongocat-macos-input-tap".to_string())
        .spawn(move || {
            run_tap_on_run_loop(
                duration,
                injected_disable,
                injected_lifecycle,
                inject_release_loss,
            )
        })
        .map_err(|_| TapProbeError::ThreadPanicked)?
        .join()
        .map_err(|_| TapProbeError::ThreadPanicked)?
}

#[cfg(target_os = "macos")]
pub fn reconcile_key_state(key_code: u16) -> bool {
    use core_graphics2::event::{CGEventSource, CGEventSourceStateID};
    CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, key_code)
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    #[link_name = "CGEventSourceButtonState"]
    fn cg_event_source_button_state(state_id: i32, button: u32) -> bool;
}

#[cfg(target_os = "macos")]
pub fn reconcile_mouse_button_state(button: u8) -> bool {
    // SAFETY: state id 0 is kCGEventSourceStateCombinedSessionState. The C
    // CGMouseButton typedef is a u32 button number, and callers only provide
    // callback values validated to the documented 0..=31 range.
    unsafe { cg_event_source_button_state(0, u32::from(button)) }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyReconciliation {
    pub still_pressed: BTreeSet<u16>,
    pub released_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ButtonReconciliation {
    pub still_pressed: BTreeSet<u8>,
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

fn reconcile_pressed_mouse_buttons_with(
    candidates: &BTreeSet<u8>,
    mut is_pressed: impl FnMut(u8) -> bool,
) -> ButtonReconciliation {
    let still_pressed = candidates
        .iter()
        .copied()
        .filter(|button| is_pressed(*button))
        .collect::<BTreeSet<_>>();
    ButtonReconciliation {
        released_count: candidates.len().saturating_sub(still_pressed.len()),
        still_pressed,
    }
}

#[cfg(target_os = "macos")]
pub fn reconcile_pressed_mouse_buttons(candidates: &BTreeSet<u8>) -> ButtonReconciliation {
    reconcile_pressed_mouse_buttons_with(candidates, reconcile_mouse_button_state)
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
    fn mouse_candidate_state_preserves_button_identity_and_recovers_release() {
        let mut candidates = MacPressedMouseCandidates::default();
        candidates.apply_event(CapturedInputEvent::MouseDown { button: 0 });
        candidates.apply_event(CapturedInputEvent::MouseDown { button: 3 });
        candidates.apply_event(CapturedInputEvent::MouseUp { button: 0 });
        assert_eq!(candidates.buttons(), &BTreeSet::from([3]));

        let empty = ButtonReconciliation::default();
        let first = candidates.reconcile(&empty, 2).unwrap();
        assert_eq!(first.released, 0);
        assert_eq!(first.pending_confirmations, 1);
        let second = candidates.reconcile(&empty, 2).unwrap();
        assert_eq!(second.released, 1);
        assert!(candidates.buttons().is_empty());
        assert_eq!(candidates.counters().reconciled_releases, 1);
    }

    #[test]
    fn mouse_reconciliation_queries_only_pressed_buttons() {
        let candidates = BTreeSet::from([0, 2, 4]);
        let mut queried = Vec::new();
        let report = reconcile_pressed_mouse_buttons_with(&candidates, |button| {
            queried.push(button);
            button == 2
        });
        assert_eq!(queried, vec![0, 2, 4]);
        assert_eq!(report.still_pressed, BTreeSet::from([2]));
        assert_eq!(report.released_count, 2);
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
        assert_eq!(candidates.counters().reset_releases, 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tap_disable_signals_coalesce_without_losing_recovery_work() {
        let signals = TapDisableSignals::default();
        signals.signal(TapDisableReason::Timeout);
        signals.signal(TapDisableReason::UserInput);
        assert_eq!(
            signals.take(),
            TAP_TIMEOUT_PENDING | TAP_USER_DISABLE_PENDING
        );
        assert_eq!(signals.take(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_lifecycle_signals_coalesce_without_losing_reset_work() {
        let signals = WorkspaceLifecycleSignals::default();
        signals.signal(WorkspaceLifecycleEvent::WillSleep);
        signals.signal(WorkspaceLifecycleEvent::DidWake);
        signals.signal(WorkspaceLifecycleEvent::SessionResigned);
        signals.signal(WorkspaceLifecycleEvent::SessionBecameActive);
        assert_eq!(
            signals.take(),
            WORKSPACE_WILL_SLEEP_PENDING
                | WORKSPACE_DID_WAKE_PENDING
                | WORKSPACE_SESSION_RESIGNED_PENDING
                | WORKSPACE_SESSION_ACTIVE_PENDING
        );
        assert_eq!(signals.take(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_lifecycle_reset_clears_candidates_once_per_signal_batch() {
        let signals = WorkspaceLifecycleSignals::default();
        let counters = TapCounters::default();
        let mut key_candidates = MacPressedKeyCandidates::default();
        let mut mouse_candidates = MacPressedMouseCandidates::default();
        key_candidates.apply_event_with(
            CapturedInputEvent::KeyDown {
                key_code: 0,
                repeat: false,
            },
            |_| false,
        );
        mouse_candidates.apply_event(CapturedInputEvent::MouseDown { button: 1 });
        signals.signal(WorkspaceLifecycleEvent::WillSleep);
        signals.signal(WorkspaceLifecycleEvent::DidWake);
        consume_workspace_lifecycle_signals(
            &signals,
            &mut key_candidates,
            &mut mouse_candidates,
            &counters,
        );
        assert!(key_candidates.keys().is_empty());
        assert!(mouse_candidates.buttons().is_empty());
        assert_eq!(key_candidates.counters().reset_releases, 1);
        assert_eq!(mouse_candidates.counters().reset_releases, 1);
        assert_eq!(
            counters.workspace_lifecycle_resets.load(Ordering::Relaxed),
            1
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn callback_boundary_contains_panics_and_records_them() {
        let panics = AtomicU64::new(0);
        run_macos_callback_boundary(&panics, || panic!("controlled callback panic"));
        assert_eq!(panics.load(Ordering::Relaxed), 1);
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
            .push_with_overflow_reset(CapturedInputEvent::MouseDown { button: 3 })
            .unwrap();
        queue.close();
        let error = queue
            .push_with_overflow_reset(CapturedInputEvent::MouseUp { button: 3 })
            .unwrap_err();
        assert_eq!(error.kind, CaptureQueueErrorKind::Closed);
        assert_eq!(
            queue.drain(),
            vec![CapturedInputEvent::MouseDown { button: 3 }]
        );
        assert_eq!(queue.diagnostics(), (0, 0, 0));
    }
}
