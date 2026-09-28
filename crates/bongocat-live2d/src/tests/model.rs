//! Loading a model and driving it with every kind of clip.

use super::*;

#[test]
fn preset_motion_and_expression_resources_load_through_live2d_adapter() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(root, ModelPackageLimits::default())
        .expect("preset model catalog");
    for id in ["standard", "keyboard", "gamepad"] {
        let committed = catalog
            .load(&ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let model = Live2dModel::load(&committed).expect("Live2D model");
        assert!(model.motion_clip("CAT_motion", 0).is_some());
        assert!(
            model
                .expression_clip("live2d_expression0.exp3.json")
                .is_some()
        );
    }
}

#[test]
fn expression_layers_apply_add_multiply_and_overwrite_to_core_parameters() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let committed = PresetModelCatalog::open(
        repository_root.join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse("standard").expect("model id"))
    .expect("preset model");
    let mut model = Live2dModel::load(&committed).expect("Live2D model");

    let clip = ExpressionClip::from_slice(
        br#"{
          "Type":"Live2D Expression",
          "FadeInTime":0,
          "Parameters":[
            {"Id":"ParamAngleX","Value":10,"Blend":"Add"},
            {"Id":"ParamEyeLOpen","Value":0.5,"Blend":"Multiply"},
            {"Id":"ParamAngleY","Value":-15,"Blend":"Overwrite"}
          ]
        }"#,
    )
    .expect("expression clip");
    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    let eye_default = model
        .core
        .parameter_value_by_id("ParamEyeLOpen")
        .expect("eye parameter")
        .expect("supported eye parameter");
    let applied = model
        .apply_expression_layers(&[ExpressionLayer {
            clip: &clip,
            weight: 1.0,
        }])
        .expect("apply expression");
    assert_eq!(applied.applied_parameter_count, 3);
    assert_eq!(
        model
            .core
            .parameter_value_by_id("ParamAngleX")
            .expect("angle x"),
        Some(10.0)
    );
    assert_eq!(
        model
            .core
            .parameter_value_by_id("ParamAngleY")
            .expect("angle y"),
        Some(-15.0)
    );
    let eye = model
        .core
        .parameter_value_by_id("ParamEyeLOpen")
        .expect("eye parameter")
        .expect("supported eye parameter");
    assert!((eye - eye_default * 0.5).abs() < 0.0001);
}

#[test]
fn part_opacity_motion_curves_use_the_core_part_sink() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let committed = PresetModelCatalog::open(
        repository_root.join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse("standard").expect("model id"))
    .expect("preset model");
    let mut model = Live2dModel::load(&committed).expect("Live2D model");
    let clip = MotionClip::from_slice(
        br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
            "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[
            {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]},
            {"Target":"PartOpacity","Id":"MissingPartSink","Segments":[0,0,0,1,1]}
          ]
        }"#,
        0.0,
        1.0,
    )
    .expect("part opacity motion");

    let parameter_before = model
        .parameter_value(ProductParameter::AngleX)
        .expect("angle parameter");
    let part_opacity_before = model
        .part_opacity_by_id("Part")
        .expect("initial part opacity");
    let status = model
        .apply_motion_with_weight(&clip, std::time::Duration::from_millis(500), 1.0)
        .expect("apply part opacity motion");
    assert_eq!(status.applied_parameter_count, 0);
    assert_eq!(status.applied_part_opacity_count, 1);
    assert_eq!(
        model.part_opacity_by_id("Part").expect("part opacity"),
        Some(0.125)
    );
    assert_eq!(
        model
            .parameter_value(ProductParameter::AngleX)
            .expect("angle parameter"),
        parameter_before
    );

    model
        .restore_part_opacity_defaults()
        .expect("restore part opacity defaults");
    assert_eq!(
        model.part_opacity_by_id("Part").expect("part opacity"),
        part_opacity_before
    );
}

