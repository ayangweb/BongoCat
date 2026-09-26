//! Building the settings snapshot and advancing its revision clock.

use super::*;

#[test]
fn config_write_failures_map_to_stable_settings_codes() {
    for (error, expected) in [
        (
            ConfigError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
            SettingsErrorCode::ConfigPermissionDenied,
        ),
        (
            ConfigError::Io(io::Error::from(io::ErrorKind::StorageFull)),
            SettingsErrorCode::ConfigStorageFull,
        ),
        (
            ConfigError::WriteTargetOccupied,
            SettingsErrorCode::ConfigTargetOccupied,
        ),
    ] {
        assert_eq!(settings_config_error_code(&error), Some(expected));
        assert_eq!(
            map_application_error(ApplicationError::Config(error)).code(),
            expected
        );
    }
}

#[test]
fn input_diagnostics_projection_is_complete_and_advances_its_own_revision() {
    let input = InputSnapshot {
        pressed_key_count: 1,
        pressed_mouse_button_count: 2,
        pressed_gamepad_button_count: 20,
        connected_gamepad_count: 21,
        diagnostics: InputDiagnostics {
            captured_down: 3,
            captured_up: 4,
            reconciled_release: 5,
            fallback_release: 26,
            released_by_reset: 6,
            duplicate_down: 7,
            unmatched_release: 8,
            invalid_source: 9,
            reset_count: 10,
            sequence_gap_count: 11,
            missing_sequence_count: 12,
            duplicate_sequence_count: 13,
            out_of_order_sequence_count: 14,
            non_monotonic_time_count: 15,
            gamepad_connections: 22,
            gamepad_disconnections: 23,
            stale_gamepad_events: 24,
            released_by_disconnect: 25,
        },
        transport: InputTransportDiagnostics {
            enqueued: 16,
            queue_full: 17,
            recovered_after_overflow: 18,
            runtime_stopped: 19,
        },
        ..InputSnapshot::default()
    };
    let projected = settings_input_diagnostics(
        &input,
        PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::PermissionDenied,
            service_error_code: Some("platform_input_permission_denied"),
            service_start_attempts: 1,
            gamepad_backend_failures: 26,
            gamepad_connection_rejections: 27,
            gamepad_button_edges: 28,
            gamepad_axis_samples: 29,
            gamepad_axis_publish_rejections: 30,
            gamepad_event_discards: 31,
            ..PlatformInputDiagnostics::default()
        },
        SettingsInputMonitoringPermission::Granted,
    );
    // The permission is handed in rather than queried here, so what the projection
    // reports is exactly what the caller resolved.
    assert_eq!(
        projected.input_monitoring_permission,
        SettingsInputMonitoringPermission::Granted
    );
    assert_eq!(
        projected.service_status,
        SettingsInputServiceStatus::PermissionDenied
    );
    assert_eq!(projected.service_start_attempts, 1);
    assert_eq!(
        projected.service_error_code,
        Some("platform_input_permission_denied")
    );
    assert_eq!(projected.pressed_key_count, 1);
    assert_eq!(projected.pressed_mouse_button_count, 2);
    assert_eq!(projected.pressed_gamepad_button_count, 20);
    assert_eq!(projected.connected_gamepad_count, 21);
    assert_eq!(projected.platform_gamepad_backend_failures, 26);
    assert_eq!(projected.platform_gamepad_connection_rejections, 27);
    assert_eq!(projected.platform_gamepad_button_edges, 28);
    assert_eq!(projected.platform_gamepad_axis_samples, 29);
    assert_eq!(projected.platform_gamepad_axis_publish_rejections, 30);
    assert_eq!(projected.platform_gamepad_event_discards, 31);
    assert_eq!(projected.captured_down, 3);
    assert_eq!(projected.captured_up, 4);
    assert_eq!(projected.reconciled_release, 5);
    assert_eq!(projected.fallback_release, 26);
    assert_eq!(projected.released_by_reset, 6);
    assert_eq!(projected.duplicate_down, 7);
    assert_eq!(projected.unmatched_release, 8);
    assert_eq!(projected.invalid_source, 9);
    assert_eq!(projected.reset_count, 10);
    assert_eq!(projected.sequence_gap_count, 11);
    assert_eq!(projected.missing_sequence_count, 12);
    assert_eq!(projected.duplicate_sequence_count, 13);
    assert_eq!(projected.out_of_order_sequence_count, 14);
    assert_eq!(projected.non_monotonic_time_count, 15);
    assert_eq!(projected.gamepad_connections, 22);
    assert_eq!(projected.gamepad_disconnections, 23);
    assert_eq!(projected.stale_gamepad_events, 24);
    assert_eq!(projected.released_by_disconnect, 25);
    assert_eq!(projected.transport_enqueued, 16);
    assert_eq!(projected.transport_queue_full, 17);
    assert_eq!(projected.transport_recovered_after_overflow, 18);
    assert_eq!(projected.transport_runtime_stopped, 19);

    let mut clock = SettingsSnapshotClock::new(Some(7));
    let _ = clock.observe_input_diagnostics(projected);
    assert_eq!(clock.revision, 0);
    let changed = SettingsInputDiagnostics {
        transport_queue_full: 20,
        ..projected
    };
    let _ = clock.observe_input_diagnostics(changed);
    assert_eq!(clock.revision, 1);
    let _ = clock.observe_input_diagnostics(changed);
    assert_eq!(clock.revision, 1);
}

