//! Applying an event to the pressed set.
//!
//! Every event carries a sequence number, and an event whose number does not
//! follow the last one applied is evidence that something was lost — so the set is
//! reset and the event is treated as the first after the gap. A duplicate or an
//! out-of-order release is refused rather than applied, because applying a stale
//! release would clear a key the user is still holding.

use super::*;

/// Physical keys with a Thock sound group that this plugin can address.
///
/// Thock maps Backspace to `del`, not `backspace`; the left and right Shift
/// keys and left Control likewise keep their own groups.
const KEY_SOUND_GROUPS: [(&str, u16); 8] = [
    ("space", 0x2c),
    ("enter", 0x28),
    ("enter", 0x58),
    ("backspace", 0x2a),
    ("tab", 0x2b),
    ("shift_left", 0xe1),
    ("shift_right", 0xe5),
    ("ctrl_left", 0xe0),
];

fn count_key_sound_group(diagnostics: &mut InputDiagnostics, key: &str, release: bool) {
    let count = match (key, release) {
        ("space", false) => &mut diagnostics.space_activity,
        ("space", true) => &mut diagnostics.space_release,
        ("enter", false) => &mut diagnostics.enter_activity,
        ("enter", true) => &mut diagnostics.enter_release,
        ("backspace", false) => &mut diagnostics.backspace_activity,
        ("backspace", true) => &mut diagnostics.backspace_release,
        ("tab", false) => &mut diagnostics.tab_activity,
        ("tab", true) => &mut diagnostics.tab_release,
        ("shift_left", false) => &mut diagnostics.shift_left_activity,
        ("shift_left", true) => &mut diagnostics.shift_left_release,
        ("shift_right", false) => &mut diagnostics.shift_right_activity,
        ("shift_right", true) => &mut diagnostics.shift_right_release,
        ("ctrl_left", false) => &mut diagnostics.ctrl_left_activity,
        ("ctrl_left", true) => &mut diagnostics.ctrl_left_release,
        _ => return,
    };
    *count = count.saturating_add(1);
}

fn key_sound_group(control: InputControl) -> Option<&'static str> {
    let InputControl::Key(key) = control else {
        return None;
    };
    KEY_SOUND_GROUPS
        .iter()
        .find_map(|(name, usage)| (*usage == key.hid_usage()).then_some(*name))
}

impl InputState {
    pub fn apply(&mut self, envelope: SequencedInputEvent) -> InputDisposition {
        let gap = if let Some(last_sequence) = self.last_sequence {
            let distance = envelope.sequence.wrapping_sub(last_sequence);
            match distance {
                0 => {
                    self.diagnostics.duplicate_sequence_count =
                        self.diagnostics.duplicate_sequence_count.saturating_add(1);
                    return InputDisposition::DuplicateSequence;
                }
                1 => 0,
                distance if distance <= u64::MAX / 2 => distance - 1,
                _ => {
                    self.diagnostics.out_of_order_sequence_count = self
                        .diagnostics
                        .out_of_order_sequence_count
                        .saturating_add(1);
                    return InputDisposition::OutOfOrderSequence;
                }
            }
        } else {
            0
        };
        self.last_sequence = Some(envelope.sequence);
        if gap > 0 {
            self.diagnostics.sequence_gap_count =
                self.diagnostics.sequence_gap_count.saturating_add(1);
            self.diagnostics.missing_sequence_count =
                self.diagnostics.missing_sequence_count.saturating_add(gap);
        }

        let event_time = envelope.event.at();
        if self
            .last_timestamp
            .is_some_and(|last_timestamp| event_time < last_timestamp)
        {
            self.diagnostics.non_monotonic_time_count =
                self.diagnostics.non_monotonic_time_count.saturating_add(1);
            self.reset(InputResetReason::NonMonotonicTime);
            return InputDisposition::ResetForNonMonotonicTime;
        }
        self.last_timestamp = Some(event_time);

        if gap > 0 {
            if matches!(envelope.event, InputEvent::Reset { .. }) {
                self.apply_event(envelope.event, envelope.sequence);
            } else {
                self.reset(InputResetReason::SequenceGap);
                self.apply_event(envelope.event, envelope.sequence);
            }
            return InputDisposition::AppliedAfterSequenceGap { missing: gap };
        }
        self.apply_event(envelope.event, envelope.sequence);
        InputDisposition::Applied
    }
}

impl InputState {
    #[cfg(test)]
    pub(crate) fn record(&self, control: InputControl) -> Option<PressedRecord> {
        self.pressed.get(&control).copied()
    }
}

