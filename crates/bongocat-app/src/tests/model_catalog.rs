//! The merged model catalog, the selection and the startup fallback.

use super::*;

#[test]
fn missing_selected_model_falls_back_to_the_standard_preset_at_startup() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut configured = store.load_or_default().expect("default config").config;
    configured.model.selected_model = Some(ModelIdentity {
        id: "ghost".to_owned(),
        source: ModelSource::Imported,
    });
    configured.model.imported_models = vec![ImportedModelMetadata {
        id: "ghost".to_owned(),
        title: "幽灵模型".to_owned(),
        input_mode: ModelInputMode::Standard,
    }];
    store.commit(&configured).expect("seed selection");

    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    // Without a render consumer the fallback cannot finish activation, but
    // the corrected selection must already be persisted and logged.
    assert!(matches!(
        application.restore_startup_model(),
        Err(ApplicationError::RenderConsumerUnavailable)
    ));
    assert_eq!(
        application.config().model.selected_model,
        Some(ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        })
    );
    assert_eq!(
        application
            .application_log_diagnostics()
            .events
            .model_selection_fallback,
        1
    );
    let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
    assert!(persisted.contains("\"standard\""));
    application.shutdown().expect("clean shutdown");
}

#[test]
fn metadata_records_for_missing_model_directories_are_pruned_at_startup() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut configured = store.load_or_default().expect("default config").config;
    configured.model.imported_models = vec![
        ImportedModelMetadata {
            id: "ghost".to_owned(),
            title: "被手动删除".to_owned(),
            input_mode: ModelInputMode::Standard,
        },
        ImportedModelMetadata {
            id: "still-there".to_owned(),
            title: "目录仍在".to_owned(),
            input_mode: ModelInputMode::Standard,
        },
    ];
    store.commit(&configured).expect("seed metadata");
    std::fs::create_dir_all(layout.models.join("still-there")).expect("model directory present");

    let application = Application::start_with_layout(layout).expect("start application");
    assert_eq!(
        application.config().model.imported_models,
        vec![ImportedModelMetadata {
            id: "still-there".to_owned(),
            title: "目录仍在".to_owned(),
            input_mode: ModelInputMode::Standard,
        }]
    );
    application.shutdown().expect("clean shutdown");
}

/// Regression for a model deleted by hand in the file manager: browsing the
/// models root leaves `.DS_Store` behind, and that single foreign file used
/// to fail the whole catalog scan, which turned the Model library page into an
/// unusable "catalog unavailable" state and also stopped stale metadata from
/// being pruned.
#[test]
fn hand_deleted_model_beside_file_manager_metadata_keeps_the_catalog_available() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut configured = store.load_or_default().expect("default config").config;
    configured.model.imported_models = vec![ImportedModelMetadata {
        id: "deleted-by-hand".to_owned(),
        title: "被手动删除".to_owned(),
        input_mode: ModelInputMode::Standard,
    }];
    store.commit(&configured).expect("seed metadata");
    std::fs::create_dir_all(&layout.models).expect("models root");
    std::fs::write(layout.models.join(".DS_Store"), b"finder metadata")
        .expect("file manager metadata");

    let application = Application::start_with_layout(layout).expect("start application");
    let catalog = application.model_catalog().expect("merged catalog");
    assert!(catalog.iter().any(|entry| {
        entry.origin() == ModelOrigin::Preset && entry.id().as_str() == "standard"
    }));
    assert!(application.config().model.imported_models.is_empty());
    application.shutdown().expect("clean shutdown");
}

