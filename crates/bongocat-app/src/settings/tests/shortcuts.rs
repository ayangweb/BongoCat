//! Persisting, suspending and rejecting the shortcut bindings.

use super::*;

#[test]
fn shortcut_config_errors_map_to_a_stable_settings_code() {
    for field in [
        "shortcuts.command_bindings",
        "shortcuts.command",
        "shortcuts.behavior",
        "shortcuts.binding",
        "shortcuts.conflict",
    ] {
        assert_eq!(
            settings_config_error_code(&ConfigError::InvalidValue(field)),
            Some(SettingsErrorCode::InvalidShortcutBindings),
            "field {field}"
        );
    }
    assert_eq!(
        settings_config_error_code(&ConfigError::InvalidValue("appearance.language")),
        None
    );
}

#[test]
fn service_persists_shortcuts_and_restores_them_after_restart() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let expected = shortcut_fixture();
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let updated = client
        .set_shortcuts_blocking(
            initial.config_revision.expect("config revision"),
            expected.clone(),
        )
        .expect("persist shortcuts");
    assert_eq!(updated.shortcuts, expected);
    assert!(updated.revision > initial.revision);
    assert_ne!(updated.config_revision, initial.config_revision);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(persisted.contains("toggle_overlay"));
    assert!(persisted.contains("Control+Alt+B"));
    assert!(persisted.contains("motion:TapBody:0"));
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");

    let restarted = Application::start_with_layout(layout).expect("application restart");
    let restarted_service = ApplicationSettingsService::start(restarted).expect("service restart");
    let restored = restarted_service
        .client()
        .read_snapshot_blocking()
        .expect("restored snapshot");
    assert_eq!(restored.shortcuts, expected);
    restarted_service
        .client()
        .shutdown_blocking()
        .expect("restarted service shutdown");
    restarted_service.join().expect("restarted service join");
}

#[test]
fn service_suspends_and_restores_shortcuts_without_persisting_capture_state() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let configured = client
        .set_shortcuts_blocking(
            initial.config_revision.expect("config revision"),
            shortcut_fixture(),
        )
        .expect("configure shortcut");
    let persisted_before_capture = std::fs::read(&layout.config).expect("persisted config");

    let suspended = client
        .suspend_shortcut_capture_blocking(
            configured.config_revision.expect("configured revision"),
            SettingsShortcuts::default(),
        )
        .expect("suspend shortcut capture");
    assert_eq!(suspended.shortcuts, configured.shortcuts);
    assert_eq!(
        std::fs::read(&layout.config).expect("capture must not persist"),
        persisted_before_capture
    );

    let resumed = client
        .resume_shortcut_capture_blocking()
        .expect("resume shortcut capture");
    assert_eq!(resumed.shortcuts, configured.shortcuts);
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_executes_application_shortcuts_from_the_platform_handoff() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("start application");
    let (sender, receiver) = std::sync::mpsc::sync_channel(4);
    let service = ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
        .expect("start settings service");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    sender
        .send(ShortcutCommand::ToggleOverlay)
        .expect("queue application shortcut");
    // Probe the cheap revision while the handoff is in flight. Building a
    // full settings snapshot scans the model catalog, which can exceed the
    // test's whole wait under the workspace's parallel load.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let updated = loop {
        let revision = client
            .read_snapshot_revision_blocking()
            .expect("shortcut revision");
        if revision > initial.revision {
            let snapshot = client.read_snapshot_blocking().expect("updated snapshot");
            if !snapshot.overlay_visible {
                break snapshot;
            }
        }
        if std::time::Instant::now() >= deadline {
            break client.read_snapshot_blocking().expect("updated snapshot");
        }
        std::thread::yield_now();
    };
    assert!(!updated.overlay_visible);
    assert_eq!(updated.config_revision, initial.config_revision);
    drop(sender);
    client.shutdown_blocking().expect("shutdown service");
    service.join().expect("join service");
}

#[test]
fn service_canonicalizes_shortcuts_before_persisting_them() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let submitted = SettingsShortcuts {
        commands: vec![SettingsShortcutBinding {
            command: " toggle_overlay ".to_owned(),
            shortcut: " shift + ctrl + b ".to_owned(),
        }],
        model_behaviors: vec![SettingsModelBehaviorBinding {
            model: SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
            behavior_id: " expression: happy ".to_owned(),
            shortcut: "cmd+option+p".to_owned(),
        }],
    };
    let expected = shortcut_fixture_with(
        "toggle_overlay",
        "Control+Shift+B",
        "expression:happy",
        "Alt+Meta+P",
    );
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let updated = client
        .set_shortcuts_blocking(initial.config_revision.expect("config revision"), submitted)
        .expect("canonicalize shortcuts");
    assert_eq!(updated.shortcuts, expected);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(!persisted.contains(" shift + ctrl + b "));
    assert!(persisted.contains("Control+Shift+B"));
    assert!(persisted.contains("expression:happy"));
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_persists_behavior_shortcut_state_and_rejects_stale_updates() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(!initial.behavior_shortcuts_enabled);
    let initial_revision = initial.config_revision.expect("config revision");

    let enabled = client
        .set_behavior_shortcuts_enabled_blocking(initial_revision, true)
        .expect("enable behavior shortcuts");
    assert!(enabled.behavior_shortcuts_enabled);
    assert!(
        std::fs::read_to_string(&layout.config)
            .expect("persisted config")
            .contains("\"model_behaviors_enabled\": true")
    );

    let error = client
        .set_behavior_shortcuts_enabled_blocking(initial_revision, false)
        .expect_err("stale behavior shortcut update");
    assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
    assert!(
        client
            .read_snapshot_blocking()
            .expect("unchanged snapshot")
            .behavior_shortcuts_enabled
    );
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");

    let restarted = Application::start_with_layout(layout).expect("application restart");
    let restarted_service =
        ApplicationSettingsService::start(restarted).expect("restarted service");
    let restarted_client = restarted_service.client();
    assert!(
        restarted_client
            .read_snapshot_blocking()
            .expect("restarted snapshot")
            .behavior_shortcuts_enabled
    );
    restarted_client
        .shutdown_blocking()
        .expect("restarted service shutdown");
    restarted_service.join().expect("restarted service join");
}

