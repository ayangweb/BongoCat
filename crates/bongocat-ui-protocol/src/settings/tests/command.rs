//! Every command carries its typed request and gets a typed reply.

use super::*;

#[test]
fn commands_are_bounded_ordered_and_receive_typed_replies() {
    let (client, endpoint) = SettingsClient::bounded(2);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetOverlayVisible {
            expected_config_revision,
            visible,
            reply,
        } = endpoint.recv_blocking().expect("first command")
        else {
            panic!("unexpected first command");
        };
        assert_eq!(expected_config_revision, 1);
        assert!(!visible);
        reply
            .respond(Ok(snapshot(2, false, true)))
            .expect("first reply");

        let SettingsCommand::SetMotionAudioEnabled {
            expected_config_revision,
            enabled,
            reply,
        } = endpoint.recv_blocking().expect("second command")
        else {
            panic!("unexpected second command");
        };
        assert_eq!(expected_config_revision, 2);
        assert!(!enabled);
        reply
            .respond(Ok(snapshot(3, false, false)))
            .expect("second reply");
    });

    let first = client.set_overlay_visible_blocking(1, false);
    let second = client.set_motion_audio_enabled_blocking(2, false);
    assert_eq!(first.expect("first snapshot").revision, 2);
    assert_eq!(second.expect("second snapshot").revision, 3);
    worker.join().expect("worker join");
}

#[test]
fn gamepad_axis_settings_command_preserves_typed_values() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetGamepadAxisSettings {
            expected_config_revision,
            settings,
            reply,
        } = endpoint.recv_blocking().expect("gamepad command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(
            settings,
            SettingsGamepadAxisSettings {
                stick_dead_zone_percent: 25,
                trigger_dead_zone_percent: 10,
            }
        );
        let mut result = snapshot(8, true, true);
        result.gamepad_axis_settings = settings;
        reply.respond(Ok(result)).expect("gamepad reply");
    });
    let result = client
        .set_gamepad_axis_settings_blocking(
            7,
            SettingsGamepadAxisSettings {
                stick_dead_zone_percent: 25,
                trigger_dead_zone_percent: 10,
            },
        )
        .expect("gamepad snapshot");
    assert_eq!(result.gamepad_axis_settings.stick_dead_zone_percent, 25);
    worker.join().expect("worker join");
}

#[test]
fn logging_settings_command_preserves_the_complete_typed_policy() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let expected = SettingsLogging {
        level: SettingsLogLevel::Trace,
        retention_days: 30,
    };
    let worker = thread::spawn(move || {
        let SettingsCommand::SetLoggingSettings {
            expected_config_revision,
            settings,
            reply,
        } = endpoint.recv_blocking().expect("logging command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(settings, expected);
        let mut result = snapshot(8, true, true);
        result.logging = settings;
        reply.respond(Ok(result)).expect("logging reply");
    });
    let result = client
        .set_logging_settings_blocking(7, expected)
        .expect("logging snapshot");
    assert_eq!(result.logging, expected);
    worker.join().expect("worker join");
}

#[test]
fn appearance_theme_command_preserves_typed_selection() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetAppearanceTheme {
            expected_config_revision,
            theme,
            reply,
        } = endpoint.recv_blocking().expect("appearance theme command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(theme, SettingsTheme::Dark);
        let mut result = snapshot(8, true, true);
        result.appearance_theme = theme;
        reply.respond(Ok(result)).expect("appearance theme reply");
    });
    let result = client
        .set_appearance_theme_blocking(7, SettingsTheme::Dark)
        .expect("appearance theme snapshot");
    assert_eq!(result.appearance_theme, SettingsTheme::Dark);
    worker.join().expect("worker join");
}

#[test]
fn language_command_preserves_typed_selection() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetLanguage {
            expected_config_revision,
            language,
            reply,
        } = endpoint.recv_blocking().expect("language command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(language, SettingsLanguage::ChineseSimplified);
        let mut result = snapshot(8, true, true);
        result.language = language;
        reply.respond(Ok(result)).expect("language reply");
    });
    let result = client
        .set_language_blocking(7, SettingsLanguage::ChineseSimplified)
        .expect("language snapshot");
    assert_eq!(result.language, SettingsLanguage::ChineseSimplified);
    worker.join().expect("worker join");
}

#[test]
fn status_icon_command_preserves_typed_visibility() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetStatusIconVisible {
            expected_config_revision,
            visible,
            reply,
        } = endpoint.recv_blocking().expect("status icon command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert!(!visible);
        let mut result = snapshot(8, true, true);
        result.status_icon_visible = visible;
        reply.respond(Ok(result)).expect("status icon reply");
    });
    let result = client
        .set_status_icon_visible_blocking(7, false)
        .expect("status icon snapshot");
    assert!(!result.status_icon_visible);
    worker.join().expect("worker join");
}

