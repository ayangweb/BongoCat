//! The store end to end: import, cover, delete, and id allocation.

use super::*;

#[test]
fn ordinary_import_accepts_legacy_physics_without_fps() {
    let data = tempdir().expect("store root");
    let store = model_store(data.path());
    let source = tempdir().expect("source root");
    fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[],"Physics":"model.physics3.json"}}"#,
    )
    .expect("model3");
    fs::write(
        source.path().join("model.physics3.json"),
        r#"{
          "Version":3,
          "Meta":{
            "PhysicsSettingCount":1,"TotalInputCount":1,"TotalOutputCount":1,"VertexCount":2,
            "EffectiveForces":{"Gravity":{"X":0,"Y":-1},"Wind":{"X":0,"Y":0}},
            "PhysicsDictionary":[{"Id":"Physics1","Name":""}]
          },
          "PhysicsSettings":[{
            "Id":"Physics1",
            "Input":[{"Source":{"Target":"Parameter","Id":"ParamInput"},"Weight":100,"Type":"X","Reflect":false}],
            "Output":[{"Destination":{"Target":"Parameter","Id":"ParamOutput"},"VertexIndex":1,"Scale":1,"Weight":100,"Type":"Angle","Reflect":false}],
            "Vertices":[
              {"Position":{"X":0,"Y":0},"Mobility":0.8,"Delay":0.8,"Acceleration":1,"Radius":0},
              {"Position":{"X":0,"Y":10},"Mobility":0.8,"Delay":0.8,"Acceleration":1,"Radius":10}
            ],
            "Normalization":{"Position":{"Minimum":-10,"Default":0,"Maximum":10},"Angle":{"Minimum":-10,"Default":0,"Maximum":10}}
          }]
        }"#,
    )
    .expect("legacy physics");
    for (directory, name) in [("left-keys", "KeyA"), ("right-keys", "LeftArrow")] {
        let path = source.path().join("resources").join(directory);
        fs::create_dir_all(&path).expect("key directory");
        fs::write(path.join(format!("{name}.png")), b"fixture").expect("key image");
    }

    let (model, mode) = store
        .import_with_observer_and_input_mode(
            ModelId::parse("legacy-physics").expect("model id"),
            source.path(),
            |_| {},
            || false,
        )
        .expect("legacy physics import");
    assert_eq!(mode, ModelStoreInputMode::Keyboard);
    assert_eq!(
        model
            .physics_definition()
            .expect("physics definition")
            .expect("declared physics")
            .fps,
        0.0
    );
}

#[test]
fn ordinary_package_without_key_artwork_is_rejected_without_leaving_a_model() {
    let data = tempdir().expect("store root");
    let store = model_store(data.path());
    let source = tempdir().expect("source root");
    fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    )
    .expect("model3");

    let error = store
        .import(ModelId::parse("empty").expect("model id"), source.path())
        .expect_err("a package without key artwork must be rejected");
    assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
    assert!(store.list().expect("empty catalog").entries.is_empty());
    assert!(!store.root().join("empty").exists());
}

#[test]
fn shared_fixture_import_preserves_parser_rejection_and_catalog_contract() {
    let data = tempdir().expect("fixture store root");
    let store = model_store(data.path());
    let accepted = store
        .import(
            ModelId::parse("fixture-accepted").expect("accepted model id"),
            fixture("combined-parameters-accepted"),
        )
        .expect("accepted fixture import");
    assert_eq!(accepted.id().as_str(), "fixture-accepted");

    let rejected = store
        .import(
            ModelId::parse("fixture-rejected").expect("rejected model id"),
            fixture("missing-moc"),
        )
        .expect_err("rejected fixture import");
    assert_eq!(rejected.code, ModelStoreDiagnostic::InvalidPackage);
    assert_eq!(store.list().expect("fixture catalog").entries.len(), 1);
}

