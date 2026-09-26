use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use bongocat_input::{
    GamepadButton, GamepadConnection, HandSide, InputBindings, InputControl, InputDiagnostics,
    InputEdge, InputEvent, InputResetReason, InputSource, InputTransportDiagnostics,
    MonotonicMillis, MouseButton, NormalizedCursorPosition, SequencedInputEvent,
};
#[cfg(test)]
use bongocat_input::{GamepadButtonKey, PhysicalKey};
use bongocat_render::{KeyIdentity, KeyPress, KeyPressSet, KeySide};

const DEFAULT_MISSING_CONFIRMATIONS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReconciliationPolicy {
    missing_confirmations: u8,
}

impl ReconciliationPolicy {
    pub const fn missing_confirmations(self) -> u8 {
        self.missing_confirmations
    }
}

impl Default for ReconciliationPolicy {
    fn default() -> Self {
        Self {
            missing_confirmations: DEFAULT_MISSING_CONFIRMATIONS,
        }
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PressedRecord {
    source: InputSource,
    pressed_at: MonotonicMillis,
    last_reconciled_at: Option<MonotonicMillis>,
    runtime_observed_at: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct InputState {
    pressed: BTreeMap<InputControl, PressedRecord>,
    active_gamepads: BTreeSet<GamepadConnection>,
    missing_confirmations: BTreeMap<InputControl, u8>,
    policy: ReconciliationPolicy,
    diagnostics: InputDiagnostics,
    last_sequence: Option<u64>,
    last_timestamp: Option<MonotonicMillis>,
    last_reset_reason: Option<InputResetReason>,
}

impl InputState {
    pub fn is_gamepad_connected(&self, connection: GamepadConnection) -> bool {
        self.active_gamepads.contains(&connection)
    }

    #[cfg(test)]
    pub fn apply(&mut self, envelope: SequencedInputEvent) -> InputDisposition {
        let observed_at = Duration::from_millis(envelope.event.at().value());
        self.apply_observed(envelope, observed_at)
    }

    pub fn apply_observed(
        &mut self,
        envelope: SequencedInputEvent,
        observed_at: Duration,
    ) -> InputDisposition {
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
                self.apply_event(envelope.event, observed_at);
            } else {
                self.reset(InputResetReason::SequenceGap);
                self.apply_event(envelope.event, observed_at);
            }
            return InputDisposition::AppliedAfterSequenceGap { missing: gap };
        }
        self.apply_event(envelope.event, observed_at);
        InputDisposition::Applied
    }

    pub fn expire_keyboard_fallback(&mut self, now: Duration, timeout_ms: u32) -> usize {
        if timeout_ms == 0 {
            return 0;
        }
        let timeout = Duration::from_millis(u64::from(timeout_ms));
        let expired = self
            .pressed
            .iter()
            .filter_map(|(control, record)| {
                matches!(control, InputControl::Key(_))
                    .then_some(())
                    .and_then(|()| now.checked_sub(record.runtime_observed_at))
                    .is_some_and(|elapsed| elapsed >= timeout)
                    .then_some(*control)
            })
            .collect::<Vec<_>>();
        for control in &expired {
            self.pressed.remove(control);
            self.missing_confirmations.remove(control);
        }
        self.diagnostics.fallback_release = self
            .diagnostics
            .fallback_release
            .saturating_add(expired.len() as u64);
        expired.len()
    }

    pub fn force_reset(&mut self, reason: InputResetReason) {
        self.reset(reason);
    }

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

    #[cfg(test)]
    pub fn model_snapshot(
        &self,
        bindings: &InputBindings,
        cursor: NormalizedCursorPosition,
    ) -> ModelInputSnapshot {
        self.model_snapshot_with_filter(bindings, cursor, ModelInputFilter::default())
    }

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

    #[cfg(test)]
    fn record(&self, control: InputControl) -> Option<PressedRecord> {
        self.pressed.get(&control).copied()
    }

    fn apply_event(&mut self, event: InputEvent, observed_at: Duration) {
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
                                    last_reconciled_at: None,
                                    runtime_observed_at: observed_at,
                                });
                                self.diagnostics.captured_down =
                                    self.diagnostics.captured_down.saturating_add(1);
                            }
                            std::collections::btree_map::Entry::Occupied(mut entry) => {
                                if matches!(control, InputControl::Key(_)) {
                                    entry.get_mut().runtime_observed_at =
                                        entry.get().runtime_observed_at.max(observed_at);
                                }
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

    fn reset(&mut self, reason: InputResetReason) {
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

    fn disconnect_gamepad(&mut self, connection: GamepadConnection) {
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

#[cfg(test)]
mod tests {
    use super::*;

    const A: InputControl = InputControl::Key(PhysicalKey::KEY_A);
    const CTRL: InputControl = InputControl::Key(PhysicalKey::LEFT_CONTROL);
    const ALT: InputControl = InputControl::Key(PhysicalKey::LEFT_ALT);

    fn edge(sequence: u64, at: u64, control: InputControl, edge: InputEdge) -> SequencedInputEvent {
        SequencedInputEvent {
            sequence,
            event: InputEvent::Edge {
                control,
                edge,
                source: InputSource::Capture,
                at: MonotonicMillis::new(at),
            },
        }
    }

    #[test]
    fn issue_47_lost_release_is_reconciled_without_clearing_held_keys_early() {
        let mut state = InputState::default();
        state.apply(edge(0, 0, CTRL, InputEdge::Down));
        state.apply(edge(1, 1, ALT, InputEdge::Down));
        state.apply(edge(2, 2, A, InputEdge::Down));
        state.apply(edge(3, 3, ALT, InputEdge::Up));
        state.apply(edge(4, 4, CTRL, InputEdge::Up));

        state.apply(SequencedInputEvent {
            sequence: 5,
            event: InputEvent::Reconcile {
                pressed: BTreeSet::new(),
                at: MonotonicMillis::new(250),
            },
        });
        assert!(state.record(A).is_some());
        state.apply(SequencedInputEvent {
            sequence: 6,
            event: InputEvent::Reconcile {
                pressed: BTreeSet::new(),
                at: MonotonicMillis::new(500),
            },
        });

        assert_eq!(state.snapshot().pressed_key_count, 0);
        assert_eq!(state.snapshot().diagnostics.reconciled_release, 1);
    }

    #[test]
    fn sequence_gap_resets_unknown_state_before_current_edge() {
        let mut state = InputState::default();
        state.apply(edge(10, 0, CTRL, InputEdge::Down));
        assert_eq!(
            state.apply(edge(12, 1, A, InputEdge::Down)),
            InputDisposition::AppliedAfterSequenceGap { missing: 1 }
        );
        assert!(state.record(CTRL).is_none());
        assert!(state.record(A).is_some());
        assert_eq!(
            state.snapshot().last_reset_reason,
            Some(InputResetReason::SequenceGap)
        );
    }

    #[test]
    fn input_sequence_tracker_handles_u64_wraparound() {
        let mut state = InputState::default();
        state.apply(edge(u64::MAX - 2, 0, A, InputEdge::Down));
        assert_eq!(
            state.apply(edge(u64::MAX, 1, A, InputEdge::Up)),
            InputDisposition::AppliedAfterSequenceGap { missing: 1 }
        );
        assert_eq!(
            state.apply(edge(0, 2, A, InputEdge::Down)),
            InputDisposition::Applied
        );
        assert_eq!(state.snapshot().pressed_key_count, 1);
        assert_eq!(state.snapshot().diagnostics.missing_sequence_count, 1);
        assert_eq!(
            state.apply(edge(u64::MAX, 3, A, InputEdge::Up)),
            InputDisposition::OutOfOrderSequence
        );
    }

    #[test]
    fn duplicate_and_out_of_order_sequences_never_apply_release() {
        let mut state = InputState::default();
        state.apply(edge(4, 10, A, InputEdge::Down));
        assert_eq!(
            state.apply(edge(4, 11, A, InputEdge::Up)),
            InputDisposition::DuplicateSequence
        );
        assert_eq!(
            state.apply(edge(3, 12, A, InputEdge::Up)),
            InputDisposition::OutOfOrderSequence
        );
        assert!(state.record(A).is_some());
    }

    #[test]
    fn non_monotonic_time_resets_pressed_state() {
        let mut state = InputState::default();
        state.apply(edge(0, 10, A, InputEdge::Down));
        assert_eq!(
            state.apply(edge(1, 9, CTRL, InputEdge::Down)),
            InputDisposition::ResetForNonMonotonicTime
        );
        assert_eq!(state.snapshot().pressed_key_count, 0);
        assert_eq!(
            state.snapshot().last_reset_reason,
            Some(InputResetReason::NonMonotonicTime)
        );
    }

    #[test]
    fn pressed_record_retains_source_and_monotonic_times() {
        let mut state = InputState::default();
        state.apply(edge(0, 10, A, InputEdge::Down));
        state.apply(SequencedInputEvent {
            sequence: 1,
            event: InputEvent::Reconcile {
                pressed: BTreeSet::from([A]),
                at: MonotonicMillis::new(250),
            },
        });
        assert_eq!(
            state.record(A),
            Some(PressedRecord {
                source: InputSource::Capture,
                pressed_at: MonotonicMillis::new(10),
                last_reconciled_at: Some(MonotonicMillis::new(250)),
                runtime_observed_at: Duration::from_millis(10),
            })
        );
    }

    #[test]
    fn keyboard_fallback_expires_at_deadline_and_repeat_refreshes_it() {
        let mut state = InputState::default();
        state.apply_observed(edge(0, 900, A, InputEdge::Down), Duration::from_millis(10));
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_millis(509), 500),
            0
        );
        state.apply_observed(edge(1, 901, A, InputEdge::Down), Duration::from_millis(400));
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_millis(899), 500),
            0
        );
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_millis(900), 500),
            1
        );
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_key_count, 0);
        assert_eq!(snapshot.diagnostics.fallback_release, 1);
        assert_eq!(snapshot.diagnostics.duplicate_down, 1);
    }

    #[test]
    fn keyboard_fallback_zero_and_non_monotonic_clock_do_not_release() {
        let mut state = InputState::default();
        state.apply_observed(edge(0, 1, A, InputEdge::Down), Duration::from_secs(5));
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_secs(60), 0),
            0
        );
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_secs(4), 500),
            0
        );
        assert_eq!(state.snapshot().pressed_key_count, 1);
    }

    #[test]
    fn keyboard_fallback_never_expires_mouse_or_gamepad_controls() {
        let connection = GamepadConnection {
            device_id: 0,
            generation: 1,
        };
        let gamepad = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::South,
        });
        let mut state = InputState::default();
        state.apply_observed(
            SequencedInputEvent {
                sequence: 0,
                event: InputEvent::GamepadConnected {
                    connection,
                    at: MonotonicMillis::new(0),
                },
            },
            Duration::ZERO,
        );
        state.apply_observed(
            edge(
                1,
                1,
                InputControl::Mouse(MouseButton::Left),
                InputEdge::Down,
            ),
            Duration::ZERO,
        );
        state.apply_observed(edge(2, 2, gamepad, InputEdge::Down), Duration::ZERO);
        assert_eq!(
            state.expire_keyboard_fallback(Duration::from_secs(60), 1),
            0
        );
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_mouse_button_count, 1);
        assert_eq!(snapshot.pressed_gamepad_button_count, 1);
    }

    #[test]
    fn reset_clears_keyboard_and_mouse_together() {
        let mut state = InputState::default();
        state.apply(edge(0, 0, A, InputEdge::Down));
        state.apply(edge(
            1,
            1,
            InputControl::Mouse(MouseButton::Left),
            InputEdge::Down,
        ));
        state.apply(SequencedInputEvent {
            sequence: 2,
            event: InputEvent::Reset {
                reason: InputResetReason::QueueOverflow,
                at: MonotonicMillis::new(2),
            },
        });
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_key_count, 0);
        assert_eq!(snapshot.pressed_mouse_button_count, 0);
        assert_eq!(snapshot.diagnostics.released_by_reset, 2);
    }

    #[test]
    fn model_snapshot_applies_bindings_without_exposing_pressed_keys() {
        let right = PhysicalKey::from_hid_usage(0x4f);
        let bindings = InputBindings::new(BTreeMap::from([
            (PhysicalKey::KEY_A, HandSide::Left),
            (right, HandSide::Right),
        ]));
        let mut state = InputState::default();
        state.apply(edge(0, 0, A, InputEdge::Down));
        state.apply(edge(1, 1, InputControl::Key(right), InputEdge::Down));
        state.apply(edge(
            2,
            2,
            InputControl::Mouse(MouseButton::Left),
            InputEdge::Down,
        ));
        assert_eq!(
            state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
            ModelInputSnapshot {
                key_presses: {
                    let mut presses = KeyPressSet::default();
                    presses.push(KeyPress::keyboard(
                        PhysicalKey::KEY_A.hid_usage(),
                        KeySide::Left,
                    ));
                    presses.push(KeyPress::keyboard(right.hid_usage(), KeySide::Right));
                    presses
                },
                left_hand_down: true,
                right_hand_down: true,
                mouse_left_down: true,
                mouse_right_down: false,
                ..ModelInputSnapshot::default()
            }
        );
        state.force_reset(InputResetReason::Test);
        assert_eq!(
            state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
            ModelInputSnapshot::default()
        );
    }

    #[test]
    fn source_filters_keep_keyboard_and_gamepad_model_input_independent() {
        let connection = GamepadConnection {
            device_id: 3,
            generation: 1,
        };
        let gamepad = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::South,
        });
        let bindings = InputBindings::with_gamepad_hands(
            BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]),
            BTreeMap::from([(GamepadButton::South, HandSide::Right)]),
        );
        let mut state = InputState::default();
        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            },
        });
        state.apply(edge(1, 1, A, InputEdge::Down));
        state.apply(edge(2, 2, gamepad, InputEdge::Down));

        let keyboard_ignored = state.model_snapshot_with_filter(
            &bindings,
            NormalizedCursorPosition::default(),
            ModelInputFilter {
                ignore_keyboard: true,
                ignore_gamepad: false,
            },
        );
        assert!(!keyboard_ignored.left_hand_down);
        assert!(keyboard_ignored.right_hand_down);
        assert_eq!(
            keyboard_ignored
                .key_presses
                .iter()
                .map(|press| (press.key, press.side))
                .collect::<Vec<_>>(),
            vec![(KeyIdentity::Gamepad(GamepadButton::South), KeySide::Right)],
            "the surviving source's own overlay, and only that one"
        );

        let gamepad_ignored = state.model_snapshot_with_filter(
            &bindings,
            NormalizedCursorPosition::default(),
            ModelInputFilter {
                ignore_keyboard: false,
                ignore_gamepad: true,
            },
        );
        assert!(gamepad_ignored.left_hand_down);
        assert!(!gamepad_ignored.right_hand_down);
        assert_eq!(
            gamepad_ignored
                .key_presses
                .iter()
                .map(|press| (press.key, press.side))
                .collect::<Vec<_>>(),
            vec![(
                KeyIdentity::Keyboard(PhysicalKey::KEY_A.hid_usage()),
                KeySide::Left
            )],
            "the ignored source contributes no overlay of its own"
        );

        let all_ignored = state.model_snapshot_with_filter(
            &bindings,
            NormalizedCursorPosition::default(),
            ModelInputFilter {
                ignore_keyboard: true,
                ignore_gamepad: true,
            },
        );
        assert_eq!(all_ignored, ModelInputSnapshot::default());
    }

    /// The globe key travels the same path as every other key.
    ///
    /// Its usage is `0xff03` — Apple's vendor page folded into the same `u16` a
    /// Keyboard/Keypad usage uses — so this pins that nothing on the way to the
    /// model snapshot narrows it to a byte, indexes a table by it, or treats a
    /// usage above `0x00ff` as "not a key". A press without a hand assignment is
    /// dropped, so the binding is what makes it observable.
    #[test]
    fn the_globe_key_projects_through_the_model_snapshot_like_any_other_key() {
        assert_eq!(PhysicalKey::GLOBE.hid_usage(), 0xff03);
        let unbound = InputBindings::new(BTreeMap::new());
        let bindings = InputBindings::new(BTreeMap::from([(PhysicalKey::GLOBE, HandSide::Left)]));
        let mut state = InputState::default();
        state.apply(edge(
            0,
            0,
            InputControl::Key(PhysicalKey::GLOBE),
            InputEdge::Down,
        ));

        assert_eq!(
            state.model_snapshot(&unbound, NormalizedCursorPosition::default()),
            ModelInputSnapshot::default(),
            "an unbound globe key is inert, like any other unbound key"
        );
        assert_eq!(
            state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
            ModelInputSnapshot {
                key_presses: {
                    let mut presses = KeyPressSet::default();
                    presses.push(KeyPress::keyboard(
                        PhysicalKey::GLOBE.hid_usage(),
                        KeySide::Left,
                    ));
                    presses
                },
                left_hand_down: true,
                ..ModelInputSnapshot::default()
            }
        );

        state.apply(edge(
            1,
            1,
            InputControl::Key(PhysicalKey::GLOBE),
            InputEdge::Up,
        ));
        assert_eq!(
            state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
            ModelInputSnapshot::default(),
            "and it releases like any other key"
        );
    }

    #[test]
    fn gamepad_button_edges_project_to_stick_parameters_and_reset_cleanly() {
        let connection = GamepadConnection {
            device_id: 2,
            generation: 7,
        };
        let left_stick = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::LeftStick,
        });
        let right_stick = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::RightStick,
        });
        let mut state = InputState::default();
        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            },
        });
        state.apply(edge(1, 1, left_stick, InputEdge::Down));
        state.apply(edge(2, 2, right_stick, InputEdge::Down));
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_gamepad_button_count, 2);
        assert_eq!(snapshot.connected_gamepad_count, 1);
        assert_eq!(
            state.model_snapshot(
                &InputBindings::default(),
                NormalizedCursorPosition::default()
            ),
            ModelInputSnapshot {
                stick_left_down: true,
                stick_right_down: true,
                ..ModelInputSnapshot::default()
            }
        );

        state.force_reset(InputResetReason::DeviceRemoved);
        assert_eq!(state.snapshot().pressed_gamepad_button_count, 0);
        assert_eq!(state.snapshot().connected_gamepad_count, 0);
        assert_eq!(
            state.model_snapshot(
                &InputBindings::default(),
                NormalizedCursorPosition::default()
            ),
            ModelInputSnapshot::default()
        );
    }

    #[test]
    fn reset_rejects_stale_gamepad_edges_until_a_new_generation_connects() {
        let first = GamepadConnection {
            device_id: 1,
            generation: 4,
        };
        let second = GamepadConnection {
            device_id: 1,
            generation: 5,
        };
        let button = |connection| {
            InputControl::Gamepad(GamepadButtonKey {
                connection,
                button: GamepadButton::South,
            })
        };
        let mut state = InputState::default();

        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection: first,
                at: MonotonicMillis::new(0),
            },
        });
        state.apply(edge(1, 1, button(first), InputEdge::Down));
        assert_eq!(state.snapshot().pressed_gamepad_button_count, 1);

        state.apply(SequencedInputEvent {
            sequence: 2,
            event: InputEvent::Reset {
                reason: InputResetReason::QueueOverflow,
                at: MonotonicMillis::new(2),
            },
        });
        state.apply(edge(3, 3, button(first), InputEdge::Down));
        assert_eq!(state.snapshot().pressed_gamepad_button_count, 0);
        assert_eq!(state.snapshot().diagnostics.stale_gamepad_events, 1);

        state.apply(SequencedInputEvent {
            sequence: 4,
            event: InputEvent::GamepadConnected {
                connection: second,
                at: MonotonicMillis::new(4),
            },
        });
        state.apply(edge(5, 5, button(second), InputEdge::Down));
        assert_eq!(state.snapshot().connected_gamepad_count, 1);
        assert_eq!(state.snapshot().pressed_gamepad_button_count, 1);
    }

    /// A bound gamepad button projects both halves of the reaction the keyboard
    /// path already had: the paw and the button's own overlay. The overlay is
    /// the half that was missing — a gamepad press used to reach the renderer as
    /// a bare HID usage, which no gamepad button can be, so the model moved a paw
    /// for every button and showed the pressed button for none of them.
    #[test]
    fn configured_gamepad_buttons_project_to_the_bound_hand_and_its_overlay() {
        let connection = GamepadConnection {
            device_id: 4,
            generation: 2,
        };
        let south = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::South,
        });
        let east = InputControl::Gamepad(GamepadButtonKey {
            connection,
            button: GamepadButton::East,
        });
        let bindings = InputBindings::with_gamepad_hands(
            BTreeMap::new(),
            BTreeMap::from([
                (GamepadButton::South, HandSide::Left),
                (GamepadButton::East, HandSide::Right),
            ]),
        );
        let overlays = |state: &InputState| {
            state
                .model_snapshot(&bindings, NormalizedCursorPosition::default())
                .key_presses
                .iter()
                .map(|press| (press.key, press.side))
                .collect::<Vec<_>>()
        };
        let mut state = InputState::default();
        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            },
        });
        state.apply(edge(1, 1, south, InputEdge::Down));
        assert_eq!(
            state.model_snapshot(&bindings, NormalizedCursorPosition::default()),
            ModelInputSnapshot {
                key_presses: {
                    let mut presses = KeyPressSet::default();
                    presses.push(KeyPress::gamepad(GamepadButton::South, KeySide::Left));
                    presses
                },
                left_hand_down: true,
                ..ModelInputSnapshot::default()
            }
        );
        state.apply(edge(2, 2, east, InputEdge::Down));
        assert!(
            state
                .model_snapshot(&bindings, NormalizedCursorPosition::default())
                .right_hand_down
        );
        assert_eq!(
            overlays(&state),
            vec![
                (KeyIdentity::Gamepad(GamepadButton::South), KeySide::Left),
                (KeyIdentity::Gamepad(GamepadButton::East), KeySide::Right),
            ],
            "one overlay per hand, each the button that hand last saw pressed"
        );
        state.apply(edge(3, 3, south, InputEdge::Up));
        let after_release = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
        assert!(!after_release.left_hand_down);
        assert!(after_release.right_hand_down);
        assert_eq!(
            overlays(&state),
            vec![(KeyIdentity::Gamepad(GamepadButton::East), KeySide::Right)],
            "releasing one button leaves the other hand's overlay alone"
        );
    }

    /// A gamepad button the model has no artwork for must be inert, exactly like
    /// an unbound key (ADR-0042), and the two stick buttons must keep their own
    /// parameters whether or not they are bound.
    #[test]
    fn an_unbound_gamepad_button_is_inert_and_the_sticks_keep_their_parameters() {
        let connection = GamepadConnection {
            device_id: 5,
            generation: 1,
        };
        let control = |button| InputControl::Gamepad(GamepadButtonKey { connection, button });
        let bindings = InputBindings::with_gamepad_hands(
            BTreeMap::new(),
            BTreeMap::from([(GamepadButton::Start, HandSide::Right)]),
        );
        let mut state = InputState::default();
        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            },
        });
        state.apply(edge(1, 1, control(GamepadButton::Select), InputEdge::Down));
        let unbound = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
        assert!(!unbound.left_hand_down);
        assert!(!unbound.right_hand_down);
        assert_eq!(unbound.key_presses.iter().count(), 0);

        state.apply(edge(
            2,
            2,
            control(GamepadButton::LeftStick),
            InputEdge::Down,
        ));
        let stick = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
        assert!(
            stick.stick_left_down,
            "a stick press drives the stick artwork, not a paw"
        );
        assert_eq!(stick.key_presses.iter().count(), 0);

        state.apply(edge(3, 3, control(GamepadButton::Start), InputEdge::Down));
        let bound = state.model_snapshot(&bindings, NormalizedCursorPosition::default());
        assert!(bound.right_hand_down);
        assert_eq!(
            bound
                .key_presses
                .iter()
                .map(|press| (press.key, press.side))
                .collect::<Vec<_>>(),
            vec![(KeyIdentity::Gamepad(GamepadButton::Start), KeySide::Right)]
        );
    }

    #[test]
    fn disconnect_is_scoped_and_keyboard_reconciliation_ignores_gamepads() {
        let first = GamepadConnection {
            device_id: 0,
            generation: 1,
        };
        let second = GamepadConnection {
            device_id: 1,
            generation: 1,
        };
        let first_button = InputControl::Gamepad(GamepadButtonKey {
            connection: first,
            button: GamepadButton::South,
        });
        let second_button = InputControl::Gamepad(GamepadButtonKey {
            connection: second,
            button: GamepadButton::East,
        });
        let mut state = InputState::default();
        for (sequence, connection) in [(0, first), (1, second)] {
            state.apply(SequencedInputEvent {
                sequence,
                event: InputEvent::GamepadConnected {
                    connection,
                    at: MonotonicMillis::new(sequence),
                },
            });
        }
        state.apply(edge(2, 2, first_button, InputEdge::Down));
        state.apply(edge(3, 3, second_button, InputEdge::Down));
        state.apply(SequencedInputEvent {
            sequence: 4,
            event: InputEvent::Reconcile {
                pressed: BTreeSet::new(),
                at: MonotonicMillis::new(4),
            },
        });
        state.apply(SequencedInputEvent {
            sequence: 5,
            event: InputEvent::GamepadDisconnected {
                connection: first,
                at: MonotonicMillis::new(5),
            },
        });

        let snapshot = state.snapshot();
        assert_eq!(snapshot.connected_gamepad_count, 1);
        assert_eq!(snapshot.pressed_gamepad_button_count, 1);
        assert!(state.record(first_button).is_none());
        assert!(state.record(second_button).is_some());
        assert_eq!(snapshot.diagnostics.released_by_disconnect, 1);
    }

    #[test]
    fn reconciliation_cannot_synthesize_a_pressed_control() {
        let mut state = InputState::default();
        state.apply(SequencedInputEvent {
            sequence: 0,
            event: InputEvent::Edge {
                control: A,
                edge: InputEdge::Down,
                source: InputSource::Reconciliation,
                at: MonotonicMillis::new(0),
            },
        });
        assert_eq!(state.snapshot().pressed_key_count, 0);
        assert_eq!(state.snapshot().diagnostics.invalid_source, 1);
    }
}
