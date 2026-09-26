//! Importing a model, cancelling it and mapping its diagnostics.

use super::*;

/// An import queues one cover capture per installed model for the GPUI thread,
/// and the bytes that projection of the model produced land on the package
/// cover the settings catalog reports.
#[test]
fn service_queues_a_cover_capture_per_imported_model_and_installs_the_result() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let application = Application::start_with_layout(layout).expect("start application");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let signals = ApplicationMainThreadSignals::default();
    let service = ApplicationSettingsService::start_with_shortcut_receiver_and_signals(
        application,
        receiver,
        signals.clone(),
    )
    .expect("start settings service");
    let client = service.client();
    client
        .import_model_blocking(SettingsModelImportRequest {
            title: "我的猫".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import model");

    // The worker installs the model and hands it over; it never renders it.
    let queued = signals.take_model_cover_captures();
    assert_eq!(queued.len(), 1);
    let key = queued[0].key().clone();
    assert_eq!(key.origin, SettingsModelOrigin::Imported);
    assert_eq!(queued[0].model().id().as_str(), key.id);
    // Draining is what the GPUI loop does: a second poll has nothing left.
    assert!(signals.take_model_cover_captures().is_empty());

    let canonical_models_root = models_root.canonicalize().expect("canonical models root");
    let expected_cover = canonical_models_root
        .join(&key.id)
        .join("resources/cover.png");
    let captured = b"\x89PNG\r\n\x1a\ncaptured cat";
    let covered = client
        .replace_model_cover_blocking(key.clone(), captured.to_vec())
        .expect("install captured cover");
    let entry = covered
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.id == key.id && entry.origin == key.origin)
        .expect("covered entry");
    assert_eq!(entry.cover, Some(expected_cover.clone()));
    assert_eq!(
        std::fs::read(&expected_cover).expect("installed cover"),
        captured
    );

    // A capture that produced something other than a PNG is refused under the
    // same contract a user-chosen cover passes, and the installed one stays.
    assert_eq!(
        client
            .replace_model_cover_blocking(key.clone(), b"not an image".to_vec())
            .expect_err("a non-PNG capture is refused")
            .code(),
        SettingsErrorCode::ModelCoverInvalid
    );
    assert_eq!(
        std::fs::read(&expected_cover).expect("unchanged cover"),
        captured
    );

    drop(sender);
    client.shutdown_blocking().expect("shutdown service");
    service.join().expect("join service");
}

#[test]
fn service_imports_a_model_without_selecting_it_and_refreshes_the_catalog() {
    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");

    let imported = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "送葬人 · 标准模式".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("import model");
    assert!(imported.revision > initial.revision);
    assert_eq!(
        imported.active_model, None,
        "import must not implicitly activate the model"
    );
    let first = imported
        .model_catalog
        .entries
        .iter()
        .find(|entry| entry.origin == SettingsModelOrigin::Imported)
        .expect("installed entry");
    assert_eq!(first.title, "送葬人 · 标准模式");
    assert_eq!(
        first.input_mode,
        Some(SettingsModelMode::Standard),
        "the ordinary package is classified from its left-keys artwork, not its title"
    );
    assert!(matches!(
        &first.availability,
        SettingsModelAvailability::Ready { .. }
    ));
    assert!(
        bongocat_model::ModelId::parse(&first.id).is_ok(),
        "the store key must be a portable id"
    );
    assert!(models_root.join(&first.id).join("猫.model3.json").is_file());

    // Importing the same source folder again stays independent: both ids
    // are service-generated UUIDs, never derived from titles or names.
    let second = client
        .import_model_blocking(SettingsModelImportRequest {
            title: "经典小键盘 · 标准模式".to_owned(),
            source_root: model_fixture(),
            selected_mver_modes: Vec::new(),
        })
        .expect("second import of the same source");
    assert!(second.revision > imported.revision);
    let installed: Vec<_> = second
        .model_catalog
        .entries
        .iter()
        .filter(|entry| entry.origin == SettingsModelOrigin::Imported)
        .collect();
    assert_eq!(
        installed.len(),
        2,
        "both imports stay installed side by side"
    );
    assert_ne!(installed[0].id, installed[1].id, "ids are generated UUIDs");
    assert!(
        installed
            .iter()
            .any(|entry| entry.title == "经典小键盘 · 标准模式")
    );
    for entry in &installed {
        assert_eq!(entry.input_mode, Some(SettingsModelMode::Standard));
        assert!(matches!(
            &entry.availability,
            SettingsModelAvailability::Ready { .. }
        ));
        assert!(models_root.join(&entry.id).join("猫.model3.json").is_file());
    }

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn service_observes_import_cancellation_without_committing_or_revising_catalog() {
    let source = tempdir().expect("model source");
    std::fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    std::fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    )
    .expect("model3");
    std::fs::File::create(source.path().join("payload.bin"))
        .and_then(|file| file.set_len(16 * 1024 * 1024))
        .expect("large payload");

    let base = tempdir().expect("temporary storage");
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let models_root = layout.models.clone();
    let application = Application::start_with_layout(layout).expect("application start");
    let service = ApplicationSettingsService::start(application).expect("service start");
    let client = service.client();
    let initial = client.read_snapshot_blocking().expect("initial snapshot");

    let operation = client
        .start_model_import_blocking(SettingsModelImportRequest {
            title: "cancelled-model".to_owned(),
            source_root: source.path().to_owned(),
            selected_mver_modes: Vec::new(),
        })
        .expect("start import");
    let operation_id = operation.operation_id();
    assert!(operation.cancel());
    let final_result = operation.final_result_blocking();
    assert_eq!(final_result.operation_id, operation_id);
    assert_eq!(
        final_result.result.expect_err("cancelled import").code(),
        SettingsErrorCode::ModelImportCancelled
    );
    let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
    assert_eq!(unchanged.revision, initial.revision);
    assert!(!models_root.join("cancelled-model").exists());

    client.shutdown_blocking().expect("service shutdown");
    service.join().expect("service join");
}

