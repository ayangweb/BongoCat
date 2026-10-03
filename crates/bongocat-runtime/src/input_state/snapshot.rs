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
    /// Which keyboard modifiers are held right now, with the two sides apart.
    ///
    /// The counts above cannot answer "is the left shift down", and the model
    /// projection cannot either: it drops any key the current model's artwork
    /// cannot draw, and modifiers are exactly the keys a model leaves out. This
    /// is the pressed set read through the modifier vocabulary instead, so a
    /// consumer that watches for one configured key does not have to take a copy
    /// of the pressed set or depend on which model is loaded.
    pub pressed_modifiers: PressedModifiers,
    pub connected_gamepad_count: usize,
    pub last_reset_reason: Option<InputResetReason>,
    pub last_input_sequence: Option<u64>,
    pub diagnostics: InputDiagnostics,
    pub transport: InputTransportDiagnostics,
}

/// How the captured input is projected into the model view.
///
/// The pressed-state owner keeps the raw keyboard and gamepad edges intact for
/// diagnostics and recovery. These switches only affect the immutable model
/// input projection, so disabling one input family cannot strand a pressed key
/// or remove its eventual release from the input pipeline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModelInputFilter {
    pub(crate) ignore_keyboard: bool,
    pub(crate) ignore_gamepad: bool,
    /// Whether the key-image layer draws every held key rather than one per
    /// hand.
    ///
    /// This is a display choice and not a source gate: it changes how many
    /// pictures a chord produces and in which order they are stacked, and it
    /// never changes which controls are held, released or reconciled.
    pub(crate) show_all_pressed_keys: bool,
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