#[test]
fn merged_model_catalog_retains_source_identity_for_duplicate_ids() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    seed_installed_model(&layout.models, "standard");
    let application = Application::start_with_layout(layout).expect("start application");

    let catalog = application.model_catalog().expect("merged catalog");
    let duplicate = catalog
        .iter()
        .filter(|entry| entry.id().as_str() == "standard")
        .map(ModelCatalogEntry::origin)
        .collect::<Vec<_>>();
    assert_eq!(
        duplicate,
        [
            bongocat_model::ModelOrigin::Preset,
            bongocat_model::ModelOrigin::Installed,
        ]
    );
    assert!(catalog.windows(2).all(|entries| {
        entries[0].origin() == ModelOrigin::Preset || entries[1].origin() == ModelOrigin::Installed
    }));
    application.shutdown().expect("clean shutdown");
}

/// The Model library page lists the build's presets first, in mode order, and the
/// imported models after them in the order they were imported.
///
/// Both halves used to be one list sorted by id, which put the presets in
/// reverse — `gamepad` < `keyboard` < `standard` alphabetically — and let a
/// new import land anywhere among them instead of at the end. The order is
/// state rather than a per-run accident, so this pins it across a restart
/// and across a deletion too.
#[test]
fn model_catalog_lists_presets_in_mode_order_then_installed_models_in_import_order() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

    assert_eq!(
        catalog_ids(&application),
        vec!["standard", "keyboard", "gamepad"],
        "the presets are the three modes, in mode order"
    );

    let first = import_one(&mut application, "第一只猫", source.clone());
    let second = import_one(&mut application, "第二只猫", source);
    let imported = vec![
        "standard".to_owned(),
        "keyboard".to_owned(),
        "gamepad".to_owned(),
        first.id().as_str().to_owned(),
        second.id().as_str().to_owned(),
    ];
    assert_eq!(
        catalog_ids(&application),
        imported,
        "each import joins the end of the page, after every preset"
    );
    application.shutdown().expect("clean shutdown");

    let mut restarted = Application::start_with_layout(layout).expect("restart the application");
    assert_eq!(
        catalog_ids(&restarted),
        imported,
        "the order is configuration, not the order one run happened to build"
    );

    restarted
        .delete_model(ModelOrigin::Installed, first.id().as_str())
        .expect("delete the first import");
    assert_eq!(
        catalog_ids(&restarted),
        vec![
            "standard".to_owned(),
            "keyboard".to_owned(),
            "gamepad".to_owned(),
            second.id().as_str().to_owned(),
        ],
        "removing an import leaves the rest in place"
    );
    restarted.shutdown().expect("clean shutdown");
}

/// A package copied into the store root by hand never went through an
/// import, so it has no place in the import order. It still has to appear —
/// the store scan is what makes a user's own directory visible — and the
/// page must not reshuffle every time the scan walks the directory in a
/// different order, so those entries are ordered by id instead.
#[test]
fn hand_copied_model_lands_after_the_imports_in_id_order() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
    let imported = import_one(&mut application, "导入的猫", source);

    // Deliberately seeded in the opposite order to the ids, so passing this
    // cannot come from the order the scan walked the directory in.
    seed_installed_model(&layout.models, "zz-copied-by-hand");
    seed_installed_model(&layout.models, "aa-copied-by-hand");
    assert_eq!(
        catalog_ids(&application),
        vec![
            "standard".to_owned(),
            "keyboard".to_owned(),
            "gamepad".to_owned(),
            imported.id().as_str().to_owned(),
            "aa-copied-by-hand".to_owned(),
            "zz-copied-by-hand".to_owned(),
        ]
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn installed_duplicate_selection_persists_its_origin_across_restart() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    seed_installed_model(&layout.models, "standard");
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    let selected = application
        .select_model(ModelOrigin::Installed, "standard")
        .expect("select installed duplicate");
    assert_eq!(
        selected
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert_eq!(
        application.active_model_origin(),
        Some(ModelOrigin::Installed)
    );
    assert_eq!(
        application.config().model.selected_model,
        Some(ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::Imported,
        })
    );
    application.shutdown().expect("clean shutdown");

    let mut restarted = Application::start_with_layout(layout).expect("restart application");
    assert_eq!(
        restarted.config().model.selected_model,
        Some(ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::Imported,
        })
    );
    restarted
        .select_model(ModelOrigin::Installed, "standard")
        .expect("reload installed duplicate");
    assert_eq!(
        restarted.active_model_origin(),
        Some(ModelOrigin::Installed)
    );
    restarted.shutdown().expect("clean restart shutdown");
}

