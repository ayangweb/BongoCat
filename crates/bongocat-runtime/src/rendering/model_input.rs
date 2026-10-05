//! Turning the product's input into the model's parameters.
//!
//! The input arrives as product-level facts — this key is down, the stick is
//! here — and the model's parameters are named by the bindings. A binding that
//! names a parameter the model does not have is dropped rather than written to
//! whatever happens to be at that index.

use super::*;

/// Whether a stick is in use: displaced from center, or its button held.
///
/// The dead zone has already been applied to the axes by the time the snapshot
/// is built, so a stick resting at center reads zero however noisy the hardware
/// is, and a stick held slightly off center keeps its artwork for as long as the
/// user keeps it there. This is the same condition the pre-rewrite input layer
/// used (`moved || pressed`), computed from the values the snapshot already
/// carries rather than from a second copy of them that could disagree.
const fn stick_active(pressed: bool, x: f32, y: f32) -> bool {
    pressed || x != 0.0 || y != 0.0
}

pub(crate) fn apply_model_input(
    model: &mut Live2dModel,
    input: ModelInputSnapshot,
    settings: ModelSettings,
) -> Result<(), RuntimeRenderErrorCode> {
    let (pointer_x, pointer_y, pointer_z) = if settings.ignore_pointer {
        (0.0, 0.0, 0.0)
    } else {
        // The two axes are corrected independently. X and Z share one sign
        // because they are the same horizontal sweep seen from two directions;
        // Y is its own axis, so a model that is wrong only up and down is fixed
        // without also reversing the way it turns to follow the cursor.
        let horizontal_sign = if settings.mirror_pointer_tracking_horizontal {
            -1.0
        } else {
            1.0
        };
        let vertical_sign = if settings.mirror_pointer_tracking_vertical {
            -1.0
        } else {
            1.0
        };
        (
            input.pointer_x * horizontal_sign,
            input.pointer_y * vertical_sign,
            input.pointer_z * horizontal_sign,
        )
    };
    // `CatParamStickShowLeftHand` / `CatParamStickShowRightHand` are what the
    // shipped `gamepad` preset and a converted BongoCatMver gamepad model read
    // as "this stick is in use", and they were never written, so the two analog
    // sticks a gamepad-mode model draws never appeared. The pre-rewrite input
    // layer also pressed that side's paw for the same condition, so the stick is
    // folded into the paw here too.
    //
    // The paw only follows when the model declares the stick. A model without
    // `CatParamStickShowLeftHand` has no stick to show, and a paw pressing down
    // for an image that can never appear is feedback for something the user
    // cannot see (ADR-0042). The check is a lookup in the model's own parameter
    // table, which is the same answer `set_normalized_parameter` reports as
    // `Unsupported`, so the gate costs nothing and cannot disagree with it.
    let stick_left_active = stick_active(
        input.stick_left_down,
        input.stick_left_x,
        input.stick_left_y,
    );
    let stick_right_active = stick_active(
        input.stick_right_down,
        input.stick_right_x,
        input.stick_right_y,
    );
    let left_hand_down = input.left_hand_down
        || (stick_left_active
            && model
                .parameter_range(ProductParameter::StickShowLeftHand)
                .is_some());
    let right_hand_down = input.right_hand_down
        || (stick_right_active
            && model
                .parameter_range(ProductParameter::StickShowRightHand)
                .is_some());
    for (parameter, value) in [
        (ProductParameter::MouseX, pointer_x),
        (ProductParameter::MouseY, pointer_y),
        (ProductParameter::AngleX, pointer_x),
        (ProductParameter::AngleY, pointer_y),
        (ProductParameter::AngleZ, pointer_z),
        (ProductParameter::EyeBallX, pointer_x),
        (ProductParameter::EyeBallY, pointer_y),
        (ProductParameter::LeftHandDown, f32::from(left_hand_down)),
        (ProductParameter::RightHandDown, f32::from(right_hand_down)),
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
        (
            ProductParameter::StickShowLeftHand,
            f32::from(stick_left_active),
        ),
        (
            ProductParameter::StickShowRightHand,
            f32::from(stick_right_active),
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
