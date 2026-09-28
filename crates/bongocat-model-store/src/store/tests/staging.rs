//! The directories an operation owns, and who holds the lock.

use super::*;

#[test]
fn startup_recovers_only_well_formed_owned_operation_directories() {
    let data = tempdir().expect("data root");
    let root = data.path().join("models");
    fs::create_dir_all(root.join(".importing-alpha-10-20")).expect("import staging");
    fs::create_dir_all(root.join(".deleting-beta-10-21")).expect("delete staging");
    fs::create_dir_all(root.join(".importing-not-owned")).expect("unowned directory");

    let store = ModelStore::new(
        &root,
        data.path().join("locks/models.writer.lock"),
        ModelPackageLimits::default(),
    )
    .expect("model store");
    assert_eq!(
        store.recovery(),
        ModelStoreRecovery {
            abandoned_imports_removed: 1,
            abandoned_deletions_removed: 1,
        }
    );
    assert!(root.join(".importing-not-owned").is_dir());
}

#[test]
fn import_rejects_a_source_that_contains_the_destination_store() {
    let source = tempdir().expect("source");
    fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    )
    .expect("model3");
    let store = model_store(source.path());

    let error = store
        .import(
            ModelId::parse("recursive").expect("model id"),
            source.path(),
        )
        .expect_err("recursive source must fail");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceContainsStore);
    assert!(store.list().expect("empty catalog").entries.is_empty());
}

#[test]
fn store_lock_makes_contention_observable() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(data.path().join("locks/models.writer.lock"))
        .expect("open store lock");
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => panic!("test lock unexpectedly busy"),
        Err(TryLockError::Error(error)) => panic!("test lock failed: {error}"),
    }

    let error = store.list().expect_err("contended store must fail");
    assert_eq!(error.code, ModelStoreDiagnostic::StoreBusy);
}