#[test]
fn model_motion_curves_apply_eye_blink_lip_sync_and_render_opacity() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let committed = PresetModelCatalog::open(
        repository_root.join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse("standard").expect("model id"))
    .expect("preset model");
    let mut model = Live2dModel::load(&committed).expect("Live2D model");
    assert_eq!(
        model.eye_blink_parameter_ids,
        ["ParamEyeLOpen", "ParamEyeROpen"]
    );
    assert!(model.lip_sync_parameter_ids.is_empty());
    model
        .lip_sync_parameter_ids
        .push("ParamMouthOpenY".to_owned());
    let clip = MotionClip::from_slice(
        br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
            "CurveCount":5,"TotalSegmentCount":5,"TotalPointCount":10,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[
            {"Target":"Model","Id":"EyeBlink","Segments":[0,0.5,0,1,0.5]},
            {"Target":"Model","Id":"LipSync","Segments":[0,0.2,0,1,0.2]},
            {"Target":"Model","Id":"Opacity","Segments":[0,0.4,0,1,0.4]},
            {"Target":"Parameter","Id":"ParamEyeLOpen","Segments":[0,0.8,0,1,0.8]},
            {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0.3,0,1,0.3]}
          ]
        }"#,
        0.0,
        0.0,
    )
    .expect("model effect motion");

    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    let status = model
        .apply_motion(&clip, std::time::Duration::from_millis(500))
        .expect("apply model curves");
    assert_eq!(status.applied_parameter_count, 2);
    assert_eq!(status.applied_eye_blink_count, 2);
    assert_eq!(status.applied_lip_sync_count, 1);
    assert!(status.model_opacity_applied);
    for (id, expected) in [
        ("ParamEyeLOpen", 0.4),
        ("ParamEyeROpen", 0.5),
        ("ParamMouthOpenY", 0.5),
    ] {
        let actual = model
            .core
            .parameter_value_by_id(id)
            .expect("parameter value")
            .expect("supported parameter");
        assert!((actual - expected).abs() < 0.0001, "{id}: {actual}");
    }
    let snapshot = model.update_and_snapshot().expect("render snapshot");
    assert!((snapshot.model_opacity - 0.4).abs() < 0.0001);

    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    let snapshot = model.update_and_snapshot().expect("next render snapshot");
    assert!((snapshot.model_opacity - 0.4).abs() < 0.0001);
}

#[test]
fn declared_physics_drives_a_parameter_without_a_model_motion() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("destination directory");
        for entry in fs::read_dir(source).expect("source directory") {
            let entry = entry.expect("source entry");
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).expect("copied model file");
            }
        }
    }

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let package = tempfile::tempdir().expect("temporary package");
    let source = repository_root.join("resources/models/standard");
    let catalog_root = package.path().join("catalog");
    let model_root = catalog_root.join("physics");
    copy_tree(&source, &model_root);
    let model_path = model_root.join("cat.model3.json");
    let mut model_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&model_path).expect("model3")).expect("model3 JSON");
    model_json["FileReferences"]["Physics"] = json!("cat.physics3.json");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model_json).expect("model3 JSON serialization"),
    )
    .expect("updated model3");
    fs::write(
        model_root.join("cat.physics3.json"),
        r#"{
          "Version":3,
          "Meta":{
            "PhysicsSettingCount":1,"TotalInputCount":1,"TotalOutputCount":1,"VertexCount":2,"Fps":60,
            "EffectiveForces":{"Gravity":{"X":0,"Y":-1},"Wind":{"X":0,"Y":0}},
            "PhysicsDictionary":[{"Id":"PhysicsSetting1","Name":"test"}]
          },
          "PhysicsSettings":[{
            "Id":"PhysicsSetting1",
            "Input":[{"Source":{"Target":"Parameter","Id":"ParamAngleX"},"Weight":100,"Type":"X","Reflect":false}],
            "Output":[{"Destination":{"Target":"Parameter","Id":"ParamAngleY"},"VertexIndex":1,"Scale":1,"Weight":100,"Type":"X","Reflect":false}],
            "Vertices":[
              {"Position":{"X":0,"Y":0},"Mobility":1,"Delay":0,"Acceleration":0,"Radius":0},
              {"Position":{"X":0,"Y":10},"Mobility":1,"Delay":0,"Acceleration":0,"Radius":10}
            ],
            "Normalization":{"Position":{"Minimum":-10,"Default":0,"Maximum":10},"Angle":{"Minimum":-10,"Default":0,"Maximum":10}}
          }]
        }"#,
    )
    .expect("physics fixture");

    let committed = PresetModelCatalog::open(&catalog_root, ModelPackageLimits::default())
        .expect("catalog")
        .load(&ModelId::parse("physics").expect("model id"))
        .expect("committed model");
    let mut model = Live2dModel::load(&committed).expect("Live2D model");
    model
        .set_parameter(ProductParameter::AngleX, 30.0)
        .expect("input");
    model
        .apply_physics(std::time::Duration::from_millis(100))
        .expect("physics evaluation");
    let output = model
        .parameter_value_by_id("ParamAngleY")
        .expect("physics value")
        .expect("physics parameter");
    assert!(output.abs() > 0.1, "physics output: {output}");
}
