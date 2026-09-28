//! Only an unsupported state refuses to be changed.

use super::*;

#[test]
fn startup_item_command_preserves_the_requested_state() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetStartupItemEnabled { enabled, reply } =
            endpoint.recv_blocking().expect("startup item command")
        else {
            panic!("unexpected command");
        };
        assert!(enabled);
        let mut updated = snapshot(2, true, true);
        updated.startup_item = SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled);
        reply.respond(Ok(updated)).expect("startup item reply");
    });

    let updated = client
        .set_startup_item_enabled_blocking(true)
        .expect("startup item snapshot");
    assert_eq!(
        updated.startup_item,
        SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)
    );
    worker.join().expect("worker join");
}

#[test]
fn only_unsupported_startup_states_reject_mutation() {
    let actionable = [
        SettingsStartupItemState::Disabled,
        SettingsStartupItemState::Enabled,
        SettingsStartupItemState::Stale,
        SettingsStartupItemState::RequiresApproval,
        SettingsStartupItemState::NotFound,
    ];
    assert!(actionable.into_iter().all(|state| state.can_set_enabled()));
    assert!(
        !SettingsStartupItemState::Unsupported(
            SettingsStartupItemUnsupportedReason::BuildEnvironment
        )
        .can_set_enabled()
    );
}
