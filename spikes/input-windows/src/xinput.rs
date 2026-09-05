use bongocat_input_queue_spike::{
    LatestValues, LatestValuesDiagnostics, QueueErrorKind, ReliableQueue,
};
use std::collections::{BTreeMap, BTreeSet};

const XINPUT_SLOT_COUNT: u8 = 4;
const BUTTON_THRESHOLD: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct XInputConnection {
    pub device_id: u8,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum XInputButton {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum XInputAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

impl XInputAxis {
    const ALL: [Self; 6] = [
        Self::LeftStickX,
        Self::LeftStickY,
        Self::RightStickX,
        Self::RightStickY,
        Self::LeftTrigger,
        Self::RightTrigger,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct XInputAxisKey {
    pub connection: XInputConnection,
    pub axis: XInputAxis,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct XInputSnapshot {
    pub button_bits: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XInputResetReason {
    QueueOverflow,
    ServiceStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XInputEvent {
    DeviceConnected {
        connection: XInputConnection,
    },
    DeviceDisconnected {
        connection: XInputConnection,
    },
    ButtonDown {
        connection: XInputConnection,
        button: XInputButton,
    },
    ButtonUp {
        connection: XInputConnection,
        button: XInputButton,
    },
    Reset {
        reason: XInputResetReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequencedXInputEvent {
    pub sequence: u64,
    pub event: XInputEvent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XInputProducerDiagnostics {
    pub polls: u64,
    pub query_errors: u64,
    pub connections: u64,
    pub disconnections: u64,
    pub button_down: u64,
    pub button_up: u64,
    pub reliable_overflows: u64,
    pub reliable_discarded: u64,
    pub rejected_after_close: u64,
}

#[derive(Clone, Debug)]
struct ActiveSlot {
    connection: XInputConnection,
    pressed: BTreeSet<XInputButton>,
}

#[derive(Debug)]
pub struct XInputProducer {
    reliable: ReliableQueue<SequencedXInputEvent>,
    axes: LatestValues<XInputAxisKey, f32>,
    slots: BTreeMap<u8, ActiveSlot>,
    next_generation: u64,
    next_sequence: u64,
    diagnostics: XInputProducerDiagnostics,
    closed: bool,
}

impl XInputProducer {
    pub fn new(reliable_capacity: usize) -> Self {
        assert!(
            reliable_capacity >= 2,
            "reliable capacity must hold reset and recovered event"
        );
        Self {
            reliable: ReliableQueue::with_capacity(reliable_capacity),
            axes: LatestValues::with_capacity(
                usize::from(XINPUT_SLOT_COUNT) * XInputAxis::ALL.len(),
            ),
            slots: BTreeMap::new(),
            next_generation: 1,
            next_sequence: 0,
            diagnostics: XInputProducerDiagnostics::default(),
            closed: false,
        }
    }

    pub fn observe_slot(&mut self, slot: u8, snapshot: Option<XInputSnapshot>) {
        assert!(slot < XINPUT_SLOT_COUNT, "XInput slot must be in 0..4");
        if self.closed {
            self.diagnostics.rejected_after_close += 1;
            return;
        }
        self.diagnostics.polls += 1;
        match (self.slots.get(&slot).cloned(), snapshot) {
            (None, None) => {}
            (None, Some(snapshot)) => {
                let connection = XInputConnection {
                    device_id: slot,
                    generation: self.next_generation,
                };
                self.next_generation = self.next_generation.saturating_add(1);
                self.slots.insert(
                    slot,
                    ActiveSlot {
                        connection,
                        pressed: BTreeSet::new(),
                    },
                );
                self.enqueue(XInputEvent::DeviceConnected { connection });
                self.diagnostics.connections += 1;
                self.apply_snapshot(connection, snapshot);
            }
            (Some(active), None) => {
                self.slots.remove(&slot);
                self.axes
                    .discard_where(|key| key.connection == active.connection);
                self.enqueue(XInputEvent::DeviceDisconnected {
                    connection: active.connection,
                });
                self.diagnostics.disconnections += 1;
            }
            (Some(active), Some(snapshot)) => self.apply_snapshot(active.connection, snapshot),
        }
    }

    pub fn record_query_error(&mut self) {
        self.diagnostics.query_errors += 1;
    }

    pub fn drain_events(&mut self) -> Vec<SequencedXInputEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.reliable.pop() {
            events.push(event);
        }
        events
    }

    pub fn drain_axes(&mut self) -> Vec<(XInputAxisKey, f32)> {
        self.axes.drain()
    }

    pub fn active_connections(&self) -> Vec<XInputConnection> {
        self.slots.values().map(|slot| slot.connection).collect()
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        if !self.slots.is_empty() {
            self.enqueue(XInputEvent::Reset {
                reason: XInputResetReason::ServiceStop,
            });
            self.slots.clear();
        }
        self.reliable.close();
        self.axes.close();
        self.closed = true;
    }

    pub fn diagnostics(&self) -> XInputProducerDiagnostics {
        let mut diagnostics = self.diagnostics;
        diagnostics.reliable_overflows = self.reliable.overflow_count();
        diagnostics.reliable_discarded = self.reliable.recovery_discard_count();
        diagnostics
    }

    pub fn axis_diagnostics(&self) -> LatestValuesDiagnostics {
        self.axes.diagnostics()
    }

    pub fn axes_fully_accounted(&self) -> bool {
        self.axes.is_fully_accounted()
    }

    fn apply_snapshot(&mut self, connection: XInputConnection, snapshot: XInputSnapshot) {
        let Some(old_pressed) = self
            .slots
            .get(&connection.device_id)
            .filter(|slot| slot.connection == connection)
            .map(|slot| slot.pressed.clone())
        else {
            return;
        };
        let pressed = pressed_buttons(snapshot);
        for button in old_pressed
            .difference(&pressed)
            .copied()
            .collect::<Vec<_>>()
        {
            self.enqueue(XInputEvent::ButtonUp { connection, button });
            self.diagnostics.button_up += 1;
        }
        for button in pressed
            .difference(&old_pressed)
            .copied()
            .collect::<Vec<_>>()
        {
            self.enqueue(XInputEvent::ButtonDown { connection, button });
            self.diagnostics.button_down += 1;
        }
        if let Some(active) = self.slots.get_mut(&connection.device_id) {
            active.pressed = pressed;
        }

        let axis_values = [
            (XInputAxis::LeftStickX, normalize_thumb(snapshot.left_x)),
            (XInputAxis::LeftStickY, normalize_thumb(snapshot.left_y)),
            (XInputAxis::RightStickX, normalize_thumb(snapshot.right_x)),
            (XInputAxis::RightStickY, normalize_thumb(snapshot.right_y)),
            (
                XInputAxis::LeftTrigger,
                normalize_trigger(snapshot.left_trigger),
            ),
            (
                XInputAxis::RightTrigger,
                normalize_trigger(snapshot.right_trigger),
            ),
        ];
        for (axis, value) in axis_values {
            self.axes
                .replace(XInputAxisKey { connection, axis }, value)
                .expect("four XInput slots cannot exceed the fixed axis key capacity");
        }
    }

    fn enqueue(&mut self, event: XInputEvent) {
        let sequence = self.take_sequence();
        let item = SequencedXInputEvent { sequence, event };
        let reset = SequencedXInputEvent {
            sequence,
            event: XInputEvent::Reset {
                reason: XInputResetReason::QueueOverflow,
            },
        };
        match self.reliable.push_with_overflow_reset(item, reset) {
            Ok(()) => {}
            Err(error) if error.kind == QueueErrorKind::Full => {
                for active in self.slots.values_mut() {
                    active.pressed.clear();
                }
                let recovered = SequencedXInputEvent {
                    sequence: self.take_sequence(),
                    event: error.item.event,
                };
                self.reliable
                    .push(recovered)
                    .expect("reset leaves capacity for the recovered XInput event");
            }
            Err(_) => self.diagnostics.rejected_after_close += 1,
        }
    }

    fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }
}

fn pressed_buttons(snapshot: XInputSnapshot) -> BTreeSet<XInputButton> {
    const BUTTON_BITS: [(XInputButton, u16); 14] = [
        (XInputButton::South, 0x1000),
        (XInputButton::East, 0x2000),
        (XInputButton::West, 0x4000),
        (XInputButton::North, 0x8000),
        (XInputButton::LeftShoulder, 0x0100),
        (XInputButton::RightShoulder, 0x0200),
        (XInputButton::Select, 0x0020),
        (XInputButton::Start, 0x0010),
        (XInputButton::LeftStick, 0x0040),
        (XInputButton::RightStick, 0x0080),
        (XInputButton::DpadUp, 0x0001),
        (XInputButton::DpadDown, 0x0002),
        (XInputButton::DpadLeft, 0x0004),
        (XInputButton::DpadRight, 0x0008),
    ];
    let mut pressed = BUTTON_BITS
        .into_iter()
        .filter_map(|(button, bit)| (snapshot.button_bits & bit != 0).then_some(button))
        .collect::<BTreeSet<_>>();
    if normalize_trigger(snapshot.left_trigger) >= BUTTON_THRESHOLD {
        pressed.insert(XInputButton::LeftTrigger);
    }
    if normalize_trigger(snapshot.right_trigger) >= BUTTON_THRESHOLD {
        pressed.insert(XInputButton::RightTrigger);
    }
    pressed
}

pub fn normalize_thumb(value: i16) -> f32 {
    if value < 0 {
        f32::from(value) / 32768.0
    } else {
        f32::from(value) / 32767.0
    }
}

pub fn normalize_trigger(value: u8) -> f32 {
    f32::from(value) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_preserves_full_signed_range_without_adapter_deadzone() {
        assert_eq!(normalize_thumb(i16::MIN), -1.0);
        assert_eq!(normalize_thumb(0), 0.0);
        assert_eq!(normalize_thumb(i16::MAX), 1.0);
        assert_eq!(normalize_trigger(0), 0.0);
        assert_eq!(normalize_trigger(u8::MAX), 1.0);
    }

    #[test]
    fn polling_generates_connection_button_edges_and_keyed_axes() {
        let mut producer = XInputProducer::new(16);
        producer.observe_slot(
            0,
            Some(XInputSnapshot {
                button_bits: 0x1000,
                left_x: 8192,
                ..XInputSnapshot::default()
            }),
        );
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        let events = producer.drain_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[0].event,
            XInputEvent::DeviceConnected { .. }
        ));
        assert!(matches!(events[1].event, XInputEvent::ButtonDown { .. }));
        assert!(matches!(events[2].event, XInputEvent::ButtonUp { .. }));
        assert_eq!(
            events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(producer.drain_axes().len(), XInputAxis::ALL.len());
        assert!(producer.axes_fully_accounted());
    }

    #[test]
    fn disconnect_discards_axes_and_reconnects_same_slot_with_new_generation() {
        let mut producer = XInputProducer::new(16);
        producer.observe_slot(2, Some(XInputSnapshot::default()));
        let first = producer.active_connections()[0];
        producer.observe_slot(2, None);
        assert!(producer.drain_axes().is_empty());
        producer.observe_slot(2, Some(XInputSnapshot::default()));
        let second = producer.active_connections()[0];
        assert_eq!(first.device_id, second.device_id);
        assert!(second.generation > first.generation);
        assert_eq!(producer.axis_diagnostics().discarded, 6);
    }

    #[test]
    fn axis_flood_coalesces_without_blocking_button_release() {
        let mut producer = XInputProducer::new(4);
        for value in 0..10_000i16 {
            producer.observe_slot(
                0,
                Some(XInputSnapshot {
                    left_x: value,
                    ..XInputSnapshot::default()
                }),
            );
        }
        producer.observe_slot(
            0,
            Some(XInputSnapshot {
                button_bits: 0x1000,
                ..XInputSnapshot::default()
            }),
        );
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        assert_eq!(producer.diagnostics().reliable_overflows, 0);
        let events = producer.drain_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2].event, XInputEvent::ButtonUp { .. }));
        assert!(producer.axis_diagnostics().coalesced > 50_000);
    }

    #[test]
    fn overflow_resets_then_replays_rejected_button_edge() {
        let mut producer = XInputProducer::new(2);
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        producer.observe_slot(
            0,
            Some(XInputSnapshot {
                button_bits: 0x1000,
                ..XInputSnapshot::default()
            }),
        );
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        let events = producer.drain_events();
        assert!(matches!(
            events[0].event,
            XInputEvent::Reset {
                reason: XInputResetReason::QueueOverflow
            }
        ));
        assert!(matches!(events[1].event, XInputEvent::ButtonUp { .. }));
        assert_eq!(producer.diagnostics().reliable_overflows, 1);
        assert_eq!(producer.diagnostics().reliable_discarded, 2);
    }

    #[test]
    fn shutdown_flushes_axes_and_rejects_late_poll() {
        let mut producer = XInputProducer::new(8);
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        producer.close();
        assert!(producer.active_connections().is_empty());
        assert_eq!(producer.drain_axes().len(), XInputAxis::ALL.len());
        assert!(producer.axes_fully_accounted());
        producer.observe_slot(0, Some(XInputSnapshot::default()));
        assert_eq!(producer.diagnostics().rejected_after_close, 1);
    }
}
