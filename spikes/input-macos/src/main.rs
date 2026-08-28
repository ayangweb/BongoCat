use bongocat_input_macos_spike::{
    CaptureAction, CaptureEvent, MacCaptureLifecycle, PermissionState,
};

#[cfg(target_os = "macos")]
use bongocat_input_macos_spike::{input_monitoring_preflight, request_input_monitoring_access};

fn main() {
    let request = std::env::args().any(|arg| arg == "--request");
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
            println!(
                "input-macos-spike: tap start is permitted; CGEventTap creation remains a separate opt-in probe"
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = request;
        println!("input-macos-spike: target OS is not macOS; probe skipped");
    }
}