#[test]
fn taskbar_icon_command_preserves_typed_visibility() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetTaskbarIconVisible {
            expected_config_revision,
            visible,
            reply,
        } = endpoint.recv_blocking().expect("taskbar icon command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert!(!visible);
        let mut result = snapshot(8, true, true);
        result.taskbar_icon_visible = visible;
        reply.respond(Ok(result)).expect("taskbar icon reply");
    });
    let result = client
        .set_taskbar_icon_visible_blocking(7, false)
        .expect("taskbar icon snapshot");
    assert!(!result.taskbar_icon_visible);
    worker.join().expect("worker join");
}

#[test]
fn automatic_update_check_command_preserves_typed_preference() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetCheckForUpdatesAutomatically {
            expected_config_revision,
            enabled,
            reply,
        } = endpoint.recv_blocking().expect("automatic update command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert!(!enabled);
        let mut result = snapshot(8, true, true);
        result.check_for_updates_automatically = enabled;
        reply.respond(Ok(result)).expect("automatic update reply");
    });
    let result = client
        .set_check_for_updates_automatically_blocking(7, false)
        .expect("automatic update snapshot");
    assert!(!result.check_for_updates_automatically);
    worker.join().expect("worker join");
}

#[test]
fn automatic_update_settings_read_returns_only_the_schedule() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::ReadAutomaticUpdateSettings { reply } = endpoint
            .recv_blocking()
            .expect("automatic update settings command")
        else {
            panic!("unexpected command");
        };
        reply
            .respond(Ok(AutomaticUpdateSettings {
                enabled: true,
                interval_hours: 48,
            }))
            .expect("automatic update settings reply");
    });
    assert_eq!(
        client
            .read_automatic_update_settings_blocking()
            .expect("automatic update settings"),
        AutomaticUpdateSettings {
            enabled: true,
            interval_hours: 48,
        }
    );
    worker.join().expect("worker join");
}

#[test]
fn check_for_updates_interval_command_preserves_typed_value() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetCheckForUpdatesIntervalHours {
            expected_config_revision,
            interval_hours,
            reply,
        } = endpoint
            .recv_blocking()
            .expect("check-for-updates interval command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(interval_hours, 48);
        let mut result = snapshot(8, true, true);
        result.check_for_updates_interval_hours = interval_hours;
        reply
            .respond(Ok(result))
            .expect("check-for-updates interval reply");
    });
    let result = client
        .set_check_for_updates_interval_hours_blocking(7, 48)
        .expect("check-for-updates interval snapshot");
    assert_eq!(result.check_for_updates_interval_hours, 48);
    worker.join().expect("worker join");
}

#[test]
fn maximum_fps_command_preserves_typed_value() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetMaximumFps {
            expected_config_revision,
            maximum_fps,
            reply,
        } = endpoint.recv_blocking().expect("maximum FPS command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(maximum_fps, 120);
        let mut result = snapshot(8, true, true);
        result.maximum_fps = maximum_fps;
        reply.respond(Ok(result)).expect("maximum FPS reply");
    });
    let result = client
        .set_maximum_fps_blocking(7, 120)
        .expect("maximum FPS snapshot");
    assert_eq!(result.maximum_fps, 120);
    worker.join().expect("worker join");
}

#[test]
fn release_fallback_timeout_command_preserves_typed_value() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetReleaseFallbackTimeout {
            expected_config_revision,
            timeout_ms,
            reply,
        } = endpoint
            .recv_blocking()
            .expect("release fallback timeout command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(timeout_ms, 1_500);
        let mut result = snapshot(8, true, true);
        result.release_fallback_timeout_ms = timeout_ms;
        reply
            .respond(Ok(result))
            .expect("release fallback timeout reply");
    });
    let result = client
        .set_release_fallback_timeout_blocking(7, 1_500)
        .expect("release fallback timeout snapshot");
    assert_eq!(result.release_fallback_timeout_ms, 1_500);
    worker.join().expect("worker join");
}

#[test]
fn shortcut_command_preserves_typed_bindings() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let shortcuts = SettingsShortcuts {
        commands: vec![SettingsShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "Control+Alt+B".to_owned(),
        }],
        model_behaviors: vec![SettingsModelBehaviorBinding {
            model: SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
            behavior_id: "motion:TapBody:0".to_owned(),
            shortcut: "Control+Alt+M".to_owned(),
        }],
    };
    let expected = shortcuts.clone();
    let worker = thread::spawn(move || {
        let SettingsCommand::SetShortcuts {
            expected_config_revision,
            shortcuts,
            reply,
        } = endpoint.recv_blocking().expect("shortcut command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(shortcuts, expected);
        let mut result = snapshot(8, true, true);
        result.shortcuts = shortcuts;
        reply.respond(Ok(result)).expect("shortcut reply");
    });

    let result = client
        .set_shortcuts_blocking(7, shortcuts)
        .expect("shortcut snapshot");
    assert_eq!(result.shortcuts.commands.len(), 1);
    assert_eq!(result.shortcuts.model_behaviors.len(), 1);
    worker.join().expect("worker join");
}

