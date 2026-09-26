//! Forwarding a connectivity notice to the settings worker.

use super::*;

#[test]
fn the_frame_source_queues_a_gamepad_notice_once_per_transition() {
    // A live service is the only way to tell "queued" from "still owed", so
    // the test drives a real bounded endpoint and counts the commands.
    let (client, endpoint) = SettingsClient::bounded(8);
    let mut observer = GamepadConnectionObserver::default();

    // The first frame only establishes the baseline: a gamepad that is
    // already attached is announced by the input service afterwards.
    observer.observe(0, &client);
    assert!(endpoint.try_recv().is_err());
    observer.observe(1, &client);
    assert!(matches!(
        endpoint.try_recv(),
        Ok(SettingsCommand::GamepadConnectionChanged)
    ));
    // Staying connected is not a transition.
    observer.observe(3, &client);
    assert!(endpoint.try_recv().is_err());
    observer.observe(0, &client);
    assert!(matches!(
        endpoint.try_recv(),
        Ok(SettingsCommand::GamepadConnectionChanged)
    ));
}

#[test]
fn a_gamepad_notice_the_service_could_not_take_is_retried() {
    // One slot, so the transition that matters lands on a full queue.
    let (client, endpoint) = SettingsClient::bounded(1);
    let mut observer = GamepadConnectionObserver::default();
    observer.observe(0, &client);
    observer.observe(1, &client);
    // The next transition arrives while the first notice still occupies the
    // single slot, which is the one failure the producer can see.
    observer.observe(0, &client);
    assert!(matches!(
        endpoint.try_recv(),
        Ok(SettingsCommand::GamepadConnectionChanged)
    ));
    // The service took the earlier notice; the refused transition is retried
    // even though nothing about the observed state has changed since.
    observer.observe(0, &client);
    assert!(matches!(
        endpoint.try_recv(),
        Ok(SettingsCommand::GamepadConnectionChanged)
    ));
    // A closed service ends the retries instead of spinning on them.
    drop(endpoint);
    observer.observe(1, &client);
}