#[test]
fn every_model_store_import_diagnostic_has_a_stable_ui_code() {
    let cases = [
        (
            ModelStoreDiagnostic::AlreadyExists,
            SettingsErrorCode::ModelAlreadyInstalled,
        ),
        (
            ModelStoreDiagnostic::Cancelled,
            SettingsErrorCode::ModelImportCancelled,
        ),
        (
            ModelStoreDiagnostic::InvalidPackage,
            SettingsErrorCode::ModelImportInvalidPackage,
        ),
        (
            ModelStoreDiagnostic::SourceContainsStore,
            SettingsErrorCode::ModelImportSourceInvalid,
        ),
        (
            ModelStoreDiagnostic::SourceChanged,
            SettingsErrorCode::ModelImportSourceChanged,
        ),
        (
            ModelStoreDiagnostic::SourceSymlinkUnsupported,
            SettingsErrorCode::ModelImportSourceUnsupported,
        ),
        (
            ModelStoreDiagnostic::SourceEntryUnsupported,
            SettingsErrorCode::ModelImportSourceUnsupported,
        ),
        (
            ModelStoreDiagnostic::SourceConversionFailed,
            SettingsErrorCode::ModelImportSourceUnsupported,
        ),
        (
            ModelStoreDiagnostic::StoreBusy,
            SettingsErrorCode::ModelStoreBusy,
        ),
        (
            ModelStoreDiagnostic::IoError,
            SettingsErrorCode::ModelImportFailed,
        ),
        (
            ModelStoreDiagnostic::NotFound,
            SettingsErrorCode::ModelImportFailed,
        ),
        (
            ModelStoreDiagnostic::StoreEntryUnsupported,
            SettingsErrorCode::ModelImportFailed,
        ),
    ];

    for (diagnostic, expected) in cases {
        assert_eq!(map_model_store_import_diagnostic(diagnostic), expected);
    }
    // Enumerating the cases is only useful while it stays complete, so a new
    // store diagnostic cannot reach the settings service unmapped.
    for diagnostic in ModelStoreDiagnostic::ALL {
        assert!(
            cases.iter().any(|(case, _)| *case == diagnostic),
            "{diagnostic:?} has no import result code"
        );
    }
}

#[test]
fn every_model_store_delete_diagnostic_has_a_stable_ui_code() {
    let cases = [
        (
            ModelStoreDiagnostic::NotFound,
            SettingsErrorCode::ModelNotFound,
        ),
        (
            ModelStoreDiagnostic::StoreBusy,
            SettingsErrorCode::ModelStoreBusy,
        ),
        (
            ModelStoreDiagnostic::AlreadyExists,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::Cancelled,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::InvalidPackage,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::IoError,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::SourceContainsStore,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::SourceChanged,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::SourceSymlinkUnsupported,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::SourceEntryUnsupported,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::SourceConversionFailed,
            SettingsErrorCode::ModelDeleteFailed,
        ),
        (
            ModelStoreDiagnostic::StoreEntryUnsupported,
            SettingsErrorCode::ModelDeleteFailed,
        ),
    ];

    for (diagnostic, expected) in cases {
        assert_eq!(map_model_store_delete_diagnostic(diagnostic), expected);
    }
    for diagnostic in ModelStoreDiagnostic::ALL {
        assert!(
            cases.iter().any(|(case, _)| *case == diagnostic),
            "{diagnostic:?} has no delete result code"
        );
    }
}
