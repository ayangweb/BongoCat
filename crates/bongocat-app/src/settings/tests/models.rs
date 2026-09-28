//! The model catalog, its metadata and the model commands.

use super::*;

#[test]
fn service_maps_ignore_input_shortcuts_to_model_settings() {
    fn wait_for_snapshot<F>(client: &SettingsClient, mut predicate: F) -> SettingsSnapshot
    where
        F: FnMut(&SettingsSnapshot) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = client.read_snapshot_blocking().expect("settings snapshot");
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "ignore-input shortcut did not update the settings snapshot"
            );
            std::thread::yield_now();
        }
    }

    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("start application");
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let service = ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
        .expect("start settings service");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    let mut revision = initial.config_revision.expect("config revision");

    for (command, field) in [
        (ShortcutCommand::ToggleIgnoreMouseInput, "ignore_pointer"),
        (
            ShortcutCommand::ToggleIgnoreKeyboardInput,
            "ignore_keyboard",
        ),
        (ShortcutCommand::ToggleIgnoreGamepadInput, "ignore_gamepad"),
    ] {
        sender.send(command).expect("queue ignore-input shortcut");
        let updated = wait_for_snapshot(&client, |snapshot| {
            snapshot.config_revision != Some(revision)
                && match field {
                    "ignore_pointer" => snapshot.model_settings.ignore_pointer,
                    "ignore_keyboard" => snapshot.model_settings.ignore_keyboard,
                    "ignore_gamepad" => snapshot.model_settings.ignore_gamepad,
                    _ => false,
                }
        });
        revision = updated.config_revision.expect("updated config revision");
    }

    let final_snapshot = client.read_snapshot_blocking().expect("final snapshot");
    assert!(final_snapshot.model_settings.ignore_pointer);
    assert!(final_snapshot.model_settings.ignore_keyboard);
    assert!(final_snapshot.model_settings.ignore_gamepad);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(persisted.contains("\"ignore_pointer\": true"));
    assert!(persisted.contains("\"ignore_keyboard\": true"));
    assert!(persisted.contains("\"ignore_gamepad\": true"));

    drop(sender);
    client.shutdown_blocking().expect("shutdown service");
    service.join().expect("join service");
}

/// The whole chain a gamepad plug travels: the frame source's notice, the
/// settings service reading the runtime's own answer, and the model that ends
/// up on screen.
#[test]
fn a_gamepad_connection_notice_switches_the_model_the_settings_service_owns() {
    fn wait_for_snapshot<F>(client: &SettingsClient, mut predicate: F) -> SettingsSnapshot
    where
        F: FnMut(&SettingsSnapshot) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = client.read_snapshot_blocking().expect("settings snapshot");
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "the gamepad connection never reached the active model"
            );
            std::thread::yield_now();
        }
    }

    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout.clone()).expect("start application");
    // The producers are handles into the runtime the application owns, so
    // they are taken before the service takes ownership of it.
    let input = application.input_producer();
    let axis = application.gamepad_axis_producer();
    let service = ApplicationSettingsService::start(application).expect("start settings service");
    let client = service.client();

    let initial = client.read_snapshot_blocking().expect("initial snapshot");
    assert!(!initial.gamepad_auto_switch.enabled);
    assert_eq!(initial.gamepad_auto_switch.connected_model, None);
    // Startup activates a model before anything else runs, and the "last
    // model used" targets are exactly that history.
    let standard = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let started = client
        .select_model_blocking(
            initial.config_revision.expect("config revision"),
            standard.clone(),
        )
        .expect("activate the startup model");
    let gamepad_model = SettingsModelKey {
        id: "gamepad".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let configured = client
        .set_gamepad_auto_switch_blocking(
            started.config_revision.expect("config revision"),
            SettingsGamepadAutoSwitch {
                enabled: true,
                connected_model: Some(gamepad_model.clone()),
                disconnected_model: None,
            },
        )
        .expect("enable the auto switch");
    assert!(configured.gamepad_auto_switch.enabled);
    assert_eq!(
        configured.gamepad_auto_switch.connected_model,
        Some(gamepad_model.clone())
    );
    let revision = configured.config_revision.expect("config revision");

    // A notice with no gamepad attached is a no-op: the model that matches
    // the current state is already the one on screen.
    client
        .notify_gamepad_connection_changed()
        .expect("queue the notice");
    let untouched = client
        .read_snapshot_blocking()
        .expect("snapshot after notice");
    assert_eq!(untouched.active_model, Some(standard.clone()));

    let connection = axis.connect(0).expect("gamepad connection");
    input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection event");
    // The frame source only notices the transition after the runtime has
    // applied it, so the notice is queued against an observed state.
    wait_for_snapshot(&client, |snapshot| {
        snapshot.input_diagnostics.connected_gamepad_count == 1
    });
    client
        .notify_gamepad_connection_changed()
        .expect("queue the notice");
    let connected = wait_for_snapshot(&client, |snapshot| {
        snapshot.active_model.as_ref() == Some(&gamepad_model)
    });
    assert_ne!(
        connected.config_revision,
        Some(revision),
        "the automatic switch is an ordinary model selection and is persisted"
    );

    input
        .publish(InputEvent::GamepadDisconnected {
            connection,
            at: MonotonicMillis::new(1),
        })
        .expect("disconnection event");
    wait_for_snapshot(&client, |snapshot| {
        snapshot.input_diagnostics.connected_gamepad_count == 0
    });
    client
        .notify_gamepad_connection_changed()
        .expect("queue the notice");
    let disconnected = wait_for_snapshot(&client, |snapshot| {
        snapshot.active_model.as_ref() == Some(&standard)
    });
    assert_eq!(disconnected.input_diagnostics.connected_gamepad_count, 0);
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(persisted.contains("\"gamepad_auto_switch\""));
    assert!(persisted.contains("\"id\": \"gamepad\""));

    client.shutdown_blocking().expect("shutdown service");
    service.join().expect("join service");
}

