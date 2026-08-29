#[cfg(target_os = "macos")]
use bongocat_input_macos_spike::{
    CaptureAction, CaptureEvent, MacCaptureLifecycle, PermissionState, TapDisableReason,
    TapProbeReport, WorkspaceLifecycleInjection, input_monitoring_preflight,
    post_event_access_preflight, reconcile_pressed_key_codes, reconcile_pressed_mouse_buttons,
    request_input_monitoring_access, run_listen_only_tap,
};
#[cfg(target_os = "macos")]
use std::{collections::BTreeSet, time::Duration};

fn main() {
    #[cfg(target_os = "macos")]
    {
        let request = std::env::args().any(|arg| arg == "--request");
        let inject_release_loss = std::env::args().any(|arg| arg == "--inject-release-loss");
        let summary_only = std::env::args().any(|arg| arg == "--summary-only");
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
        let button_state_code =
            argument_value("--button-state").and_then(|value| value.parse::<u8>().ok());
        let cycles = argument_value("--cycles")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if cycles == 0 {
            eprintln!("input-macos-spike: --cycles must be greater than zero");
            std::process::exit(2);
        }
        let before = input_monitoring_preflight();
        let after = if request {
            request_input_monitoring_access()
        } else {
            before
        };
        println!(
            "input-macos-spike: preflight={} request={} granted={} post_event_preflight={}",
            before,
            request,
            after,
            post_event_access_preflight(),
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
        if let Some(button) = button_state_code {
            if button > 31 {
                eprintln!("input-macos-spike: --button-state must be in 0..=31");
                std::process::exit(2);
            }
            if after {
                let candidates = BTreeSet::from([button]);
                let report = reconcile_pressed_mouse_buttons(&candidates);
                println!(
                    "input-macos-spike: button-state reconciliation checked={} still_pressed={} released={}",
                    candidates.len(),
                    report.still_pressed.len(),
                    report.released_count
                );
            } else {
                println!(
                    "input-macos-spike: button-state query skipped because permission is denied"
                );
            }
        }
        if let Some(tap_ms) = tap_ms {
            if !after {
                println!("input-macos-spike: tap probe skipped because permission is denied");
            } else {
                let mut summary = CycleSummary::default();
                for cycle in 0..cycles {
                    match run_listen_only_tap(
                        Duration::from_millis(tap_ms),
                        injected_disable,
                        injected_lifecycle,
                        inject_release_loss,
                    ) {
                        Ok(report) => {
                            if let Err(error) = validate_cycle_report(
                                &report,
                                injected_disable,
                                injected_lifecycle,
                                inject_release_loss,
                            ) {
                                print_cycle_report(cycle + 1, &report);
                                eprintln!(
                                    "input-macos-spike: tap cycle={} validation failed: {error}",
                                    cycle + 1
                                );
                                std::process::exit(1);
                            }
                            summary.record(&report);
                            if !summary_only {
                                print_cycle_report(cycle + 1, &report);
                            }
                        }
                        Err(error) => {
                            eprintln!("input-macos-spike: tap cycle={} error={error:?}", cycle + 1);
                            std::process::exit(1);
                        }
                    }
                }
                summary.print();
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        println!("input-macos-spike: target OS is not macOS; probe skipped");
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct CycleSummary {
    completed: usize,
    key_down: u64,
    key_up: u64,
    reconciled_releases: u64,
    candidate_resets: u64,
    queue_overflows: u64,
    sequence_gaps: u64,
    sequence_duplicates_or_out_of_order: u64,
    callback_panics: u64,
}

#[cfg(target_os = "macos")]
impl CycleSummary {
    fn record(&mut self, report: &TapProbeReport) {
        self.completed += 1;
        self.key_down += report.key_down;
        self.key_up += report.key_up;
        self.reconciled_releases += report.reconciled_releases;
        self.candidate_resets += report.candidate_resets;
        self.queue_overflows += report.queue_overflows;
        self.sequence_gaps += report.sequence_gaps;
        self.sequence_duplicates_or_out_of_order += report.sequence_duplicates_or_out_of_order;
        self.callback_panics += report.callback_panics + report.workspace_callback_panics;
    }

    fn print(&self) {
        println!(
            "input-macos-spike: summary completed_cycles={} key_down={} key_up={} reconciled_releases={} candidate_resets={} queue_overflows={} sequence_gaps={} sequence_duplicates_or_out_of_order={} callback_panics={} clean_shutdown=true",
            self.completed,
            self.key_down,
            self.key_up,
            self.reconciled_releases,
            self.candidate_resets,
            self.queue_overflows,
            self.sequence_gaps,
            self.sequence_duplicates_or_out_of_order,
            self.callback_panics,
        );
    }
}

#[cfg(target_os = "macos")]
fn validate_cycle_report(
    report: &TapProbeReport,
    injected_disable: Option<TapDisableReason>,
    injected_lifecycle: Option<WorkspaceLifecycleInjection>,
    inject_release_loss: bool,
) -> Result<(), String> {
    if !report.started {
        return Err("tap did not start".to_string());
    }
    if !report.finished_enabled {
        return Err("tap was not enabled before shutdown".to_string());
    }
    if report.callback_panics != 0 || report.workspace_callback_panics != 0 {
        return Err("a callback panic was contained".to_string());
    }
    if report.queue_overflows != 0 || report.queue_closed_events != 0 {
        return Err("the callback queue lost or rejected an event".to_string());
    }
    if report.sequence_gaps != 0 || report.sequence_duplicates_or_out_of_order != 0 {
        return Err("the callback queue observed a non-contiguous sequence".to_string());
    }
    if report.queued_events != report.consumed_events + report.queue_discarded_events {
        return Err("the callback queue did not account for every accepted event".to_string());
    }
    if report.workspace_observers_registered != report.workspace_observers_removed {
        return Err("workspace observers were not removed exactly once".to_string());
    }
    if let Some(reason) = injected_disable {
        let observed = match reason {
            TapDisableReason::Timeout => report.disabled_by_timeout,
            TapDisableReason::UserInput => report.disabled_by_user,
        };
        if report.injected_disables != 1 || observed != 1 || report.reenabled != 1 {
            return Err("disable injection did not complete one recovery".to_string());
        }
    }
    if injected_lifecycle.is_some()
        && (report.workspace_lifecycle_resets != 1
            || report.workspace_callbacks_ignored_after_close != 1)
    {
        return Err("lifecycle injection did not reset and close its callback gate".to_string());
    }
    if inject_release_loss
        && (report.synthetic_events_posted != 2
            || report.key_down < 1
            || report.key_up < 1
            || report.intentionally_dropped_releases != 1
            || report.reconciliation_runs < 2
            || report.reconciled_releases != 1
            || report.pressed_candidates_before_shutdown != 0)
    {
        return Err("release-loss injection left an uncorrected pressed candidate".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn print_cycle_report(cycle: usize, report: &TapProbeReport) {
    println!(
        "input-macos-spike: tap cycle={} started={} finished_enabled={} key_down={} key_up={} flags_changed={} mouse_down={} mouse_up={} disabled_timeout={} disabled_user={} injected_disables={} reenabled={} callback_panics={} queued_events={} consumed_events={} queue_overflows={} queue_recovery_resets={} queue_discarded_events={} queue_closed_events={} sequence_gaps={} sequence_duplicates_or_out_of_order={} reconciliation_runs={} reconciled_releases={} candidate_resets={} candidate_reset_releases={} duplicate_down={} unmatched_up={} workspace_observers_registered={} workspace_observers_removed={} workspace_will_sleep={} workspace_did_wake={} workspace_session_resigned={} workspace_session_active={} workspace_lifecycle_resets={} workspace_callback_panics={} workspace_callbacks_ignored_after_close={} synthetic_events_posted={} intentionally_dropped_releases={} pressed_candidates_before_shutdown={}",
        cycle,
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
        report.sequence_gaps,
        report.sequence_duplicates_or_out_of_order,
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
    );
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn healthy_report() -> TapProbeReport {
        TapProbeReport {
            started: true,
            finished_enabled: true,
            workspace_observers_registered: 4,
            workspace_observers_removed: 4,
            ..TapProbeReport::default()
        }
    }

    #[test]
    fn accepts_a_healthy_cycle() {
        validate_cycle_report(&healthy_report(), None, None, false).unwrap();
    }

    #[test]
    fn rejects_silent_smoke_failures() {
        let mut report = healthy_report();
        report.queue_overflows = 1;
        assert!(validate_cycle_report(&report, None, None, false).is_err());

        let mut report = healthy_report();
        report.workspace_observers_removed = 3;
        assert!(validate_cycle_report(&report, None, None, false).is_err());

        let mut report = healthy_report();
        report.finished_enabled = false;
        assert!(validate_cycle_report(&report, None, None, false).is_err());

        let mut report = healthy_report();
        report.sequence_gaps = 1;
        assert!(validate_cycle_report(&report, None, None, false).is_err());

        let mut report = healthy_report();
        report.queued_events = 2;
        report.consumed_events = 1;
        assert!(validate_cycle_report(&report, None, None, false).is_err());
    }

    #[test]
    fn requires_release_loss_to_finish_by_reconciliation() {
        let mut report = healthy_report();
        report.synthetic_events_posted = 2;
        report.key_down = 1;
        report.key_up = 1;
        report.intentionally_dropped_releases = 1;
        report.reconciliation_runs = 2;
        report.reconciled_releases = 1;
        validate_cycle_report(&report, None, None, true).unwrap();

        report.pressed_candidates_before_shutdown = 1;
        assert!(validate_cycle_report(&report, None, None, true).is_err());
    }

    #[test]
    fn requires_disable_and_lifecycle_injections_to_recover() {
        let mut report = healthy_report();
        report.injected_disables = 1;
        report.disabled_by_timeout = 1;
        report.reenabled = 1;
        validate_cycle_report(&report, Some(TapDisableReason::Timeout), None, false).unwrap();

        report.workspace_lifecycle_resets = 1;
        report.workspace_callbacks_ignored_after_close = 1;
        validate_cycle_report(
            &report,
            Some(TapDisableReason::Timeout),
            Some(WorkspaceLifecycleInjection::All),
            false,
        )
        .unwrap();
    }
}
