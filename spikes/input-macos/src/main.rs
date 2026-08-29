#[cfg(target_os = "macos")]
use bongocat_input_macos_spike::{
    CaptureAction, CaptureEvent, MacCaptureLifecycle, PermissionState, TapDisableReason,
    WorkspaceLifecycleInjection, input_monitoring_preflight, reconcile_pressed_key_codes,
    request_input_monitoring_access, run_listen_only_tap,
};
#[cfg(target_os = "macos")]
use std::{collections::BTreeSet, time::Duration};

fn main() {
    #[cfg(target_os = "macos")]
    {
        let request = std::env::args().any(|arg| arg == "--request");
        let inject_release_loss = std::env::args().any(|arg| arg == "--inject-release-loss");
        let tap_ms = argument_value("--tap-ms").and_then(|value| value.parse::<u64>().ok());
        let injected_disable = match argument_value("--inject-disable").as_deref() {
            None => None,
            Some("timeout") => Some(TapDisableReason::Timeout),
            Some("user") => Some(TapDisableReason::UserInput),
            Some(value) => {
                eprintln!(
                    "input-macos-spike: invalid --inject-disable value {value:?}; expected timeout or user"
                );
                std::process::exit(2);
            }
        };
        let injected_lifecycle = match argument_value("--inject-lifecycle").as_deref() {
            None => None,
            Some("session") => Some(WorkspaceLifecycleInjection::Session),
            Some("sleep") => Some(WorkspaceLifecycleInjection::Sleep),
            Some("wake") => Some(WorkspaceLifecycleInjection::Wake),
            Some("all") => Some(WorkspaceLifecycleInjection::All),
            Some(value) => {
                eprintln!(
                    "input-macos-spike: invalid --inject-lifecycle value {value:?}; expected session, sleep, wake, or all"
                );
                std::process::exit(2);
            }
        };
        let key_state_code =
            argument_value("--key-state").and_then(|value| value.parse::<u16>().ok());
        let cycles = argument_value("--cycles")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let before = input_monitoring_preflight();
        let after = if request {
            request_input_monitoring_access()
        } else {
            before
        };
        println!(
            "input-macos-spike: preflight={} request={} granted={}",
            before, request, after
        );
        let mut lifecycle = MacCaptureLifecycle::default();
        let permission = if after {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        };
        let decision = lifecycle.apply(CaptureEvent::PermissionChecked(permission));
        println!(
            "input-macos-spike: permission={:?} tap={:?} action={:?}",
            decision.permission, decision.tap, decision.action
        );
        if decision.action == Some(CaptureAction::StartTap) {
            println!("input-macos-spike: tap start is permitted");
        }
        if let Some(key_code) = key_state_code {
            if after {
                let candidates = BTreeSet::from([key_code]);
                let report = reconcile_pressed_key_codes(&candidates);
                println!(
                    "input-macos-spike: key-state reconciliation checked={} still_pressed={} released={}",
                    candidates.len(),
                    report.still_pressed.len(),
                    report.released_count
                );
            } else {
                println!("input-macos-spike: key-state query skipped because permission is denied");
            }
        }
        if let Some(tap_ms) = tap_ms {
            if !after {
                println!("input-macos-spike: tap probe skipped because permission is denied");
            } else {
                for cycle in 0..cycles {
                    match run_listen_only_tap(
                        Duration::from_millis(tap_ms),
                        injected_disable,
                        injected_lifecycle,
                        inject_release_loss,
                    ) {
                        Ok(report) => {
                            if inject_release_loss {
                                assert_eq!(report.synthetic_events_posted, 2);
                                assert!(report.key_down >= 1);
                                assert!(report.key_up >= 1);
                                assert_eq!(report.intentionally_dropped_releases, 1);
                                assert!(report.reconciliation_runs >= 2);
                                assert_eq!(report.reconciled_releases, 1);
                                assert_eq!(report.pressed_candidates_before_shutdown, 0);
                            }
                            println!(
                                "input-macos-spike: tap cycle={} started={} finished_enabled={} key_down={} key_up={} flags_changed={} mouse_down={} mouse_up={} disabled_timeout={} disabled_user={} injected_disables={} reenabled={} callback_panics={} queued_events={} consumed_events={} queue_overflows={} queue_recovery_resets={} queue_discarded_events={} queue_closed_events={} reconciliation_runs={} reconciled_releases={} candidate_resets={} candidate_reset_releases={} duplicate_down={} unmatched_up={} workspace_observers_registered={} workspace_observers_removed={} workspace_will_sleep={} workspace_did_wake={} workspace_session_resigned={} workspace_session_active={} workspace_lifecycle_resets={} workspace_callback_panics={} workspace_callbacks_ignored_after_close={} synthetic_events_posted={} intentionally_dropped_releases={} pressed_candidates_before_shutdown={}",
                                cycle + 1,
                                report.started,
                                report.finished_enabled,
                                report.key_down,
                                report.key_up,
                                report.flags_changed,
                                report.mouse_down,
                                report.mouse_up,
                                report.disabled_by_timeout,
                                report.disabled_by_user,
                                report.injected_disables,
                                report.reenabled,
                                report.callback_panics,
                                report.queued_events,
                                report.consumed_events,
                                report.queue_overflows,
                                report.queue_recovery_resets,
                                report.queue_discarded_events,
                                report.queue_closed_events,
                                report.reconciliation_runs,
                                report.reconciled_releases,
                                report.candidate_resets,
                                report.candidate_reset_releases,
                                report.duplicate_down,
                                report.unmatched_up,
                                report.workspace_observers_registered,
                                report.workspace_observers_removed,
                                report.workspace_will_sleep,
                                report.workspace_did_wake,
                                report.workspace_session_resigned,
                                report.workspace_session_active,
                                report.workspace_lifecycle_resets,
                                report.workspace_callback_panics,
                                report.workspace_callbacks_ignored_after_close,
                                report.synthetic_events_posted,
                                report.intentionally_dropped_releases,
                                report.pressed_candidates_before_shutdown,
                            )
                        }
                        Err(error) => {
                            println!("input-macos-spike: tap cycle={} error={error:?}", cycle + 1)
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        println!("input-macos-spike: target OS is not macOS; probe skipped");
    }
}

#[cfg(target_os = "macos")]
fn argument_value(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}
