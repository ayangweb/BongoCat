//! Loading, committing under a revision check and recovering a crash.

use super::*;

#[test]
fn storage_layout_uses_the_domain_specific_window_state_filename() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
    assert_eq!(
        layout.window_state,
        base.path()
            .join(BUNDLE_ID)
            .join("development")
            .join("window-state.json")
    );
    assert_eq!(
        WINDOW_STATE_WRITER_LOCK_FILE_NAME,
        "window-state.writer.lock"
    );
}

#[test]
fn environments_have_identical_shape_and_disjoint_roots() {
    let base = tempdir().expect("temp directory");
    let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
    let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
    assert_ne!(development.root, production.root);
    let relative_shape = |layout: &StorageLayout| {
        [
            &layout.config,
            &layout.window_state,
            &layout.models,
            &layout.backups,
            &layout.logs,
            &layout.updates,
            &layout.locks,
        ]
        .into_iter()
        .map(|path| {
            path.strip_prefix(&layout.root)
                .expect("layout child")
                .to_owned()
        })
        .collect::<Vec<_>>()
    };
    assert_eq!(relative_shape(&development), relative_shape(&production));
    assert!(!development.root.starts_with(&production.root));
    assert!(!production.root.starts_with(&development.root));
}

#[test]
fn production_first_load_never_copies_development_configuration() {
    let base = tempdir().expect("temp directory");
    let development = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("development config store");
    let production = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("production config store");

    let mut development_config = development
        .load_or_default()
        .expect("development default")
        .config;
    development_config.appearance.theme = Theme::Dark;
    development
        .commit(&development_config)
        .expect("development commit");
    let development_bytes = fs::read(&development.layout().config).expect("development bytes");

    let loaded_production = production.load_or_default().expect("production default");
    assert_eq!(loaded_production.config, NativeConfig::default());
    assert_eq!(
        fs::read(&development.layout().config).expect("development remains unchanged"),
        development_bytes
    );
    assert_ne!(
        fs::read(&production.layout().config).expect("production bytes"),
        development_bytes
    );
}

#[test]
fn load_creates_valid_default_and_commit_is_revision_checked() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let loaded = store.load_or_default().expect("default config");
    let mut config = loaded.config;
    let initial_revision = loaded.revision;
    assert_eq!(config, NativeConfig::default());

    config.appearance.theme = Theme::Dark;
    let next_revision = store
        .commit_if_revision(&config, initial_revision)
        .expect("revision checked commit");
    assert_ne!(next_revision, initial_revision);
    let stale = store.commit_if_revision(&config, initial_revision);
    assert!(matches!(stale, Err(ConfigError::RevisionConflict { .. })));
    assert_eq!(config_backup_paths(store.layout()).len(), 1);
}

#[test]
fn injected_permission_and_storage_failures_preserve_current_and_clean_temp() {
    for (failure, expected_reason) in [
        (
            InjectedConfigWriteFailure::PermissionDenied,
            ConfigWriteFailureReason::PermissionDenied,
        ),
        (
            InjectedConfigWriteFailure::StorageFull,
            ConfigWriteFailureReason::StorageFull,
        ),
    ] {
        let base = tempdir().expect("temp directory");
        let mut store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let loaded = store.load_or_default().expect("default config");
        let original = fs::read(&store.layout().config).expect("original config");
        let mut next = loaded.config;
        next.appearance.theme = Theme::Dark;
        store.inject_write_failure(failure);

        let error = store
            .commit_if_revision(&next, loaded.revision)
            .expect_err("injected write failure");
        assert_eq!(error.write_failure_reason(), Some(expected_reason));
        assert_eq!(
            fs::read(&store.layout().config).expect("preserved current"),
            original
        );
        assert!(!config_temp_path(&store.layout().config).exists());
    }
}

