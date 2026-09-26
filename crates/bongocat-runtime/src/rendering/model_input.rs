//! Turning the product's input into the model's parameters.
//!
//! The input arrives as product-level facts — this key is down, the stick is
//! here — and the model's parameters are named by the bindings. A binding that
//! names a parameter the model does not have is dropped rather than written to
//! whatever happens to be at that index.

use super::*;

pub(crate) fn apply_model_input(
    model: &mut Live2dModel,
    input: ModelInputSnapshot,
    settings: ModelSettings,
) -> Result<(), RuntimeRenderErrorCode> {
    let (pointer_x, pointer_y, pointer_z) = if settings.ignore_pointer {
        (0.0, 0.0, 0.0)
    } else {
        let horizontal_sign = if settings.mirror_pointer_tracking {
            -1.0
        } else {
            1.0
        };
        (
            input.pointer_x * horizontal_sign,
            input.pointer_y,
            input.pointer_z * horizontal_sign,
        )
    };
    for (parameter, value) in [
        (ProductParameter::MouseX, pointer_x),
        (ProductParameter::MouseY, pointer_y),
        (ProductParameter::AngleX, pointer_x),
        (ProductParameter::AngleY, pointer_y),
        (ProductParameter::AngleZ, pointer_z),
        (ProductParameter::EyeBallX, pointer_x),
        (ProductParameter::EyeBallY, pointer_y),
        (
            ProductParameter::LeftHandDown,
            f32::from(input.left_hand_down),
        ),
        (
            ProductParameter::RightHandDown,
            f32::from(input.right_hand_down),
        ),
        (
            ProductParameter::MouseLeftDown,
            f32::from(input.mouse_left_down),
        ),
        (
            ProductParameter::MouseRightDown,
            f32::from(input.mouse_right_down),
        ),
        (
            ProductParameter::StickLeftDown,
            f32::from(input.stick_left_down),
        ),
        (
            ProductParameter::StickRightDown,
            f32::from(input.stick_right_down),
        ),
        (ProductParameter::StickLeftX, input.stick_left_x),
        (ProductParameter::StickLeftY, input.stick_left_y),
        (ProductParameter::StickRightX, input.stick_right_x),
        (ProductParameter::StickRightY, input.stick_right_y),
    ] {
        match model.set_normalized_parameter(parameter, value) {
            Ok(ParameterUpdate::Applied { .. } | ParameterUpdate::Unsupported) => {}
            Err(error) => {
                return Err(map_live2d_error(
                    error,
                    RuntimeRenderErrorCode::ModelEvaluationFailed,
                ));
            }
        }
    }
    Ok(())
}
