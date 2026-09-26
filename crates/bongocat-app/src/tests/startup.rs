//! Starting the application, loading configuration and handing out the renderer.

use super::*;

#[test]
fn build_environment_is_compiled_into_the_application() {
    assert!(matches!(
        BUILD_ENVIRONMENT,
        BuildEnvironment::Development | BuildEnvironment::Production
    ));
}

#[test]
fn application_loads_config_updates_runtime_and_stops() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    assert!(application.runtime_client().snapshot().overlay_visible);
    assert_eq!(application.config().appearance.theme, ConfigTheme::System);
    assert!(!application.config().overlay.click_through);
    assert!(
        !application
            .runtime_client()
            .snapshot()
            .overlay_settings
            .click_through
    );

    let snapshot = application
        .set_overlay_visible(false)
        .expect("update overlay visibility");
    assert!(!snapshot.overlay_visible);
    assert!(
        std::fs::read_to_string(&config_path)
            .expect("persisted config")
            .find("\"visible\"")
            .is_none()
    );

    let overlay_settings = OverlaySettings {
        click_through: true,
        always_on_top: false,
        scale_percent: 150,
        opacity_percent: 75,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 1,
        keep_inside_screen: false,
    };
    let settings_snapshot = application
        .set_overlay_settings(overlay_settings)
        .expect("update overlay settings");
    assert_eq!(settings_snapshot.overlay_settings, overlay_settings);
    assert_eq!(
        application.config().overlay.scale_percent,
        overlay_settings.scale_percent
    );
    assert!(application.config().overlay.click_through);
    assert!(application.config().overlay.hide_on_pointer_hover);
    assert_eq!(
        application
            .config()
            .overlay
            .hide_on_pointer_hover_delay_seconds,
        1
    );
    assert!(!application.config().overlay.keep_inside_screen);

    application
        .set_appearance_theme(ConfigTheme::Dark)
        .expect("update appearance theme");
    assert_eq!(application.config().appearance.theme, ConfigTheme::Dark);

    let audio_snapshot = application
        .set_motion_audio_enabled(true)
        .expect("enable motion audio");
    assert!(audio_snapshot.motion_audio_enabled);
    assert!(application.config().model.play_motion_audio);

    let frame_rate_snapshot = application
        .set_maximum_fps(120)
        .expect("update maximum FPS");
    assert_eq!(frame_rate_snapshot.maximum_fps, 120);
    assert_eq!(application.config().overlay.maximum_fps, 120);

    let persisted = std::fs::read_to_string(config_path).expect("persisted config");
    assert!(persisted.contains("\"play_motion_audio\": true"));
    assert!(persisted.contains("\"scale_percent\": 150"));
    assert!(persisted.contains("\"click_through\": true"));
    assert!(persisted.contains("\"maximum_fps\": 120"));
    assert!(persisted.contains("\"theme\": \"dark\""));
    let stopped = application.shutdown().expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);

    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(restarted.config().overlay.click_through);
    assert!(
        restarted
            .runtime_client()
            .snapshot()
            .overlay_settings
            .click_through
    );
    assert_eq!(restarted.runtime_client().snapshot().maximum_fps, 120);
    assert_eq!(restarted.config().appearance.theme, ConfigTheme::Dark);
    assert!(restarted.runtime_client().snapshot().overlay_visible);
    restarted.shutdown().expect("clean restart shutdown");
}

/// A muted model activation must not decode audio. Enabling the setting in
/// the same session must queue preparation for the active model before any
/// later motion can publish `Play`, without reloading the model or restarting
/// the application.
#[test]
fn a_runtime_motion_audio_opt_in_prepares_the_active_model_without_restarting() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    assert!(!application.config().model.play_motion_audio);
    assert!(!application.runtime_client().snapshot().motion_audio_enabled);

    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare standard model while audio is disabled");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let frame = wait_for_model_commit_frame(&consumer, token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: frame.model_commit.expect("commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit standard model");
    let activated = application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("standard model activation");
    assert!(!activated.motion_audio_enabled);
    assert_eq!(activated.motion_audio.prepare_requests, 0);
    assert_eq!(activated.motion_audio.prepared_resources, 0);
    assert_eq!(activated.motion_audio.play_requests, 0);

    let enabled = application
        .set_motion_audio_enabled(true)
        .expect("enable motion audio without restarting");
    assert!(enabled.motion_audio_enabled);
    assert!(application.config().model.play_motion_audio);

    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    let prepared = loop {
        let snapshot = application.runtime_client().snapshot();
        if snapshot.motion_audio.prepare_requests == 1
            && snapshot.motion_audio.prepared_resources > 0
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "runtime opt-in did not prepare the active model audio cache"
        );
        std::thread::yield_now();
    };
    assert!(prepared.motion_audio_enabled);
    assert_eq!(prepared.motion_audio.play_requests, 0);
    application.shutdown().expect("clean shutdown");
}

