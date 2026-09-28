//! Progress that only moves forward, and a cancel that cleans up.

use super::*;

#[test]
fn import_progress_is_monotonic_and_cancellation_removes_partial_state() {
    let source = tempdir().expect("source");
    fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    )
    .expect("model3");
    fs::write(source.path().join("payload.bin"), vec![0_u8; 512 * 1024]).expect("payload");
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let cancelled = Cell::new(false);
    let progress = RefCell::new(Vec::new());

    let error = store
        .import_with_observer(
            ModelId::parse("cancelled").expect("model id"),
            source.path(),
            |update| {
                progress.borrow_mut().push(update);
                if update.stage == ModelImportStage::Copying && update.bytes_copied >= 65_536 {
                    cancelled.set(true);
                }
            },
            || cancelled.get(),
        )
        .expect_err("cancelled import");

    assert_eq!(error.code, ModelStoreDiagnostic::Cancelled);
    assert!(store.list().expect("empty catalog").entries.is_empty());
    assert!(!store.root().join("cancelled").exists());
    assert!(
        fs::read_dir(store.root())
            .expect("store entries")
            .all(|entry| !entry
                .expect("store entry")
                .file_name()
                .to_string_lossy()
                .starts_with(IMPORTING_PREFIX))
    );
    let progress = progress.into_inner();
    assert!(progress.len() >= 3);
    for updates in progress.windows(2) {
        assert!(updates[0].stage <= updates[1].stage);
        assert!(updates[0].files_copied <= updates[1].files_copied);
        assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
    }
}
