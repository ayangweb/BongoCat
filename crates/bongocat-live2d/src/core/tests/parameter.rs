//! A parameter the model has is driven, and one it does not is refused.

use super::*;

#[test]
fn preset_product_parameters_resolve_and_drive_drawables() {
    for id in ["standard", "keyboard", "gamepad"] {
        let committed = preset_model(id);
        let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
        let expected_parameters: &[ProductParameter] = match id {
            "standard" => &[
                ProductParameter::AngleX,
                ProductParameter::AngleY,
                ProductParameter::AngleZ,
                ProductParameter::EyeBallX,
                ProductParameter::EyeBallY,
                ProductParameter::LeftHandDown,
                ProductParameter::MouseX,
                ProductParameter::MouseY,
                ProductParameter::MouseLeftDown,
                ProductParameter::MouseRightDown,
            ],
            "keyboard" => &[
                ProductParameter::AngleX,
                ProductParameter::AngleY,
                ProductParameter::AngleZ,
                ProductParameter::EyeBallX,
                ProductParameter::EyeBallY,
                ProductParameter::LeftHandDown,
                ProductParameter::RightHandDown,
            ],
            "gamepad" => &[
                ProductParameter::AngleX,
                ProductParameter::AngleY,
                ProductParameter::AngleZ,
                ProductParameter::EyeBallX,
                ProductParameter::EyeBallY,
                ProductParameter::LeftHandDown,
                ProductParameter::RightHandDown,
                ProductParameter::StickLeftDown,
                ProductParameter::StickRightDown,
                ProductParameter::StickShowLeftHand,
                ProductParameter::StickShowRightHand,
                ProductParameter::StickLeftX,
                ProductParameter::StickLeftY,
                ProductParameter::StickRightX,
                ProductParameter::StickRightY,
            ],
            _ => unreachable!("preset model list is fixed"),
        };
        for parameter in ProductParameter::ALL {
            assert_eq!(
                model.parameter_range(parameter).is_some(),
                expected_parameters.contains(&parameter),
                "{id} support mismatch for {}",
                parameter.id()
            );
        }
        let baseline = model.update_and_snapshot().expect("baseline snapshot");
        let range = model
            .parameter_range(ProductParameter::LeftHandDown)
            .expect("left hand parameter");
        let update = model
            .set_parameter(ProductParameter::LeftHandDown, f32::MAX)
            .expect("set parameter");
        assert_eq!(
            update,
            ParameterUpdate::Applied {
                value: range.maximum,
                clamped: true,
            }
        );
        assert_eq!(
            model
                .parameter_value(ProductParameter::LeftHandDown)
                .expect("read parameter"),
            Some(range.maximum)
        );
        let pressed = model.update_and_snapshot().expect("pressed snapshot");
        assert!(
            pressed
                .drawables
                .iter()
                .any(|drawable| drawable.dynamic_flags.vertex_positions_changed),
            "{id} left hand must mark changed drawable vertices"
        );
        let mut pressed_visual = pressed;
        let mut baseline_visual = baseline;
        clear_dynamic_flags(&mut pressed_visual);
        clear_dynamic_flags(&mut baseline_visual);
        assert_ne!(
            pressed_visual, baseline_visual,
            "{id} left hand must affect drawable values"
        );
    }
}

#[test]
fn additive_parameter_updates_preserve_the_reference_breath_semantics() {
    let committed = preset_model("standard");
    let moc_path = committed.root().join(&committed.index().moc);
    let mut core = CoreModel::load(&moc_path).expect("load Cubism model");
    core.set_parameter_by_id("ParamAngleX", -15.0, 1.0)
        .expect("base angle");
    let update = core
        .add_parameter_by_id("ParamAngleX", 10.0, 0.5)
        .expect("additive breath contribution");
    assert_eq!(
        update,
        ParameterUpdate::Applied {
            value: -10.0,
            clamped: false,
        }
    );

    let clamped = core
        .add_parameter_by_id("ParamAngleX", 100.0, 1.0)
        .expect("clamped additive contribution");
    assert_eq!(
        clamped,
        ParameterUpdate::Applied {
            value: 30.0,
            clamped: true,
        }
    );
}

#[test]
fn parameter_updates_reject_non_finite_and_report_unsupported_ids() {
    let committed = preset_model("keyboard");
    let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
    let error = model
        .set_parameter(ProductParameter::LeftHandDown, f32::NAN)
        .expect_err("NaN must fail");
    assert_eq!(error.code, Live2dErrorCode::ParameterValueInvalid);
    assert_eq!(
        model
            .set_parameter(ProductParameter::MouseLeftDown, 1.0)
            .expect("unsupported is not corrupt"),
        ParameterUpdate::Unsupported
    );
    assert_eq!(model.parameter_range(ProductParameter::MouseLeftDown), None);
}