/// Overlay visibility is runtime-only: every fresh process starts visible,
/// and a session hide never adds a preference to `config.json`.
#[test]
fn startup_starts_the_overlay_visible_without_persisting_visibility() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    assert!(application.runtime_client().snapshot().overlay_visible);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(!persisted.contains("\"visible\""));
    application
        .set_overlay_visible(false)
        .expect("hide overlay for this session");
    assert!(!application.runtime_client().snapshot().overlay_visible);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(!persisted.contains("\"visible\""));
    application.shutdown().expect("clean shutdown");

    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(restarted.runtime_client().snapshot().overlay_visible);
    restarted.shutdown().expect("clean restart shutdown");
}

#[test]
fn system_language_is_resolved_at_start_without_overwriting_the_preference() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let application = Application::start_with_layout_internal(
        layout.clone(),
        repository_preset_root().as_path(),
        false,
        Language::ChineseSimplified,
    )
    .expect("start with simplified Chinese system language");
    assert_eq!(application.config().appearance.language, Language::System);
    assert_eq!(
        application.effective_language(),
        Language::ChineseSimplified
    );
    application.shutdown().expect("first shutdown");

    let restarted = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        false,
        Language::EnglishUnitedStates,
    )
    .expect("restart with English system language");
    assert_eq!(restarted.config().appearance.language, Language::System);
    assert_eq!(
        restarted.effective_language(),
        Language::EnglishUnitedStates
    );
    restarted.shutdown().expect("restart shutdown");
}

#[test]
fn application_projects_model_interaction_settings_at_startup() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.model.mirror = true;
    config.model.mirror_pointer_tracking = true;
    config.model.ignore_keyboard = true;
    config.model.ignore_gamepad = true;
    config.model.ignore_pointer = true;
    config.model.random_behavior.enabled = true;
    config.model.random_behavior.interval_seconds = 17;
    store.commit(&config).expect("persist model settings");
    drop(store);

    let application = Application::start_with_layout(layout).expect("start application");
    assert_eq!(
        application.runtime_client().snapshot().model_settings,
        ModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_keyboard: true,
            ignore_gamepad: true,
            ignore_pointer: true,
        }
    );
    assert_eq!(
        application
            .runtime_client()
            .snapshot()
            .random_behavior_settings,
        RandomBehaviorSettings {
            enabled: true,
            interval_seconds: 17,
        }
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_starts_from_validated_config_backup_after_corruption() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.overlay.opacity_percent = 87;
    store.commit(&config).expect("older config commit");
    config.overlay.opacity_percent = 93;
    store.commit(&config).expect("newer config commit");
    std::fs::write(&layout.config, b"corrupt-current").expect("corrupt current config");

    let application = Application::start_with_layout(layout.clone()).expect("recover startup");
    assert_eq!(application.config().overlay.opacity_percent, 87);
    assert!(application.runtime_client().snapshot().overlay_visible);
    assert!(
        std::fs::read_dir(&layout.backups)
            .expect("backup directory")
            .any(|entry| {
                entry
                    .expect("backup entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config-corrupt-")
            })
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_uses_defaults_when_current_and_backups_are_invalid() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    ConfigStore::new(layout.clone()).expect("config store");
    std::fs::create_dir_all(
        layout
            .config
            .parent()
            .expect("configuration parent directory"),
    )
    .expect("configuration directory");
    std::fs::write(&layout.config, b"invalid-current").expect("invalid current config");

    let application = Application::start_with_layout(layout.clone()).expect("default startup");
    assert_eq!(application.config(), &NativeConfig::default());
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_starts_from_interrupted_config_without_exposing_storage_details() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut current = store.load_or_default().expect("default config").config;
    let interrupted_bytes = std::fs::read(&layout.config).expect("committed config bytes");
    current.overlay.opacity_percent = 87;
    store.commit(&current).expect("current config commit");
    std::fs::write(layout.config.with_extension("json.tmp"), interrupted_bytes)
        .expect("interrupted config temp");

    let application = Application::start_with_layout(layout.clone()).expect("recover startup");
    assert_eq!(application.config().overlay.opacity_percent, 87);
    assert!(!layout.config.with_extension("json.tmp").exists());
    application.shutdown().expect("clean shutdown");
}

#[test]
fn development_and_production_applications_never_share_roots() {
    let base = tempdir().expect("temp directory");
    let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
    let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
    let development_root = development.root.clone();
    let production_root = production.root.clone();
    let development_logs = development.logs.clone();
    let production_logs = production.logs.clone();

    let mut development_app =
        Application::start_with_layout(development).expect("development application");
    let mut production_app =
        Application::start_with_layout(production).expect("production application");

    assert!(development_root.join("config.json").is_file());
    assert!(production_root.join("config.json").is_file());
    assert_ne!(development_root, production_root);

    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
    import_one(&mut development_app, "same-id", &source);
    import_one(&mut production_app, "same-id", source);
    let development_ids = installed_catalog_ids(&development_app);
    let production_ids = installed_catalog_ids(&production_app);
    assert_eq!(development_ids.len(), 1);
    assert_eq!(production_ids.len(), 1);
    // Store keys are UUIDs generated inside each environment, so the same
    // import hint never produces the same identity across environments.
    assert_ne!(development_ids, production_ids);
    assert!(
        development_root
            .join("models")
            .join(&development_ids[0])
            .is_dir()
    );
    assert!(
        production_root
            .join("models")
            .join(&production_ids[0])
            .is_dir()
    );
    development_app.record_log(ApplicationLogEvent::shutdown_failed());
    production_app.record_log(ApplicationLogEvent::panicked());
    assert_ne!(development_logs, production_logs);
    let development_log = std::fs::read_to_string(
        std::fs::read_dir(&development_logs)
            .expect("development logs")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("application-") && name.ends_with(".log")
                })
            })
            .expect("development application log"),
    )
    .expect("development log contents");
    let production_log = std::fs::read_to_string(
        std::fs::read_dir(&production_logs)
            .expect("production logs")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("application-") && name.ends_with(".log")
                })
            })
            .expect("production application log"),
    )
    .expect("production log contents");
    assert!(development_log.contains("shutdown_failed"));
    assert!(!development_log.contains("panicked"));
    assert!(production_log.contains("panicked"));
    assert!(!production_log.contains("shutdown_failed"));
    development_app.shutdown().expect("development shutdown");
    production_app.shutdown().expect("production shutdown");
}

