use bongocat_input_macos_spike::{
    CaptureAction, CaptureEvent, MacCaptureLifecycle, PermissionState,
};
#[cfg(target_os = "macos")]
use bongocat_input_macos_spike::{reconcile_key_state, run_listen_only_tap};
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use bongocat_input_macos_spike::{input_monitoring_preflight, request_input_monitoring_access};

fn main() {
    let request = std::env::args().any(|arg| arg == "--request");
    let tap_ms = argument_value("--tap-ms").and_then(|value| value.parse::<u64>().ok());
    let key_state_code = argument_value("--key-state").and_then(|value| value.parse::<u16>().ok());
    let cycles = argument_value("--cycles")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    #[cfg(target_os = "macos")]
    {
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
                println!(
                    "input-macos-spike: key-state query completed pressed={}",
                    reconcile_key_state(key_code)
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
                    match run_listen_only_tap(Duration::from_millis(tap_ms)) {
                        Ok(report) => println!(
                            "input-macos-spike: tap cycle={} started={} finished_enabled={} key_down={} key_up={} flags_changed={} mouse_down={} mouse_up={} disabled_timeout={} disabled_user={} reenabled={} callback_panics={}",
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
                            report.reenabled,
                            report.callback_panics,
                        ),
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
        let _ = request;
        println!("input-macos-spike: target OS is not macOS; probe skipped");
    }
}

fn argument_value(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}
