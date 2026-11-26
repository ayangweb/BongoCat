//! One settings control producing one confirmed configuration revision.

use super::*;

#[cfg(target_os = "windows")]
#[gpui_kit::test]
fn window_background_keeps_the_latest_color_while_a_save_is_in_flight(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.overlay.window_mode = true;
    initial.overlay.click_through = true;
    let original = initial.overlay;
    view.update(visual, |view, _| view.snapshot = Some(initial));
    view.update(visual, |view, cx| {
        view.set_window_background_color(super::super::window_mode::rgb_color([255, 0, 255]), cx);
        view.flush_pending_settings(cx);
    });
    visual.run_until_parked();
    let crate::SettingsCommand::SetOverlaySettings {
        expected_config_revision,
        settings,
        reply,
    } = endpoint.try_recv().unwrap()
    else {
        panic!("typed overlay command");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(settings.window_background_color, [255, 0, 255]);
    assert!(settings.click_through);
    view.update(visual, |view, cx| {
        view.set_window_background_color(super::super::window_mode::rgb_color([0, 0, 255]), cx);
        view.flush_pending_settings(cx);
    });
    visual.run_until_parked();
    assert!(endpoint.try_recv().is_err());
    let mut confirmed = crate::tests::snapshot(8, true, true);
    confirmed.config_revision = Some(8);
    confirmed.overlay = settings;
    reply.respond(Ok(confirmed)).unwrap();
    visual.run_until_parked();
    let crate::SettingsCommand::SetOverlaySettings {
        expected_config_revision,
        settings,
        reply,
    } = endpoint.try_recv().unwrap()
    else {
        panic!("latest typed overlay command");
    };
    assert_eq!(expected_config_revision, 8);
    assert_eq!(settings.window_background_color, [0, 0, 255]);
    assert_eq!(
        settings,
        SettingsOverlay {
            window_background_color: [0, 0, 255],
            ..original
        }
    );
    let mut confirmed = crate::tests::snapshot(9, true, true);
    confirmed.config_revision = Some(9);
    confirmed.overlay = settings;
    reply.respond(Ok(confirmed)).unwrap();
    visual.run_until_parked();
    assert!(!view.read_with(visual, |view, _| {
        view.window_background_debouncer.is_pending()
    }));
}

#[gpui_kit::test]
fn connection_draft_waits_for_apply_and_survives_a_failed_request(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    view.update(visual, |view, _| view.snapshot = Some(initial));
    view.update(visual, |view, cx| {
        view.edit_network(NetworkEditor::Connection, cx);
        view.set_network_client_source(SettingsNetworkClientSource::Manual, cx);
        view.set_network_client_url("ws://192.168.1.20:8787".into(), cx);
        view.set_network_client_token("test-key".into(), cx);
    });
    visual.run_until_parked();
    assert!(endpoint.try_recv().is_err());
    view.update(visual, |view, cx| view.apply_network_edit(cx));
    visual.run_until_parked();
    let crate::SettingsCommand::SetNetworkSettings {
        settings, reply, ..
    } = endpoint.try_recv().unwrap()
    else {
        panic!("expected one composed connection command");
    };
    assert!(settings.client.enabled);
    assert_eq!(
        settings.client.url.as_deref(),
        Some("ws://192.168.1.20:8787")
    );
    assert_eq!(settings.client.token.as_deref(), Some("test-key"));
    reply
        .respond(Err(SettingsError::new(
            SettingsErrorCode::NetworkRoomFailed,
        )))
        .unwrap();
    visual.run_until_parked();
    assert!(view.read_with(visual, |view, _| view.network_draft.is_some()));
    view.update(visual, |view, cx| view.cancel_network_edit(cx));
    assert!(view.read_with(visual, |view, _| view.network_draft.is_none()));
}

#[test]
fn settings_error_display_matches_the_english_catalog_copy() {
    for code in SettingsErrorCode::ALL {
        let error = SettingsError::new(code);
        assert_eq!(
            settings_error(SettingsLanguage::English, error),
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

#[cfg(target_os = "linux")]
struct AlphaPageHarness {
    view: Entity<SettingsView>,
    snapshot: SettingsSnapshot,
    status: crate::SettingsAlphaSystemStatus,
}

#[cfg(target_os = "linux")]
impl Render for AlphaPageHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                Settings::new("alpha-test-settings").page(SettingPage::new("Alpha").group(
                    alpha_output::group(
                        &self.view,
                        Some(&self.snapshot),
                        SettingsLanguage::English,
                        Some(self.status),
                        false,
                    ),
                )),
            )
    }
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_install_button_sends_the_management_command(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    let status = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(true),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::NotInstalled,
    };
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.alpha_status_checked = true;
            view.snapshot = Some(snapshot.clone());
        });
        Root::new(
            cx.new(|_| AlphaPageHarness {
                view,
                snapshot,
                status,
            }),
            window,
            cx,
        )
    });
    visual.simulate_resize(size(px(800.0), px(1200.0)));
    visual.update(|window, cx| window.render_frame(cx));
    visual.update(|window, cx| window.click("alpha-install", cx));
    visual.run_until_parked();
    let crate::SettingsCommand::ManageAlphaCamera {
        expected_config_revision,
        action,
        reply,
    } = endpoint.try_recv().expect("install command")
    else {
        panic!("clicking install must request camera management");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(action, crate::SettingsAlphaManagementAction::Install);
    reply
        .respond(Err(
            crate::SettingsAlphaManagementError::AuthorizationDenied,
        ))
        .unwrap();
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_install_button_is_disabled_without_the_driver(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    let status = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(false),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::NotInstalled,
    };
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.alpha_status_checked = true;
            view.snapshot = Some(snapshot.clone());
        });
        Root::new(
            cx.new(|_| AlphaPageHarness {
                view,
                snapshot,
                status,
            }),
            window,
            cx,
        )
    });
    visual.simulate_resize(size(px(800.0), px(1200.0)));
    visual.update(|window, cx| window.render_frame(cx));
    visual.update(|window, cx| window.click("alpha-install", cx));
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "clicking the disabled install button must not invoke management"
    );
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_install_button_remains_reachable_at_minimum_window_size(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    let status = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(true),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::NotInstalled,
    };
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.alpha_status_checked = true;
            view.snapshot = Some(snapshot.clone());
        });
        Root::new(
            cx.new(|_| AlphaPageHarness {
                view,
                snapshot,
                status,
            }),
            window,
            cx,
        )
    });
    visual.simulate_resize(size(px(800.0), px(600.0)));
    visual.update(|window, cx| window.render_frame(cx));
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: point(px(600.0), px(450.0)),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.0), px(-800.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.update(|window, cx| window.render_frame(cx));
    visual.update(|window, cx| window.click("alpha-install", cx));
    visual.run_until_parked();
    let crate::SettingsCommand::ManageAlphaCamera {
        expected_config_revision,
        action,
        reply,
    } = endpoint.try_recv().expect("install command")
    else {
        panic!("clicking install must request camera management");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(action, crate::SettingsAlphaManagementAction::Install);
    reply
        .respond(Err(
            crate::SettingsAlphaManagementError::AuthorizationDenied,
        ))
        .unwrap();
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_uninstall_button_sends_the_management_command(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    let status = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(true),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::Installed,
    };
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.alpha_status_checked = true;
            view.snapshot = Some(snapshot.clone());
        });
        Root::new(
            cx.new(|_| AlphaPageHarness {
                view,
                snapshot,
                status,
            }),
            window,
            cx,
        )
    });
    visual.simulate_resize(size(px(800.0), px(1200.0)));
    visual.update(|window, cx| window.render_frame(cx));
    visual.update(|window, cx| window.click("alpha-uninstall", cx));
    visual.run_until_parked();
    let crate::SettingsCommand::ManageAlphaCamera {
        expected_config_revision,
        action,
        reply,
    } = endpoint.try_recv().expect("uninstall command")
    else {
        panic!("clicking uninstall must request camera management");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(action, crate::SettingsAlphaManagementAction::Uninstall);
    reply
        .respond(Err(
            crate::SettingsAlphaManagementError::AuthorizationDenied,
        ))
        .unwrap();
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_install_reports_denial_and_rechecks_real_state(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    view.update(visual, |view, _| view.snapshot = Some(initial.clone()));
    view.update(visual, |view, cx| {
        view.manage_alpha_camera(crate::SettingsAlphaManagementAction::Install, cx);
    });
    visual.run_until_parked();
    let crate::SettingsCommand::ManageAlphaCamera {
        expected_config_revision,
        action,
        reply,
    } = endpoint.try_recv().expect("management command")
    else {
        panic!("install must request camera management");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(action, crate::SettingsAlphaManagementAction::Install);
    assert!(view.read_with(visual, |view, _| view.alpha_operation_pending));
    reply
        .respond(Err(
            crate::SettingsAlphaManagementError::AuthorizationDenied,
        ))
        .unwrap();
    visual.run_until_parked();
    let crate::SettingsCommand::ReadAlphaSystemStatus { reply } =
        endpoint.try_recv().expect("status refresh")
    else {
        panic!("denial must refresh device state");
    };
    let observed = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(true),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::NotInstalled,
    };
    reply.respond(Ok(observed)).unwrap();
    visual.run_until_parked();
    let crate::SettingsCommand::ReadSnapshot { reply } =
        endpoint.try_recv().expect("snapshot refresh")
    else {
        panic!("denial must refresh the settings snapshot");
    };
    reply.respond(Ok(initial)).unwrap();
    visual.run_until_parked();
    assert!(view.read_with(visual, |view, _| {
        !view.alpha_operation_pending && view.alpha_system_status == Some(observed)
    }));
    assert!(
        !alpha_output::error_text(
            SettingsLanguage::ChineseSimplified,
            crate::SettingsAlphaManagementError::AuthorizationDenied
        )
        .is_empty()
    );
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_refresh_shows_the_observed_system_state(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, cx| view.refresh_alpha_status(cx));
    visual.run_until_parked();
    let crate::SettingsCommand::ReadAlphaSystemStatus { reply } =
        endpoint.try_recv().expect("Alpha status request")
    else {
        panic!("refresh must request the system status");
    };
    let observed = crate::SettingsAlphaSystemStatus {
        driver_installed: Some(true),
        module_loaded: Some(false),
        management_available: true,
        management_unavailable: None,
        camera: crate::SettingsAlphaInstallationStatus::NotInstalled,
    };
    reply.respond(Ok(observed)).unwrap();
    visual.run_until_parked();
    assert_eq!(
        view.read_with(visual, |view, _| view.alpha_system_status),
        Some(observed)
    );
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn alpha_switch_sends_a_typed_persisted_setting(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.alpha_output_enabled = false;
    view.update(visual, |view, _| view.snapshot = Some(initial));
    view.update(visual, |view, cx| view.set_alpha_output_enabled(true, cx));
    visual.run_until_parked();
    let crate::SettingsCommand::SetAlphaOutputEnabled {
        expected_config_revision,
        enabled,
        reply,
    } = endpoint.try_recv().expect("Alpha output command")
    else {
        panic!("the switch must use the typed Alpha output command");
    };
    assert_eq!(expected_config_revision, 7);
    assert!(enabled);
    let mut confirmed = crate::tests::snapshot(8, true, true);
    confirmed.config_revision = Some(8);
    confirmed.alpha_output_enabled = true;
    reply.respond(Ok(confirmed)).unwrap();
    visual.run_until_parked();
    assert!(view.read_with(visual, |view, _| {
        view.snapshot.as_ref().unwrap().alpha_output_enabled
    }));
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
