//! The bundled presets, and a store that stays usable when one breaks.

use super::*;

#[test]
fn preset_catalog_is_the_only_path_from_bundled_resources_to_committed_models() {
    let catalog = PresetModelCatalog::open(
        repository_root().join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog");
    for id in ["standard", "keyboard", "gamepad"] {
        let model = catalog
            .load(&ModelId::parse(id).expect("model id"))
            .expect("committed preset");
        assert_eq!(model.origin(), ModelOrigin::Preset);
        assert_eq!(model.id().as_str(), id);
        assert_eq!(model.root().parent(), Some(catalog.root()));
        assert!(!model.index().textures.is_empty());
    }
}

#[test]
fn a_stray_file_in_the_preset_root_never_takes_the_catalog_down() {
    // A file manager dropping a `.DS_Store` next to the bundled models is
    // routine, so listing must skip what cannot be a model id instead of
    // reporting the whole catalog unavailable.
    let root = tempdir().expect("preset root");
    fs::write(root.path().join(".DS_Store"), b"junk").expect("stray file");
    fs::write(root.path().join("not a model"), b"junk").expect("stray file");
    fs::create_dir(root.path().join("standard")).expect("model directory");
    fs::write(
        root.path().join("standard").join("cat.model3.json"),
        br#"{"version":3}"#,
    )
    .expect("model json");

    let catalog = PresetModelCatalog::open(root.path(), ModelPackageLimits::default())
        .expect("preset catalog");
    let entries = catalog.list().expect("preset catalog listing");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id().as_str(), "standard");
}

#[test]
fn preset_catalog_is_sorted_and_retains_invalid_entries() {
    let root = tempdir().expect("preset catalog");
    for id in ["zeta", "alpha"] {
        let model = root.path().join(id);
        fs::create_dir(&model).expect("preset directory");
        fs::write(model.join("model.moc3"), b"moc").expect("preset moc");
        fs::write(
            model.join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("preset model3");
    }
    fs::remove_file(root.path().join("alpha/model.moc3")).expect("corrupt alpha preset");

    let catalog = PresetModelCatalog::open(root.path(), ModelPackageLimits::default())
        .expect("open preset catalog")
        .list()
        .expect("list preset catalog");
    assert_eq!(
        catalog
            .iter()
            .map(|entry| entry.id().as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert_eq!(catalog[0].origin(), ModelOrigin::Preset);
    assert!(matches!(
        catalog[0],
        ModelCatalogEntry::Invalid {
            code: ModelDiagnostic::ModelMocMissing,
            ..
        }
    ));
    assert!(catalog[0].snapshot().is_none());
    assert_eq!(catalog[1].origin(), ModelOrigin::Preset);
    assert!(catalog[1].snapshot().is_some());
}

#[cfg(unix)]
#[test]
fn preset_catalog_rejects_a_symlinked_model_entry() {
    use std::os::unix::fs::symlink;

    let catalog_root = tempdir().expect("catalog");
    symlink(
        repository_root().join("resources/models/standard"),
        catalog_root.path().join("standard"),
    )
    .expect("symlink preset");
    let catalog = PresetModelCatalog::open(catalog_root.path(), ModelPackageLimits::default())
        .expect("catalog root");
    let error = catalog
        .load(&ModelId::parse("standard").expect("model id"))
        .expect_err("symlinked preset must fail");
    assert_eq!(
        error.code,
        ModelDiagnostic::ModelSymlinkDirectoryUnsupported
    );
}
