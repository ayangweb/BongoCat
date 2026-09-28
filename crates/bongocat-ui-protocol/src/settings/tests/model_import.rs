//! The request, the progress, the cancellation and the title.

use super::*;

#[test]
fn model_source_display_names_are_extension_aware_but_never_panic() {
    assert_eq!(
        model_source_display_name(&PathBuf::from("/models/我的猫 · 标准模式.zip"), false,)
            .as_deref(),
        Some("我的猫 · 标准模式")
    );
    // The extension match is case-insensitive and never assumed to be ASCII
    // adjacent: a name ending in four bytes of a multi-byte character must
    // be returned as it stands instead of being sliced mid-character.
    assert_eq!(
        model_source_display_name(&PathBuf::from("/models/ARCHIVE.ZIP"), false).as_deref(),
        Some("ARCHIVE")
    );
    assert_eq!(
        model_source_display_name(&PathBuf::from("/models/猫猫猫"), false).as_deref(),
        Some("猫猫猫")
    );
    assert_eq!(
        model_source_display_name(&PathBuf::from("/models/model.moc3"), false).as_deref(),
        Some("model.moc3")
    );
    assert_eq!(model_source_display_name(&PathBuf::from("/"), false), None);
    assert_eq!(
        model_source_display_name(&PathBuf::from("/models/   "), false).as_deref(),
        None
    );
}

#[test]
fn a_directory_keeps_its_archive_like_name() {
    let root = PathBuf::from("/models/bongocat-model-source-name-test.zip");
    // A folder may legitimately be called `something.zip`; only a file
    // carries the extension that the suggestion drops.
    assert_eq!(
        model_source_display_name(&root, true).as_deref(),
        Some("bongocat-model-source-name-test.zip")
    );
    assert_eq!(
        model_source_display_name(&root, false).as_deref(),
        Some("bongocat-model-source-name-test")
    );
}

#[test]
fn model_import_command_preserves_the_typed_request() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let expected = SettingsModelImportRequest {
        title: "custom-model".to_owned(),
        source_root: PathBuf::from("selected/model"),
        selected_mver_modes: vec![SettingsMverMode::Keyboard],
    };
    let worker = thread::spawn({
        let expected = expected.clone();
        move || {
            let SettingsCommand::ImportModel {
                request,
                operation,
                reply,
            } = endpoint.recv_blocking().expect("import command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(request, expected);
            assert_eq!(operation.operation_id().get(), 1);
            reply
                .respond(Ok(snapshot(2, true, true)))
                .expect("import reply");
        }
    });

    let imported = client
        .import_model_blocking(expected)
        .expect("import snapshot");
    assert_eq!(imported.revision, 2);
    worker.join().expect("worker join");
}

#[test]
fn inspect_model_source_command_returns_the_typed_content() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::InspectModelSource { source_root, reply } =
            endpoint.recv_blocking().expect("inspect command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(source_root, PathBuf::from("selected/model"));
        reply
            .respond(Ok(SettingsModelSourceContent::Mver {
                modes: vec![SettingsMverMode::Standard, SettingsMverMode::Gamepad],
            }))
            .expect("inspect reply");
    });

    let content = client
        .inspect_model_source_blocking(PathBuf::from("selected/model"))
        .expect("inspect content");
    assert_eq!(
        content,
        SettingsModelSourceContent::Mver {
            modes: vec![SettingsMverMode::Standard, SettingsMverMode::Gamepad,],
        }
    );
    worker.join().expect("worker join");
}

#[test]
fn import_operations_share_monotonic_ids_progress_and_cancellation() {
    let (client, _endpoint) = SettingsClient::bounded(2);
    let clone = client.clone();
    let (first, first_control, _) = client.prepare_model_import().expect("first operation");
    let (second, _, _) = clone.prepare_model_import().expect("second operation");

    assert_eq!(first.operation_id().get(), 1);
    assert_eq!(second.operation_id().get(), 2);
    assert_eq!(
        first.progress(),
        SettingsModelImportProgress {
            stage: SettingsModelImportStage::Preparing,
            files_copied: 0,
            bytes_copied: 0,
        }
    );
    assert!(first_control.report_progress(SettingsModelImportProgress {
        stage: SettingsModelImportStage::Copying,
        files_copied: 2,
        bytes_copied: 4_096,
    }));
    assert!(!first_control.report_progress(SettingsModelImportProgress {
        stage: SettingsModelImportStage::Preparing,
        files_copied: 1,
        bytes_copied: 128,
    }));
    assert_eq!(first.progress().files_copied, 2);
    assert_eq!(first.progress().bytes_copied, 4_096);
    let monitor = first.monitor();
    assert_eq!(monitor.operation_id(), first.operation_id());
    assert!(monitor.cancel());
    assert!(!first.cancel());
    assert!(monitor.is_cancelled());
    assert!(first_control.is_cancelled());
}

#[test]
fn import_operation_returns_a_typed_final_result() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let worker = thread::spawn(move || {
        let SettingsCommand::ImportModel {
            operation, reply, ..
        } = endpoint.recv_blocking().expect("import command")
        else {
            panic!("unexpected command");
        };
        assert_eq!(operation.operation_id().get(), 1);
        reply
            .respond(Ok(snapshot(7, true, true)))
            .expect("import reply");
    });

    let operation = client
        .start_model_import_blocking(SettingsModelImportRequest {
            title: "custom-model".to_owned(),
            source_root: PathBuf::from("selected/model"),
            selected_mver_modes: Vec::new(),
        })
        .expect("start import");
    let operation_id = operation.operation_id();
    let final_result = operation.final_result_blocking();
    assert_eq!(final_result.operation_id, operation_id);
    assert_eq!(final_result.result.expect("final snapshot").revision, 7);
    worker.join().expect("worker join");
}

#[test]
fn model_delete_command_preserves_source_identity() {
    let (client, endpoint) = SettingsClient::bounded(1);
    let expected = SettingsModelKey {
        id: "custom-model".to_owned(),
        origin: SettingsModelOrigin::Imported,
    };
    let worker = thread::spawn({
        let expected = expected.clone();
        move || {
            let SettingsCommand::DeleteModel { model, reply } =
                endpoint.recv_blocking().expect("delete command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(model, expected);
            reply
                .respond(Ok(snapshot(3, true, true)))
                .expect("delete reply");
        }
    });

    let deleted = client
        .delete_model_blocking(expected)
        .expect("delete snapshot");
    assert_eq!(deleted.revision, 3);
    worker.join().expect("worker join");
}
