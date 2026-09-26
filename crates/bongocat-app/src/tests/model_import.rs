//! Importing a model package or a legacy BongoCatMver source.

use super::*;

/// One BongoCatMver source describes several models: importing it installs
/// one converted model per mode, each with its own store key and a title
/// that tells them apart.
#[test]
fn importing_a_legacy_source_installs_one_titled_model_per_mode() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application =
        Application::start_with_layout(layout.clone()).expect("start application");
    application
        .set_language(Language::ChineseSimplified)
        .expect("set language");
    let source = base.path().join("Bongo Cat Mver");
    fs::create_dir(&source).expect("legacy source");
    legacy_source_fixture(&source);

    let installed = application
        .import_models("我的猫", &source)
        .expect("import legacy source");
    assert_eq!(installed.len(), 3, "one model per configured mode");
    assert_eq!(
        application
            .config()
            .model
            .imported_models
            .iter()
            .map(|metadata| metadata.title.as_str())
            .collect::<Vec<_>>(),
        vec![
            "我的猫 · 标准模式",
            "我的猫 · 键盘模式",
            "我的猫 · 手柄模式"
        ]
    );
    assert_eq!(
        application
            .config()
            .model
            .imported_models
            .iter()
            .map(|metadata| metadata.input_mode)
            .collect::<Vec<_>>(),
        vec![
            ModelInputMode::Standard,
            ModelInputMode::Keyboard,
            ModelInputMode::Gamepad
        ],
        "the selected Mver section, not the converted directory shape, owns the stored mode"
    );
    let gamepad_id = installed[2].id().as_str().to_owned();
    application
        .set_model_title(ModelOrigin::Installed, gamepad_id.clone(), "我的手柄")
        .expect("rename converted gamepad model");
    assert_eq!(
        application
            .config()
            .model
            .imported_models
            .iter()
            .find(|metadata| metadata.id == gamepad_id.as_str())
            .expect("renamed gamepad metadata")
            .input_mode,
        ModelInputMode::Gamepad,
        "renaming a model changes only its title, never its stored mode"
    );
    let ids = installed
        .iter()
        .map(|model| model.id().as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 3, "each model has its own store key");

    // The standard mode has one paw per key and no right hand; the split
    // modes publish both sides, with the gamepad's own vocabulary.
    assert_eq!(installed[0].index().entry, "cat.model3.json");
    assert!(
        installed[0]
            .root()
            .join("resources/left-keys/KeyA.png")
            .is_file()
    );
    assert!(
        installed[0]
            .root()
            .join("resources/left-keys/KeyB.png")
            .is_file()
    );
    assert!(!installed[0].root().join("resources/right-keys").exists());
    assert!(
        installed[1]
            .root()
            .join("resources/left-keys/KeyA.png")
            .is_file()
    );
    assert!(
        installed[1]
            .root()
            .join("resources/right-keys/LeftArrow.png")
            .is_file()
    );
    // The converted gamepad model's key images must land on the product's own
    // button names, because those are the only names the runtime's binding
    // table and the renderer's key-image resolver know. This is the check
    // that the bundled preset's rename and the Mver conversion agree: if the
    // conversion emitted a parallel vocabulary, an imported gamepad model
    // would install successfully and then show no key image for any button.
    let gamepad_root = installed[2].root().to_path_buf();
    let mut converted = std::collections::BTreeSet::new();
    for directory in ["left-keys", "right-keys"] {
        for entry in fs::read_dir(gamepad_root.join("resources").join(directory))
            .expect("converted key directory")
            .filter_map(Result::ok)
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            converted.insert(name);
        }
    }
    assert_eq!(
        converted,
        std::collections::BTreeSet::from([
            "DpadUp.png".to_owned(),
            "LeftTrigger.png".to_owned(),
            "South.png".to_owned(),
            "Start.png".to_owned(),
        ]),
        "the converted gamepad model must ship the product's button names"
    );
    for name in &converted {
        let stem = name.trim_end_matches(".png");
        assert!(
            bongocat_input::GamepadButton::ALL
                .iter()
                .any(|button| button.key_image_name() == stem),
            "{name} is not a gamepad button's image name"
        );
    }
    // And every converted name is one the binding table can actually use, so
    // the model is not just correctly named but correctly bound. The binding
    // is read straight off the installed package's key images, which is
    // exactly what model activation does.
    let bindings = input_bindings_for_model(
        ModelOrigin::Installed,
        installed[2].id().as_str(),
        &KeyImageInventory::read(&gamepad_root),
    );
    for (button, expected) in [
        (GamepadButton::DpadUp, HandSide::Left),
        (GamepadButton::LeftTrigger, HandSide::Left),
        (GamepadButton::South, HandSide::Right),
        (GamepadButton::Start, HandSide::Right),
    ] {
        assert_eq!(
            bindings.hand_for_gamepad(button),
            Some(expected),
            "{button:?} must bind to the hand its own directory chose"
        );
    }
    for model in &installed {
        assert!(model.root().join("resources/background.png").is_file());
        assert!(model.root().join("resources/cover.png").is_file());
    }

    // The legacy folder is read, never written into.
    assert!(
        source
            .join("img/standard/cat_model/cat.model3.json")
            .is_file()
    );
    let catalog = application.model_catalog().expect("model catalog");
    assert_eq!(
        catalog
            .iter()
            .filter(|entry| entry.origin() == ModelOrigin::Installed)
            .count(),
        3
    );
    let expected_modes = installed
        .iter()
        .map(|model| {
            (
                model.id().as_str().to_owned(),
                application.model_input_mode(ModelOrigin::Installed, model.id().as_str()),
            )
        })
        .collect::<Vec<_>>();
    application.shutdown().expect("clean shutdown");

    let restarted = Application::start_with_layout(layout).expect("restart application");
    assert_eq!(
        restarted
            .config()
            .model
            .imported_models
            .iter()
            .map(|metadata| (metadata.id.as_str(), Some(metadata.input_mode)))
            .collect::<Vec<_>>(),
        expected_modes
            .iter()
            .map(|(id, mode)| (id.as_str(), *mode))
            .collect::<Vec<_>>(),
        "every converted mode is read back from config after restart"
    );
    for (id, mode) in expected_modes {
        assert_eq!(
            restarted.model_input_mode(ModelOrigin::Installed, &id),
            mode
        );
    }
    restarted.shutdown().expect("clean shutdown");
}