#[test]
fn deleting_the_live_installed_model_falls_back_to_the_standard_preset() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
    let active_id = import_one(&mut application, "active", source)
        .id()
        .as_str()
        .to_owned();
    application
        .select_model(ModelOrigin::Installed, active_id.as_str())
        .expect("activate model");

    application
        .delete_model(ModelOrigin::Installed, active_id.as_str())
        .expect("selected model deletion switches away first");
    assert!(installed_catalog_ids(&application).is_empty());
    assert_eq!(
        application.config().model.selected_model,
        Some(ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        })
    );
    assert_eq!(application.active_model_origin(), Some(ModelOrigin::Preset));
    application.shutdown().expect("clean shutdown");
}

#[test]
fn installed_duplicate_can_be_deleted_while_same_id_preset_is_selected() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    seed_installed_model(&layout.models, "standard");
    let mut application = Application::start_with_layout(layout).expect("start application");
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("select preset");

    application
        .delete_model(ModelOrigin::Installed, "standard")
        .expect("delete installed duplicate");
    assert!(installed_catalog_ids(&application).is_empty());

    let preset_error = application
        .delete_model(ModelOrigin::Preset, "standard")
        .expect_err("preset deletion must fail");
    assert!(matches!(
        preset_error,
        ApplicationError::PresetModelDeletion(_)
    ));
    application.shutdown().expect("clean shutdown");
}

#[test]
fn configured_installed_model_deletion_falls_back_before_restart_activation() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    let selected_id = import_one(&mut application, "selected", source)
        .id()
        .as_str()
        .to_owned();
    application
        .select_model(ModelOrigin::Installed, selected_id.as_str())
        .expect("select installed model");
    application.shutdown().expect("clean shutdown");

    let mut restarted = Application::start_with_layout(layout).expect("restart application");
    assert!(restarted.runtime_client().snapshot().active_model.is_none());
    // Nothing is live yet, so the recorded selection is the only fact that
    // names this model — and it is enough on its own to switch away first.
    restarted
        .delete_model(ModelOrigin::Installed, selected_id.as_str())
        .expect("configured model deletion switches away first");
    assert!(installed_catalog_ids(&restarted).is_empty());
    assert_eq!(
        restarted.config().model.selected_model,
        Some(ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        })
    );
    restarted.shutdown().expect("clean restart shutdown");
}

#[test]
fn rejected_gpu_model_switch_restores_the_previous_config_selection() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let config_path = layout.config.clone();
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    let initial_token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare initial model");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let initial_frame = wait_for_model_commit_frame(&consumer, initial_token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: initial_frame.model_commit.expect("initial commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit initial model");
    application
        .runtime_client()
        .wait_for_command(initial_token.command_sequence, RUNTIME_TIMEOUT)
        .expect("initial model activation");

    let switch = std::thread::spawn(move || {
        let rejected = matches!(
            application.select_model(ModelOrigin::Preset, "keyboard"),
            Err(ApplicationError::RuntimeCommandFailed(_))
        );
        (application, rejected)
    });
    let candidate = wait_for_any_model_commit_frame(&consumer);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: candidate.model_commit.expect("candidate commit token"),
            outcome: ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
        })
        .expect("reject candidate model");
    let (application, rejected) = switch.join().expect("selection worker");
    assert!(rejected);
    assert_eq!(
        application
            .runtime_client()
            .snapshot()
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert_eq!(application.config().model.selected_model, None);
    let persisted = std::fs::read_to_string(config_path).expect("restored config");
    assert!(persisted.contains("\"selected_model\": null"));
    application.shutdown().expect("clean shutdown");
}