#[test]
fn a_cover_replacement_lands_on_the_package_cover_and_leaves_no_staging_file() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let id = ModelId::parse("cover").expect("model id");
    let installed = store
        .import(id.clone(), fixture("非 ASCII 模型"))
        .expect("import model");

    let replacement = b"\x89PNG\r\n\x1a\nreplacement".to_vec();
    let cover = store
        .replace_cover(&id, &replacement)
        .expect("replace cover");

    assert_eq!(cover, installed.root().join("resources/cover.png"));
    assert_eq!(fs::read(&cover).expect("stored cover"), replacement);
    let leftovers = fs::read_dir(installed.root().join("resources"))
        .expect("resources directory")
        .map(|entry| entry.expect("resource entry").file_name())
        .filter(|name| name.to_string_lossy().ends_with(".new"))
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "staging files left behind: {leftovers:?}"
    );
}

#[test]
fn a_cover_cannot_be_replaced_on_a_model_that_is_not_installed() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let error = store
        .replace_cover(&ModelId::parse("absent").expect("model id"), b"bytes")
        .expect_err("missing model");
    assert_eq!(error.code, ModelStoreDiagnostic::NotFound);
}

#[test]
fn imports_valid_package_without_modifying_source() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let source = fixture("非 ASCII 模型");
    let source_moc = fs::read(source.join("模型 数据.moc3")).expect("source moc");
    let installed = store
        .import(ModelId::parse("unicode").expect("model id"), &source)
        .expect("import model");

    assert_eq!(installed.root(), store.root().join("unicode"));
    assert_eq!(installed.index().moc, "模型 数据.moc3");
    assert_eq!(
        fs::read(source.join("模型 数据.moc3")).expect("source unchanged"),
        source_moc
    );
    assert!(installed.root().join("猫.model3.json").is_file());
}

#[cfg(unix)]
#[test]
fn model_store_data_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let installed = store
        .import(
            ModelId::parse("private").expect("model id"),
            fixture("非 ASCII 模型"),
        )
        .expect("import model");

    assert_eq!(
        fs::metadata(store.root())
            .expect("model root metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(installed.root())
            .expect("installed model metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(installed.root().join("猫.model3.json"))
            .expect("installed model file metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(data.path().join("locks"))
            .expect("lock directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(data.path().join("locks/models.writer.lock"))
            .expect("lock file metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn duplicate_id_never_overwrites_installed_model() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let id = ModelId::parse("unicode").expect("model id");
    let installed = store
        .import(id.clone(), fixture("非 ASCII 模型"))
        .expect("first import");
    fs::write(installed.root().join("user-marker"), b"keep").expect("marker");

    let error = store
        .import(id, fixture("非 ASCII 模型"))
        .expect_err("duplicate import");
    assert_eq!(error.code, ModelStoreDiagnostic::AlreadyExists);
    assert_eq!(
        fs::read(installed.root().join("user-marker")).expect("marker preserved"),
        b"keep"
    );
}

#[test]
fn invalid_package_leaves_no_destination_or_staging_directory() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let error = store
        .import(
            ModelId::parse("broken").expect("model id"),
            fixture("missing-moc"),
        )
        .expect_err("invalid import");
    assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
    assert!(store.list().expect("empty catalog").entries.is_empty());
}

/// The store's only source shape is the folder a user picked, so a path that
/// is not one has to fail cleanly rather than half-import or panic.
///
/// This is the property the archive source's removal left behind: the
/// picker never returns a file, but the store is a public API and a caller
/// that hands it one still has to get a stable diagnostic and an untouched
/// store — not a panic from code that assumed a directory, and not a
/// half-written staging tree.
#[test]
fn a_source_that_is_not_a_folder_fails_without_touching_the_store() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let sources = tempdir().expect("sources");
    let file = sources.path().join("model.package");
    fs::write(&file, b"not a model folder").expect("source file");

    let error = store
        .import(ModelId::parse("a-file").expect("model id"), &file)
        .expect_err("a file is not a model folder");
    assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
    assert_store_holds_no_entries(&store);

    // Describing it is not an error either: the answer is "a package", and
    // the import above is what reports the real diagnostic.
    assert_eq!(
        store.inspect_source(&file).expect("describe a file"),
        ModelSourceContent::Package
    );
    assert_store_holds_no_entries(&store);
}

#[cfg(unix)]
#[test]
fn import_rejects_even_internal_symbolic_links() {
    use std::os::unix::fs::symlink;

    let source = tempdir().expect("source");
    fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
    symlink("model.moc3", source.path().join("alias.moc3")).expect("symlink");
    fs::write(
        source.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"alias.moc3","Textures":[]}}"#,
    )
    .expect("model3");
    let data = tempdir().expect("data root");
    let store = model_store(data.path());

    let error = store
        .import(ModelId::parse("linked").expect("model id"), source.path())
        .expect_err("symlink import");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceSymlinkUnsupported);
    assert!(store.list().expect("empty catalog").entries.is_empty());
}

