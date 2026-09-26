//! One settings control producing one confirmed configuration revision.

use super::*;

#[test]
fn settings_error_display_matches_the_english_catalog_copy() {
    for code in SettingsErrorCode::ALL {
        let error = SettingsError::new(code);
        assert_eq!(
            settings_error(SettingsLanguage::EnglishUnitedStates, error),
            error.to_string(),
            "the protocol error text and catalog drifted for {code:?}"
        );
        assert!(!settings_error(SettingsLanguage::ChineseSimplified, error).is_empty());
    }
}

#[test]
fn shutdown_flush_chains_each_patch_from_the_latest_confirmed_revision() {
    let mut current_revision = Some(7);
    let first_response_revision = 8;
    assert!(accepts_snapshot_revision(
        current_revision,
        first_response_revision
    ));
    current_revision = Some(first_response_revision);

    let second_expected_revision = current_revision.expect("first patch must confirm");
    assert_eq!(second_expected_revision, 8);
    assert!(accepts_snapshot_revision(current_revision, 9));
}

#[gpui_kit::test]
fn logging_flush_sends_one_complete_policy_and_chains_the_confirmed_revision(
    cx: &mut TestAppContext,
) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.logging = SettingsLogging::default();
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_logging_level(SettingsLogLevel::Debug, cx);
    });
    visual.run_until_parked();
    let first_command = endpoint.try_recv().expect("first logging policy");
    let crate::SettingsCommand::SetLoggingSettings {
        expected_config_revision,
        settings: first_policy,
        reply: first_reply,
    } = first_command
    else {
        panic!("the level change must use the typed logging command");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(
        first_policy,
        SettingsLogging {
            level: SettingsLogLevel::Debug,
            retention_days: 7,
        }
    );

    view.update(visual, |view, cx| {
        view.set_logging_retention_days_value(14.0, cx);
        view.flush_pending_settings(cx);
    });
    assert_eq!(
        view.read_with(visual, |view, _| view
            .logging_settings_debouncer
            .pending_value()
            .copied()),
        Some(SettingsLogging {
            level: SettingsLogLevel::Debug,
            retention_days: 14,
        }),
        "the second edit must extend the first policy instead of replacing one field"
    );
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "the newer complete policy must wait behind the in-flight command"
    );

    let mut confirmed = crate::tests::snapshot(8, true, true);
    confirmed.config_revision = Some(8);
    confirmed.logging = first_policy;
    first_reply.respond(Ok(confirmed)).expect("first reply");
    visual.run_until_parked();

    let second_command = endpoint.try_recv().expect("flushed logging policy");
    let crate::SettingsCommand::SetLoggingSettings {
        expected_config_revision,
        settings: second_policy,
        reply: second_reply,
    } = second_command
    else {
        panic!("the flush must continue through the logging command");
    };
    assert_eq!(expected_config_revision, 8);
    assert_eq!(
        second_policy,
        SettingsLogging {
            level: SettingsLogLevel::Debug,
            retention_days: 14,
        }
    );
    let mut completed = crate::tests::snapshot(9, true, true);
    completed.config_revision = Some(9);
    completed.logging = second_policy;
    second_reply.respond(Ok(completed)).expect("second reply");
    visual.run_until_parked();

    assert!(view.read_with(visual, |view, _| {
        !view.flush_pending_requested
            && view.logging_settings_debouncer.pending_value().is_none()
            && view
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.config_revision)
                == Some(9)
    }));
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn update_interval_edits_are_debounced_and_flushed_with_the_latest_revision(
    cx: &mut TestAppContext,
) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.check_for_updates_automatically = true;
    initial.check_for_updates_interval_hours = 24;
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_check_for_updates_interval_hours(48.0, cx);
    });
    visual.run_until_parked();
    let first_command = endpoint
        .try_recv()
        .expect("first automatic update interval");
    let crate::SettingsCommand::SetCheckForUpdatesIntervalHours {
        expected_config_revision,
        interval_hours: first_interval,
        reply: first_reply,
    } = first_command
    else {
        panic!("the interval change must use the typed settings command");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(first_interval, 48);

    view.update(visual, |view, cx| {
        view.set_check_for_updates_interval_hours(12.0, cx);
        view.flush_pending_settings(cx);
    });
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "the newer interval must wait behind the in-flight command"
    );

    let mut confirmed = crate::tests::snapshot(8, true, true);
    confirmed.config_revision = Some(8);
    confirmed.check_for_updates_interval_hours = first_interval;
    first_reply
        .respond(Ok(confirmed))
        .expect("first interval reply");
    visual.run_until_parked();

    let second_command = endpoint
        .try_recv()
        .expect("flushed automatic update interval");
    let crate::SettingsCommand::SetCheckForUpdatesIntervalHours {
        expected_config_revision,
        interval_hours: second_interval,
        reply: second_reply,
    } = second_command
    else {
        panic!("the flush must continue through the interval command");
    };
    assert_eq!(expected_config_revision, 8);
    assert_eq!(second_interval, 12);

    let mut completed = crate::tests::snapshot(9, true, true);
    completed.config_revision = Some(9);
    completed.check_for_updates_interval_hours = second_interval;
    second_reply
        .respond(Ok(completed))
        .expect("second interval reply");
    visual.run_until_parked();

    assert!(view.read_with(visual, |view, _| {
        !view.flush_pending_requested
            && view
                .check_for_updates_interval_debouncer
                .pending_value()
                .is_none()
            && view
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.config_revision)
                == Some(9)
    }));
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn the_update_interval_is_inert_while_automatic_checks_are_off(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    snapshot.check_for_updates_automatically = false;
    snapshot.check_for_updates_interval_hours = 24;
    view.update(visual, |view, _| view.snapshot = Some(snapshot));

    view.update(visual, |view, cx| {
        view.set_check_for_updates_interval_hours(48.0, cx);
    });
    visual.run_until_parked();

    assert!(endpoint.try_recv().is_err());
    assert!(view.read_with(visual, |view, _| {
        view.check_for_updates_interval_debouncer
            .pending_value()
            .is_none()
    }));
}