#[test]
fn post_replace_verification_failure_restores_current_v1_bytes() {
    let base = tempdir().expect("temp directory");
    let mut store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let loaded = store.load_or_default().expect("default config");
    let original = fs::read(&store.layout().config).expect("original config");
    let mut next = loaded.config;
    next.appearance.theme = Theme::Dark;
    store.inject_write_failure(InjectedConfigWriteFailure::VerificationCorruption);

    assert!(matches!(
        store.commit_if_revision(&next, loaded.revision),
        Err(ConfigError::RecoveryVerificationFailed)
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("restored config"),
        original
    );
    assert!(!config_temp_path(&store.layout().config).exists());
}

#[test]
fn occupied_temp_file_or_directory_is_retained_and_never_replaces_current() {
    for directory in [false, true] {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        let loaded = store.load_or_default().expect("default config");
        let original = fs::read(&store.layout().config).expect("original config");
        let occupied = config_temp_path(&store.layout().config);
        if directory {
            fs::create_dir(&occupied).expect("occupied temp directory");
        } else {
            fs::write(&occupied, b"unowned occupied target").expect("occupied temp file");
        }
        let mut next = loaded.config;
        next.appearance.theme = Theme::Dark;

        let error = store
            .commit_if_revision(&next, loaded.revision)
            .expect_err("occupied target failure");
        assert!(matches!(error, ConfigError::WriteTargetOccupied));
        assert_eq!(
            error.write_failure_reason(),
            Some(ConfigWriteFailureReason::TargetOccupied)
        );
        assert_eq!(
            fs::read(&store.layout().config).expect("preserved current"),
            original
        );
        if directory {
            assert!(occupied.is_dir());
        } else {
            assert_eq!(
                fs::read(&occupied).expect("preserved occupied file"),
                b"unowned occupied target"
            );
        }
    }
}

#[test]
fn interrupted_commit_preserves_valid_current_and_archives_stale_temp() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let current = store.load_or_default().expect("default config").config;
    let mut candidate = current.clone();
    candidate.appearance.language = Language::ChineseSimplified;
    let candidate_bytes = write_interrupted_temp(&store, &candidate);

    let recovered = store.load_or_default().expect("recover stale temp");
    assert_eq!(recovered.config, current);
    assert_eq!(
        recovered.interrupted_recovery,
        Some(InterruptedConfigRecovery::ArchivedStaleTemp)
    );
    assert_eq!(
        fs::read(
            interrupted_archive_paths(store.layout())
                .first()
                .expect("interrupted archive"),
        )
        .expect("interrupted archive bytes"),
        candidate_bytes
    );
    assert!(!config_temp_path(&store.layout().config).exists());

    let reloaded = store.load_or_default().expect("idempotent reload");
    assert_eq!(reloaded.interrupted_recovery, None);
    assert_eq!(interrupted_archive_paths(store.layout()).len(), 1);
}

#[test]
fn interrupted_commit_promotes_valid_temp_when_current_is_missing_or_invalid() {
    for invalid_current in [None, Some(b"invalid-current".as_slice())] {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Production,
        ))
        .expect("config store");
        if let Some(bytes) = invalid_current {
            fs::write(&store.layout().config, bytes).expect("invalid current config");
        }
        let mut candidate = NativeConfig::default();
        candidate.overlay.scale_percent = if invalid_current.is_some() { 125 } else { 150 };
        write_interrupted_temp(&store, &candidate);

        let recovered = store.load_or_default().expect("promote interrupted temp");
        assert_eq!(recovered.config, candidate);
        assert_eq!(
            recovered.interrupted_recovery,
            Some(InterruptedConfigRecovery::PromotedTemp {
                replaced_invalid_current: invalid_current.is_some(),
            })
        );
        assert!(!config_temp_path(&store.layout().config).exists());
        assert!(interrupted_archive_paths(store.layout()).is_empty());
        let quarantines = config_quarantine_paths(store.layout());
        assert_eq!(quarantines.len(), usize::from(invalid_current.is_some()));
        if let Some(bytes) = invalid_current {
            assert_eq!(fs::read(&quarantines[0]).expect("quarantine bytes"), bytes);
        }
    }
}

#[test]
fn interrupted_commit_archives_invalid_temp_without_replacing_current_or_defaulting_it() {
    for create_current in [false, true] {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        let expected = if create_current {
            store.load_or_default().expect("default config").config
        } else {
            NativeConfig::default()
        };
        let invalid_temp = b"{interrupted";
        fs::write(config_temp_path(&store.layout().config), invalid_temp)
            .expect("invalid interrupted temp");

        let recovered = store.load_or_default().expect("archive invalid temp");
        assert_eq!(recovered.config, expected);
        assert_eq!(
            recovered.interrupted_recovery,
            Some(InterruptedConfigRecovery::ArchivedInvalidTemp)
        );
        let archives = interrupted_archive_paths(store.layout());
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(&archives[0]).expect("archive bytes"), invalid_temp);
    }
}