#[test]
fn delete_retires_only_the_selected_installed_model() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let fixture = fixture("非 ASCII 模型");
    let alpha = ModelId::parse("alpha").expect("model id");
    let beta = ModelId::parse("beta").expect("model id");
    store.import(alpha.clone(), &fixture).expect("import alpha");
    store.import(beta.clone(), &fixture).expect("import beta");

    store.delete(&alpha).expect("delete alpha");
    assert_eq!(store.list().expect("catalog").entries.len(), 1);
    assert_eq!(
        store.load(&alpha).expect_err("alpha removed").code,
        ModelStoreDiagnostic::NotFound
    );
    assert_eq!(store.load(&beta).expect("beta preserved").id(), &beta);
}

#[test]
fn allocate_unique_id_generates_distinct_portable_ids() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..32 {
        let id = store.allocate_unique_id().expect("allocate");
        assert_eq!(id.as_str().len(), 36, "hyphenated UUID v4 length");
        assert!(ModelId::parse(id.as_str()).is_ok());
        assert!(seen.insert(id.as_str().to_owned()), "ids must be unique");
    }
}

/// A BongoCatMver source is one user-picked folder that describes several
/// models. The store must recognize it from its own bytes while leaving a
/// genuine BongoCat package alone.
#[test]
fn legacy_sources_are_described_by_their_own_bytes() {
    use crate::mver::fixture;

    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let sources = tempdir().expect("sources");
    let legacy = sources.path().join("Bongo Cat Mver");
    fs::create_dir(&legacy).expect("legacy source");
    fixture::legacy_source(&legacy, &fixture::all_modes(), true);

    assert_eq!(
        store
            .inspect_source(&legacy)
            .expect("describe legacy directory"),
        ModelSourceContent::Mver {
            modes: MverInputMode::ALL.to_vec(),
        }
    );

    // A genuine package is still a package.
    assert_eq!(
        store
            .inspect_source(fixture("非 ASCII 模型"))
            .expect("describe package"),
        ModelSourceContent::Package
    );
    let directory = sources.path().join("looks-like-a-package");
    write_package_directory(&directory);
    assert_eq!(
        store
            .inspect_source(&directory)
            .expect("describe package directory"),
        ModelSourceContent::Package
    );
}

/// Installing a legacy source produces one installed model per mode, each
/// with the structure the runtime and the settings page expect.
#[test]
fn legacy_import_installs_one_model_per_configured_mode() {
    use crate::mver::fixture;

    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let sources = tempdir().expect("sources");
    let legacy = sources.path().join("Bongo Cat Mver");
    fs::create_dir(&legacy).expect("legacy source");
    fixture::legacy_source(&legacy, &fixture::all_modes(), true);

    let mut installed = Vec::new();
    for mode in MverInputMode::ALL {
        let id = store.allocate_unique_id().expect("allocate id");
        installed.push(
            store
                .import_mver_with_observer(id, mode, &legacy, |_| {}, || false)
                .unwrap_or_else(|error| panic!("{} failed: {error:?}", mode.as_str())),
        );
    }

    assert_eq!(store.list().expect("catalog").entries.len(), 3);
    assert_ne!(installed[0].id(), installed[1].id());
    for (model, mode) in installed.iter().zip(MverInputMode::ALL) {
        assert_eq!(model.index().entry, "cat.model3.json");
        assert_eq!(model.index().moc, "model.moc3");
        assert!(
            model
                .root()
                .join(format!("resources/left-keys/{}.png", left_key_name(mode)))
                .is_file(),
            "{} must install a composed left key image",
            mode.as_str()
        );
        assert!(
            model.root().join("resources/background.png").is_file(),
            "{} must install its background",
            mode.as_str()
        );
        assert_eq!(
            model.root().join("resources/right-keys").is_dir(),
            mode != MverInputMode::Standard,
            "only the split modes expose a right hand ({})",
            mode.as_str()
        );
    }
    // The legacy source itself is never modified.
    assert!(legacy.join("config.json").is_file());
    assert!(legacy.join("img").is_dir());
    // A conversion leaves no staging directory behind.
    assert!(
        fs::read_dir(store.root())
            .expect("store entries")
            .all(|entry| !entry
                .expect("store entry")
                .file_name()
                .to_string_lossy()
                .starts_with(IMPORTING_PREFIX))
    );
}

