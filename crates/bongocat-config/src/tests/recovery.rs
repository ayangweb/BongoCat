//! Backups, quarantines and the fallback to defaults.

use super::*;

#[test]
fn invalid_v1_current_without_backup_is_replaced_with_defaults() {
    let mut wrong_type = serde_json::to_value(NativeConfig::default()).expect("config value");
    wrong_type["overlay"]["scale_percent"] = serde_json::Value::String("yes".to_owned());
    let mut out_of_range = serde_json::to_value(NativeConfig::default()).expect("config value");
    out_of_range["overlay"]["opacity_percent"] = serde_json::Value::from(0);
    let mut unknown = serde_json::to_value(NativeConfig::default()).expect("config value");
    unknown["unexpected_field"] = serde_json::Value::Bool(true);
    let cases = [
        b"not-json".to_vec(),
        serde_json::to_vec_pretty(&wrong_type).expect("wrong type bytes"),
        serde_json::to_vec_pretty(&out_of_range).expect("out of range bytes"),
        serde_json::to_vec_pretty(&unknown).expect("unknown field bytes"),
    ];

    for bytes in cases {
        let base = tempdir().expect("temp directory");
        let store = ConfigStore::new(StorageLayout::under(
            base.path(),
            BuildEnvironment::Development,
        ))
        .expect("config store");
        fs::write(&store.layout().config, &bytes).expect("invalid config");

        let loaded = store.load_or_default().expect("default fallback");
        assert_eq!(loaded.config, NativeConfig::default());
        assert_eq!(loaded.recovery, None);
        let quarantines = config_quarantine_paths(store.layout());
        assert_eq!(quarantines.len(), 1);
        assert_eq!(fs::read(&quarantines[0]).expect("quarantine bytes"), bytes);
        assert!(config_backup_paths(store.layout()).is_empty());
    }
}

#[test]
fn corrupt_current_config_recovers_newest_valid_backup_and_is_idempotent() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.overlay.scale_percent = 110;
    store.commit(&config).expect("first config commit");
    config.overlay.scale_percent = 120;
    store.commit(&config).expect("second config commit");

    let invalid = br#"{"schema_version":2,"overlay":{"#;
    fs::write(&store.layout().config, invalid).expect("corrupt current config");
    let recovered = store.load_or_default().expect("recover valid backup");
    assert_eq!(recovered.config.overlay.scale_percent, 110);
    let recovery = recovered.recovery.expect("recovery diagnostic");
    assert_eq!(recovery.source_schema_version(), SCHEMA_VERSION);
    assert_eq!(recovery.skipped_newer_backups(), 0);
    let quarantines = config_quarantine_paths(store.layout());
    assert_eq!(quarantines.len(), 1);
    assert_eq!(
        fs::read(&quarantines[0]).expect("quarantine bytes"),
        invalid
    );

    let reloaded = store.load_or_default().expect("reload recovered config");
    assert_eq!(reloaded.config, recovered.config);
    assert_eq!(reloaded.revision, recovered.revision);
    assert_eq!(reloaded.recovery, None);
    assert_eq!(config_quarantine_paths(store.layout()).len(), 1);
}

#[test]
fn recovery_skips_future_format_schema_and_revision_mismatch() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    for scale_percent in [105, 110, 120, 130] {
        config.overlay.scale_percent = scale_percent;
        store.commit(&config).expect("config commit");
    }

    let backups = config_backup_paths(store.layout());
    let mut future_schema: ConfigBackup =
        serde_json::from_slice(&fs::read(&backups[1]).expect("backup bytes"))
            .expect("backup envelope");
    future_schema.source_schema_version = SCHEMA_VERSION + 1;
    future_schema.config["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
    fs::write(
        &backups[1],
        serde_json::to_vec_pretty(&future_schema).expect("future schema backup"),
    )
    .expect("replace backup with future schema");
    let mut revision_mismatch: ConfigBackup =
        serde_json::from_slice(&fs::read(&backups[2]).expect("backup bytes"))
            .expect("backup envelope");
    revision_mismatch.source_revision = "0000000000000000".to_owned();
    fs::write(
        &backups[2],
        serde_json::to_vec_pretty(&revision_mismatch).expect("revision mismatch backup"),
    )
    .expect("replace backup with revision mismatch");
    let mut future_format: ConfigBackup =
        serde_json::from_slice(&fs::read(&backups[3]).expect("backup bytes"))
            .expect("backup envelope");
    future_format.backup_format_version = BACKUP_FORMAT_VERSION + 1;
    fs::write(
        &backups[3],
        serde_json::to_vec_pretty(&future_format).expect("future format backup"),
    )
    .expect("replace backup with future format");

    fs::write(&store.layout().config, b"invalid-current").expect("corrupt current config");
    let recovered = store.load_or_default().expect("recover older backup");
    assert_eq!(recovered.config, NativeConfig::default());
    assert_eq!(
        recovered
            .recovery
            .expect("recovery diagnostic")
            .skipped_newer_backups(),
        3
    );
}