#[gpui_kit::test]
fn a_failed_logging_policy_keeps_the_latest_complete_value_for_retry(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(11, true, true);
    initial.config_revision = Some(11);
    initial.logging = SettingsLogging::default();
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_logging_retention_days_value(30.0, cx);
    });
    visual.run_until_parked();
    let first_command = endpoint.try_recv().expect("first logging policy");
    let crate::SettingsCommand::SetLoggingSettings {
        reply: first_reply, ..
    } = first_command
    else {
        panic!("the retention change must use the typed logging command");
    };

    view.update(visual, |view, cx| {
        view.set_logging_level(SettingsLogLevel::Trace, cx);
    });
    first_reply
        .respond(Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated)))
        .expect("stale logging reply");
    visual.run_until_parked();

    let refreshed_command = endpoint.try_recv().expect("stale-error snapshot refresh");
    let crate::SettingsCommand::ReadSnapshot {
        reply: refresh_reply,
    } = refreshed_command
    else {
        panic!("a stale logging command must refresh before retrying");
    };
    let mut refreshed = crate::tests::snapshot(12, true, true);
    refreshed.config_revision = Some(12);
    refreshed.logging = SettingsLogging::default();
    refresh_reply
        .respond(Ok(refreshed))
        .expect("refreshed snapshot reply");
    visual.run_until_parked();

    assert_eq!(
        view.read_with(visual, |view, _| view
            .logging_settings_debouncer
            .pending_value()
            .copied()),
        Some(SettingsLogging {
            level: SettingsLogLevel::Trace,
            retention_days: 30,
        }),
        "failure must retain the complete unacknowledged policy"
    );
    view.update(visual, |view, cx| view.flush_pending_settings(cx));
    visual.run_until_parked();

    let retry_command = endpoint.try_recv().expect("retained logging policy retry");
    let crate::SettingsCommand::SetLoggingSettings {
        expected_config_revision,
        settings,
        reply: retry_reply,
    } = retry_command
    else {
        panic!("the retry must use the typed logging command");
    };
    assert_eq!(expected_config_revision, 12);
    assert_eq!(
        settings,
        SettingsLogging {
            level: SettingsLogLevel::Trace,
            retention_days: 30,
        }
    );
    let mut completed = crate::tests::snapshot(13, true, true);
    completed.config_revision = Some(13);
    completed.logging = settings;
    retry_reply.respond(Ok(completed)).expect("retry reply");
    visual.run_until_parked();
    assert!(view.read_with(visual, |view, _| {
        view.logging_settings_debouncer.pending_value().is_none()
    }));
}
