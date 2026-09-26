//! Starting the worker, its capability seams and its shutdown.

use super::*;

#[test]
fn service_opens_anonymous_backup_location_without_advancing_revision() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
        SettingsStartupItemState::Disabled,
    )));
    let backup_location = Arc::new(TestBackupLocation::new());
    let service = ApplicationSettingsService::start_with_capabilities(
        application,
        startup_item,
        backup_location.clone(),
        Arc::new(TestDiagnosticsExport),
    )
    .expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let opened = client
        .open_config_backup_location_blocking()
        .expect("open backup location");
    assert_eq!(opened, initial);
    assert_eq!(backup_location.invocations.load(Ordering::Acquire), 1);

    backup_location.fail.store(true, Ordering::Release);
    let error = client
        .open_config_backup_location_blocking()
        .expect_err("backup location failure");
    assert_eq!(error.code(), SettingsErrorCode::BackupLocationOpenFailed);
    assert_eq!(
        error.to_string(),
        "The configuration backup folder could not be opened"
    );
    assert!(
        !error
            .to_string()
            .contains(base.path().to_string_lossy().as_ref())
    );
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged, initial);
    assert_eq!(backup_location.invocations.load(Ordering::Acquire), 2);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_applies_and_persists_status_icon_visibility_transactionally() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let status_icon = Arc::new(TestStatusIcon::new(true));
    let service =
        ApplicationSettingsService::start_with_status_icon(application, status_icon.clone())
            .expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(initial.status_icon_visible);
    let hidden = client
        .set_status_icon_visible_blocking(initial.config_revision.expect("config revision"), false)
        .expect("hide status icon");
    assert!(!hidden.status_icon_visible);
    assert!(!status_icon.visible());
    assert_eq!(status_icon.updates(), vec![false]);

    let stale = client
        .set_status_icon_visible_blocking(initial.config_revision.expect("config revision"), true)
        .expect_err("stale status icon update");
    assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
    assert_eq!(status_icon.updates(), vec![false]);

    status_icon.fail_updates.store(true, Ordering::Release);
    let failed = client
        .set_status_icon_visible_blocking(
            hidden.config_revision.expect("hidden config revision"),
            true,
        )
        .expect_err("platform update failure");
    assert_eq!(failed.code(), SettingsErrorCode::StatusIconUpdateFailed);
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged, hidden);
    assert!(!status_icon.visible());

    status_icon.fail_updates.store(false, Ordering::Release);
    let occupied = layout.config.with_extension("json.tmp");
    std::fs::create_dir(&occupied).expect("occupied temp target");
    let persist_failed = client
        .set_status_icon_visible_blocking(
            hidden.config_revision.expect("hidden config revision"),
            true,
        )
        .expect_err("config persist failure");
    assert_eq!(
        persist_failed.code(),
        SettingsErrorCode::ConfigTargetOccupied
    );
    assert!(!status_icon.visible());
    assert_eq!(status_icon.updates(), vec![false, true, true, false]);
    assert_eq!(
        client
            .read_snapshot_blocking()
            .expect("rolled back snapshot"),
        hidden
    );
    std::fs::remove_dir(occupied).expect("remove occupied temp target");

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(!restarted.config().system.show_status_icon);
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_applies_and_persists_taskbar_icon_visibility_transactionally() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let taskbar_icon = Arc::new(TestTaskbarIcon::new(true));
    let service =
        ApplicationSettingsService::start_with_taskbar_icon(application, taskbar_icon.clone())
            .expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(initial.taskbar_icon_visible);
    let hidden = client
        .set_taskbar_icon_visible_blocking(initial.config_revision.expect("config revision"), false)
        .expect("hide taskbar icon");
    assert!(!hidden.taskbar_icon_visible);
    assert!(!taskbar_icon.visible());
    assert_eq!(taskbar_icon.updates(), vec![false]);

    let stale = client
        .set_taskbar_icon_visible_blocking(initial.config_revision.expect("config revision"), true)
        .expect_err("stale taskbar icon update");
    assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
    assert_eq!(taskbar_icon.updates(), vec![false]);

    taskbar_icon.fail_updates.store(true, Ordering::Release);
    let failed = client
        .set_taskbar_icon_visible_blocking(
            hidden.config_revision.expect("hidden config revision"),
            true,
        )
        .expect_err("platform update failure");
    assert_eq!(failed.code(), SettingsErrorCode::TaskbarIconUpdateFailed);
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged, hidden);
    assert!(!taskbar_icon.visible());

    taskbar_icon.fail_updates.store(false, Ordering::Release);
    let occupied = layout.config.with_extension("json.tmp");
    std::fs::create_dir(&occupied).expect("occupied temp target");
    let persist_failed = client
        .set_taskbar_icon_visible_blocking(
            hidden.config_revision.expect("hidden config revision"),
            true,
        )
        .expect_err("config persist failure");
    assert_eq!(
        persist_failed.code(),
        SettingsErrorCode::ConfigTargetOccupied
    );
    assert!(!taskbar_icon.visible());
    assert_eq!(taskbar_icon.updates(), vec![false, true, true, false]);
    assert_eq!(
        client
            .read_snapshot_blocking()
            .expect("rolled back snapshot"),
        hidden
    );
    std::fs::remove_dir(occupied).expect("remove occupied temp target");

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(!restarted.config().system.show_taskbar_icon);
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_exports_diagnostics_and_reports_the_result_in_a_new_snapshot() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
        SettingsStartupItemState::Disabled,
    )));
    let backup_location = Arc::new(TestBackupLocation::new());
    let service = ApplicationSettingsService::start_with_capabilities(
        application,
        startup_item,
        backup_location,
        Arc::new(TestDiagnosticsExport),
    )
    .expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(initial.diagnostics_export.is_none());
    let exported = client
        .export_diagnostics_blocking()
        .expect("export diagnostics");
    assert!(exported.revision > initial.revision);
    assert_eq!(
        exported.diagnostics_export,
        Some(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 1,
            preview_bundle_format_version: 1,
            preview_bundle_bytes_written: 2,
            preview_bundle_entry_count: 3,
            preview_bundle_skipped_source_files: 0,
        })
    );
    let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
    assert_eq!(refreshed, exported);
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn failed_diagnostics_retry_preserves_the_last_successful_snapshot_result() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start_with_capabilities(
        application,
        Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        ))),
        Arc::new(TestBackupLocation::new()),
        Arc::new(FailingDiagnosticsExport {
            calls: AtomicUsize::new(0),
        }),
    )
    .expect("service start");
    let client = service.client();

    let exported = client
        .export_diagnostics_blocking()
        .expect("initial diagnostics export");
    let retry_error = client
        .export_diagnostics_blocking()
        .expect_err("diagnostics retry must expose the provider failure");
    assert_eq!(
        retry_error.code(),
        SettingsErrorCode::DiagnosticsExportFailed
    );
    let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
    assert_eq!(refreshed, exported);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_routes_open_settings_to_the_gpui_signal_without_touching_ui() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("start application");
    let (sender, receiver) = std::sync::mpsc::sync_channel(2);
    let signals = ApplicationMainThreadSignals::default();
    let service = ApplicationSettingsService::start_with_shortcut_receiver_and_signals(
        application,
        receiver,
        signals.clone(),
    )
    .expect("start settings service");
    sender
        .send(ShortcutCommand::OpenSettings)
        .expect("queue open settings");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut observed = false;
    while !observed && std::time::Instant::now() < deadline {
        observed = signals.take_open_settings_request();
        std::thread::yield_now();
    }
    assert!(observed);
    drop(sender);
    service
        .client()
        .shutdown_blocking()
        .expect("shutdown service");
    service.join().expect("join service");
}

