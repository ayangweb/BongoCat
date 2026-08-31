use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::NormalizedCursorPosition;

pub const DEFAULT_MISSING_CONFIRMATIONS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonotonicMillis(u64);

impl MonotonicMillis {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalKey(u16);

impl PhysicalKey {
    pub const KEY_A: Self = Self(0x04);
    pub const LEFT_CONTROL: Self = Self(0xe0);
    pub const LEFT_ALT: Self = Self(0xe2);

    pub const fn from_hid_usage(usage: u16) -> Self {
        Self(usage)
    }

    pub const fn hid_usage(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GamepadConnection {
    pub device_id: u8,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GamepadButton {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftStick,
    RightStick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl GamepadButton {
    pub const ALL: [Self; 16] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Select,
        Self::Start,
        Self::LeftStick,
        Self::RightStick,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GamepadButtonKey {
    pub connection: GamepadConnection,
    pub button: GamepadButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

impl GamepadAxis {
    pub const ALL: [Self; 6] = [
        Self::LeftStickX,
        Self::LeftStickY,
        Self::RightStickX,
        Self::RightStickY,
        Self::LeftTrigger,
        Self::RightTrigger,
    ];

    pub const fn is_trigger(self) -> bool {
        matches!(self, Self::LeftTrigger | Self::RightTrigger)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GamepadAxisKey {
    pub connection: GamepadConnection,
    pub axis: GamepadAxis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputControl {
    Key(PhysicalKey),
    Mouse(MouseButton),
    Gamepad(GamepadButtonKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandSide {
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputBindings {
    key_hands: BTreeMap<PhysicalKey, HandSide>,
}

impl InputBindings {
    pub fn new(key_hands: BTreeMap<PhysicalKey, HandSide>) -> Self {
        Self { key_hands }
    }

    pub fn hand_for(&self, key: PhysicalKey) -> Option<HandSide> {
        self.key_hands.get(&key).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEdge {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    Capture,
    Reconciliation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputResetReason {
    SessionLock,
    Sleep,
    DeviceRemoved,
    ServiceRestart,
    QueueOverflow,
    PermissionChanged,
    SequenceGap,
    NonMonotonicTime,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Edge {
        control: InputControl,
        edge: InputEdge,
        source: InputSource,
        at: MonotonicMillis,
    },
    Reconcile {
        pressed: BTreeSet<InputControl>,
        at: MonotonicMillis,
    },
    Reset {
        reason: InputResetReason,
        at: MonotonicMillis,
    },
}

impl InputEvent {
    const fn at(&self) -> MonotonicMillis {
        match self {
            Self::Edge { at, .. } | Self::Reconcile { at, .. } | Self::Reset { at, .. } => *at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInputEvent {
    pub sequence: u64,
    pub event: InputEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconciliationPolicy {
    missing_confirmations: u8,
}

impl ReconciliationPolicy {
    pub const fn new(missing_confirmations: u8) -> Option<Self> {
        if missing_confirmations == 0 {
            None
        } else {
            Some(Self {
                missing_confirmations,
            })
        }
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDiagnostics {
    pub captured_down: u64,
    pub captured_up: u64,
    pub reconciled_release: u64,
    pub released_by_reset: u64,
    pub duplicate_down: u64,
    pub unmatched_release: u64,
    pub invalid_source: u64,
    pub reset_count: u64,
    pub sequence_gap_count: u64,
    pub missing_sequence_count: u64,
    pub duplicate_sequence_count: u64,
    pub out_of_order_sequence_count: u64,
    pub non_monotonic_time_count: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputTransportDiagnostics {
    pub enqueued: u64,
    pub queue_full: u64,
    pub recovered_after_overflow: u64,
    pub runtime_stopped: u64,
}

#[derive(Debug, Default)]
pub(crate) struct InputTransportCounters {
    enqueued: AtomicU64,
    queue_full: AtomicU64,
    recovered_after_overflow: AtomicU64,
    runtime_stopped: AtomicU64,
}

impl InputTransportCounters {
    pub(crate) fn enqueued(&self) {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn queue_full(&self) {
        self.queue_full.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn recovered_after_overflow(&self) {
        self.recovered_after_overflow
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn runtime_stopped(&self) {
        self.runtime_stopped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> InputTransportDiagnostics {
        InputTransportDiagnostics {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            recovered_after_overflow: self.recovered_after_overflow.load(Ordering::Relaxed),
            runtime_stopped: self.runtime_stopped.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    pub pressed_key_count: usize,
    pub pressed_mouse_button_count: usize,
    pub pressed_gamepad_button_count: usize,
    pub last_reset_reason: Option<InputResetReason>,
    pub last_input_sequence: Option<u64>,
    pub diagnostics: InputDiagnostics,
    pub transport: InputTransportDiagnostics,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelInputSnapshot {
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
pub enum InputDisposition {
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
}

#[derive(Debug, Default)]
pub(crate) struct InputState {
    pressed: BTreeMap<InputControl, PressedRecord>,
    missing_confirmations: BTreeMap<InputControl, u8>,
    policy: ReconciliationPolicy,
    diagnostics: InputDiagnostics,
    last_sequence: Option<u64>,
    last_timestamp: Option<MonotonicMillis>,
    last_reset_reason: Option<InputResetReason>,
}

impl InputState {
    pub(crate) fn apply(&mut self, envelope: SequencedInputEvent) -> InputDisposition {
        let gap = if let Some(last_sequence) = self.last_sequence {
            if envelope.sequence == last_sequence {
                self.diagnostics.duplicate_sequence_count =
                    self.diagnostics.duplicate_sequence_count.saturating_add(1);
                return InputDisposition::DuplicateSequence;
            }
            if envelope.sequence < last_sequence {
                self.diagnostics.out_of_order_sequence_count = self
                    .diagnostics
                    .out_of_order_sequence_count
                    .saturating_add(1);
                return InputDisposition::OutOfOrderSequence;
            }
            envelope.sequence - last_sequence - 1
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
                self.apply_event(envelope.event);
            } else {
                self.reset(InputResetReason::SequenceGap);
                self.apply_event(envelope.event);
            }
            return InputDisposition::AppliedAfterSequenceGap { missing: gap };
        }
        self.apply_event(envelope.event);
        InputDisposition::Applied
    }

    pub(crate) fn force_reset(&mut self, reason: InputResetReason) {
        self.reset(reason);
    }

    pub(crate) fn snapshot(&self) -> InputSnapshot {
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
            last_reset_reason: self.last_reset_reason,
            last_input_sequence: self.last_sequence,
            diagnostics: self.diagnostics,
            transport: InputTransportDiagnostics::default(),
        }
    }

    pub(crate) fn model_snapshot(
        &self,
        bindings: &InputBindings,
        cursor: NormalizedCursorPosition,
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
        for control in self.pressed.keys() {
            match control {
                InputControl::Key(key) => match bindings.hand_for(*key) {
                    Some(HandSide::Left) => snapshot.left_hand_down = true,
                    Some(HandSide::Right) => snapshot.right_hand_down = true,
                    None => {}
                },
                InputControl::Gamepad(button) => match button.button {
                    GamepadButton::LeftStick => snapshot.stick_left_down = true,
                    GamepadButton::RightStick => snapshot.stick_right_down = true,
                    _ => {}
                },
                InputControl::Mouse(_) => {}
            }
        }
        snapshot
    }

    #[cfg(test)]
    fn record(&self, control: InputControl) -> Option<PressedRecord> {
        self.pressed.get(&control).copied()
    }

    fn apply_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Edge {
                control,
                edge,
                source,
                at,
            } => match edge {
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
                            });
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
                        }
                        InputSource::Reconciliation => {
                            if released {
                                self.diagnostics.reconciled_release =
                                    self.diagnostics.reconciled_release.saturating_add(1);
                            }
                        }
                    }
                }
            },
            InputEvent::Reconcile { pressed, at } => {
                let controls = self.pressed.keys().copied().collect::<Vec<_>>();
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
        self.missing_confirmations.clear();
        self.last_reset_reason = Some(reason);
        self.diagnostics.reset_count = self.diagnostics.reset_count.saturating_add(1);
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
            })
        );
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
        state.apply(edge(0, 0, left_stick, InputEdge::Down));
        state.apply(edge(1, 1, right_stick, InputEdge::Down));
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_gamepad_button_count, 2);
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
        assert_eq!(
            state.model_snapshot(
                &InputBindings::default(),
                NormalizedCursorPosition::default()
            ),
            ModelInputSnapshot::default()
        );
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