/// A conversion is cancellable and, like every other import, leaves nothing
/// behind when it is.
#[test]
fn legacy_import_reports_monotonic_progress_and_cancels_without_partial_state() {
    use crate::mver::fixture;

    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let sources = tempdir().expect("sources");
    let legacy = sources.path().join("Bongo Cat Mver");
    fs::create_dir(&legacy).expect("legacy source");
    fixture::legacy_source(&legacy, &fixture::all_modes(), true);

    let progress = RefCell::new(Vec::new());
    let id = store.allocate_unique_id().expect("allocate id");
    store
        .import_mver_with_observer(
            id.clone(),
            MverInputMode::Standard,
            &legacy,
            |update| progress.borrow_mut().push(update),
            || false,
        )
        .expect("observed conversion");
    let progress = progress.into_inner();
    assert_eq!(
        progress.first().map(|update| update.stage),
        Some(ModelImportStage::Preparing)
    );
    assert_eq!(
        progress.last().map(|update| update.stage),
        Some(ModelImportStage::Committing)
    );
    assert!(
        progress
            .iter()
            .any(|update| update.stage == ModelImportStage::Copying)
    );
    assert!(
        progress
            .iter()
            .any(|update| update.stage == ModelImportStage::Validating)
    );
    for updates in progress.windows(2) {
        assert!(updates[0].stage <= updates[1].stage);
        assert!(updates[0].files_copied <= updates[1].files_copied);
        assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
    }

    let cancelled = Cell::new(false);
    let error = store
        .import_mver_with_observer(
            store.allocate_unique_id().expect("allocate id"),
            MverInputMode::Keyboard,
            &legacy,
            |update| {
                if update.stage == ModelImportStage::Copying && update.files_copied > 0 {
                    cancelled.set(true);
                }
            },
            || cancelled.get(),
        )
        .expect_err("cancelled conversion");
    assert_eq!(error.code, ModelStoreDiagnostic::Cancelled);
    let catalog = store.list().expect("catalog");
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].id(), &id);
    assert!(
        fs::read_dir(store.root())
            .expect("store entries")
            .all(|entry| !entry
                .expect("store entry")
                .file_name()
                .to_string_lossy()
                .starts_with(IMPORTING_PREFIX))
    );
}

/// Asking for a mode the source does not carry is a stable diagnostic, not
/// an installed model.
#[test]
fn legacy_import_rejects_a_mode_the_source_does_not_carry() {
    use crate::mver::fixture;

    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let sources = tempdir().expect("sources");
    let legacy = sources.path().join("Bongo Cat Mver");
    fs::create_dir(&legacy).expect("legacy source");
    fixture::legacy_source(
        &legacy,
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );

    let error = store
        .import_mver_with_observer(
            store.allocate_unique_id().expect("allocate id"),
            MverInputMode::Gamepad,
            &legacy,
            |_| {},
            || false,
        )
        .expect_err("missing input mode");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceConversionFailed);
    assert_store_holds_no_entries(&store);
}