#[test]
fn failed_installed_model_preparation_preserves_the_active_model() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let mut application = Application::start_with_layout(layout).expect("start application");
    let fixtures = repository_root().join("shared/fixtures/model-fixtures/cases");

    let active_id = import_one(&mut application, "active", fixtures.join("非 ASCII 模型"))
        .id()
        .as_str()
        .to_owned();
    let broken_id = import_one(&mut application, "broken", fixtures.join("非 ASCII 模型"))
        .id()
        .as_str()
        .to_owned();

    let active = application
        .select_model(ModelOrigin::Installed, active_id.as_str())
        .expect("activate valid model");
    let active_revision = active.revision;
    assert_eq!(
        active
            .active_model
            .as_ref()
            .expect("active model")
            .id
            .as_str(),
        active_id
    );

    std::fs::remove_file(models_root.join(&broken_id).join("模型 数据.moc3"))
        .expect("corrupt installed model");

    let error = application
        .select_model(ModelOrigin::Installed, broken_id.as_str())
        .expect_err("invalid model must be rejected");
    assert!(matches!(error, ApplicationError::ModelStore(_)));
    let preserved = application.runtime_client().snapshot();
    assert_eq!(preserved.revision, active_revision);
    assert_eq!(preserved.active_model, active.active_model);
    application.shutdown().expect("clean shutdown");
}

#[cfg(target_os = "macos")]
#[test]
fn application_owns_the_rendering_runtime_and_issues_one_consumer() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare preset model");
    assert_eq!(token.model_generation, 0);
    assert!(
        application
            .runtime_client()
            .snapshot()
            .active_model
            .is_none()
    );

    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    assert!(matches!(
        application.take_render_consumer(),
        Err(ApplicationError::RenderConsumerUnavailable)
    ));
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    let frame = loop {
        if let Some(frame) = consumer.take_latest() {
            break frame;
        }
        assert!(
            Instant::now() < deadline,
            "runtime did not publish a render frame"
        );
        std::thread::yield_now();
    };
    assert_eq!(frame.model_generation, 0);
    assert!(!frame.snapshot.drawables.is_empty());
    assert_eq!(frame.model_commit, Some(token));
    consumer
        .report_model_commit(bongocat_render::ModelCommitFeedback {
            token,
            outcome: bongocat_render::ModelCommitOutcome::Prepared,
        })
        .expect("report prepared GPU model");
    let activated = application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("commit preset model");
    assert_eq!(
        activated
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert!(activated.pending_model.is_none());

    let stopped = application.shutdown().expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
}
