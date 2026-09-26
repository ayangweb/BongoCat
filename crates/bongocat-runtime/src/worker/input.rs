//! Turning the coalesced input channels into one model input snapshot.
//!
//! Cursor moves and gamepad axes may be merged to their latest value, but a key or
//! button edge is never dropped to make room: the channels are drained
//! independently and the reliable edges are always applied.

use super::super::transport::SnapshotCell;
use super::GamepadAxisValues;
use crate::input_state::{InputState, ModelInputFilter};
use crate::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn expire_keyboard_fallback(
    input_state: &mut InputState,
    timeout_ms: u32,
    input_bindings: &InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
    model_settings: ModelSettings,
    snapshot: &SnapshotCell,
    now: Duration,
) {
    if input_state.expire_keyboard_fallback(now, timeout_ms) == 0 {
        return;
    }
    publish(snapshot, |current| {
        current.input = input_state.snapshot();
        current.model_input = compose_model_input(
            input_state,
            input_bindings,
            normalized_cursor,
            gamepad_axis_values,
            gamepad_axis_settings,
            model_settings,
        );
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn consume_cursor(
    cursor_producer: &CursorProducer,
    snapshot: &SnapshotCell,
    input_state: &InputState,
    input_bindings: &InputBindings,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
    smoother: &mut CursorSmoother,
    normalized_cursor: &mut NormalizedCursorPosition,
    model_settings: ModelSettings,
    now: Duration,
) {
    let sample = cursor_producer.take();
    if let Some(sample) = sample {
        smoother.set_target(sample, now);
    }
    let advanced = smoother.advance(now);
    if sample.is_none() && !advanced {
        return;
    }
    *normalized_cursor = smoother.normalized();
    publish(snapshot, |current| {
        if let Some(sample) = sample {
            current.cursor.sample = Some(sample);
        }
        current.model_input = compose_model_input(
            input_state,
            input_bindings,
            *normalized_cursor,
            gamepad_axis_values,
            gamepad_axis_settings,
            model_settings,
        );
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn consume_gamepad_axes(
    producer: &GamepadAxisProducer,
    snapshot: &SnapshotCell,
    values: &mut GamepadAxisValues,
    input_state: &InputState,
    input_bindings: &InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    settings: GamepadAxisSettings,
    model_settings: ModelSettings,
) {
    if !values.consume(producer) {
        return;
    }
    publish(snapshot, |current| {
        current.model_input = compose_model_input(
            input_state,
            input_bindings,
            normalized_cursor,
            values,
            settings,
            model_settings,
        );
    });
}

pub(crate) fn compose_model_input(
    input_state: &InputState,
    input_bindings: &InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
    model_settings: ModelSettings,
) -> ModelInputSnapshot {
    let mut input = input_state.model_snapshot_with_filter(
        input_bindings,
        normalized_cursor,
        ModelInputFilter {
            ignore_keyboard: model_settings.ignore_keyboard,
            ignore_gamepad: model_settings.ignore_gamepad,
        },
    );
    let axes = if model_settings.ignore_gamepad {
        [0.0; 6]
    } else {
        gamepad_axis_values.project(input_state, gamepad_axis_settings)
    };
    input.stick_left_x = axes[GamepadAxis::LeftStickX as usize];
    input.stick_left_y = axes[GamepadAxis::LeftStickY as usize];
    input.stick_right_x = axes[GamepadAxis::RightStickX as usize];
    input.stick_right_y = axes[GamepadAxis::RightStickY as usize];
    input.left_trigger = axes[GamepadAxis::LeftTrigger as usize];
    input.right_trigger = axes[GamepadAxis::RightTrigger as usize];
    input
}
