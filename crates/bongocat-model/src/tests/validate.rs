//! The per-resource contract, and what each malformed shape costs.

use super::*;

#[test]
fn fixture_contract_rejects_missing_ambiguous_and_escaping_packages() {
    for (name, expected) in [
        ("missing-moc", ModelDiagnostic::ModelMocMissing),
        ("multiple-model3", ModelDiagnostic::ModelEntryAmbiguous),
        ("path-traversal", ModelDiagnostic::ModelReferenceEscapesRoot),
    ] {
        let error = PreparedModel::prepare(
            ModelId::parse("fixture").expect("model id"),
            fixture(name),
            ModelPackageLimits::default(),
        )
        .expect_err("fixture must be rejected");
        assert_eq!(error.code, expected, "fixture {name}");
    }
}

#[test]
fn malformed_model3_is_rejected_before_resource_resolution() {
    let source = fixture("malformed-model3-json");
    let package = tempdir().expect("temporary package");
    fs::copy(
        source.join("cat.model3.json.invalid"),
        package.path().join("cat.model3.json"),
    )
    .expect("materialize malformed entry");

    let error = PreparedModel::prepare(
        ModelId::parse("malformed").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect_err("malformed model3 must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
}

#[test]
fn accepts_non_ascii_resource_paths() {
    let prepared = PreparedModel::prepare(
        ModelId::parse("unicode").expect("model id"),
        fixture("非 ASCII 模型"),
        ModelPackageLimits::default(),
    )
    .expect("non-ASCII package");
    assert_eq!(prepared.index().moc, "模型 数据.moc3");
}

#[test]
fn model_groups_are_retained_and_blank_identifiers_are_rejected() {
    let package = tempdir().expect("package");
    fs::write(package.path().join("model.moc3"), b"moc").expect("moc");
    fs::write(
        package.path().join("cat.model3.json"),
        r#"{
          "Version":3,
          "FileReferences":{"Moc":"model.moc3","Textures":[]},
          "Groups":[
            {"Target":"Parameter","Name":"LipSync","Ids":["ParamMouthOpenY"]},
            {"Target":"FutureTarget","Name":"Metadata","Ids":[]}
          ]
        }"#,
    )
    .expect("model3");
    let prepared = PreparedModel::prepare(
        ModelId::parse("groups").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect("grouped model");
    assert_eq!(
        prepared.index().groups,
        [
            ModelGroup {
                target: "Parameter".to_owned(),
                name: "LipSync".to_owned(),
                ids: vec!["ParamMouthOpenY".to_owned()],
            },
            ModelGroup {
                target: "FutureTarget".to_owned(),
                name: "Metadata".to_owned(),
                ids: vec![],
            },
        ]
    );

    fs::write(
        package.path().join("cat.model3.json"),
        r#"{
          "Version":3,
          "FileReferences":{"Moc":"model.moc3","Textures":[]},
          "Groups":[{"Target":"Parameter","Name":"LipSync","Ids":[" "]}]
        }"#,
    )
    .expect("invalid model3");
    let error = PreparedModel::prepare(
        ModelId::parse("groups").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect_err("blank group parameter id");
    assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
    assert!(error.detail.contains("group parameter id"));
}

#[test]
fn sidecar_contracts_reject_invalid_display_expression_and_motion_resources() {
    let package = tempdir().expect("package");
    let limits = ModelPackageLimits::default();
    let display = package.path().join("display.cdi3.json");
    fs::write(
        &display,
        r#"{"Version":3,"Parameters":[{"Id":"ParamA","GroupId":"missing","Name":"A"}]}"#,
    )
    .expect("display resource");
    let error = validate_display_info_resource(
        &display,
        "display.cdi3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect_err("undeclared display group must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

    let expression = package.path().join("expression.exp3.json");
    fs::write(
        &expression,
        r#"{"Type":"Live2D Expression","Parameters":[{"Id":"ParamA","Value":1},{"Id":"ParamA","Value":2}]}"#,
    )
    .expect("expression resource");
    let error = validate_expression_resource(
        &expression,
        "expression.exp3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect_err("duplicate expression parameter must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

    let motion = package.path().join("motion.motion3.json");
    fs::write(
        &motion,
        r#"{
          "Version":3,
          "Meta":{"Duration":1,"Fps":30,"Loop":false,"AreBeziersRestricted":true,"CurveCount":1,"TotalSegmentCount":0,"TotalPointCount":0,"UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[]
        }"#,
    )
    .expect("motion resource");
    let error = validate_motion_resource(
        &motion,
        "motion.motion3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect_err("mismatched motion count must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

    fs::write(
        &motion,
        r#"{
          "Version":3,
          "Meta":{"Duration":1,"Fps":30,"Loop":false,"AreBeziersRestricted":true,"CurveCount":0,"TotalSegmentCount":0,"TotalPointCount":0,"UserDataCount":1,"TotalUserDataSize":1},
          "Curves":[],
          "UserData":[{"Time":2,"Value":"x"}]
        }"#,
    )
    .expect("invalid user data resource");
    let error = validate_motion_resource(
        &motion,
        "motion.motion3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect_err("out-of-range motion user data must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
}

#[test]
fn pose_contract_rejects_invalid_groups_parts_and_links() {
    let package = tempdir().expect("package");
    let limits = ModelPackageLimits::default();
    let pose = package.path().join("model.pose3.json");
    fs::write(
        &pose,
        r#"{
          "Type":"Live2D Pose",
          "FadeInTime":0.5,
          "Groups":[[
            {"Id":"PartArmA","Link":["PartArmB"]},
            {"Id":"PartArmB"}
          ]]
        }"#,
    )
    .expect("valid pose resource");
    validate_pose_resource(
        &pose,
        "model.pose3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("valid pose resource must be accepted");

    fs::write(
        &pose,
        r#"{
          "Type":"Live2D Pose",
          "Groups":[[
            {"Id":"PartArmA","Link":["PartArmA"]}
          ]]
        }"#,
    )
    .expect("invalid pose resource");
    let error = validate_pose_resource(
        &pose,
        "model.pose3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect_err("self-referential pose link must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
    assert!(error.detail.contains("not self-referential"));
}

#[test]
fn physics_contract_validates_counts_weights_and_vertex_references() {
    let package = tempdir().expect("package");
    let limits = ModelPackageLimits::default();
    let physics = package.path().join("model.physics3.json");
    const VALID_PHYSICS: &str = r#"{
      "Version":3,
      "Meta":{
        "PhysicsSettingCount":1,"TotalInputCount":1,"TotalOutputCount":1,"VertexCount":2,"Fps":60,
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
    }"#;
    fs::write(&physics, VALID_PHYSICS).expect("valid physics resource");
    validate_physics_resource(
        &physics,
        "model.physics3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("valid physics resource must be accepted");
    let definition = load_physics_definition(
        &physics,
        "model.physics3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("typed physics definition");
    assert_eq!(definition.fps, 60.0);
    assert_eq!(definition.settings.len(), 1);
    assert_eq!(definition.settings[0].inputs[0].parameter_id, "ParamInput");
    assert_eq!(
        definition.settings[0].outputs[0].parameter_id,
        "ParamOutput"
    );

    // Older exported models can omit Meta.Fps. The runtime already has a
    // frame-delta path for that representation, so preserve it instead of
    // rejecting an otherwise valid legacy package at import time.
    fs::write(&physics, VALID_PHYSICS.replace("\"Fps\":60,", ""))
        .expect("legacy physics resource without Fps");
    validate_physics_resource(
        &physics,
        "model.physics3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("legacy physics resource without Fps must be accepted");
    let legacy_definition = load_physics_definition(
        &physics,
        "model.physics3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("legacy typed physics definition");
    assert_eq!(legacy_definition.fps, 0.0);

    fs::write(package.path().join("model.moc3"), b"moc").expect("moc resource");
    fs::write(
        package.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[],"Physics":"model.physics3.json"}}"#,
    )
    .expect("model3 resource");
    let prepared = PreparedModel::prepare(
        ModelId::parse("physics-without-fps").expect("model id"),
        package.path(),
        limits,
    )
    .expect("model package without physics Meta.Fps must be accepted");
    assert_eq!(
        prepared.index().physics.as_deref(),
        Some("model.physics3.json")
    );
    assert_eq!(
        prepared
            .physics_definition()
            .expect("prepared physics definition")
            .expect("declared physics definition")
            .fps,
        0.0
    );

    for (invalid, detail) in [
        (
            VALID_PHYSICS.replace("\"TotalOutputCount\":1", "\"TotalOutputCount\":2"),
            "Meta counts",
        ),
        (
            VALID_PHYSICS.replace(
                "\"Weight\":100,\"Type\":\"X\"",
                "\"Weight\":101,\"Type\":\"X\"",
            ),
            "input Weight",
        ),
        (
            VALID_PHYSICS.replace("\"VertexIndex\":1", "\"VertexIndex\":2"),
            "VertexIndex",
        ),
        (
            VALID_PHYSICS.replace("\"Fps\":60,", "\"Fps\":0,"),
            "Meta.Fps",
        ),
        (
            VALID_PHYSICS.replace("\"Fps\":60,", "\"Fps\":null,"),
            "Meta.Fps",
        ),
    ] {
        fs::write(&physics, invalid).expect("invalid physics resource");
        let error = validate_physics_resource(
            &physics,
            "model.physics3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("invalid physics resource must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
        assert!(error.detail.contains(detail), "{}", error.detail);
    }

    fs::write(
        &physics,
        VALID_PHYSICS.replace("\"VertexIndex\":1", "\"VertexIndex\":2"),
    )
    .expect("invalid model physics resource");
    let error = PreparedModel::prepare(
        ModelId::parse("physics").expect("model id"),
        package.path(),
        limits,
    )
    .expect_err("model prepare must validate declared physics");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
    assert!(error.detail.contains("VertexIndex"));
}

#[test]
fn model_user_data_contract_validates_metadata_and_unique_targets() {
    let package = tempdir().expect("package");
    let limits = ModelPackageLimits::default();
    let user_data = package.path().join("model.userdata3.json");
    const VALID_USER_DATA: &str = r#"{
      "Version":3,
      "Meta":{"UserDataCount":1,"TotalUserDataSize":3},
      "UserData":[{"Target":"ArtMesh","Id":"Drawable1","Value":"tag"}]
    }"#;
    fs::write(&user_data, VALID_USER_DATA).expect("valid user data resource");
    validate_model_user_data_resource(
        &user_data,
        "model.userdata3.json",
        limits.maximum_json_bytes,
        limits.maximum_json_depth,
    )
    .expect("valid user data resource must be accepted");

    for (invalid, detail) in [
        (
            VALID_USER_DATA.replace("\"UserDataCount\":1", "\"UserDataCount\":2"),
            "UserDataCount",
        ),
        (
            VALID_USER_DATA.replace("\"TotalUserDataSize\":3", "\"TotalUserDataSize\":4"),
            "byte size",
        ),
    ] {
        fs::write(&user_data, invalid).expect("invalid user data resource");
        let error = validate_model_user_data_resource(
            &user_data,
            "model.userdata3.json",
            limits.maximum_json_bytes,
            limits.maximum_json_depth,
        )
        .expect_err("invalid user data resource must be rejected");
        assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
        assert!(error.detail.contains(detail), "{}", error.detail);
    }

    fs::write(package.path().join("model.moc3"), b"moc").expect("moc resource");
    fs::write(
        package.path().join("cat.model3.json"),
        r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[],"UserData":"model.userdata3.json"}}"#,
    )
    .expect("model3 resource");
    fs::write(
        &user_data,
        r#"{
          "Version":3,
          "Meta":{"UserDataCount":2,"TotalUserDataSize":6},
          "UserData":[
            {"Target":"ArtMesh","Id":"Drawable1","Value":"tag"},
            {"Target":"ArtMesh","Id":"Drawable1","Value":"tag"}
          ]
        }"#,
    )
    .expect("duplicate model user data resource");
    let error = PreparedModel::prepare(
        ModelId::parse("model-user-data").expect("model id"),
        package.path(),
        limits,
    )
    .expect_err("model prepare must validate declared user data");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
    assert!(error.detail.contains("unique non-empty"));
}

#[test]
fn rejects_declared_texture_dimensions_before_decode() {
    let source = fixture("oversized-texture");
    let package = tempdir().expect("temporary package");
    fs::create_dir(package.path().join("textures")).expect("texture directory");
    for file in ["cat.model3.json", "placeholder.moc3"] {
        fs::copy(source.join(file), package.path().join(file)).expect("copy fixture file");
    }
    let hex =
        fs::read_to_string(source.join("textures/huge.png.hex")).expect("read encoded texture");
    let bytes = hex
        .trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                .expect("hex byte")
        })
        .collect::<Vec<_>>();
    fs::write(package.path().join("textures/huge.png"), bytes).expect("write texture");

    let error = PreparedModel::prepare(
        ModelId::parse("oversized").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect_err("oversized texture must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelTextureDimensionExceeded);
}
