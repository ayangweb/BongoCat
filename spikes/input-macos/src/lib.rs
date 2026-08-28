#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

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