#[test]
fn dropping_service_joins_shortcut_forwarder_while_sender_is_alive() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("start application");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let service = ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
        .expect("start settings service");

    drop(service);

    assert!(
        sender.send(ShortcutCommand::OpenSettings).is_err(),
        "shortcut receiver must be dropped before the service drop returns"
    );
}

#[test]
fn service_observes_and_updates_startup_item_without_touching_config() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
        SettingsStartupItemState::Disabled,
    )));
    let service =
        ApplicationSettingsService::start_with_startup_item(application, startup_item.clone())
            .expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let initial_config = std::fs::read(&config_path).expect("initial config");
    assert_eq!(
        initial.startup_item,
        SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)
    );

    startup_item.replace(SettingsStartupItemStatus::ReadError(
        SettingsStartupItemError::StateReadFailed,
    ));
    let read_failed = client
        .read_snapshot_blocking()
        .expect("read failure remains a snapshot");
    assert!(read_failed.revision > initial.revision);
    assert_eq!(
        read_failed.startup_item,
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::StateReadFailed)
    );

    startup_item.replace(SettingsStartupItemStatus::State(
        SettingsStartupItemState::Stale,
    ));
    let externally_changed = client.read_snapshot_blocking().expect("external change");
    assert!(externally_changed.revision > read_failed.revision);
    assert_eq!(
        externally_changed.startup_item,
        SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)
    );

    let enabled = client
        .set_startup_item_enabled_blocking(true)
        .expect("enable startup item");
    assert!(enabled.revision > externally_changed.revision);
    assert_eq!(
        enabled.startup_item,
        SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)
    );
    assert_eq!(
        std::fs::read(&config_path).expect("unchanged config"),
        initial_config
    );

    startup_item.fail_updates.store(true, Ordering::Release);
    let failed = client
        .set_startup_item_enabled_blocking(false)
        .expect_err("failed startup update");
    assert_eq!(failed.code(), SettingsErrorCode::StartupItemUpdateFailed);
    let unchanged = client.read_snapshot_blocking().expect("unchanged state");
    assert_eq!(unchanged.revision, enabled.revision);
    assert_eq!(unchanged.startup_item, enabled.startup_item);
    assert_eq!(
        std::fs::read(&config_path).expect("config after failure"),
        initial_config
    );

    let stopped = client.shutdown_blocking().expect("service shutdown");
    assert_eq!(stopped.startup_item, unchanged.startup_item);
    service.join().expect("service join");
}

