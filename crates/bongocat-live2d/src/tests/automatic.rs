//! The reference breath, and the targets it drives.

use super::*;

#[test]
fn automatic_effects_match_reference_breath_and_eye_blink() {
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
    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    assert_eq!(model.breath_parameter_ids, ["ParamBreath"]);

    model
        .set_parameter(ProductParameter::AngleX, -15.0)
        .expect("product angle input");
    model
        .apply_automatic_effects(std::time::Duration::ZERO, 0.0)
        .expect("additive reference breath");
    assert_eq!(
        model
            .core
            .parameter_value_by_id("ParamAngleX")
            .expect("angle value")
            .expect("supported angle parameter"),
        -15.0,
        "reference breath must add to, not blend away, mouse input"
    );

    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    let breath_range = model
        .core
        .parameter_range_by_id("ParamBreath")
        .expect("breath range");
    let applied = model
        .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
        .expect("automatic effects");
    assert_eq!(applied, 7);
    assert_eq!(
        model
            .core
            .parameter_value_by_id("ParamEyeLOpen")
            .expect("left eye"),
        Some(0.0)
    );
    assert_eq!(
        model
            .core
            .parameter_value_by_id("ParamEyeROpen")
            .expect("right eye"),
        Some(0.0)
    );
    let breath = model
        .core
        .parameter_value_by_id("ParamBreath")
        .expect("breath")
        .expect("supported breath parameter");
    assert!(
        breath > breath_range.default && breath < breath_range.maximum,
        "reference breath must stay within its authored range: {breath}"
    );
}

#[test]
fn reference_breath_does_not_require_a_model_group() {
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
    let breath_default = model
        .core
        .parameter_value_by_id("ParamBreath")
        .expect("breath value")
        .expect("supported breath parameter");
    let breath_range = model
        .core
        .parameter_range_by_id("ParamBreath")
        .expect("breath range");
    model.breath_parameter_ids.clear();

    model
        .restore_parameter_defaults()
        .expect("restore parameter defaults");
    assert_eq!(
        model
            .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
            .expect("automatic effects without Breath"),
        7,
        "the fixed reference targets remain active without a model3 Breath group"
    );
    let breath = model
        .core
        .parameter_value_by_id("ParamBreath")
        .expect("breath value")
        .expect("supported breath parameter");
    assert_ne!(
        breath, breath_default,
        "the conventional reference target should not be ignored"
    );

    for step in 0..=40 {
        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        model
            .apply_automatic_effects(std::time::Duration::from_millis(step * 100), 1.0)
            .expect("reference automatic effects");
        let breath = model
            .core
            .parameter_value_by_id("ParamBreath")
            .expect("breath value")
            .expect("supported breath parameter");
        assert!(breath.is_finite(), "reference step {step}: {breath}");
        let lower = breath_range.default
            + (breath_range.minimum - breath_range.default) * AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT;
        let upper = breath_range.default
            + (breath_range.maximum - breath_range.default) * AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT;
        assert!(
            (lower..=upper).contains(&breath),
            "reference step {step}: {breath} escaped [{lower}, {upper}]"
        );
    }
}

#[test]
fn all_preset_models_expose_the_automatic_effect_parameter_contract() {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let catalog = PresetModelCatalog::open(
        repository_root.join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog");
    for id in ["standard", "keyboard", "gamepad"] {
        let committed = catalog
            .load(&ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        assert_eq!(
            model.eye_blink_parameter_ids,
            ["ParamEyeLOpen", "ParamEyeROpen"],
            "{id} EyeBlink group"
        );
        assert_eq!(
            model.breath_parameter_ids,
            ["ParamBreath"],
            "{id} Breath group"
        );
        let breath_range = model
            .core
            .parameter_range_by_id("ParamBreath")
            .unwrap_or_else(|| panic!("{id} breath parameter"));
        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        assert_eq!(
            model
                .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
                .expect("automatic effects"),
            7,
            "{id} automatic effect count"
        );
        let breath = model
            .core
            .parameter_value_by_id("ParamBreath")
            .expect("breath value")
            .expect("supported breath parameter");
        assert!(
            breath > breath_range.default && breath < breath_range.maximum,
            "{id} reference breath stayed within its authored range: {breath}"
        );
    }
}