#[test]
fn importing_a_legacy_source_installs_only_the_selected_modes() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    application
        .set_language(Language::ChineseSimplified)
        .expect("set language");
    let source = base.path().join("Bongo Cat Mver");
    fs::create_dir(&source).expect("legacy source");
    legacy_source_fixture(&source);

    let installed = application
        .import_models_with_selected_modes_with_observer(
            "仅选模式",
            &source,
            vec![MverInputMode::Gamepad, MverInputMode::Keyboard],
            |_| {},
            || false,
        )
        .expect("import selected legacy modes");

    // The application reports and stores the selection in the store's mode
    // order, and the unselected Standard mode never reaches the catalog.
    assert_eq!(installed.len(), 2);
    assert_eq!(
        application
            .config()
            .model
            .imported_models
            .iter()
            .map(|metadata| metadata.title.as_str())
            .collect::<Vec<_>>(),
        vec!["仅选模式 · 键盘模式", "仅选模式 · 手柄模式"]
    );
    assert_eq!(
        application
            .config()
            .model
            .imported_models
            .iter()
            .map(|metadata| metadata.input_mode)
            .collect::<Vec<_>>(),
        vec![ModelInputMode::Keyboard, ModelInputMode::Gamepad]
    );
    assert!(
        installed[0]
            .root()
            .join("resources/right-keys/LeftArrow.png")
            .is_file()
    );
    assert!(
        installed[1]
            .root()
            .join("resources/left-keys/DpadUp.png")
            .is_file()
    );
    assert_eq!(
        application
            .model_catalog()
            .expect("model catalog")
            .iter()
            .filter(|entry| entry.origin() == ModelOrigin::Installed)
            .count(),
        2
    );
    application.shutdown().expect("clean shutdown");
}

