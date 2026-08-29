use bongocat_input_windows_spike::{RawKeyboardPacket, decode_keyboard_packet};

#[cfg(target_os = "windows")]
use bongocat_input_windows_spike::{MouseButton, PhysicalKey};
#[cfg(target_os = "windows")]
use std::collections::BTreeSet;

#[cfg(target_os = "windows")]
mod windows_capture;

fn main() {
    #[cfg(target_os = "windows")]
    if let Some(cycles) = argument_value("--synthetic-pointer-flood-cycles")
        .and_then(|value| value.parse::<usize>().ok())
    {
        let report = windows_capture::run_synthetic_pointer_flood_smoke(
            std::time::Duration::from_secs(5),
            cycles,
        )
        .expect("Windows synthetic pointer flood smoke failed");
        let expected_pairs = cycles * windows_capture::SYNTHETIC_PRESSURE_KEY_COUNT;
        let expected_edges = expected_pairs * 2;
        let expected_pointer_inputs =
            expected_pairs * windows_capture::SYNTHETIC_POINTER_MOVES_PER_KEY_PAIR;
        assert!(report.registered, "Raw Input devices were not registered");
        assert_eq!(
            report.synthetic_inputs_sent,
            (expected_edges + expected_pointer_inputs) as u64
        );
        assert_eq!(
            report.synthetic_pointer_inputs_requested,
            expected_pointer_inputs as u64
        );
        assert!(
            report.mouse_messages >= cycles as u64,
            "pointer flood produced too few Raw Input mouse messages"
        );
        assert_eq!(report.synthetic_expected_edges, expected_edges as u64);
        assert_eq!(report.synthetic_edges_seen, expected_edges as u64);
        assert_eq!(report.synthetic_down_edges, expected_pairs as u64);
        assert_eq!(report.synthetic_up_edges, expected_pairs as u64);
        assert_eq!(report.synthetic_order_errors, 0);
        assert_eq!(report.synthetic_expected_edges_remaining, 0);
        assert_eq!(report.captured_down, expected_pairs as u64);
        assert_eq!(report.captured_up, expected_pairs as u64);
        assert_eq!(report.duplicate_down, 0);
        assert_eq!(report.unmatched_up, 0);
        assert_eq!(report.decode_errors, 0);
        assert_eq!(report.callback_panics, 0);
        assert_eq!(report.pressed_candidates_remaining, 0);
        assert!(report.clean_shutdown);
        print_registration_report(report);
        return;
    }

    #[cfg(target_os = "windows")]
    if let Some(cycles) = argument_value("--synthetic-edge-pressure-cycles")
        .and_then(|value| value.parse::<usize>().ok())
    {
        let report = windows_capture::run_synthetic_edge_pressure_smoke(
            std::time::Duration::from_secs(3),
            cycles,
        )
        .expect("Windows synthetic edge pressure smoke failed");
        let expected_pairs = cycles * windows_capture::SYNTHETIC_PRESSURE_KEY_COUNT;
        let expected_edges = expected_pairs * 2;
        assert!(report.registered, "Raw Input devices were not registered");
        assert_eq!(report.synthetic_inputs_sent, expected_edges as u64);
        assert_eq!(report.synthetic_expected_edges, expected_edges as u64);
        assert_eq!(report.synthetic_edges_seen, expected_edges as u64);
        assert_eq!(report.synthetic_down_edges, expected_pairs as u64);
        assert_eq!(report.synthetic_up_edges, expected_pairs as u64);
        assert_eq!(report.synthetic_order_errors, 0);
        assert_eq!(report.synthetic_expected_edges_remaining, 0);
        assert_eq!(report.captured_down, expected_pairs as u64);
        assert_eq!(report.captured_up, expected_pairs as u64);
        assert_eq!(report.duplicate_down, 0);
        assert_eq!(report.unmatched_up, 0);
        assert_eq!(report.decode_errors, 0);
        assert_eq!(report.callback_panics, 0);
        assert_eq!(report.pressed_candidates_remaining, 0);
        assert!(report.clean_shutdown);
        print_registration_report(report);
        return;
    }

    #[cfg(target_os = "windows")]
    if let Some(milliseconds) = argument_value("--synthetic-release-recovery-ms")
        .and_then(|value| value.parse::<u64>().ok())
    {
        let report = windows_capture::run_synthetic_release_recovery_smoke(
            std::time::Duration::from_millis(milliseconds),
        )
        .expect("Windows synthetic release recovery smoke failed");
        assert!(report.registered, "Raw Input devices were not registered");
        assert_eq!(report.synthetic_inputs_sent, 2);
        assert!(
            report.raw_messages >= 2,
            "SendInput produced no WM_INPUT pair"
        );
        assert!(
            report.keyboard_edges >= 2,
            "SendInput produced no keyboard edge pair"
        );
        assert_eq!(report.intentionally_dropped_releases, 1);
        assert_eq!(report.reconciled_releases, 1);
        assert!(report.reconciliation_runs >= 2);
        assert_eq!(report.reconciliation_query_errors, 0);
        assert_eq!(report.pressed_candidates_remaining, 0);
        assert_eq!(report.callback_panics, 0);
        assert!(report.clean_shutdown);
        print_registration_report(report);
        return;
    }

    #[cfg(target_os = "windows")]
    if std::env::args().any(|argument| argument == "--key-state-smoke") {
        let candidates = BTreeSet::from([
            PhysicalKey::ControlLeft,
            PhysicalKey::AltLeft,
            PhysicalKey::A,
        ]);
        let report = windows_capture::query_pressed_keys(&candidates)
            .expect("GetAsyncKeyState reconciliation smoke failed");
        assert_eq!(report.queried, candidates.len());
        assert_eq!(report.unqueryable, 0);
        assert!(!report.reset_required);
        println!(
            "input-windows-spike: key_state_checked={} currently_pressed={} unqueryable={} reset_required={}",
            report.queried,
            report.still_pressed.len(),
            report.unqueryable,
            report.reset_required,
        );
        return;
    }

    #[cfg(target_os = "windows")]
    if std::env::args().any(|argument| argument == "--mouse-button-state-smoke") {
        let candidates = BTreeSet::from([
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
        ]);
        let report = windows_capture::query_pressed_mouse_buttons(&candidates)
            .expect("GetAsyncKeyState mouse button reconciliation smoke failed");
        assert_eq!(report.queried, candidates.len());
        println!(
            "input-windows-spike: mouse_buttons_checked={} currently_pressed={}",
            report.queried,
            report.still_pressed.len(),
        );
        return;
    }

    #[cfg(target_os = "windows")]
    if let Some(milliseconds) =
        argument_value("--lifecycle-smoke-ms").and_then(|value| value.parse::<u64>().ok())
    {
        let report =
            windows_capture::run_lifecycle_smoke(std::time::Duration::from_millis(milliseconds))
                .expect("Windows input lifecycle smoke failed");
        assert!(report.registered, "Raw Input devices were not registered");
        assert!(
            report.session_notifications_registered,
            "session notifications were not registered"
        );
        assert!(
            report.session_notifications_unregistered,
            "session notifications were not unregistered"
        );
        assert_eq!(report.session_change_resets, 2);
        assert_eq!(report.power_change_resets, 2);
        assert_eq!(report.reset_releases, 4);
        assert!(report.clean_shutdown);
        assert_eq!(report.callback_panics, 0);
        print_registration_report(report);
        return;
    }

    #[cfg(target_os = "windows")]
    if let Some(milliseconds) =
        argument_value("--reconcile-smoke-ms").and_then(|value| value.parse::<u64>().ok())
    {
        let report =
            windows_capture::run_registration_smoke(std::time::Duration::from_millis(milliseconds))
                .expect("Raw Input reconciliation scheduler smoke failed");
        assert!(report.registered, "Raw Input devices were not registered");
        assert!(
            report.clean_shutdown,
            "Raw Input window did not shut down cleanly"
        );
        assert!(
            report.reconciliation_runs >= 2,
            "Raw Input reconciliation scheduler did not run twice"
        );
        assert_eq!(
            report.reconciliation_query_errors, 0,
            "Windows key-state reconciliation query failed"
        );
        assert_eq!(report.callback_panics, 0, "Raw Input callback panicked");
        print_registration_report(report);
        return;
    }

    #[cfg(target_os = "windows")]
    if let Some(milliseconds) =
        argument_value("--register-smoke-ms").and_then(|value| value.parse::<u64>().ok())
    {
        let report =
            windows_capture::run_registration_smoke(std::time::Duration::from_millis(milliseconds))
                .expect("Raw Input registration smoke failed");
        assert!(report.registered, "Raw Input devices were not registered");
        assert!(
            report.clean_shutdown,
            "Raw Input window did not shut down cleanly"
        );
        assert_eq!(report.decode_errors, 0, "Raw Input decoding failed");
        assert_eq!(report.callback_panics, 0, "Raw Input callback panicked");
        assert_eq!(
            report.service_stopped_resets, 1,
            "Raw Input shutdown did not reset pressed candidates"
        );
        print_registration_report(report);
        return;
    }

    let down = decode_keyboard_packet(RawKeyboardPacket {
        make_code: 0x1e,
        flags: 0,
    });
    let up = decode_keyboard_packet(RawKeyboardPacket {
        make_code: 0x1e,
        flags: 0x0001,
    });
    println!(
        "input-windows-spike: decoded_edges={} down={} up={}",
        2, down.pressed, up.pressed
    );
}