#[test]
fn future_interrupted_schema_is_preserved_without_touching_current() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("config store");
    let current = store.load_or_default().expect("default config").config;
    let mut future = serde_json::to_value(NativeConfig::default()).expect("future value");
    future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
    future["future_section"] = serde_json::json!({ "new_field": true });
    let future_bytes = serde_json::to_vec_pretty(&future).expect("future bytes");
    let temp_path = config_temp_path(&store.layout().config);
    fs::write(&temp_path, &future_bytes).expect("future interrupted temp");

    assert!(matches!(
        store.load_or_default(),
        Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read(&temp_path).expect("preserved future temp"),
        future_bytes
    );
    assert_eq!(
        store.load_or_default().err().map(|error| error.to_string()),
        Some(format!("unsupported schema_version {}", SCHEMA_VERSION + 1))
    );
    assert_eq!(
        parse_config(&fs::read(&store.layout().config).expect("current bytes"))
            .expect("current config")
            .0,
        current
    );
    assert!(interrupted_archive_paths(store.layout()).is_empty());
}

#[test]
fn interrupted_archives_are_bounded_environment_local_and_ignore_unowned_files() {
    let base = tempdir().expect("temp directory");
    let development = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("development store");
    let production = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("production store");
    let mut current = development
        .load_or_default()
        .expect("development config")
        .config;
    production.load_or_default().expect("production config");
    let unowned = development
        .layout()
        .backups
        .join("config-interrupted-note.bin");
    fs::write(&unowned, b"keep").expect("unowned marker");

    for index in 0..6 {
        current.overlay.scale_percent = 100 + index;
        write_interrupted_temp(&development, &current);
        development.load_or_default().expect("archive stale temp");
    }

    let archives = interrupted_archive_paths(development.layout());
    assert_eq!(archives.len(), MAX_INTERRUPTED_ARCHIVES);
    assert!(
        archives
            .iter()
            .map(|path| fs::metadata(path).expect("archive metadata").len())
            .sum::<u64>()
            <= MAX_INTERRUPTED_ARCHIVE_BYTES
    );
    assert_eq!(fs::read(unowned).expect("unowned marker"), b"keep");
    assert!(interrupted_archive_paths(production.layout()).is_empty());
}

#[test]
fn forced_process_exit_releases_writer_lock_and_recovers_synced_temp() {
    let base = tempdir().expect("temp directory");
    let ready = base.path().join("crash-probe.ready");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let current = store.load_or_default().expect("default config").config;
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--ignored")
        .arg("--exact")
        .arg("tests::store::interrupted_commit_crash_probe_child")
        .arg("--nocapture")
        .env(CRASH_PROBE_BASE, base.path())
        .env(CRASH_PROBE_READY, &ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn crash probe");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        if let Some(status) = child.try_wait().expect("poll crash probe") {
            panic!("crash probe exited before ready: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "crash probe did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut rejected = current.clone();
    rejected.overlay.scale_percent = 175;
    assert!(matches!(
        store.commit(&rejected),
        Err(ConfigError::LockUnavailable)
    ));

    child.kill().expect("terminate crash probe");
    child.wait().expect("wait for crash probe");
    let recovered = store.load_or_default().expect("recover after process exit");
    assert_eq!(recovered.config, current);
    assert_eq!(
        recovered.interrupted_recovery,
        Some(InterruptedConfigRecovery::ArchivedStaleTemp)
    );
    assert_eq!(interrupted_archive_paths(store.layout()).len(), 1);
    assert!(!config_temp_path(&store.layout().config).exists());
}

#[test]
#[ignore = "spawned by forced_process_exit_releases_writer_lock_and_recovers_synced_temp"]
fn interrupted_commit_crash_probe_child() {
    let Some(base) = std::env::var_os(CRASH_PROBE_BASE) else {
        return;
    };
    let ready = PathBuf::from(std::env::var_os(CRASH_PROBE_READY).expect("crash probe ready path"));
    let store = ConfigStore::new(StorageLayout::under(base, BuildEnvironment::Development))
        .expect("crash probe store");
    let mut candidate = store.load_or_default().expect("crash probe config").config;
    candidate.overlay.scale_percent = 150;
    let _lock = store
        .acquire_writer_lock()
        .expect("crash probe writer lock");
    write_interrupted_temp(&store, &candidate);
    fs::write(ready, b"ready").expect("crash probe ready marker");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
