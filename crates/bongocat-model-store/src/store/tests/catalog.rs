//! A catalogue that stays usable when one entry is broken.

use super::*;

#[cfg(unix)]
#[test]
fn catalog_and_delete_never_follow_an_installed_symlink() {
    use std::os::unix::fs::symlink;

    let data = tempdir().expect("data root");
    let outside = tempdir().expect("outside root");
    fs::write(outside.path().join("keep"), b"outside").expect("outside marker");
    let store = model_store(data.path());
    symlink(outside.path(), store.root().join("linked")).expect("installed symlink");
    let id = ModelId::parse("linked").expect("model id");

    let catalog = store.list().expect("catalog");
    assert!(catalog.entries.is_empty());
    assert_eq!(catalog.skipped_entries, 1);
    assert_eq!(
        store.delete(&id).expect_err("delete rejects symlink").code,
        ModelStoreDiagnostic::StoreEntryUnsupported
    );
    assert_eq!(
        fs::read(outside.path().join("keep")).expect("outside marker preserved"),
        b"outside"
    );
}

#[test]
fn catalog_survives_platform_metadata_in_the_store_root() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let fixture = fixture("非 ASCII 模型");
    store
        .import(ModelId::parse("alpha").expect("model id"), &fixture)
        .expect("import alpha");
    // Browsing the store root in Finder (for example to delete an installed
    // model by hand) drops `.DS_Store` next to the model directories. It is
    // file-manager state, never a model, and must not make the catalog
    // unavailable.
    fs::write(store.root().join(".DS_Store"), b"finder metadata").expect("finder metadata");

    let catalog = store.list().expect("catalog");
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].id().as_str(), "alpha");
    assert_eq!(catalog.skipped_entries, 0);
}

#[test]
fn catalog_skips_foreign_entries_without_hiding_the_remaining_models() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let fixture = fixture("非 ASCII 模型");
    store
        .import(ModelId::parse("alpha").expect("model id"), &fixture)
        .expect("import alpha");
    fs::write(store.root().join("notes.txt"), b"user note").expect("foreign file");
    // A directory whose name is not a portable model id can never be an
    // installed model, so it is skipped instead of entering the catalog.
    fs::create_dir(store.root().join("not a model id")).expect("foreign directory");
    fs::write(store.root().join("Thumbs.db"), b"windows metadata").expect("windows metadata");
    // A directory carrying a valid model id stays visible even when it holds
    // no usable package; only entries that cannot be models are skipped.
    fs::create_dir(store.root().join("empty-model")).expect("empty model directory");

    let catalog = store.list().expect("catalog");
    assert_eq!(catalog.entries.len(), 2);
    assert_eq!(catalog.entries[0].id().as_str(), "alpha");
    assert!(matches!(
        catalog.entries[1],
        ModelCatalogEntry::Invalid { .. }
    ));
    assert_eq!(catalog.entries[1].id().as_str(), "empty-model");
    assert_eq!(catalog.skipped_entries, 2);
}

#[test]
fn catalog_is_sorted_and_reports_corrupt_packages_without_hiding_valid_models() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let fixture = fixture("非 ASCII 模型");
    store
        .import(ModelId::parse("zeta").expect("model id"), &fixture)
        .expect("import zeta");
    let alpha = store
        .import(ModelId::parse("alpha").expect("model id"), &fixture)
        .expect("import alpha");
    fs::remove_file(alpha.root().join("模型 数据.moc3")).expect("corrupt alpha");

    let catalog = store.list().expect("catalog");
    assert_eq!(catalog.entries.len(), 2);
    assert_eq!(catalog.entries[0].id().as_str(), "alpha");
    assert!(matches!(
        catalog.entries[0],
        ModelCatalogEntry::Invalid { .. }
    ));
    assert_eq!(catalog.entries[0].origin(), ModelOrigin::Installed);
    assert!(catalog.entries[0].snapshot().is_none());
    assert_eq!(catalog.entries[1].id().as_str(), "zeta");
    assert!(matches!(
        catalog.entries[1],
        ModelCatalogEntry::Ready { .. }
    ));
    assert_eq!(catalog.entries[1].origin(), ModelOrigin::Installed);
    assert!(catalog.entries[1].snapshot().is_some());
    assert_eq!(
        store
            .load(&ModelId::parse("zeta").expect("model id"))
            .expect("load installed model")
            .id()
            .as_str(),
        "zeta"
    );

    drop(store);
    let reopened = model_store(data.path());
    assert_eq!(
        reopened.list().expect("persistent catalog").entries.len(),
        2
    );
}
