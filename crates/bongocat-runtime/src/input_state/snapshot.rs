//! What a consumer of the runtime sees.
//!
//! Two shapes, deliberately different. The full snapshot is for the diagnostics
//! view and says which controls are held and since when. The model snapshot is for
//! the model, and it never carries a pressed key: it carries the product
//! parameters the bindings map them to, so a model cannot learn that a key is down
//! and act differently from one that is not.

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    pub pressed_key_count: usize,
    pub pressed_mouse_button_count: usize,
    pub pressed_gamepad_button_count: usize,
    pub connected_gamepad_count: usize,
    pub last_reset_reason: Option<InputResetReason>,
    pub last_input_sequence: Option<u64>,
    pub diagnostics: InputDiagnostics,
    pub transport: InputTransportDiagnostics,
}

/// Source gates applied while projecting captured input into the model view.
///
/// The pressed-state owner keeps the raw keyboard and gamepad edges intact for
/// diagnostics and recovery. These gates only affect the immutable model input
/// projection, so disabling one input family cannot strand a pressed key or
/// remove its eventual release from the input pipeline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModelInputFilter {
    pub(crate) ignore_keyboard: bool,
    pub(crate) ignore_gamepad: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelInputSnapshot {
    pub key_presses: KeyPressSet,
    pub left_hand_down: bool,
    pub right_hand_down: bool,
    pub mouse_left_down: bool,
    pub mouse_right_down: bool,
    pub stick_left_down: bool,
    pub stick_right_down: bool,
    pub stick_left_x: f32,
    pub stick_left_y: f32,
    pub stick_right_x: f32,
    pub stick_right_y: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub pointer_x: f32,
    pub pointer_y: f32,
    pub pointer_z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputDisposition {
    Applied,
    AppliedAfterSequenceGap { missing: u64 },
    DuplicateSequence,
    OutOfOrderSequence,
    ResetForNonMonotonicTime,
}
