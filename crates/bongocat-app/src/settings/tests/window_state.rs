//! Persisting the window placement across restarts and upgrades.

use super::*;

#[test]
fn settings_window_layout_flushes_on_shutdown_and_restores_after_restart() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let expected = SettingsWindowPlacement::new(-240, 96, 960, 720, true)
        .expect("valid settings window placement");
    service.window_state().update(expected);

    service
        .client()
        .shutdown_blocking()
        .expect("service shutdown");
    service.join().expect("service join");

    let restarted = Application::start_with_layout(layout.clone()).expect("application restart");
    assert_eq!(
        restarted.settings_window_placement(),
        Some(WindowPlacement::new(-240, 96, 960, 720, true).expect("valid persisted placement"))
    );
    let config = std::fs::read_to_string(&layout.config).expect("configuration remains readable");
    assert!(!config.contains("settings_window"));
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn settings_window_layout_is_saved_while_running_and_survives_product_updates() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let expected = SettingsWindowPlacement::new(-360, 144, 1040, 760, false)
        .expect("valid settings window placement");
    let window_state = service.window_state();
    let revision = window_state.update(expected).expect("changed placement");
    assert!(window_state.request_persist_if_current(revision));
    client
        .update_overlay_window_placement(-640, 220, 420, 560)
        .expect("publish overlay placement");
    let initial = client
        .read_snapshot_blocking()
        .expect("wait for queued window placement writes");

    let window_state = WindowStateStore::new(layout.clone())
        .load_or_default()
        .state;
    assert_eq!(
        window_state.settings_window,
        Some(WindowPlacement::new(-360, 144, 1040, 760, false).expect("valid persisted placement"))
    );
    assert_eq!(
        window_state.overlay_window,
        Some(OverlayWindowPlacement::new(-640, 220, 420, 560).expect("valid overlay placement"))
    );
    let selected = client
        .select_model_blocking(
            initial.config_revision.expect("config revision"),
            SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
        )
        .expect("select model");
    client
        .set_overlay_visible_blocking(selected.config_revision.expect("config revision"), false)
        .expect("update configuration");

    let persisted_state = WindowStateStore::new(layout).load_or_default().state;
    assert_eq!(
        persisted_state.settings_window,
        Some(WindowPlacement::new(-360, 144, 1040, 760, false).expect("valid persisted placement"))
    );
    assert_eq!(
        persisted_state.overlay_window,
        Some(OverlayWindowPlacement::new(-640, 220, 420, 560).expect("valid overlay placement"))
    );
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn corrupt_window_state_never_blocks_configuration_or_runtime_startup() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let loaded = store.load_or_default().expect("default config");
    let config_before = std::fs::read(&layout.config).expect("config bytes");
    std::fs::write(&layout.window_state, b"corrupt-window-state").expect("corrupt state fixture");

    let application = Application::start_with_layout(layout.clone())
        .expect("corrupt state must not block application startup");
    assert_eq!(application.settings_window_placement(), None);
    assert_eq!(application.config(), &loaded.config);
    assert_eq!(
        std::fs::read(&layout.config).expect("config preserved"),
        config_before
    );
    assert_eq!(
        std::fs::read(&layout.window_state).expect("state preserved until flush"),
        b"corrupt-window-state"
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn future_window_state_is_preserved_without_failing_service_shutdown() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    store.load_or_default().expect("default config");
    let future = br#"{"schema_version":2,"settings_window":null,"overlay_window":null}"#;
    std::fs::write(&layout.window_state, future).expect("future state");

    let application = Application::start_with_layout(layout.clone())
        .expect("future state must not block startup");
    let service = ApplicationSettingsService::start(application).expect("service start");
    service
        .client()
        .shutdown_blocking()
        .expect("future state must not fail shutdown");
    service.join().expect("service join");
    assert_eq!(
        std::fs::read(&layout.window_state).expect("future state preserved"),
        future
    );
}

#[test]
fn window_state_write_failure_is_reported_after_runtime_still_stops() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    service.window_state().update(
        SettingsWindowPlacement::new(20, 40, 800, 600, false)
            .expect("valid settings window placement"),
    );
    let state_lock = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(layout.locks.join(WINDOW_STATE_WRITER_LOCK_FILE_NAME))
        .expect("state writer lock");
    state_lock.lock().expect("hold state writer lock");

    let error = service
        .client()
        .shutdown_blocking()
        .expect_err("state lock must report persistence failure");
    assert_eq!(error.code(), SettingsErrorCode::WindowStatePersistFailed);
    service
        .join()
        .expect("runtime shutdown and service join still complete");
    state_lock.unlock().expect("release state writer lock");
}
