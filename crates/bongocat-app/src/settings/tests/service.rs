//! Applying one settings command and rejecting a stale one.

use super::*;

#[test]
fn service_uses_defaults_when_current_and_backups_are_invalid() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    std::fs::write(&layout.config, b"invalid-current").expect("invalid current config");

    let application = Application::start_with_layout(layout.clone()).expect("default startup");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let snapshot = client.read_snapshot_blocking().expect("snapshot");
    assert_eq!(snapshot.runtime_health, RuntimeHealth::Ready);
    let updated = client
        .set_overlay_visible_blocking(
            snapshot.config_revision.expect("configuration revision"),
            false,
        )
        .expect("business command remains available");
    assert_eq!(updated.config_revision, snapshot.config_revision);
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");

    let reloaded = store.load_or_default().expect("reloaded defaults").config;
    assert_eq!(reloaded, bongocat_config::NativeConfig::default());
    assert!(
        std::fs::read_dir(&layout.backups)
            .expect("backup directory")
            .any(|entry| entry
                .expect("backup entry")
                .file_name()
                .to_string_lossy()
                .starts_with("config-corrupt-"))
    );
}

#[test]
fn service_advances_settings_revision_once_for_one_control_change() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let updated = client
        .set_overlay_visible_blocking(
            initial.config_revision.expect("config revision"),
            !initial.overlay_visible,
        )
        .expect("toggle overlay visibility");
    assert_eq!(updated.revision, initial.revision.saturating_add(1));
    assert_eq!(updated.config_revision, initial.config_revision);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_persists_and_projects_the_selected_appearance_theme() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert_eq!(initial.appearance_theme, SettingsTheme::System);
    let updated = client
        .set_appearance_theme_blocking(
            initial.config_revision.expect("config revision"),
            SettingsTheme::Dark,
        )
        .expect("select dark theme");
    assert_eq!(updated.appearance_theme, SettingsTheme::Dark);
    assert_eq!(updated.revision, initial.revision.saturating_add(1));
    assert_ne!(updated.config_revision, initial.config_revision);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert_eq!(
        restarted.config().appearance.theme,
        bongocat_config::Theme::Dark
    );
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_persists_and_projects_the_selected_language() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert_eq!(initial.language, SettingsLanguage::System);
    assert_eq!(
        initial.resolved_language,
        SettingsLanguage::EnglishUnitedStates
    );
    let updated = client
        .set_language_blocking(
            initial.config_revision.expect("config revision"),
            SettingsLanguage::ChineseSimplified,
        )
        .expect("select simplified Chinese");
    assert_eq!(updated.language, SettingsLanguage::ChineseSimplified);
    assert_eq!(
        updated.resolved_language,
        SettingsLanguage::ChineseSimplified
    );
    assert_eq!(updated.revision, initial.revision.saturating_add(1));
    assert_ne!(updated.config_revision, initial.config_revision);

    let stale = client
        .set_language_blocking(
            initial.config_revision.expect("config revision"),
            SettingsLanguage::EnglishUnitedStates,
        )
        .expect_err("reject stale language update");
    assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert_eq!(
        restarted.config().appearance.language,
        bongocat_config::Language::ChineseSimplified
    );
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_persists_automatic_update_preferences_and_rejects_stale_revisions() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(!initial.check_for_updates_automatically);
    assert_eq!(initial.check_for_updates_interval_hours, 24);
    let custom_interval = client
        .set_check_for_updates_interval_hours_blocking(
            initial.config_revision.expect("config revision"),
            48,
        )
        .expect("set automatic update interval");
    assert!(!custom_interval.check_for_updates_automatically);
    assert_eq!(custom_interval.check_for_updates_interval_hours, 48);

    let enabled = client
        .set_check_for_updates_automatically_blocking(
            custom_interval
                .config_revision
                .expect("custom interval config revision"),
            true,
        )
        .expect("enable automatic update checks");
    assert!(enabled.check_for_updates_automatically);
    assert_eq!(enabled.check_for_updates_interval_hours, 48);

    let disabled = client
        .set_check_for_updates_automatically_blocking(
            enabled.config_revision.expect("enabled config revision"),
            false,
        )
        .expect("disable automatic update checks");
    assert!(!disabled.check_for_updates_automatically);
    assert_eq!(disabled.check_for_updates_interval_hours, 48);

    let stale_enabled = client
        .set_check_for_updates_automatically_blocking(
            initial.config_revision.expect("initial config revision"),
            true,
        )
        .expect_err("reject stale automatic update preference");
    assert_eq!(stale_enabled.code(), SettingsErrorCode::SnapshotOutdated);
    // Reverting the boolean restores the custom-interval content revision, so use
    // the enabled snapshot's revision to exercise a genuinely stale interval command.
    let stale_interval = client
        .set_check_for_updates_interval_hours_blocking(
            enabled.config_revision.expect("enabled config revision"),
            72,
        )
        .expect_err("reject stale automatic update interval");
    assert_eq!(stale_interval.code(), SettingsErrorCode::SnapshotOutdated);
    assert_eq!(
        client.read_snapshot_blocking().expect("unchanged snapshot"),
        disabled
    );

    for invalid_interval in [
        0,
        bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS + 1,
    ] {
        let invalid = client
            .set_check_for_updates_interval_hours_blocking(
                disabled.config_revision.expect("disabled config revision"),
                invalid_interval,
            )
            .expect_err("reject invalid automatic update interval");
        assert_eq!(invalid.code(), SettingsErrorCode::ConfigPersistFailed);
    }
    assert_eq!(
        client
            .read_snapshot_blocking()
            .expect("invalid interval is unchanged"),
        disabled
    );
    assert_eq!(
        client
            .read_automatic_update_settings_blocking()
            .expect("automatic update schedule"),
        AutomaticUpdateSettings {
            enabled: false,
            interval_hours: 48,
        }
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(!restarted.config().updates.check_automatically);
    assert_eq!(restarted.config().updates.check_interval_hours, 48);
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_persists_logging_settings_and_applies_them_after_commit() {
    use bongocat_ui_protocol::{SettingsLogLevel, SettingsLogging};

    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let controller = application.log_settings_controller();
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert_eq!(initial.logging, SettingsLogging::default());
    let initial_revision = initial.config_revision.expect("initial config revision");
    let committed = client
        .set_logging_settings_blocking(
            initial_revision,
            SettingsLogging {
                level: SettingsLogLevel::Trace,
                retention_days: 30,
            },
        )
        .expect("commit logging settings");
    assert_eq!(committed.logging.level, SettingsLogLevel::Trace);
    assert_eq!(committed.logging.retention_days, 30);
    assert_ne!(committed.config_revision, initial.config_revision);
    assert_eq!(
        controller.settings(),
        bongocat_log::LogSettings {
            level: bongocat_log::LogLevel::Trace,
            retention_days: 30,
        }
    );
    let committed_config = fs::read(&config_path).expect("committed config");

    let stale = client
        .set_logging_settings_blocking(
            initial_revision,
            SettingsLogging {
                level: SettingsLogLevel::Error,
                retention_days: 1,
            },
        )
        .expect_err("stale logging settings");
    assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
    assert_eq!(
        fs::read(&config_path).expect("preserved config"),
        committed_config
    );
    assert_eq!(controller.settings().retention_days, 30);

    let current_revision = committed.config_revision.expect("current config revision");
    for invalid_retention in [0, 31] {
        let invalid = client
            .set_logging_settings_blocking(
                current_revision,
                SettingsLogging {
                    level: SettingsLogLevel::Info,
                    retention_days: invalid_retention,
                },
            )
            .expect_err("invalid logging retention");
        assert_eq!(invalid.code(), SettingsErrorCode::ConfigPersistFailed);
        assert_eq!(
            fs::read(&config_path).expect("preserved config"),
            committed_config
        );
        assert_eq!(controller.settings().retention_days, 30);
    }

    let occupied = config_path.with_extension("json.tmp");
    fs::create_dir(&occupied).expect("occupied config target");
    let persist_failed = client
        .set_logging_settings_blocking(
            current_revision,
            SettingsLogging {
                level: SettingsLogLevel::Warn,
                retention_days: 14,
            },
        )
        .expect_err("logging persistence failure");
    assert_eq!(
        persist_failed.code(),
        SettingsErrorCode::ConfigTargetOccupied
    );
    assert_eq!(
        client.read_snapshot_blocking().expect("unchanged snapshot"),
        committed
    );
    assert_eq!(controller.settings().level, bongocat_log::LogLevel::Trace);
    fs::remove_dir(occupied).expect("remove occupied config target");

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
    let restarted = Application::start_with_layout(layout).expect("application restart");
    assert_eq!(
        restarted.config().logging.level,
        bongocat_config::LoggingLevel::Trace
    );
    assert_eq!(restarted.config().logging.retention_days, 30);
    assert_eq!(
        restarted.log_settings_controller().settings(),
        bongocat_log::LogSettings {
            level: bongocat_log::LogLevel::Trace,
            retention_days: 30,
        }
    );
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn service_orders_updates_persists_them_and_stops_runtime() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let initial_config_revision = initial.config_revision.expect("config revision");
    assert_eq!(initial.model_catalog.entries.len(), 3);
    assert!(initial.model_catalog.error.is_none());
    assert!(initial.model_catalog.entries.iter().all(|entry| {
        entry.origin == SettingsModelOrigin::BuiltIn
            && matches!(&entry.availability, SettingsModelAvailability::Ready { .. })
    }));
    assert_eq!(
        initial
            .model_catalog
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry.input_mode))
            .collect::<Vec<_>>(),
        vec![
            ("standard", Some(SettingsModelMode::Standard)),
            ("keyboard", Some(SettingsModelMode::Keyboard)),
            ("gamepad", Some(SettingsModelMode::Gamepad)),
        ]
    );
    let standard = initial
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == "standard" && entry.origin == SettingsModelOrigin::BuiltIn)
        .expect("standard model entry");
    let SettingsModelAvailability::Ready { behaviors, .. } = &standard.availability else {
        panic!("standard model is ready");
    };
    assert!(behaviors.contains(&SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    }));
    assert!(behaviors.contains(&SettingsModelBehavior::Expression {
        name: "live2d_expression0.exp3.json".to_owned(),
    }));
    // Every preset ships its own folder and cover, so the catalog the page
    // renders has a real image and a real "open folder" target to work with.
    assert!(
        standard
            .directory
            .as_ref()
            .is_some_and(|path| path.is_dir())
    );
    assert!(standard.cover.as_ref().is_some_and(|path| path.is_file()));
    let selected = client
        .select_model_blocking(
            initial_config_revision,
            SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
        )
        .expect("select preset model");
    let selected_config_revision = selected.config_revision.expect("config revision");
    assert_eq!(
        selected.active_model,
        Some(SettingsModelKey {
            id: "keyboard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        })
    );
    let overlay_settings = SettingsOverlay {
        click_through: true,
        always_on_top: false,
        scale_percent: 125,
        opacity_percent: 80,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 1,
        keep_inside_screen: false,
    };
    let configured = client
        .set_overlay_settings_blocking(selected_config_revision, overlay_settings)
        .expect("update overlay settings");
    assert_eq!(
        configured.overlay, overlay_settings,
        "settings snapshot must acknowledge the committed overlay settings"
    );
    let hidden = client
        .set_overlay_visible_blocking(configured.config_revision.expect("config revision"), false)
        .expect("hide overlay");
    // A fresh v1 configuration is silent, so the update that gets persisted
    // has to enable audio to leave a value the file can prove.
    let audio_enabled = client
        .set_motion_audio_enabled_blocking(hidden.config_revision.expect("config revision"), true)
        .expect("enable motion audio");
    let model_settings = bongocat_ui_protocol::SettingsModelSettings {
        mirror: true,
        mirror_pointer_tracking: true,
        ignore_keyboard: true,
        ignore_gamepad: true,
        ignore_pointer: true,
    };
    let configured_model = client
        .set_model_settings_blocking(
            audio_enabled.config_revision.expect("config revision"),
            model_settings,
        )
        .expect("update model settings");
    assert_eq!(configured_model.model_settings, model_settings);
    let random_behavior = SettingsRandomBehavior {
        enabled: true,
        interval_seconds: 9,
    };
    let configured_random_behavior = client
        .set_random_behavior_settings_blocking(
            configured_model.config_revision.expect("config revision"),
            random_behavior,
        )
        .expect("update random behavior settings");
    assert_eq!(configured_random_behavior.random_behavior, random_behavior);
    let configured_frame_rate = client
        .set_maximum_fps_blocking(
            configured_random_behavior
                .config_revision
                .expect("config revision"),
            120,
        )
        .expect("update maximum FPS");
    assert_eq!(configured_frame_rate.maximum_fps, 120);
    let configured_fallback = client
        .set_release_fallback_timeout_blocking(
            configured_frame_rate
                .config_revision
                .expect("config revision"),
            1_500,
        )
        .expect("update release fallback timeout");
    assert_eq!(configured_fallback.release_fallback_timeout_ms, 1_500);
    let gamepad_settings = bongocat_ui_protocol::SettingsGamepadAxisSettings {
        stick_dead_zone_percent: 20,
        trigger_dead_zone_percent: 10,
    };
    let configured_gamepad = client
        .set_gamepad_axis_settings_blocking(
            configured_fallback
                .config_revision
                .expect("config revision"),
            gamepad_settings,
        )
        .expect("update gamepad settings");
    assert_eq!(configured_gamepad.gamepad_axis_settings, gamepad_settings);
    assert!(hidden.revision > initial.revision);
    assert!(audio_enabled.revision > hidden.revision);
    assert!(!audio_enabled.overlay_visible);
    assert!(audio_enabled.motion_audio_enabled);

    let persisted = std::fs::read_to_string(config_path).expect("persisted config");
    assert!(!persisted.contains("\"visible\""));
    assert!(persisted.contains("\"play_motion_audio\": true"));
    assert!(persisted.contains("\"selected_model\": {"));
    assert!(persisted.contains("\"id\": \"keyboard\""));
    assert!(persisted.contains("\"source\": \"built_in\""));
    assert!(persisted.contains("\"click_through\": true"));
    assert!(persisted.contains("\"opacity_percent\": 80"));
    assert!(persisted.contains("\"keep_inside_screen\": false"));
    assert!(persisted.contains("\"mirror\": true"));
    assert!(persisted.contains("\"mirror_pointer_tracking\": true"));
    assert!(persisted.contains("\"ignore_keyboard\": true"));
    assert!(persisted.contains("\"ignore_gamepad\": true"));
    assert!(persisted.contains("\"ignore_pointer\": true"));
    assert!(persisted.contains("\"stick_dead_zone\": 0.2"));
    assert!(persisted.contains("\"trigger_dead_zone\": 0.1"));
    assert!(persisted.contains("\"maximum_fps\": 120"));
    assert!(persisted.contains("\"release_fallback_timeout_ms\": 1500"));
    assert!(persisted.contains("\"random_behavior\": {"));
    assert!(persisted.contains("\"enabled\": true"));
    assert!(persisted.contains("\"interval_seconds\": 9"));

    let stopped = client.shutdown_blocking().expect("service shutdown");
    assert_eq!(stopped.runtime_health, RuntimeHealth::Stopped);
    service.join().expect("service join");

    let restarted = Application::start_with_layout(layout).expect("application restart");
    assert_eq!(
        restarted
            .runtime_client()
            .snapshot()
            .release_fallback_timeout_ms,
        1_500
    );
    assert_eq!(
        restarted.runtime_client().snapshot().model_settings,
        ModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_keyboard: true,
            ignore_gamepad: true,
            ignore_pointer: true,
        }
    );
    assert_eq!(
        restarted
            .runtime_client()
            .snapshot()
            .random_behavior_settings,
        RandomBehaviorSettings {
            enabled: true,
            interval_seconds: 9,
        }
    );
    assert!(
        !restarted
            .runtime_client()
            .snapshot()
            .overlay_settings
            .keep_inside_screen
    );
    restarted
        .shutdown()
        .expect("restarted application shutdown");
}