/// Import the community BongoCat model the maintainer points at.
///
/// Models downloaded for the old `rdev`-based BongoCat carry third-party
/// artwork and cannot be committed to the repository, so the real-world case
/// is covered by pointing `BONGOCAT_PACKAGE_SAMPLE` at one — the folder the
/// model was exported as. The test is skipped when the variable is unset,
/// which keeps it out of the default `cargo test` line.
///
/// What it asserts is the whole point of the rewrite: every pre-rename key
/// image the source shipped is installed under its canonical name with the
/// same bytes, no pre-rename name survives, every other key image is copied
/// verbatim, and the source the user picked is not written to.
#[test]
fn imports_the_bongo_cat_sample_named_by_the_environment() {
    use std::collections::BTreeMap;

    let Some(source) = std::env::var_os("BONGOCAT_PACKAGE_SAMPLE") else {
        return;
    };
    let source = PathBuf::from(source);
    let data = tempdir().expect("data root");
    let store = model_store(data.path());

    let source_images = source_key_images(&source);
    assert!(
        !source_images.is_empty(),
        "{} ships no key images",
        source.display()
    );
    let mut expected: BTreeMap<(String, String), Vec<u8>> = BTreeMap::new();
    for ((directory, name), bytes) in source_images.clone() {
        let canonical = match name.as_str() {
            "Alt.png" => "AltLeft.png",
            "AltGr.png" => "AltRight.png",
            "Return.png" => "Enter.png",
            _ => {
                expected.insert((directory, name), bytes);
                continue;
            }
        };
        expected.insert((directory, canonical.to_owned()), bytes);
    }

    let installed = store
        .import(store.allocate_unique_id().expect("allocate id"), &source)
        .expect("import sample");

    for directory in ["resources/left-keys", "resources/right-keys"] {
        let mut actual = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(installed.root().join(directory)) {
            for entry in entries {
                let entry = entry.expect("installed key image");
                if entry.file_type().expect("installed entry type").is_dir() {
                    continue;
                }
                actual.insert(
                    entry.file_name().into_string().expect("key image name"),
                    fs::read(entry.path()).expect("read installed key image"),
                );
            }
        }
        let want = expected
            .iter()
            .filter(|((expected_directory, _), _)| expected_directory == directory)
            .map(|((_, name), bytes)| (name.clone(), bytes.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, want, "{directory} contents after import");
    }

    // The user's source is an input, not a working copy: it keeps its own
    // names and bytes.
    assert_eq!(
        source_key_images(&source),
        source_images,
        "the source must not be rewritten by the import"
    );
}

/// Convert the legacy application folder the maintainer points at.
///
/// A real BongoCatMver installation bundles third-party model artwork and
/// the legacy application itself, so it cannot be committed to the
/// repository; the real-world case is covered by pointing
/// `BONGOCAT_MVER_SAMPLE` at one instead. The test is skipped when the
/// variable is unset, which keeps it out of the default `cargo test` line.
/// What it asserts is what makes a conversion usable: every mode the
/// inspector found installs as a real package with a resolved entry, moc and
/// texture, its overlays are decodable PNGs of one size, and the legacy
/// source is left untouched.
#[test]
fn converts_the_legacy_sample_named_by_the_environment() {
    use image::ImageReader;
    use std::collections::BTreeSet;

    let Some(source) = std::env::var_os("BONGOCAT_MVER_SAMPLE") else {
        return;
    };
    let source = PathBuf::from(source);
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let content = store.inspect_source(&source).expect("describe sample");
    let ModelSourceContent::Mver { modes } = &content else {
        panic!("sample {} is not a BongoCatMver source", source.display());
    };
    assert!(!modes.is_empty(), "sample has no convertible mode");

    for mode in modes {
        let id = store.allocate_unique_id().expect("allocate id");
        let installed = store
            .import_mver_with_observer(id, *mode, &source, |_| {}, || false)
            .unwrap_or_else(|error| panic!("{} failed: {error:?}", mode.as_str()));
        assert_eq!(installed.index().entry, "cat.model3.json");
        assert!(installed.root().join(&installed.index().moc).is_file());
        assert!(!installed.index().textures.is_empty());
        for texture in &installed.index().textures {
            assert!(installed.root().join(&texture.file).is_file());
        }

        let left_keys = installed.root().join("resources/left-keys");
        assert!(
            left_keys.is_dir(),
            "{} has no left key images",
            mode.as_str()
        );
        let mut sizes = BTreeSet::new();
        for entry in fs::read_dir(&left_keys).expect("left key images") {
            let path = entry.expect("left key image").path();
            let image = ImageReader::open(&path)
                .expect("open key image")
                .decode()
                .unwrap_or_else(|error| panic!("{} is not decodable: {error}", path.display()));
            sizes.insert((image.width(), image.height()));
        }
        assert_eq!(sizes.len(), 1, "every overlay shares one canvas size");
    }

    // The legacy folder is a reference source, never an install target.
    assert!(source.join("config.json").is_file());
    assert!(source.join("img").is_dir());
}
