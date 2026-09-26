//! What applying a clip actually changed.
//!
//! A clip can name parameters the model does not have, and dropping them
//! silently would leave the caller believing the model moved when it did not.
//! These counts are what makes that visible.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionApplyStatus {
    pub finished: bool,
    pub applied_parameter_count: usize,
    pub applied_part_opacity_count: usize,
    pub applied_eye_blink_count: usize,
    pub applied_lip_sync_count: usize,
    pub model_opacity_applied: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExpressionApplyStatus {
    pub applied_parameter_count: usize,
}
