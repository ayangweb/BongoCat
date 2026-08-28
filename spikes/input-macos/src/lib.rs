#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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
        }
    }
}

#[cfg(target_os = "macos")]
fn run_tap_on_run_loop(duration: Duration) -> Result<TapProbeReport, TapProbeError> {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use core_graphics2::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
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
    });
    let (disabled_tx, disabled_rx) = mpsc::sync_channel::<()>(8);
    let callback_counters = Arc::clone(&counters);
    let callback = move |_proxy: core_graphics2::event::CGEventTapProxy,
                         event_type: CGEventType,
                         _event: &core_graphics2::event::CGEvent| {
        let outcome = catch_unwind(AssertUnwindSafe(|| match event_type {
            CGEventType::KeyDown => callback_counters.key_down.fetch_add(1, Ordering::Relaxed),
            CGEventType::KeyUp => callback_counters.key_up.fetch_add(1, Ordering::Relaxed),
            CGEventType::FlagsChanged => callback_counters
                .flags_changed
                .fetch_add(1, Ordering::Relaxed),
            CGEventType::LeftMouseDown
            | CGEventType::RightMouseDown
            | CGEventType::OtherMouseDown => {
                callback_counters.mouse_down.fetch_add(1, Ordering::Relaxed)
            }
            CGEventType::LeftMouseUp | CGEventType::RightMouseUp | CGEventType::OtherMouseUp => {
                callback_counters.mouse_up.fetch_add(1, Ordering::Relaxed)
            }
            CGEventType::TapDisabledByTimeout => {
                let _ = disabled_tx.try_send(());
                callback_counters
                    .disabled_by_timeout
                    .fetch_add(1, Ordering::Relaxed)
            }
            CGEventType::TapDisabledByUserInput => {
                let _ = disabled_tx.try_send(());
                callback_counters
                    .disabled_by_user
                    .fetch_add(1, Ordering::Relaxed)
            }
            _ => 0,
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
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = remaining.min(Duration::from_millis(20));
        // SAFETY: the run-loop mode remains valid for the duration of this loop.
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, slice, true);
        while let Ok(()) = disabled_rx.try_recv() {
            tap.enable(true);
            counters.reenabled.fetch_add(1, Ordering::Relaxed);
        }
    }
    let finished_enabled = tap.is_enabled();
    tap.enable(false);
    run_loop.remove_source(&source, unsafe { kCFRunLoopDefaultMode });
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
}