impl InputState {
    /// `sequence` is the number the envelope carried, kept on the record of
    /// every control it presses so the key-image layer can stack a chord in the
    /// order the queue delivered it (see [`PressedRecord::pressed_sequence`]).
    pub(crate) fn apply_event(&mut self, event: InputEvent, sequence: u64) {
        match event {
            InputEvent::GamepadConnected { connection, .. } => {
                if self.active_gamepads.iter().any(|active| {
                    active.device_id == connection.device_id
                        && active.generation >= connection.generation
                }) {
                    self.diagnostics.stale_gamepad_events =
                        self.diagnostics.stale_gamepad_events.saturating_add(1);
                    return;
                }
                let replaced = self
                    .active_gamepads
                    .iter()
                    .copied()
                    .filter(|active| active.device_id == connection.device_id)
                    .collect::<BTreeSet<_>>();
                for previous in replaced {
                    self.disconnect_gamepad(previous);
                }
                self.active_gamepads.insert(connection);
                self.diagnostics.gamepad_connections =
                    self.diagnostics.gamepad_connections.saturating_add(1);
            }
            InputEvent::GamepadDisconnected { connection, .. } => {
                if self.active_gamepads.contains(&connection) {
                    self.disconnect_gamepad(connection);
                    self.diagnostics.gamepad_disconnections =
                        self.diagnostics.gamepad_disconnections.saturating_add(1);
                } else {
                    self.diagnostics.stale_gamepad_events =
                        self.diagnostics.stale_gamepad_events.saturating_add(1);
                }
            }
            InputEvent::Edge {
                control,
                edge,
                source,
                at,
            } => {
                if let InputControl::Gamepad(key) = control
                    && !self.active_gamepads.contains(&key.connection)
                {
                    self.diagnostics.stale_gamepad_events =
                        self.diagnostics.stale_gamepad_events.saturating_add(1);
                    return;
                }
                match edge {
                    InputEdge::Down => {
                        if source != InputSource::Capture {
                            self.diagnostics.invalid_source =
                                self.diagnostics.invalid_source.saturating_add(1);
                            return;
                        }
                        self.missing_confirmations.remove(&control);
                        match self.pressed.entry(control) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(PressedRecord {
                                    source,
                                    pressed_at: at,
                                    pressed_sequence: sequence,
                                    last_reconciled_at: None,
                                });
                                if matches!(control, InputControl::Key(_)) {
                                    self.diagnostics.keyboard_activity =
                                        self.diagnostics.keyboard_activity.saturating_add(1);
                                    if let Some(group) = key_sound_group(control) {
                                        count_key_sound_group(&mut self.diagnostics, group, false);
                                    }
                                } else if matches!(control, InputControl::Mouse(_)) {
                                    self.diagnostics.mouse_activity =
                                        self.diagnostics.mouse_activity.saturating_add(1);
                                }
                                self.diagnostics.captured_down =
                                    self.diagnostics.captured_down.saturating_add(1);
                            }
                            std::collections::btree_map::Entry::Occupied(_) => {
                                self.diagnostics.duplicate_down =
                                    self.diagnostics.duplicate_down.saturating_add(1);
                            }
                        }
                    }
                    InputEdge::Up => {
                        self.missing_confirmations.remove(&control);
                        let released = self.pressed.remove(&control).is_some();
                        if !released {
                            self.diagnostics.unmatched_release =
                                self.diagnostics.unmatched_release.saturating_add(1);
                        }
                        match source {
                            InputSource::Capture => {
                                self.diagnostics.captured_up =
                                    self.diagnostics.captured_up.saturating_add(1);
                                // A captured release is a physical edge whatever
                                // the pressed set believed about it; a
                                // reconciled one is the runtime's own correction
                                // and is counted by the reconciliation counters.
                                match control {
                                    InputControl::Key(_) => {
                                        self.diagnostics.keyboard_release =
                                            self.diagnostics.keyboard_release.saturating_add(1);
                                        if let Some(group) = key_sound_group(control) {
                                            count_key_sound_group(
                                                &mut self.diagnostics,
                                                group,
                                                true,
                                            );
                                        }
                                    }
                                    InputControl::Mouse(_) => {
                                        self.diagnostics.mouse_release =
                                            self.diagnostics.mouse_release.saturating_add(1);
                                    }
                                    InputControl::Gamepad(_) => {}
                                }
                            }
                            InputSource::Reconciliation => {
                                if released {
                                    self.diagnostics.reconciled_release =
                                        self.diagnostics.reconciled_release.saturating_add(1);
                                }
                            }
                        }
                    }
                }
            }
            InputEvent::Reconcile { pressed, at } => {
                let controls = self
                    .pressed
                    .keys()
                    .filter(|control| !matches!(control, InputControl::Gamepad(_)))
                    .copied()
                    .collect::<Vec<_>>();
                for control in controls {
                    if pressed.contains(&control) {
                        self.missing_confirmations.remove(&control);
                        if let Some(record) = self.pressed.get_mut(&control) {
                            record.last_reconciled_at = Some(at);
                        }
                        continue;
                    }
                    let confirmations = self.missing_confirmations.entry(control).or_insert(0);
                    *confirmations = confirmations.saturating_add(1);
                    if *confirmations >= self.policy.missing_confirmations() {
                        self.missing_confirmations.remove(&control);
                        self.pressed.remove(&control);
                        self.diagnostics.reconciled_release =
                            self.diagnostics.reconciled_release.saturating_add(1);
                    }
                }
            }
            InputEvent::Reset { reason, .. } => self.reset(reason),
        }
    }
}