/// Login startup is gated on the build environment, not on the platform.
///
/// Written as an equality so the same test covers both feature sets: the
/// Development build the workspace tests run as, and the `production` build
/// the release pipeline compiles.
#[test]
fn login_startup_is_gated_on_the_build_environment() {
    assert_eq!(
        startup_item_available(),
        BUILD_ENVIRONMENT == BuildEnvironment::Production,
        "the gate must open for released builds and stay shut for development ones"
    );
}

/// A development build reports login startup as unavailable and refuses to change it.
///
/// Only the Development direction is observable here: the released direction
/// would have to register a real login item on the machine running the test.
#[cfg(not(feature = "production"))]
#[test]
fn a_development_build_reports_login_startup_as_unavailable() {
    let unavailable = SettingsStartupItemState::Unsupported(
        SettingsStartupItemUnsupportedReason::BuildEnvironment,
    );

    assert_eq!(
        system_startup_item_state(),
        SettingsStartupItemStatus::State(unavailable)
    );
    // Both requested directions answer with the capability rather than with a
    // failure: the switch that would send this command renders disabled, and a
    // command that still arrives must not raise an error the user cannot act on.
    for enabled in [true, false] {
        assert_eq!(
            system_set_startup_item_enabled(enabled),
            Ok(unavailable),
            "a development build must not report a failed login-startup write"
        );
    }
}

#[test]
fn client_reports_closed_service_without_exposing_application_errors() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    assert_eq!(
        client
            .read_snapshot_blocking()
            .expect_err("closed service")
            .code(),
        SettingsErrorCode::ServiceUnavailable
    );
}

#[test]
fn service_opens_the_application_log_directory_without_advancing_revision() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let log_location = Arc::new(TestLogLocation::new());
    let service =
        ApplicationSettingsService::start_with_log_location(application, log_location.clone())
            .expect("service start");
    let client = service.client();
    let before = client.read_snapshot_blocking().expect("initial snapshot");

    let opened = client
        .open_logs_location_blocking()
        .expect("open application log folder");
    assert_eq!(log_location.invocations.load(Ordering::Acquire), 1);
    assert_eq!(
        opened.config_revision, before.config_revision,
        "opening a log folder is not a configuration change"
    );

    log_location.fail.store(true, Ordering::Release);
    assert_eq!(
        client
            .open_logs_location_blocking()
            .expect_err("failed open")
            .code(),
        SettingsErrorCode::LogLocationOpenFailed
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_opens_a_models_own_folder_without_advancing_revision() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let canonical_models_root = models_root.canonicalize().expect("canonical models root");
    let model_location = Arc::new(TestModelLocation::new());
    let service =
        ApplicationSettingsService::start_with_model_location(application, model_location.clone())
            .expect("service start");
    let client = service.client();

    client
        .import_model_blocking(SettingsModelImportRequest {
            title: "我的猫".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import model");
    let snapshot = client.read_snapshot_blocking().expect("snapshot");
    let entry = snapshot
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.origin == SettingsModelOrigin::Imported)
        .expect("installed entry");
    let key = SettingsModelKey {
        id: entry.id.clone(),
        origin: SettingsModelOrigin::Imported,
    };

    let opened = client
        .open_model_location_blocking(key.clone())
        .expect("open model folder");
    assert_eq!(
        model_location.opened(),
        vec![canonical_models_root.join(&key.id)]
    );
    assert_eq!(
        opened.config_revision, snapshot.config_revision,
        "opening a folder is not a configuration change"
    );

    // A file manager that refuses is reported as its own outcome rather than
    // as a silent no-op.
    model_location.fail.store(true, Ordering::Release);
    assert_eq!(
        client
            .open_model_location_blocking(key.clone())
            .expect_err("failed open")
            .code(),
        SettingsErrorCode::ModelLocationOpenFailed
    );

    // A model whose directory is gone has nothing to open.
    std::fs::remove_dir_all(models_root.join(&key.id)).expect("remove model directory");
    assert_eq!(
        client
            .open_model_location_blocking(key)
            .expect_err("missing model directory")
            .code(),
        SettingsErrorCode::ModelLocationOpenFailed
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn dropping_the_service_performs_a_fallback_shutdown_and_join() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    drop(service);
}