#[test]
fn service_rejects_invalid_shortcuts_without_mutating_config() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let original_config = std::fs::read(&layout.config).expect("initial config");
    let cases = [
        SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: "unknown".to_owned(),
                shortcut: "Control+Alt+B".to_owned(),
            }],
            ..SettingsShortcuts::default()
        },
        SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+".to_owned(),
            }],
            ..SettingsShortcuts::default()
        },
        SettingsShortcuts {
            model_behaviors: vec![SettingsModelBehaviorBinding {
                model: SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
                behavior_id: "physics:0".to_owned(),
                shortcut: "Control+Alt+M".to_owned(),
            }],
            ..SettingsShortcuts::default()
        },
    ];
    for shortcuts in cases {
        let error = client
            .set_shortcuts_blocking(initial.config_revision.expect("config revision"), shortcuts)
            .expect_err("invalid shortcut binding");
        assert_eq!(error.code(), SettingsErrorCode::InvalidShortcutBindings);
        assert_eq!(
            std::fs::read(&layout.config).expect("config remains readable"),
            original_config
        );
    }
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged, initial);
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_rejects_stale_shortcuts_without_mutating_config_or_snapshot() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let committed = client
        .set_shortcuts_blocking(
            initial.config_revision.expect("config revision"),
            shortcut_fixture(),
        )
        .expect("first shortcut update");
    let committed_config = std::fs::read(&layout.config).expect("committed config");
    let stale = SettingsShortcuts {
        commands: vec![SettingsShortcutBinding {
            command: "toggle_mirror".to_owned(),
            shortcut: "Control+Alt+X".to_owned(),
        }],
        ..SettingsShortcuts::default()
    };
    let error = client
        .set_shortcuts_blocking(initial.config_revision.expect("config revision"), stale)
        .expect_err("stale shortcut update");
    assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged, committed);
    assert_eq!(
        std::fs::read(&layout.config).expect("preserved config"),
        committed_config
    );
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

/// The Shortcuts page renders from this snapshot: the model catalog
/// supplies the rows and the shortcut list supplies the chord shown in each
/// one. Auto-assignment only counts if it reaches here — before it landed
/// the list stayed empty and every row rendered blank.
#[test]
fn snapshot_carries_the_auto_assigned_behavior_shortcuts() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(initial.shortcuts.model_behaviors.is_empty());
    assert!(!initial.behavior_shortcuts_enabled);

    let selected = client
        .select_model_blocking(
            initial.config_revision.expect("config revision"),
            SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
        )
        .expect("select standard model");

    // Seven behaviours, one chord each, in the legacy order.
    assert_eq!(selected.shortcuts.model_behaviors.len(), 7);
    let primary = if cfg!(target_os = "macos") {
        "Meta"
    } else {
        "Control"
    };
    for (behavior_id, slot) in [
        ("motion:CAT_motion:0", 1),
        ("motion:CAT_motion_lock:1", 4),
        ("expression:live2d_expression2.exp3.json", 7),
    ] {
        let binding = selected
            .shortcuts
            .model_behaviors
            .iter()
            .find(|binding| binding.behavior_id == behavior_id)
            .unwrap_or_else(|| panic!("{behavior_id} has no default binding"));
        assert_eq!(binding.model.id, "standard");
        assert_eq!(
            binding.shortcut,
            format!("{primary}+{slot}"),
            "{behavior_id}"
        );
    }

    // The rows themselves come from the catalog entry's behaviour list, so
    // a snapshot with bindings but no behaviours would still render empty.
    let entry = selected
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == "standard")
        .expect("standard model entry");
    match &entry.availability {
        SettingsModelAvailability::Ready { behaviors, .. } => {
            assert_eq!(behaviors.len(), 7);
        }
        SettingsModelAvailability::Invalid { .. } => {
            panic!("the bundled standard model must stay valid")
        }
    }
    assert!(selected.revision > initial.revision);
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}
