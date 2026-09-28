//! Reading the pressed set as a snapshot.
//!
//! A model snapshot is produced through a filter, and the filter is what keeps a
//! keyboard and a gamepad independent: a gamepad button is not a key press, and
//! projecting one as the other would make a reconnecting controller release keys
//! the user is still holding on the keyboard.

use super::*;

impl InputState {
    pub fn is_gamepad_connected(&self, connection: GamepadConnection) -> bool {
        self.active_gamepads.contains(&connection)
    }
}

impl InputState {
    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            pressed_key_count: self
                .pressed
                .keys()
                .filter(|control| matches!(control, InputControl::Key(_)))
                .count(),
            pressed_mouse_button_count: self
                .pressed
                .keys()
                .filter(|control| matches!(control, InputControl::Mouse(_)))
                .count(),
            pressed_gamepad_button_count: self
                .pressed
                .keys()
                .filter(|control| matches!(control, InputControl::Gamepad(_)))
                .count(),
            connected_gamepad_count: self.active_gamepads.len(),
            last_reset_reason: self.last_reset_reason,
            last_input_sequence: self.last_sequence,
            diagnostics: self.diagnostics,
            transport: InputTransportDiagnostics::default(),
        }
    }
}

impl InputState {
    #[cfg(test)]
    pub fn model_snapshot(
        &self,
        bindings: &InputBindings,
        cursor: NormalizedCursorPosition,
    ) -> ModelInputSnapshot {
        self.model_snapshot_with_filter(bindings, cursor, ModelInputFilter::default())
    }
}

impl InputState {
    pub(crate) fn model_snapshot_with_filter(
        &self,
        bindings: &InputBindings,
        cursor: NormalizedCursorPosition,
        filter: ModelInputFilter,
    ) -> ModelInputSnapshot {
        let mut snapshot = ModelInputSnapshot {
            mouse_left_down: self
                .pressed
                .contains_key(&InputControl::Mouse(MouseButton::Left)),
            mouse_right_down: self
                .pressed
                .contains_key(&InputControl::Mouse(MouseButton::Right)),
            pointer_x: cursor.x,
            pointer_y: cursor.y,
            pointer_z: cursor.z,
            ..ModelInputSnapshot::default()
        };
        let mut latest_left_key: Option<(MonotonicMillis, KeyPress)> = None;
        let mut latest_right_key: Option<(MonotonicMillis, KeyPress)> = None;
        for control in self.pressed.keys() {
            let (hand, key) = match control {
                InputControl::Key(key) if !filter.ignore_keyboard => (
                    bindings.hand_for(*key),
                    KeyIdentity::Keyboard(key.hid_usage()),
                ),
                // The stick buttons keep their own parameters: `StickLeftDown` /
                // `StickRightDown` drive the stick artwork of the model and are
                // not the same thing as a paw. A model that also ships a
                // `LeftStick.png` overlay still gets it, because the press is
                // projected exactly like every other button.
                InputControl::Gamepad(button) if !filter.ignore_gamepad => {
                    if matches!(button.button, GamepadButton::LeftStick) {
                        snapshot.stick_left_down = true;
                    } else if matches!(button.button, GamepadButton::RightStick) {
                        snapshot.stick_right_down = true;
                    }
                    (
                        bindings.hand_for_gamepad(button.button),
                        KeyIdentity::Gamepad(button.button),
                    )
                }
                InputControl::Key(_) | InputControl::Gamepad(_) | InputControl::Mouse(_) => {
                    continue;
                }
            };
            let Some(hand) = hand else {
                // No hand assignment means the model ships no artwork that draws
                // this control, so the press is dropped before it can reach the
                // renderer: a paw pressing down for an image that can never
                // appear is feedback for something the user cannot see
                // (ADR-0042). `bongocat-app::input_bindings_for_model` owns that
                // decision, and it asks the same `can_draw` the renderer draws
                // with.
                continue;
            };
            let (side, latest) = match hand {
                HandSide::Left => {
                    snapshot.left_hand_down = true;
                    (KeySide::Left, &mut latest_left_key)
                }
                HandSide::Right => {
                    snapshot.right_hand_down = true;
                    (KeySide::Right, &mut latest_right_key)
                }
            };
            let record = self.pressed.get(control).expect("pressed control record");
            let press = KeyPress { key, side };
            if latest.is_none_or(|(at, _)| record.pressed_at >= at) {
                *latest = Some((record.pressed_at, press));
            }
        }
        if let Some((_, press)) = latest_left_key {
            snapshot.key_presses.push(press);
        }
        if let Some((_, press)) = latest_right_key {
            snapshot.key_presses.push(press);
        }
        snapshot
    }
}
