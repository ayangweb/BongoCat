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
        // The four counts and the modifier set read the same pressed map, so they
        // are collected in one pass: this runs on every input edge, and walking
        // the map five times would make the cost of a fast chord five times what
        // one walk needs.
        let mut snapshot = InputSnapshot {
            pressed_modifiers: PressedModifiers::NONE,
            connected_gamepad_count: self.active_gamepads.len(),
            last_reset_reason: self.last_reset_reason,
            last_input_sequence: self.last_sequence,
            diagnostics: self.diagnostics,
            transport: InputTransportDiagnostics::default(),
            ..InputSnapshot::default()
        };
        for control in self.pressed.keys() {
            match control {
                InputControl::Key(key) => {
                    snapshot.pressed_key_count += 1;
                    if let Some(modifier) = ModifierKey::from_hid_usage(key.hid_usage()) {
                        snapshot.pressed_modifiers.insert(modifier);
                    }
                }
                InputControl::Mouse(_) => snapshot.pressed_mouse_button_count += 1,
                InputControl::Gamepad(_) => snapshot.pressed_gamepad_button_count += 1,
            }
        }
        snapshot
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
        let mut latest_left_key: Option<(PressOrder, KeyPress)> = None;
        let mut latest_right_key: Option<(PressOrder, KeyPress)> = None;
        // The layered mode needs every held key, not one per hand, so it collects
        // the presses and sorts them. The compatibility mode never looks at this
        // and leaves it empty, so the default path allocates nothing.
        let mut every_press: Vec<(PressOrder, KeyPress)> = Vec::new();
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
            let side = match hand {
                HandSide::Left => {
                    snapshot.left_hand_down = true;
                    KeySide::Left
                }
                HandSide::Right => {
                    snapshot.right_hand_down = true;
                    KeySide::Right
                }
            };
            let record = self.pressed.get(control).expect("pressed control record");
            let press = KeyPress { key, side };
            let order = PressOrder::of(record);
            if filter.show_all_pressed_keys {
                every_press.push((order, press));
            }
            let latest = match hand {
                HandSide::Left => &mut latest_left_key,
                HandSide::Right => &mut latest_right_key,
            };
            if latest.is_none_or(|(at, _)| order >= at) {
                *latest = Some((order, press));
            }
        }
        if !filter.show_all_pressed_keys {
            // Compatibility order, unchanged: the left hand's key is drawn before
            // the right hand's regardless of which was pressed first, so a model
            // that overlaps the two keeps the same stacking it always had.
            if let Some((_, press)) = latest_left_key {
                snapshot.key_presses.push(press);
            }
            if let Some((_, press)) = latest_right_key {
                snapshot.key_presses.push(press);
            }
            return snapshot;
        }
        // Oldest press first, so the key the user pressed last is the last one
        // the renderer draws and therefore the one on top. `KeyPressSet` keeps
        // insertion order, which is what makes that stack mean anything.
        every_press.sort_by_key(|(order, _)| *order);
        // A device that somehow holds more keys than the layer can draw loses
        // the oldest ones: the presses still down from a moment ago are the
        // history, and the ones the user is holding right now are what they are
        // looking at.
        let overflow = every_press.len().saturating_sub(KeyPressSet::CAPACITY);
        for (_, press) in every_press.into_iter().skip(overflow) {
            snapshot.key_presses.push(press);
        }
        snapshot
    }
}

/// When a control went down, as a total order over the held set.
///
/// The monotonic clock has millisecond resolution, so two keys of a fast chord
/// can share a timestamp; the sequence number of the edge that pressed the
/// control is what separates them, and it is monotonic even when a slow
/// adapter reports two edges in the same millisecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PressOrder {
    pressed_at: MonotonicMillis,
    pressed_sequence: u64,
}

impl PressOrder {
    const fn of(record: &PressedRecord) -> Self {
        Self {
            pressed_at: record.pressed_at,
            pressed_sequence: record.pressed_sequence,
        }
    }
}