#[test]
fn defaults_replace_current_when_all_backups_are_invalid() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.overlay.scale_percent = 110;
    store.commit(&config).expect("config commit");
    let backup = config_backup_paths(store.layout())
        .pop()
        .expect("config backup");
    fs::write(backup, b"invalid-backup").expect("corrupt backup");
    let invalid_current = b"invalid-current";
    fs::write(&store.layout().config, invalid_current).expect("corrupt current config");

    let loaded = store.load_or_default().expect("default fallback");
    assert_eq!(loaded.config, NativeConfig::default());
    assert_eq!(loaded.recovery, None);
    let quarantines = config_quarantine_paths(store.layout());
    assert_eq!(quarantines.len(), 1);
    assert_eq!(
        fs::read(&quarantines[0]).expect("quarantine bytes"),
        invalid_current
    );

    let restarted = store.load_or_default().expect("restart with defaults");
    assert_eq!(restarted.config, loaded.config);
    assert_eq!(restarted.revision, loaded.revision);
    assert_eq!(config_quarantine_paths(store.layout()).len(), 1);
}

#[test]
fn defaults_replace_current_when_there_is_no_backup() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let invalid_current = b"invalid-current-without-backup";
    fs::write(&store.layout().config, invalid_current).expect("invalid current config");

    let recovered = store.load_or_default().expect("default fallback");
    assert_eq!(recovered.config, NativeConfig::default());
    assert_eq!(recovered.recovery, None);
    let quarantines = config_quarantine_paths(store.layout());
    assert_eq!(quarantines.len(), 1);
    assert_eq!(
        fs::read(&quarantines[0]).expect("quarantine bytes"),
        invalid_current
    );

    let restarted = store.load_or_default().expect("restart with defaults");
    assert_eq!(restarted.config, recovered.config);
    assert_eq!(restarted.revision, recovered.revision);
    assert_eq!(config_quarantine_paths(store.layout()).len(), 1);
}

#[test]
fn default_fallback_never_downgrades_a_future_schema() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("config store");
    let mut future = serde_json::to_value(NativeConfig::default()).expect("future config");
    future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
    let bytes = serde_json::to_vec_pretty(&future).expect("future bytes");
    fs::write(&store.layout().config, &bytes).expect("future current config");

    assert!(matches!(
        store.load_or_default(),
        Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("preserved future config"),
        bytes
    );
    assert!(config_quarantine_paths(store.layout()).is_empty());
}

#[test]
fn future_current_schema_is_not_rolled_back_to_an_older_backup() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    config.overlay.scale_percent = 110;
    store.commit(&config).expect("config commit");
    let mut future = serde_json::to_value(&config).expect("config value");
    future["schema_version"] = serde_json::Value::from(SCHEMA_VERSION + 1);
    future["future_section"] = serde_json::json!({ "new_field": true });
    let future_bytes = serde_json::to_vec_pretty(&future).expect("future config bytes");
    fs::write(&store.layout().config, &future_bytes).expect("future current config");

    assert!(matches!(
        store.load_or_default(),
        Err(ConfigError::UnsupportedSchema(version)) if version == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("preserved future config"),
        future_bytes
    );
    assert!(config_quarantine_paths(store.layout()).is_empty());
}

