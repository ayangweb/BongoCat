//! Which family a source is, and the key artwork that says so.

use super::*;

#[test]
fn ordinary_package_mode_is_resolved_from_key_artwork_before_commit() {
    let base = tempdir().expect("mode fixture root");
    let package = |name: &str, left: &[&str], right: &[&str]| {
        let root = base.path().join(name);
        for (directory, names) in [("left-keys", left), ("right-keys", right)] {
            let path = root.join("resources").join(directory);
            fs::create_dir_all(&path).expect("key directory");
            for key in names {
                fs::write(path.join(format!("{key}.png")), b"fixture").expect("key image");
            }
        }
        root
    };

    assert_eq!(
        classify_input_mode(&package("standard", &["KeyA"], &[])).expect("standard mode"),
        ModelStoreInputMode::Standard
    );
    assert_eq!(
        classify_input_mode(&package("keyboard", &["KeyA"], &["LeftArrow"]))
            .expect("keyboard mode"),
        ModelStoreInputMode::Keyboard
    );
    assert_eq!(
        classify_input_mode(&package("gamepad-left", &["DPadUp"], &[]))
            .expect("gamepad mode from a left-hand image"),
        ModelStoreInputMode::Gamepad
    );
    assert_eq!(
        classify_input_mode(&package("gamepad-right", &[], &["East"]))
            .expect("gamepad mode from a right-hand image"),
        ModelStoreInputMode::Gamepad
    );
    let error = classify_input_mode(&package("empty", &[], &[]))
        .expect_err("a package without key artwork is not a model");
    assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
}

#[test]
fn ordinary_import_returns_the_mode_resolved_before_commit() {
    let data = tempdir().expect("store root");
    let store = model_store(data.path());
    for (id, left, right, expected) in [
        (
            "standard-import",
            &["KeyA"][..],
            &[][..],
            ModelStoreInputMode::Standard,
        ),
        (
            "keyboard-import",
            &["KeyA"][..],
            &["LeftArrow"][..],
            ModelStoreInputMode::Keyboard,
        ),
        (
            "gamepad-import",
            &["DPadUp"][..],
            &[][..],
            ModelStoreInputMode::Gamepad,
        ),
    ] {
        let source = tempdir().expect("source root");
        fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        for (directory, names) in [("left-keys", left), ("right-keys", right)] {
            let path = source.path().join("resources").join(directory);
            fs::create_dir_all(&path).expect("key directory");
            for key in names {
                fs::write(path.join(format!("{key}.png")), b"fixture").expect("key image");
            }
        }

        let (model, mode) = store
            .import_with_observer_and_input_mode(
                ModelId::parse(id).expect("model id"),
                source.path(),
                |_| {},
                || false,
            )
            .expect("classified import");
        assert_eq!(mode, expected, "mode for {id}");
        assert!(model.root().join("cat.model3.json").is_file());
    }
}

/// A model authored against the old `rdev` naming must keep working after
/// import: the installed package is rewritten to the canonical names the
/// runtime resolves, and the user's source is left exactly as it was.
#[test]
fn legacy_alt_key_images_are_renamed_on_import_without_touching_the_source() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let source = tempdir().expect("source");
    write_package_directory(source.path());
    let legacy = [
        ("resources/left-keys/Alt.png", b"alt".as_slice()),
        ("resources/left-keys/AltGr.png", b"altgr".as_slice()),
        ("resources/right-keys/Alt.png", b"hand alt".as_slice()),
    ];
    for (reference, bytes) in legacy {
        let path = source.path().join(reference);
        fs::create_dir_all(path.parent().expect("reference parent")).expect("key directory");
        fs::write(&path, bytes).expect("write legacy key image");
    }

    let installed = store
        .import(ModelId::parse("legacy").expect("model id"), source.path())
        .expect("import legacy model");

    for (reference, bytes) in [
        ("resources/left-keys/AltLeft.png", b"alt".as_slice()),
        ("resources/left-keys/AltRight.png", b"altgr".as_slice()),
        ("resources/right-keys/AltLeft.png", b"hand alt".as_slice()),
    ] {
        assert_eq!(
            fs::read(installed.root().join(reference)).expect("canonical key image"),
            bytes,
            "{reference}"
        );
    }
    for legacy in [
        "resources/left-keys/Alt.png",
        "resources/left-keys/AltGr.png",
        "resources/right-keys/Alt.png",
    ] {
        assert!(
            !installed.root().join(legacy).exists(),
            "{legacy} must not survive the import"
        );
    }
    // Renaming is the only change: the package the source described is the
    // package that was installed.
    assert_eq!(installed.index().moc, "model.moc3");
    assert_eq!(installed.index().textures.len(), 1);

    for (reference, bytes) in legacy {
        assert_eq!(
            fs::read(source.path().join(reference)).expect("source key image"),
            bytes,
            "the source keeps its own names and bytes: {reference}"
        );
    }
}

/// The rewrite never resolves a conflict by guessing: a package that ships
/// both spellings keeps the canonical file's bytes and the legacy file.
#[test]
fn an_imported_package_that_ships_both_spellings_keeps_the_canonical_image() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let source = tempdir().expect("source");
    write_package_directory(source.path());
    for (reference, bytes) in [
        ("resources/left-keys/Alt.png", b"legacy".as_slice()),
        ("resources/left-keys/AltLeft.png", b"canonical".as_slice()),
    ] {
        let path = source.path().join(reference);
        fs::create_dir_all(path.parent().expect("reference parent")).expect("key directory");
        fs::write(&path, bytes).expect("write key image");
    }

    let installed = store
        .import(ModelId::parse("both").expect("model id"), source.path())
        .expect("import model");

    assert_eq!(
        fs::read(installed.root().join("resources/left-keys/AltLeft.png"))
            .expect("canonical key image"),
        b"canonical"
    );
    assert_eq!(
        fs::read(installed.root().join("resources/left-keys/Alt.png")).expect("legacy key image"),
        b"legacy"
    );
}
