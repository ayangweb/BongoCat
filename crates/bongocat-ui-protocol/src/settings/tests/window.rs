//! The window box is validated, and stale writes coalesce.

use super::*;

#[test]
fn settings_window_state_is_validated_and_shared_across_clones() {
    assert!(SettingsWindowPlacement::new(0, 0, 639, 600, false).is_none());
    assert!(SettingsWindowPlacement::new(1_000_001, 0, 800, 600, false).is_none());

    let initial =
        SettingsWindowPlacement::new(-120, 80, 800, 600, false).expect("valid initial placement");
    let updated =
        SettingsWindowPlacement::new(240, 160, 1024, 768, true).expect("valid updated placement");
    let state = SettingsWindowState::new(Some(initial));
    let cloned = state.clone();
    cloned.update(updated);
    assert_eq!(state.placement(), Some(updated));
}

#[test]
fn settings_window_state_coalesces_stale_persist_requests() {
    let (client, endpoint) = SettingsClient::bounded(2);
    let initial =
        SettingsWindowPlacement::new(0, 0, 800, 600, false).expect("valid initial placement");
    let first =
        SettingsWindowPlacement::new(10, 20, 800, 600, false).expect("valid first placement");
    let latest =
        SettingsWindowPlacement::new(30, 40, 1024, 768, false).expect("valid latest placement");
    let state = client.track_window_state(Some(initial));
    let first_revision = state.update(first).expect("first placement changed");
    let latest_revision = state.update(latest).expect("latest placement changed");

    assert!(state.request_persist_if_current(first_revision));
    assert!(
        endpoint.try_recv().is_err(),
        "stale timer must not enqueue a write"
    );
    assert!(state.request_persist_if_current(latest_revision));
    assert!(matches!(
        endpoint.try_recv(),
        Ok(SettingsCommand::SettingsWindowPlacementChanged)
    ));
    assert!(
        endpoint.try_recv().is_err(),
        "only the latest timer may enqueue"
    );
}
