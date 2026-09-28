//! Clearing everything at once.
//!
//! A reset has to be complete or it is worse than nothing: a key left held after a
//! focus change is a cat that keeps typing at whatever window the user moved to.
//! The keyboard and the mouse clear together, and a gamepad disconnect is scoped
//! to the gamepad so it cannot release a key the keyboard is still reporting.

use super::*;

impl InputState {
    pub fn force_reset(&mut self, reason: InputResetReason) {
        self.reset(reason);
    }
}

impl InputState {
    pub(crate) fn reset(&mut self, reason: InputResetReason) {
        self.diagnostics.released_by_reset = self
            .diagnostics
            .released_by_reset
            .saturating_add(self.pressed.len() as u64);
        self.pressed.clear();
        self.active_gamepads.clear();
        self.missing_confirmations.clear();
        self.last_reset_reason = Some(reason);
        self.diagnostics.reset_count = self.diagnostics.reset_count.saturating_add(1);
    }
}

impl InputState {
    pub(crate) fn disconnect_gamepad(&mut self, connection: GamepadConnection) {
        self.active_gamepads.remove(&connection);
        let before = self.pressed.len();
        self.pressed.retain(|control, _| {
            !matches!(control, InputControl::Gamepad(key) if key.connection == connection)
        });
        self.diagnostics.released_by_disconnect = self
            .diagnostics
            .released_by_disconnect
            .saturating_add((before - self.pressed.len()) as u64);
        self.missing_confirmations.retain(|control, _| {
            !matches!(control, InputControl::Gamepad(key) if key.connection == connection)
        });
    }
}