#[test]
fn service_renames_and_covers_a_model_of_either_origin() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let overrides_root = layout.model_overrides.clone();
    let config_path = layout.config.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    // The store canonicalizes its own root, and on macOS `$TMPDIR` resolves
    // through `/private`, so the expected paths are canonical too. The roots
    // only exist once the application has created them.
    let canonical_models_root = models_root.canonicalize().expect("canonical models root");
    let canonical_overrides_root = overrides_root
        .canonicalize()
        .expect("canonical model overrides root");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    let imported = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "原始名称".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import model");
    let revision = imported.config_revision.expect("config revision");
    let entry = imported
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.origin == SettingsModelOrigin::Imported)
        .expect("installed entry")
        .clone();
    let key = SettingsModelKey {
        id: entry.id.clone(),
        origin: SettingsModelOrigin::Imported,
    };
    // The page needs the package directory and, since this fixture ships no
    // cover, must be told there is none rather than guessing a path.
    assert_eq!(entry.directory, Some(canonical_models_root.join(&entry.id)));
    assert_eq!(entry.cover, None);

    let renamed = client
        .set_model_title_blocking(revision, key.clone(), "我的猫".to_owned())
        .expect("rename model");
    let renamed_entry = renamed
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == entry.id && entry.origin == key.origin)
        .expect("renamed entry");
    assert_eq!(renamed_entry.title, "我的猫");
    // The title is configuration metadata, so it survives as configuration
    // rather than living only in the snapshot the page happens to hold.
    let persisted = std::fs::read_to_string(&config_path).expect("persisted config");
    assert!(persisted.contains("我的猫"));

    let revision = renamed.config_revision.expect("config revision");
    assert_eq!(
        client
            .set_model_title_blocking(revision, key.clone(), "   ".to_owned())
            .expect_err("an empty title is not a name")
            .code(),
        SettingsErrorCode::ModelTitleInvalid
    );

    // A cover is written into the package itself and reported back through
    // the catalog, so the page can render it without knowing the layout.
    let cover_source = base.path().join("cover-source.png");
    let cover_bytes = b"\x89PNG\r\n\x1a\npairing artwork";
    std::fs::write(&cover_source, cover_bytes).expect("cover source");
    let covered = client
        .set_model_cover_blocking(key.clone(), cover_source.clone())
        .expect("replace cover");
    let covered_entry = covered
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == key.id && entry.origin == key.origin)
        .expect("covered entry");
    let expected_cover = canonical_models_root
        .join(&key.id)
        .join("resources/cover.png");
    assert_eq!(covered_entry.cover, Some(expected_cover.clone()));
    assert_eq!(
        std::fs::read(&expected_cover).expect("installed cover"),
        cover_bytes
    );

    // A file that is not a PNG is refused, and the installed cover stays.
    let not_an_image = base.path().join("notes.txt");
    std::fs::write(&not_an_image, b"not an image").expect("plain file");
    assert_eq!(
        client
            .set_model_cover_blocking(key.clone(), not_an_image)
            .expect_err("a non-PNG cover is refused")
            .code(),
        SettingsErrorCode::ModelCoverInvalid
    );
    assert_eq!(
        std::fs::read(&expected_cover).expect("unchanged cover"),
        cover_bytes
    );

    // The same two edits on a model the build ships. Its package lives
    // inside the application bundle, so the rename goes into the preset
    // list and the cover into the user's override root: the preset is
    // customised exactly like an installed model, without either of them
    // being written into the bundle.
    let preset = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let bundled_cover = crate::repository_preset_root()
        .canonicalize()
        .expect("canonical preset root")
        .join(&preset.id)
        .join("resources/cover.png");
    let bundled_bytes = std::fs::read(&bundled_cover).expect("bundled preset cover");
    let bundled_entry = covered
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
        .expect("preset entry");
    assert_eq!(
        bundled_entry.title, "standard",
        "a preset that was never renamed is named by the id the build gave it"
    );
    assert_eq!(bundled_entry.cover, Some(bundled_cover.clone()));

    let revision = covered.config_revision.expect("config revision");
    let renamed_preset = client
        .set_model_title_blocking(revision, preset.clone(), "我的预设".to_owned())
        .expect("rename a preset model");
    let renamed_preset_entry = renamed_preset
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
        .expect("renamed preset entry");
    assert_eq!(renamed_preset_entry.title, "我的预设");
    // The preset list is its own id space: the installed model renamed
    // above kept its own record, and the same id in both lists names two
    // different models.
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).expect("persisted config"))
            .expect("config json");
    assert_eq!(
        document["model"]["built_in_models"],
        serde_json::json!([{ "id": "standard", "title": "我的预设" }])
    );

    let covered_preset = client
        .set_model_cover_blocking(preset.clone(), cover_source)
        .expect("replace a preset cover");
    let covered_preset_entry = covered_preset
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
        .expect("covered preset entry");
    let expected_preset_cover = canonical_overrides_root
        .join(&preset.id)
        .join("resources/cover.png");
    assert_eq!(
        covered_preset_entry.cover,
        Some(expected_preset_cover.clone()),
        "the replacement must be what the page draws, not the bundled artwork"
    );
    assert_eq!(
        std::fs::read(&expected_preset_cover).expect("stored preset cover"),
        cover_bytes
    );
    assert_eq!(
        std::fs::read(&bundled_cover).expect("bundled cover after the edit"),
        bundled_bytes,
        "a preset's package is app-bundled and must never be written to"
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_deletes_only_unselected_installed_source_identity() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    seed_installed_model(&layout.models, "standard");
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let current = client.read_snapshot_blocking().expect("initial snapshot");

    let selected = client
        .select_model_blocking(
            current.config_revision.expect("config revision"),
            SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
        )
        .expect("select preset duplicate");
    let deleted = client
        .delete_model_blocking(SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::Imported,
        })
        .expect("delete installed duplicate");
    assert!(selected.revision > current.revision);
    assert!(deleted.revision > selected.revision);
    assert_eq!(
        deleted.active_model,
        Some(SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        })
    );
    assert!(
        !deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Imported
        })
    );
    assert!(
        deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::BuiltIn
        })
    );

    let preset_error = client
        .delete_model_blocking(SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        })
        .expect_err("preset deletion");
    assert_eq!(
        preset_error.code(),
        SettingsErrorCode::PresetModelCannotBeDeleted
    );
    let missing_error = client
        .delete_model_blocking(SettingsModelKey {
            id: "missing".to_owned(),
            origin: SettingsModelOrigin::Imported,
        })
        .expect_err("missing installed model");
    assert_eq!(missing_error.code(), SettingsErrorCode::ModelNotFound);

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_deletes_the_selected_installed_model_and_switches_to_the_preset() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let imported = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "selected".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import model");
    let installed_id = imported
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.origin == SettingsModelOrigin::Imported)
        .expect("installed entry")
        .id
        .clone();
    let selected = client
        .select_model_blocking(
            imported.config_revision.expect("config revision"),
            SettingsModelKey {
                id: installed_id.clone(),
                origin: SettingsModelOrigin::Imported,
            },
        )
        .expect("select installed model");

    let deleted = client
        .delete_model_blocking(SettingsModelKey {
            id: installed_id.clone(),
            origin: SettingsModelOrigin::Imported,
        })
        .expect("selected model deletion switches away first");
    // One snapshot carries both halves: the package is gone from the
    // catalog, and the model that replaced it is the standard preset.
    assert!(deleted.revision >= selected.revision);
    assert!(!deleted.model_catalog.entries.iter().any(|entry| {
        entry.id == installed_id && entry.origin == SettingsModelOrigin::Imported
    }));
    assert_eq!(
        deleted.active_model.as_ref().map(|model| model.origin),
        Some(SettingsModelOrigin::BuiltIn)
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_maps_invalid_model_inputs_to_stable_errors() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();

    // The import title is free-form display text and never becomes the
    // store key: even a path-like title imports cleanly with a
    // service-generated UUID id. Deletion still validates ids strictly.
    let imported = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "../escape".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import with a path-like title");
    let imported_entry = imported
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.origin == SettingsModelOrigin::Imported)
        .expect("installed entry");
    assert_eq!(imported_entry.title, "../escape");
    assert!(bongocat_model::ModelId::parse(&imported_entry.id).is_ok());

    let invalid_delete_id = client
        .delete_model_blocking(SettingsModelKey {
            id: "../escape".to_owned(),
            origin: SettingsModelOrigin::Imported,
        })
        .expect_err("invalid delete model id");
    assert_eq!(invalid_delete_id.code(), SettingsErrorCode::InvalidModelId);

    let invalid_package_source = tempdir().expect("invalid package");
    std::fs::write(
        invalid_package_source.path().join("not-a-model.txt"),
        b"invalid",
    )
    .expect("invalid model marker");
    let invalid_package = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "invalid-package".to_owned(),
            source_root: invalid_package_source.path().to_owned(),
            selected_mver_modes: Vec::new(),
        })
        .expect_err("invalid package");
    assert_eq!(
        invalid_package.code(),
        SettingsErrorCode::ModelImportInvalidPackage
    );
    assert!(
        !invalid_package
            .to_string()
            .contains(&invalid_package_source.path().display().to_string())
    );

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}
