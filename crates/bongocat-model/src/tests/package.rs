//! Reading a package, and refusing the ones that are not safe.

use super::*;

#[test]
fn shared_custom_model_fixtures_match_product_parser_contract() {
    let fixture_root = repository_root().join("shared/fixtures/model-fixtures");
    let manifest: FixtureManifest = serde_json::from_slice(
        &fs::read(fixture_root.join("cases.json")).expect("read fixture manifest"),
    )
    .expect("strict fixture manifest");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.limits.maximum_texture_dimension,
        ModelPackageLimits::default().maximum_texture_dimension
    );
    assert!(!manifest.cases.is_empty());

    let mut case_ids = BTreeSet::new();
    let mut registered_directories = BTreeSet::new();
    for (index, case) in manifest.cases.iter().enumerate() {
        assert!(case_ids.insert(case.id.as_str()), "duplicate fixture id");
        assert!(
            registered_directories.insert(case.directory.as_str()),
            "duplicate fixture directory"
        );
        assert_eq!(
            Path::new(&case.directory).components().collect::<Vec<_>>(),
            [Component::Normal(case.directory.as_ref())],
            "fixture directory must be one normal component"
        );
        let unique_diagnostics = case
            .expected_diagnostics
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_diagnostics.len(), case.expected_diagnostics.len());
        assert_eq!(
            case.expected == FixtureExpectation::Accept,
            case.expected_diagnostics.is_empty(),
            "fixture accept/reject contract must match diagnostics"
        );

        let package = materialize_fixture_case(case);
        let source_before = snapshot_fixture_tree(package.path());
        let id = ModelId::parse(format!("fixture-{index}")).expect("fixture model id");
        let prepared =
            PreparedModel::prepare(id.clone(), package.path(), ModelPackageLimits::default());
        match (&case.expected, &prepared) {
            (FixtureExpectation::Accept, Ok(model)) => {
                assert_eq!(model.id(), &id, "fixture {} model id", case.id);
            }
            (FixtureExpectation::Reject, Err(error)) => {
                assert_eq!(
                    case.expected_diagnostics,
                    [error.code.as_str()],
                    "fixture {} diagnostic",
                    case.id
                );
                assert!(
                    stage_accepts_diagnostic(case.stage, error.code),
                    "fixture {} diagnostic must belong to declared stage",
                    case.id
                );
            }
            (FixtureExpectation::Accept, Err(error)) => {
                panic!("fixture {} unexpectedly rejected: {error}", case.id)
            }
            (FixtureExpectation::Reject, Ok(_)) => {
                panic!("fixture {} unexpectedly accepted", case.id)
            }
        }

        // Filesystem import and catalog ownership are covered by the
        // bongocat-model-store crate; this fixture test remains focused on
        // the platform-neutral package parser and its diagnostics.
        assert_eq!(
            snapshot_fixture_tree(package.path()),
            source_before,
            "fixture {} source package changed",
            case.id
        );
    }

    let actual_directories = fs::read_dir(fixture_root.join("cases"))
        .expect("list fixture directories")
        .map(|entry| {
            entry
                .expect("read fixture directory")
                .file_name()
                .into_string()
                .expect("UTF-8 fixture directory")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_directories,
        registered_directories
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        "every custom model fixture directory must be registered"
    );
}

#[test]
fn prepares_all_three_preset_packages() {
    for mode in ["standard", "keyboard", "gamepad"] {
        let root = repository_root().join("resources/models").join(mode);
        let prepared = PreparedModel::prepare(
            ModelId::parse(mode).expect("model id"),
            &root,
            ModelPackageLimits::default(),
        )
        .expect("prepare preset package");
        assert_eq!(prepared.index().model_version, 3);
        assert_eq!(prepared.index().schema_version, INDEX_SCHEMA_VERSION);
        assert!(!prepared.index().moc.is_empty());
        assert!(!prepared.index().textures.is_empty());
        let eye_blink = prepared
            .index()
            .groups
            .iter()
            .find(|group| group.target == "Parameter" && group.name == "EyeBlink")
            .expect("EyeBlink parameter group");
        assert_eq!(
            eye_blink.ids,
            ["ParamEyeLOpen".to_owned(), "ParamEyeROpen".to_owned()]
        );
        let breath = prepared
            .index()
            .groups
            .iter()
            .find(|group| group.target == "Parameter" && group.name == "Breath")
            .expect("Breath parameter group");
        assert_eq!(breath.ids, ["ParamBreath".to_owned()]);
        assert_eq!(
            prepared.root(),
            root.canonicalize().expect("canonical root")
        );
    }
}

#[test]
fn preset_sidecars_pass_product_contract_validation() {
    for mode in ["standard", "keyboard", "gamepad"] {
        let model = PreparedModel::prepare(
            ModelId::parse(mode).expect("model id"),
            repository_root().join("resources/models").join(mode),
            ModelPackageLimits::default(),
        )
        .expect("preset sidecars must be valid");
        assert!(model.index().display_info.is_some());
        assert!(!model.index().expressions.is_empty());
        assert!(
            model
                .index()
                .motion_groups
                .iter()
                .all(|group| !group.motions.is_empty())
        );
    }
}

#[test]
fn package_byte_and_file_limits_fail_before_unbounded_loading() {
    let package = tempdir().expect("package");
    let model = r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#;
    fs::write(package.path().join("cat.model3.json"), model).expect("model3");
    fs::write(package.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(package.path().join("unreferenced.bin"), b"extra").expect("extra file");

    let json_limited = PreparedModel::prepare(
        ModelId::parse("limited").expect("model id"),
        package.path(),
        ModelPackageLimits {
            maximum_json_bytes: 16,
            ..ModelPackageLimits::default()
        },
    )
    .expect_err("JSON byte limit");
    assert_eq!(json_limited.code, ModelDiagnostic::ModelJsonTooLarge);

    let file_limited = PreparedModel::prepare(
        ModelId::parse("limited").expect("model id"),
        package.path(),
        ModelPackageLimits {
            maximum_file_count: 2,
            ..ModelPackageLimits::default()
        },
    )
    .expect_err("file count limit");
    assert_eq!(file_limited.code, ModelDiagnostic::ModelFileCountExceeded);

    let package_limited = PreparedModel::prepare(
        ModelId::parse("limited").expect("model id"),
        package.path(),
        ModelPackageLimits {
            maximum_package_bytes: 1,
            ..ModelPackageLimits::default()
        },
    )
    .expect_err("package byte limit");
    assert_eq!(
        package_limited.code,
        ModelDiagnostic::ModelPackageSizeExceeded
    );
}

#[cfg(unix)]
#[test]
fn referenced_symlink_cannot_escape_package_root() {
    use std::os::unix::fs::symlink;

    let package = tempdir().expect("package");
    let outside = tempdir().expect("outside");
    fs::write(outside.path().join("model.moc3"), b"moc").expect("outside moc");
    symlink(
        outside.path().join("model.moc3"),
        package.path().join("model.moc3"),
    )
    .expect("symlink");
    fs::write(
        package.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    )
    .expect("model3");

    let error = PreparedModel::prepare(
        ModelId::parse("escape").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect_err("escaping symlink");
    assert_eq!(error.code, ModelDiagnostic::ModelReferenceSymlinkEscape);
}