#[test]
fn runtime_work_diagnostics_projection_preserves_snapshot_values() {
    let owner = RuntimeOwner::start(false, 4);
    let mut runtime = owner.client().snapshot();
    runtime.work = RuntimeWorkDiagnostics {
        budget_exceeded: 7,
        last_over_budget_ms: 19,
    };

    let projected = settings_runtime_diagnostics(&runtime);
    assert_eq!(projected.work_budget_exceeded, 7);
    assert_eq!(projected.last_over_budget_ms, 19);

    owner
        .shutdown(Duration::from_secs(1))
        .expect("runtime shutdown");
}

#[test]
fn runtime_shutdown_diagnostics_projection_preserves_snapshot_values() {
    let owner = RuntimeOwner::start(false, 4);
    let mut runtime = owner.client().snapshot();
    runtime.shutdown = bongocat_runtime::RuntimeShutdownDiagnostics {
        timed_out: 3,
        worker_panicked: 2,
    };

    let projected = settings_runtime_diagnostics(&runtime);
    assert_eq!(projected.shutdown_timed_out, 3);
    assert_eq!(projected.shutdown_worker_panicked, 2);

    owner
        .shutdown(Duration::from_secs(1))
        .expect("runtime shutdown");
}

#[test]
fn snapshot_clock_coalesces_changes_observed_in_one_snapshot() {
    let diagnostics = SettingsInputDiagnostics::default();
    let startup = SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled);
    let mut clock = SettingsSnapshotClock::new(Some(7));
    clock.observe_config(Some(8));
    let _ = clock.observe_input_diagnostics(diagnostics);
    let _ = clock.observe_startup_item(startup);
    clock.mark_catalog_changed();
    clock.observe_diagnostics_export(SettingsDiagnosticsExportStatus {
        format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
        bytes_written: 1,
        preview_bundle_format_version: 1,
        preview_bundle_bytes_written: 2,
        preview_bundle_entry_count: 3,
        preview_bundle_skipped_source_files: 0,
    });
    clock.coalesce_changes_since(0);
    assert_eq!(clock.revision, 1);

    clock.coalesce_changes_since(1);
    assert_eq!(clock.revision, 1);
}

