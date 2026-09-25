use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

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

/// HID usage of the Apple keyboard's Fn / globe key.
///
/// The key lives on Apple's vendor-defined HID page. It is folded into the
/// same `u16` vocabulary as Keyboard/Keypad usages so platform adapters do not
/// need a second physical-key type.
pub const GLOBE_KEY_USAGE: u16 = 0xff03;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalKey(u16);

impl PhysicalKey {
    pub const KEY_A: Self = Self(0x04);
    pub const LEFT_CONTROL: Self = Self(0xe0);
    pub const LEFT_ALT: Self = Self(0xe2);
    /// The Apple Fn / globe key, which lives on Apple's vendor-defined HID page
    /// rather than on the Keyboard/Keypad page; see
    /// [`GLOBE_KEY_USAGE`] for why the value is `0xff03` and why the name is
    /// `Globe` rather than `Fn`.
    pub const GLOBE: Self = Self(GLOBE_KEY_USAGE);

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
    gamepad_hands: BTreeMap<GamepadButton, HandSide>,
}

impl InputBindings {
    pub fn new(key_hands: BTreeMap<PhysicalKey, HandSide>) -> Self {
        Self::with_gamepad_hands(key_hands, BTreeMap::new())
    }

    pub fn with_gamepad_hands(
        key_hands: BTreeMap<PhysicalKey, HandSide>,
        gamepad_hands: BTreeMap<GamepadButton, HandSide>,
    ) -> Self {
        Self {
            key_hands,
            gamepad_hands,
        }
    }

    pub fn hand_for(&self, key: PhysicalKey) -> Option<HandSide> {
        self.key_hands.get(&key).copied()
    }

    pub fn hand_for_gamepad(&self, button: GamepadButton) -> Option<HandSide> {
        self.gamepad_hands.get(&button).copied()
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
    GamepadConnected {
        connection: GamepadConnection,
        at: MonotonicMillis,
    },
    GamepadDisconnected {
        connection: GamepadConnection,
        at: MonotonicMillis,
    },
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
    pub const fn at(&self) -> MonotonicMillis {
        match self {
            Self::GamepadConnected { at, .. }
            | Self::GamepadDisconnected { at, .. }
            | Self::Edge { at, .. }
            | Self::Reconcile { at, .. }
            | Self::Reset { at, .. } => *at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInputEvent {
    pub sequence: u64,
    pub event: InputEvent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDiagnostics {
    pub captured_down: u64,
    pub captured_up: u64,
    pub reconciled_release: u64,
    pub fallback_release: u64,
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
    pub gamepad_connections: u64,
    pub gamepad_disconnections: u64,
    pub stale_gamepad_events: u64,
    pub released_by_disconnect: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputTransportDiagnostics {
    pub enqueued: u64,
    pub queue_full: u64,
    pub recovered_after_overflow: u64,
    pub runtime_stopped: u64,
}

/// The result of handing a sequenced input event to the runtime command
/// transport. The input crate deliberately does not know whether the consumer
/// is a worker, a test sink, or another product adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputSubmitError {
    QueueFull,
    RuntimeStopped,
}

/// A typed hand-off from a platform input adapter to the single runtime owner.
///
/// Implementations must preserve the event order and the sequence carried by
/// [`SequencedInputEvent`]. The input producer adds the reliable-edge sequence
/// and transport accounting before invoking this trait.
pub trait InputSubmitter: Send + Sync {
    fn submit(&self, event: SequencedInputEvent) -> Result<(), InputSubmitError>;
}

#[derive(Debug, Default)]
struct InputProducerState {
    next_sequence: u64,
    recovery_pending: bool,
}

#[derive(Clone)]
pub struct InputProducer {
    submitter: std::sync::Arc<dyn InputSubmitter>,
    state: std::sync::Arc<std::sync::Mutex<InputProducerState>>,
    transport: std::sync::Arc<InputTransportCounters>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InputPublishError {
    #[error("runtime input queue is full")]
    QueueFull(InputEvent),
    #[error("runtime is stopped")]
    RuntimeStopped(InputEvent),
}

impl InputProducer {
    pub fn new(submitter: std::sync::Arc<dyn InputSubmitter>) -> Self {
        Self {
            submitter,
            state: std::sync::Arc::new(std::sync::Mutex::new(InputProducerState::default())),
            transport: std::sync::Arc::new(InputTransportCounters::default()),
        }
    }

    pub fn publish(&self, event: InputEvent) -> Result<u64, InputPublishError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input_sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.wrapping_add(1);
        let envelope = SequencedInputEvent {
            sequence: input_sequence,
            event: event.clone(),
        };
        match self.submitter.submit(envelope) {
            Ok(()) => {
                self.transport.enqueued();
                if state.recovery_pending {
                    state.recovery_pending = false;
                    self.transport.recovered_after_overflow();
                }
                Ok(input_sequence)
            }
            Err(InputSubmitError::QueueFull) => {
                state.recovery_pending = true;
                self.transport.queue_full();
                Err(InputPublishError::QueueFull(event))
            }
            Err(InputSubmitError::RuntimeStopped) => {
                self.transport.runtime_stopped();
                Err(InputPublishError::RuntimeStopped(event))
            }
        }
    }

    pub fn recover(
        &self,
        reason: InputResetReason,
        at: MonotonicMillis,
    ) -> Result<u64, InputPublishError> {
        self.publish(InputEvent::Reset { reason, at })
    }

    pub fn diagnostics(&self) -> InputTransportDiagnostics {
        self.transport.snapshot()
    }
}

#[derive(Debug, Default)]
struct InputTransportCounters {
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