/// The settings monitor drops an update that moves backwards, so folding the
/// per-model progress of one action has to keep the sequence monotone while
/// still counting every model.
#[test]
fn legacy_import_progress_stays_monotone_across_models() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    let source = base.path().join("Bongo Cat Mver");
    fs::create_dir(&source).expect("legacy source");
    legacy_source_fixture(&source);

    let updates = std::cell::RefCell::new(Vec::new());
    let installed = application
        .import_models_with_observer(
            "legacy",
            &source,
            |update| updates.borrow_mut().push(update),
            || false,
        )
        .expect("import legacy source");
    assert_eq!(installed.len(), 3);

    let updates = updates.into_inner();
    assert_eq!(
        updates.first().map(|update| update.stage),
        Some(ModelImportStage::Preparing)
    );
    assert_eq!(
        updates.last().map(|update| update.stage),
        Some(ModelImportStage::Committing)
    );
    for pair in updates.windows(2) {
        assert!(pair[0].stage <= pair[1].stage);
        assert!(pair[0].files_copied <= pair[1].files_copied);
        assert!(pair[0].bytes_copied <= pair[1].bytes_copied);
    }
    // The three conversions are counted end to end, so the last update is
    // the sum of what all of them wrote.
    let final_update = updates.last().expect("final update");
    assert_eq!(
        final_update.files_copied,
        installed
            .iter()
            .map(|model| model.index().package_file_count as u64)
            .sum::<u64>()
    );
    assert_eq!(
        final_update.bytes_copied,
        installed
            .iter()
            .map(|model| model.index().package_total_bytes)
            .sum::<u64>()
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn legacy_model_titles_stay_within_the_configuration_limit() {
    let hint = "猫".repeat(bongocat_config::MODEL_METADATA_MAXIMUM_TITLE_CHARS);
    let title = legacy_model_title(&hint, Path::new("/source"), "fallback", "标准模式");
    assert!(
        title.chars().count() <= bongocat_config::MODEL_METADATA_MAXIMUM_TITLE_CHARS,
        "{} characters",
        title.chars().count()
    );
    assert!(title.ends_with("标准模式"));
    // A blank hint still produces a distinguishable title per mode.
    assert_eq!(
        legacy_model_title("", Path::new("/source/我的猫"), "fallback", "手柄模式"),
        "我的猫 · 手柄模式"
    );
}

#[test]
fn import_progress_folding_carries_finished_models_forward() {
    let mut updates = Vec::new();
    {
        let mut accumulator = ImportProgressAccumulator::new(|update| updates.push(update));
        for files in [1_u64, 2] {
            for (stage, files_copied) in [
                (ModelImportStage::Preparing, 0),
                (ModelImportStage::Copying, files),
                (ModelImportStage::Validating, files),
                (ModelImportStage::Committing, files),
            ] {
                accumulator.report(ModelImportProgress {
                    stage,
                    files_copied,
                    bytes_copied: files_copied * 100,
                });
            }
        }
    }
    for pair in updates.windows(2) {
        assert!(pair[0].stage <= pair[1].stage, "{pair:?}");
        assert!(pair[0].files_copied <= pair[1].files_copied, "{pair:?}");
        assert!(pair[0].bytes_copied <= pair[1].bytes_copied, "{pair:?}");
    }
    let final_update = updates.last().expect("final update");
    assert_eq!(final_update.files_copied, 3, "1 file + 2 files");
    assert_eq!(final_update.bytes_copied, 300);
    // The stage never regresses to the second model's `Preparing`.
    assert_eq!(final_update.stage, ModelImportStage::Committing);
}

#[test]
fn application_imports_into_its_environment_model_store() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let mut application = Application::start_with_layout(layout).expect("start application");
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

    let imported = import_one(&mut application, "unicode", source);
    assert_eq!(
        imported.root(),
        models_root
            .canonicalize()
            .expect("canonical models root")
            .join(imported.id().as_str())
    );
    assert!(imported.root().join("猫.model3.json").is_file());

    let catalog = application.model_catalog().expect("model catalog");
    assert!(catalog.iter().any(|entry| {
        entry.origin() == bongocat_model::ModelOrigin::Installed && entry.id() == imported.id()
    }));

    application
        .delete_model(ModelOrigin::Installed, imported.id().as_str())
        .expect("delete model");
    assert!(installed_catalog_ids(&application).is_empty());
    application.shutdown().expect("clean shutdown");
}

#[test]
fn import_hints_become_titles_while_ids_stay_generated_uuids() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

    let first = import_one(&mut application, "我的猫", source.clone());
    let second = import_one(&mut application, "我的猫", source);
    assert_ne!(first.id(), second.id(), "ids are independent UUIDs");

    let installed = installed_catalog_ids(&application);
    assert_eq!(installed.len(), 2);
    assert_eq!(
        application.config().model.imported_models,
        vec![
            ImportedModelMetadata {
                id: first.id().as_str().to_owned(),
                title: "我的猫".to_owned(),
                input_mode: ModelInputMode::Standard,
            },
            ImportedModelMetadata {
                id: second.id().as_str().to_owned(),
                title: "我的猫".to_owned(),
                input_mode: ModelInputMode::Standard,
            },
        ]
    );

    application
        .delete_model(ModelOrigin::Installed, first.id().as_str())
        .expect("delete first model");
    assert_eq!(
        application.config().model.imported_models,
        vec![ImportedModelMetadata {
            id: second.id().as_str().to_owned(),
            title: "我的猫".to_owned(),
            input_mode: ModelInputMode::Standard,
        }]
    );
    application.shutdown().expect("clean shutdown");
}