#[test]
fn input_start_failures_degrade_health_without_treating_stop_as_failure() {
    for status in [
        SettingsInputServiceStatus::PermissionDenied,
        SettingsInputServiceStatus::BackendUnavailable,
        SettingsInputServiceStatus::Failed,
    ] {
        assert!(input_service_is_degraded(status));
    }
    for status in [
        SettingsInputServiceStatus::NotStarted,
        SettingsInputServiceStatus::Running,
        SettingsInputServiceStatus::Stopped,
    ] {
        assert!(!input_service_is_degraded(status));
    }
}

#[test]
fn input_service_error_code_is_preserved_without_guessing_from_status() {
    let diagnostics = settings_input_diagnostics(
        &InputSnapshot::default(),
        PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::Failed,
            service_error_code: Some("platform_input_tap_create_failed"),
            ..PlatformInputDiagnostics::default()
        },
        SettingsInputMonitoringPermission::Unsupported,
    );
    assert_eq!(
        diagnostics.service_status,
        SettingsInputServiceStatus::Failed
    );
    assert_eq!(
        diagnostics.service_error_code,
        Some("platform_input_tap_create_failed")
    );
}

#[test]
fn input_service_error_code_drops_unregistered_provider_details() {
    let diagnostics = settings_input_diagnostics(
        &InputSnapshot::default(),
        PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::Failed,
            service_error_code: Some("platform_input_private_detail"),
            ..PlatformInputDiagnostics::default()
        },
        SettingsInputMonitoringPermission::Unsupported,
    );
    assert_eq!(
        diagnostics.service_status,
        SettingsInputServiceStatus::Failed
    );
    assert_eq!(diagnostics.service_error_code, None);
}

/// The probe reports what a full snapshot would, without building one.
///
/// The application polls the revision at 20 Hz for the system menu, so the cheap
/// answer has to be exact: same value as the snapshot a client would read next, and
/// moving whenever the configuration does.
#[test]
fn the_revision_probe_matches_the_snapshot_it_stands_in_for() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let revision = client
        .read_snapshot_revision_blocking()
        .expect("initial revision");
    let snapshot = client.read_snapshot_blocking().expect("initial snapshot");
    assert_eq!(revision, snapshot.revision);
    assert_eq!(
        client
            .read_snapshot_revision_blocking()
            .expect("stable revision"),
        revision,
        "a probe between two unchanged snapshots must not invent a revision"
    );

    let changed = client
        .set_overlay_visible_blocking(
            snapshot.config_revision.expect("configuration revision"),
            false,
        )
        .expect("overlay visibility");
    assert!(changed.revision > revision);
    assert_eq!(
        client
            .read_snapshot_revision_blocking()
            .expect("changed revision"),
        changed.revision,
        "the probe must report the change the command already published"
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

/// The input-monitoring permission is a system query, not a per-snapshot one.
///
/// Snapshot builds happen for every settings command, for the window's refresh and
/// for the system-menu poll, and the macOS answer costs milliseconds, so the cache
/// has to hold it for a while and still pick up a permission the user grants while
/// the product runs.
#[test]
fn the_input_monitoring_permission_is_cached_until_it_goes_stale() {
    let started = Instant::now();
    let answer = std::cell::Cell::new(SettingsInputMonitoringPermission::Denied);
    let probes = std::cell::Cell::new(0_u32);
    let probe = || {
        probes.set(probes.get() + 1);
        answer.get()
    };
    let mut cache = InputMonitoringPermissionCache::default();

    assert_eq!(
        cache.resolve(started, probe),
        SettingsInputMonitoringPermission::Denied
    );
    assert_eq!(probes.get(), 1, "the first read queries the system");

    answer.set(SettingsInputMonitoringPermission::Granted);
    assert_eq!(
        cache.resolve(
            started + INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL / 2,
            probe
        ),
        SettingsInputMonitoringPermission::Denied,
        "a fresh answer is reused instead of re-queried"
    );
    assert_eq!(probes.get(), 1);
    assert_eq!(
        cache.resolve(
            started + INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL,
            probe
        ),
        SettingsInputMonitoringPermission::Granted,
        "a stale answer is re-queried"
    );
    assert_eq!(probes.get(), 2);
}