#[test]
fn service_rejects_stale_overlay_settings_without_mutating_runtime_or_config() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let initial_config_revision = initial.config_revision.expect("config revision");
    let original_settings = SettingsOverlay {
        click_through: false,
        always_on_top: false,
        scale_percent: 125,
        opacity_percent: 80,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 1,
        keep_inside_screen: false,
    };
    let committed = client
        .set_overlay_settings_blocking(initial_config_revision, original_settings)
        .expect("first overlay update");
    let committed_config = std::fs::read(&config_path).expect("committed config");

    let stale_settings = SettingsOverlay {
        click_through: true,
        always_on_top: true,
        scale_percent: 400,
        opacity_percent: 10,
        corner_radius_percent: 50,
        hide_on_pointer_hover: false,
        hide_on_pointer_hover_delay_seconds: 0,
        keep_inside_screen: true,
    };
    let error = client
        .set_overlay_settings_blocking(initial_config_revision, stale_settings)
        .expect_err("stale overlay update");
    assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
    assert_eq!(
        error.to_string(),
        "Settings changed elsewhere. Review the latest settings and try again."
    );
    assert!(!error.to_string().contains('/') && !error.to_string().contains('\\'));

    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged.revision, committed.revision);
    assert_eq!(unchanged.overlay, original_settings);
    assert_eq!(
        std::fs::read(&config_path).expect("preserved config"),
        committed_config
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_rejects_stale_direct_settings_without_mutating_runtime_or_config() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let initial_config_revision = initial.config_revision.expect("config revision");
    let initial_active_model = initial.active_model.clone();
    let committed = client
        .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
        .expect("commit a persistent setting");
    let committed_config = std::fs::read(&config_path).expect("committed config");

    let stale_theme_error = client
        .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
        .expect_err("stale appearance theme update");
    assert_eq!(
        stale_theme_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );

    let stale_model_error = client
        .set_model_settings_blocking(
            initial_config_revision,
            bongocat_ui_protocol::SettingsModelSettings {
                mirror: true,
                mirror_pointer_tracking: true,
                ignore_keyboard: false,
                ignore_gamepad: false,
                ignore_pointer: true,
            },
        )
        .expect_err("stale model settings update");
    assert_eq!(
        stale_model_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );
    let after_stale_model_settings = client
        .read_snapshot_blocking()
        .expect("snapshot after stale model settings");
    assert_eq!(after_stale_model_settings.revision, committed.revision);
    assert_eq!(
        after_stale_model_settings.model_settings,
        bongocat_ui_protocol::SettingsModelSettings::default()
    );
    assert_eq!(
        std::fs::read(&config_path).expect("preserved committed config"),
        committed_config
    );

    let stale_gamepad_error = client
        .set_gamepad_axis_settings_blocking(
            initial_config_revision,
            bongocat_ui_protocol::SettingsGamepadAxisSettings {
                stick_dead_zone_percent: 20,
                trigger_dead_zone_percent: 10,
            },
        )
        .expect_err("stale gamepad settings update");
    assert_eq!(
        stale_gamepad_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );

    let stale_frame_rate_error = client
        .set_maximum_fps_blocking(initial_config_revision, 120)
        .expect_err("stale maximum FPS update");
    assert_eq!(
        stale_frame_rate_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );
    let stale_fallback_error = client
        .set_release_fallback_timeout_blocking(initial_config_revision, 1_500)
        .expect_err("stale release fallback timeout update");
    assert_eq!(
        stale_fallback_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );

    let stale_model_error = client
        .select_model_blocking(
            initial_config_revision,
            SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
        )
        .expect_err("stale model selection");
    assert_eq!(
        stale_model_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );
    let after_stale_model = client
        .read_snapshot_blocking()
        .expect("snapshot after stale model");
    assert_eq!(after_stale_model.revision, committed.revision);
    assert_eq!(after_stale_model.active_model, initial_active_model);
    assert_eq!(
        std::fs::read(&config_path).expect("preserved committed config"),
        committed_config
    );

    // The fresh v1 configuration is silent (`play_motion_audio: false`), so
    // the stale request has to ask for the opposite value: a request that
    // already matched the committed config could be applied without any
    // observable difference.
    let stale_audio_error = client
        .set_motion_audio_enabled_blocking(initial_config_revision, true)
        .expect_err("stale motion audio update");
    assert_eq!(
        stale_audio_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );
    let after_stale_audio = client
        .read_snapshot_blocking()
        .expect("snapshot after stale audio");
    assert_eq!(after_stale_audio.revision, committed.revision);
    assert!(after_stale_audio.overlay_visible);
    assert!(!after_stale_audio.motion_audio_enabled);
    assert_eq!(
        std::fs::read(&config_path).expect("preserved committed config"),
        committed_config
    );

    let enabled = client
        .set_motion_audio_enabled_blocking(
            committed.config_revision.expect("config revision"),
            true,
        )
        .expect("enable motion audio");
    let enabled_config = std::fs::read(&config_path).expect("enabled config");
    let stale_visibility_error = client
        .set_overlay_visible_blocking(committed.config_revision.expect("config revision"), true)
        .expect_err("stale overlay visibility update");
    assert_eq!(
        stale_visibility_error.code(),
        SettingsErrorCode::SnapshotOutdated
    );
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged.revision, enabled.revision);
    assert!(unchanged.overlay_visible);
    assert!(unchanged.motion_audio_enabled);
    assert_eq!(
        std::fs::read(&config_path).expect("preserved enabled config"),
        enabled_config
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_reports_an_occupied_config_target_without_changing_snapshot_or_current() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let occupied = config_path.with_extension("json.tmp");
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let original = std::fs::read(&config_path).expect("initial config");
    std::fs::create_dir(&occupied).expect("occupied temp target");

    let initial_config_revision = initial.config_revision.expect("config revision");
    let error = client
        .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
        .expect_err("occupied target error");
    assert_eq!(error.code(), SettingsErrorCode::ConfigTargetOccupied);
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged.revision, initial.revision);
    assert_eq!(unchanged.overlay_visible, initial.overlay_visible);
    assert_eq!(
        std::fs::read(&config_path).expect("preserved config"),
        original
    );
    assert!(occupied.is_dir());

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn invalid_random_behavior_settings_leave_config_and_runtime_unchanged() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("application start");
    let runtime = application.runtime_client();
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let initial_config_revision = initial.config_revision.expect("config revision");
    let initial_runtime = runtime.snapshot();
    let initial_bytes = std::fs::read(&layout.config).expect("initial config bytes");

    for interval_seconds in [0, 3_601] {
        client
            .set_random_behavior_settings_blocking(
                initial_config_revision,
                SettingsRandomBehavior {
                    enabled: true,
                    interval_seconds,
                },
            )
            .expect_err("invalid random behavior settings must fail");
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect("snapshot after rejected setting")
                .config_revision,
            Some(initial_config_revision)
        );
        assert_eq!(
            runtime.snapshot().random_behavior_settings,
            initial_runtime.random_behavior_settings
        );
        assert_eq!(
            std::fs::read(&layout.config).expect("config after rejected setting"),
            initial_bytes
        );
    }
    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}