#[test]
fn recovery_quarantines_are_bounded_and_environment_local() {
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
    development_config.overlay.scale_percent = 110;
    development
        .commit(&development_config)
        .expect("development commit");
    let mut production_config = production
        .load_or_default()
        .expect("production default")
        .config;
    production_config.overlay.scale_percent = 130;
    production
        .commit(&production_config)
        .expect("production commit");

    for index in 0..6 {
        let invalid = format!("invalid-development-config-{index}");
        fs::write(&development.layout().config, invalid).expect("corrupt development config");
        development
            .load_or_default()
            .expect("recover development config");
    }

    let quarantines = config_quarantine_paths(development.layout());
    assert_eq!(quarantines.len(), MAX_CONFIG_QUARANTINES);
    assert!(
        quarantines
            .iter()
            .map(|path| fs::metadata(path).expect("quarantine metadata").len())
            .sum::<u64>()
            <= MAX_CONFIG_QUARANTINE_BYTES
    );
    assert!(config_quarantine_paths(production.layout()).is_empty());
    assert_eq!(
        production
            .load_or_default()
            .expect("production reload")
            .config
            .overlay
            .scale_percent,
        130
    );
}

#[test]
fn config_backups_are_bounded_and_do_not_remove_unowned_files() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    let mut config = store.load_or_default().expect("default config").config;
    let unowned = store.layout().backups.join("manual-note.json");
    fs::write(&unowned, b"keep").expect("unowned backup marker");

    for scale_percent in 101..=112 {
        config.overlay.scale_percent = scale_percent;
        store.commit(&config).expect("bounded backup commit");
    }

    let backup_paths = config_backup_paths(store.layout());
    assert_eq!(backup_paths.len(), MAX_CONFIG_BACKUPS);
    let total_bytes = backup_paths
        .iter()
        .map(|path| fs::metadata(path).expect("backup metadata").len())
        .sum::<u64>();
    assert!(total_bytes <= MAX_CONFIG_BACKUP_BYTES);
    assert_eq!(fs::read(unowned).expect("unowned marker"), b"keep");

    let retained_scales = backup_paths
        .iter()
        .map(|path| {
            let backup: ConfigBackup =
                serde_json::from_slice(&fs::read(path).expect("backup bytes"))
                    .expect("backup envelope");
            assert_eq!(backup.source_schema_version, SCHEMA_VERSION);
            backup.config["overlay"]["scale_percent"]
                .as_u64()
                .expect("backup scale")
        })
        .collect::<Vec<_>>();
    assert_eq!(retained_scales, (104..=111).collect::<Vec<_>>());
}

#[test]
fn backup_order_does_not_regress_when_the_wall_clock_moves_backwards() {
    let base = tempdir().expect("temp directory");
    let backups = base.path();
    let newest = backups.join("config-00000000000000000100-00004.json");
    fs::write(&newest, b"existing").expect("existing backup");

    assert_eq!(
        next_backup_path(backups, 50).expect("next backup path"),
        backups.join("config-00000000000000000100-00005.json")
    );
    assert!(!is_owned_backup_name("config-100-5.json"));
    assert!(!is_owned_backup_name("manual-note.json"));
}

#[cfg(unix)]
#[test]
fn configuration_storage_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Development,
    ))
    .expect("config store");
    store
        .commit(&NativeConfig::default())
        .expect("initial commit");
    store
        .commit(&NativeConfig {
            overlay: OverlayConfig {
                scale_percent: 110,
                ..NativeConfig::default().overlay
            },
            ..NativeConfig::default()
        })
        .expect("backup commit");

    for directory in [
        &store.layout.root,
        &store.layout.models,
        &store.layout.backups,
        &store.layout.logs,
        &store.layout.updates,
        &store.layout.locks,
    ] {
        assert_eq!(
            fs::metadata(directory)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for file in [
        store.layout.config.clone(),
        store.layout.locks.join("config.writer.lock"),
    ] {
        assert_eq!(
            fs::metadata(file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let backup = config_backup_paths(store.layout())
        .into_iter()
        .next()
        .expect("backup");
    assert_eq!(
        fs::metadata(backup)
            .expect("backup metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