#[cfg(target_os = "windows")]
fn print_registration_report(report: windows_capture::RegistrationReport) {
    println!(
        "input-windows-spike: registered={} session_notifications_registered={} session_notifications_unregistered={} clean_shutdown={} raw_messages={} keyboard_edges={} mouse_messages={} mouse_button_edges={} mouse_captured_down={} mouse_captured_up={} mouse_duplicate_down={} mouse_unmatched_up={} mouse_resets={} mouse_reset_releases={} mouse_reconciled_releases={} mouse_candidates_remaining={} device_arrivals={} device_removals={} resets={} reset_releases={} device_removed_resets={} session_change_resets={} power_change_resets={} service_stopped_resets={} unqueryable_key_resets={} state_query_unavailable_resets={} reconciliation_runs={} reconciled_releases={} reconciliation_query_errors={} decode_errors={} callback_panics={} synthetic_inputs_sent={} synthetic_pointer_inputs_requested={} synthetic_expected_edges={} synthetic_edges_seen={} synthetic_down_edges={} synthetic_up_edges={} synthetic_order_errors={} synthetic_expected_edges_remaining={} intentionally_dropped_releases={} captured_down={} captured_up={} duplicate_down={} unmatched_up={} pressed_candidates_remaining={}",
        report.registered,
        report.session_notifications_registered,
        report.session_notifications_unregistered,
        report.clean_shutdown,
        report.raw_messages,
        report.keyboard_edges,
        report.mouse_messages,
        report.mouse_button_edges,
        report.mouse_captured_down,
        report.mouse_captured_up,
        report.mouse_duplicate_down,
        report.mouse_unmatched_up,
        report.mouse_resets,
        report.mouse_reset_releases,
        report.mouse_reconciled_releases,
        report.mouse_candidates_remaining,
        report.device_arrivals,
        report.device_removals,
        report.resets,
        report.reset_releases,
        report.device_removed_resets,
        report.session_change_resets,
        report.power_change_resets,
        report.service_stopped_resets,
        report.unqueryable_key_resets,
        report.state_query_unavailable_resets,
        report.reconciliation_runs,
        report.reconciled_releases,
        report.reconciliation_query_errors,
        report.decode_errors,
        report.callback_panics,
        report.synthetic_inputs_sent,
        report.synthetic_pointer_inputs_requested,
        report.synthetic_expected_edges,
        report.synthetic_edges_seen,
        report.synthetic_down_edges,
        report.synthetic_up_edges,
        report.synthetic_order_errors,
        report.synthetic_expected_edges_remaining,
        report.intentionally_dropped_releases,
        report.captured_down,
        report.captured_up,
        report.duplicate_down,
        report.unmatched_up,
        report.pressed_candidates_remaining,
    );
}

#[cfg(target_os = "windows")]
fn argument_value(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}