#[test]
fn application_shortcut_handoff_is_typed_and_fire_and_forget() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::TriggerApplicationShortcut { command } = endpoint
            .recv_blocking()
            .expect("application shortcut command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(command, SettingsApplicationShortcut::ToggleOverlay);
    });
    client
        .enqueue_application_shortcut(SettingsApplicationShortcut::ToggleOverlay)
        .expect("queue application shortcut");
    worker.join().expect("worker join");
}

#[test]
fn configuration_backup_location_is_a_typed_command() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::OpenConfigBackupLocation { reply } =
            endpoint.recv_blocking().expect("backup location command")
        else {
            panic!("unexpected command");
        };
        reply
            .respond(Ok(snapshot(11, true, false)))
            .expect("backup location reply");
    });

    let unchanged = client
        .open_config_backup_location_blocking()
        .expect("backup location snapshot");
    assert_eq!(unchanged.revision, 11);
    assert!(unchanged.overlay_visible);
    assert!(!unchanged.motion_audio_enabled);
    worker.join().expect("worker join");
}

#[test]
fn log_location_command_is_typed_and_keeps_the_path_out_of_the_protocol() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::OpenLogsLocation { reply } =
            endpoint.recv_blocking().expect("log location command")
        else {
            panic!("unexpected command");
        };
        reply
            .respond(Ok(snapshot(13, true, true)))
            .expect("log location reply");
    });

    let unchanged = client
        .open_logs_location_blocking()
        .expect("log location snapshot");
    assert_eq!(unchanged.revision, 13);
    worker.join().expect("worker join");
}

#[test]
fn diagnostics_export_is_a_typed_command() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::ExportDiagnostics { reply } = endpoint
            .recv_blocking()
            .expect("diagnostics export command")
        else {
            panic!("unexpected command");
        };
        let mut exported = snapshot(12, true, true);
        exported.diagnostics_export = Some(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 512,
            preview_bundle_format_version: 1,
            preview_bundle_bytes_written: 768,
            preview_bundle_entry_count: 3,
            preview_bundle_skipped_source_files: 2,
        });
        reply.respond(Ok(exported)).expect("export reply");
    });

    let exported = client
        .export_diagnostics_blocking()
        .expect("diagnostics export snapshot");
    assert_eq!(
        exported.diagnostics_export,
        Some(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 512,
            preview_bundle_format_version: 1,
            preview_bundle_bytes_written: 768,
            preview_bundle_entry_count: 3,
            preview_bundle_skipped_source_files: 2,
        })
    );
    worker.join().expect("worker join");
}

#[test]
fn behavior_shortcuts_command_preserves_typed_state() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetBehaviorShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        } = endpoint
            .recv_blocking()
            .expect("behavior shortcuts command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert!(!enabled);
        let mut result = snapshot(8, true, true);
        result.behavior_shortcuts_enabled = false;
        reply.respond(Ok(result)).expect("behavior shortcuts reply");
    });

    let result = client
        .set_behavior_shortcuts_enabled_blocking(7, false)
        .expect("behavior shortcuts snapshot");
    assert!(!result.behavior_shortcuts_enabled);
    worker.join().expect("worker join");
}

#[test]
fn random_behavior_command_preserves_switch_and_interval() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let expected = SettingsRandomBehavior {
        enabled: true,
        interval_seconds: 12,
    };
    let worker = thread::spawn(move || {
        let SettingsCommand::SetRandomBehaviorSettings {
            expected_config_revision,
            settings,
            reply,
        } = endpoint.recv_blocking().expect("random behavior command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 7);
        assert_eq!(settings, expected);
        let mut result = snapshot(8, true, true);
        result.random_behavior = expected;
        reply.respond(Ok(result)).expect("random behavior reply");
    });

    let result = client
        .set_random_behavior_settings_blocking(7, expected)
        .expect("random behavior snapshot");
    assert_eq!(result.random_behavior, expected);
    worker.join().expect("worker join");
}

#[test]
fn command_shortcuts_command_preserves_typed_state() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::SetCommandShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        } = endpoint.recv_blocking().expect("command shortcuts command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(expected_config_revision, 9);
        assert!(!enabled);
        let mut result = snapshot(10, true, true);
        result.command_shortcuts_enabled = false;
        reply.respond(Ok(result)).expect("command shortcuts reply");
    });

    let result = client
        .set_command_shortcuts_enabled_blocking(9, false)
        .expect("command shortcuts snapshot");
    assert!(!result.command_shortcuts_enabled);
    worker.join().expect("worker join");
}
