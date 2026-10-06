use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// One of the eight keyboard modifiers, with the two sides kept apart.
///
/// This is a closed vocabulary over the Keyboard/Keypad HID usages `0xE0..=0xE7`
/// rather than a bitmask of modifier *families*, because the input contract
/// requires the sides to stay distinct physical keys
/// (`shared/behavior/input-semantics.md`): a configuration that names the right
/// shift has to remain distinguishable from one that names the left shift, all
/// the way from the stored document to the key the overlay watches for.
///
/// The enum is the single spelling of that vocabulary, and it is derived here
/// rather than in the configuration crate because four layers have to agree on
/// it — the stored document, the runtime's overlay settings, the settings
/// protocol and the overlay's own option struct — and a second spelling in any
/// of them would be free to drift.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ModifierKey {
    LeftControl,
    LeftShift,
    LeftAlt,
    LeftMeta,
    RightControl,
    RightShift,
    RightAlt,
    RightMeta,
}

/// Every modifier with its HID usage, in keyboard order.
///
/// This is the one table the whole vocabulary is built from: [`ModifierKey::ALL`]
/// reads it, so a variant and its usage cannot end up in different orders or with
/// one of them extended alone. The order is the HID usage order, which is also
/// left-before-right within each family, so it is also the order the keys sit in
/// on the keyboard.
const MODIFIER_KEYS: [(ModifierKey, u16); 8] = [
    (ModifierKey::LeftControl, 0xe0),
    (ModifierKey::LeftShift, 0xe1),
    (ModifierKey::LeftAlt, 0xe2),
    (ModifierKey::LeftMeta, 0xe3),
    (ModifierKey::RightControl, 0xe4),
    (ModifierKey::RightShift, 0xe5),
    (ModifierKey::RightAlt, 0xe6),
    (ModifierKey::RightMeta, 0xe7),
];

/// The first HID usage of a right-hand modifier.
const RIGHT_SIDE_FIRST_USAGE: u16 = 0xe4;

impl ModifierKey {
    /// Every modifier, in keyboard order. See [`PressedModifiers::first`] for
    /// why that order is load-bearing.
    pub const ALL: [Self; 8] = {
        let mut all = [Self::LeftControl; 8];
        let mut index = 0;
        while index < MODIFIER_KEYS.len() {
            all[index] = MODIFIER_KEYS[index].0;
            index += 1;
        }
        all
    };

    /// The HID usage the platform adapters report this modifier under.
    pub const fn hid_usage(self) -> u16 {
        MODIFIER_KEYS[self.index()].1
    }

    /// The same key as a [`PhysicalKey`], for pressed-set comparisons.
    pub const fn physical_key(self) -> PhysicalKey {
        PhysicalKey::from_hid_usage(self.hid_usage())
    }

    /// Whether this is the right-hand key of its family.
    ///
    /// The HID usage page numbers the eight modifiers left first (`0xE0`..
    /// `0xE3`) and right second, so the usage alone answers this. The table
    /// order agrees with it, and a test pins that the two never drift.
    pub const fn is_right(self) -> bool {
        self.hid_usage() >= RIGHT_SIDE_FIRST_USAGE
    }

    /// This modifier's position in [`Self::ALL`], which is also its bit in
    /// [`PressedModifiers`].
    const fn index(self) -> usize {
        self as usize
    }

    /// The canonical configuration token, in `snake_case`.
    ///
    /// This is the persisted spelling, so it has to stay stable: a document
    /// written by an earlier build has to keep naming the same key.
    pub const fn name(self) -> &'static str {
        match self {
            Self::LeftControl => "left_control",
            Self::LeftShift => "left_shift",
            Self::LeftAlt => "left_alt",
            Self::LeftMeta => "left_meta",
            Self::RightControl => "right_control",
            Self::RightShift => "right_shift",
            Self::RightAlt => "right_alt",
            Self::RightMeta => "right_meta",
        }
    }

    /// The modifier a stored token names.
    ///
    /// A token outside [`Self::ALL`] is refused rather than guessed: a setting
    /// that silently resolved to a different key would suspend the overlay for a
    /// key the user never chose, which is worse than one that does not work.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|modifier| modifier.name().eq_ignore_ascii_case(name))
    }

    /// The modifier a HID usage names, if it is one of the eight.
    pub const fn from_hid_usage(usage: u16) -> Option<Self> {
        let mut index = 0;
        while index < MODIFIER_KEYS.len() {
            if MODIFIER_KEYS[index].1 == usage {
                return Some(MODIFIER_KEYS[index].0);
            }
            index += 1;
        }
        None
    }
}

/// Which keyboard modifiers are held right now.
///
/// The pressed set lives in the runtime, which owns it and clears it on release,
/// reconcile and reset. This is that set's projection onto the modifier
/// vocabulary: a consumer that only needs to ask "is this one key down" reads it
/// instead of taking a copy of the whole pressed set, so the answer cannot fall
/// out of step with the set it came from.
///
/// Bits are positions in [`ModifierKey::ALL`], so [`Self::holds`] is a bit test
/// and the type stays `Copy`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PressedModifiers(u8);

impl PressedModifiers {
    pub const NONE: Self = Self(0);

    pub const fn holds(self, modifier: ModifierKey) -> bool {
        self.0 & (1 << modifier.index()) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Record one held modifier.
    pub fn insert(&mut self, modifier: ModifierKey) {
        self.0 |= 1 << modifier.index();
    }

    /// The held modifier that comes first on the keyboard, or `None` when none
    /// is held.
    ///
    /// A chord of two modifiers has to resolve to one stored key, and keyboard
    /// order is the only tie-break that does not depend on which edge the
    /// observer happened to see first.
    pub const fn first(self) -> Option<ModifierKey> {
        let mut index = 0;
        while index < ModifierKey::ALL.len() {
            if self.0 & (1 << index) != 0 {
                return Some(ModifierKey::ALL[index]);
            }
            index += 1;
        }
        None
    }

    /// Every held modifier, in keyboard order.
    pub fn iter(self) -> impl Iterator<Item = ModifierKey> {
        ModifierKey::ALL
            .into_iter()
            .filter(move |modifier| self.holds(*modifier))
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

    /// The key-image name this button is drawn with: the file stem of
    /// `resources/left-keys/<name>.png` or `resources/right-keys/<name>.png`.
    ///
    /// The name **is** the variant name, on purpose. A backend that names its
    /// own controls cannot leak into a model package, and the type and the
    /// artwork vocabulary cannot drift apart: renaming a variant breaks every
    /// model that ships the old stem, and adding a variant has no name to draw
    /// with until it is given one here.
    ///
    /// These are the product's names, not a backend's. The pre-rewrite Tauri
    /// input layer derived model image names from `format!("{:?}", Button)` of
    /// the third-party gamepad library, and the bundled `gamepad` model still
    /// carried those spellings, which made the shoulder buttons `LeftTrigger`
    /// and `RightTrigger` and the analog triggers `LeftTrigger2` and
    /// `RightTrigger2` — a button and a different button sharing one stem's
    /// meaning. Nothing in the current architecture needs a backend name:
    /// `bongocat-platform` maps every backend control onto this enum, and the
    /// model store rewrites a package's legacy stems on import
    /// (`bongocat-model-store::key_names`).
    pub const fn key_image_name(self) -> &'static str {
        match self {
            Self::South => "South",
            Self::East => "East",
            Self::West => "West",
            Self::North => "North",
            Self::LeftShoulder => "LeftShoulder",
            Self::RightShoulder => "RightShoulder",
            Self::LeftTrigger => "LeftTrigger",
            Self::RightTrigger => "RightTrigger",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::LeftStick => "LeftStick",
            Self::RightStick => "RightStick",
            Self::DpadUp => "DpadUp",
            Self::DpadDown => "DpadDown",
            Self::DpadLeft => "DpadLeft",
            Self::DpadRight => "DpadRight",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_modifier_names_one_distinct_left_and_right_hid_usage() {
        let mut usages = BTreeSet::new();
        for modifier in ModifierKey::ALL {
            let usage = modifier.hid_usage();
            assert!(
                usages.insert(usage),
                "{modifier:?} reuses usage {usage:#06x}"
            );
            assert_eq!(
                ModifierKey::from_hid_usage(usage),
                Some(modifier),
                "{modifier:?} must be the only modifier at usage {usage:#06x}"
            );
            assert_eq!(
                modifier.physical_key().hid_usage(),
                usage,
                "the pressed-set key and the modifier must be one physical key"
            );
        }
        assert_eq!(usages.len(), ModifierKey::ALL.len());
        assert_eq!(ModifierKey::from_hid_usage(0x04), None);
        assert_eq!(ModifierKey::from_hid_usage(0x39), None);
        assert_eq!(ModifierKey::from_hid_usage(GLOBE_KEY_USAGE), None);
    }

    #[test]
    fn the_two_sides_of_a_modifier_are_separate_configuration_values() {
        for (left, right) in [
            (ModifierKey::LeftControl, ModifierKey::RightControl),
            (ModifierKey::LeftShift, ModifierKey::RightShift),
            (ModifierKey::LeftAlt, ModifierKey::RightAlt),
            (ModifierKey::LeftMeta, ModifierKey::RightMeta),
        ] {
            assert_ne!(left.name(), right.name());
            assert_ne!(left.hid_usage(), right.hid_usage());
            assert!(!left.is_right() && right.is_right());
            assert_eq!(ModifierKey::from_name(left.name()), Some(left));
            assert_eq!(ModifierKey::from_name(right.name()), Some(right));
        }
    }

    /// `ModifierKey::ALL` is the order the table lists, the order the HID usage
    /// page numbers the keys in, and the order `PressedModifiers` stores bits in.
    /// `is_right` reads the usage boundary instead of the table, so this is what
    /// stops the two from drifting apart.
    #[test]
    fn the_table_order_is_the_keyboard_order_and_agrees_with_the_side_boundary() {
        assert_eq!(
            ModifierKey::ALL
                .iter()
                .map(|modifier| modifier.hid_usage())
                .collect::<Vec<_>>(),
            vec![0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7]
        );
        assert_eq!(
            ModifierKey::ALL
                .iter()
                .position(|modifier| modifier.is_right()),
            Some(4)
        );
    }

    #[test]
    fn a_stored_token_round_trips_and_anything_else_is_refused() {
        let mut names = BTreeSet::new();
        for modifier in ModifierKey::ALL {
            let name = modifier.name();
            assert!(
                names.insert(name),
                "{name} is claimed by more than one modifier"
            );
            assert_eq!(
                name,
                snake_case(&format!("{modifier:?}")),
                "the persisted token is the variant name in snake_case, so a renamed \
                 variant cannot leave an old document naming a key that no longer exists"
            );
            assert_eq!(ModifierKey::from_name(name), Some(modifier));
            assert_eq!(
                ModifierKey::from_name(&name.to_ascii_uppercase()),
                Some(modifier),
                "the persisted token is read case-insensitively"
            );
        }
        assert_eq!(names.len(), ModifierKey::ALL.len());
        for refused in ["", " ", "shift", "left", "left_shift_left", "0xe1"] {
            assert_eq!(
                ModifierKey::from_name(refused),
                None,
                "{refused:?} must not resolve to a modifier"
            );
        }
    }

    fn snake_case(variant: &str) -> String {
        variant
            .chars()
            .flat_map(|character| {
                if character.is_ascii_uppercase() {
                    vec!['_', character.to_ascii_lowercase()]
                } else {
                    vec![character]
                }
            })
            .skip(1)
            .collect()
    }

    #[test]
    fn the_pressed_set_projection_answers_per_modifier_and_keeps_the_sides_apart() {
        let mut pressed = PressedModifiers::default();
        assert!(pressed.is_empty());
        assert_eq!(pressed.first(), None);
        assert_eq!(pressed.iter().count(), 0);

        pressed.insert(ModifierKey::RightShift);
        assert!(pressed.holds(ModifierKey::RightShift));
        assert!(
            !pressed.holds(ModifierKey::LeftShift),
            "the right shift must not answer for the left one"
        );
        assert!(!pressed.is_empty());

        // Inserting twice is what a repeated auto-repeat edge would do, and it
        // must not change the answer.
        pressed.insert(ModifierKey::RightShift);
        assert_eq!(pressed.first(), Some(ModifierKey::RightShift));
        assert_eq!(
            pressed.iter().collect::<Vec<_>>(),
            vec![ModifierKey::RightShift]
        );

        pressed.insert(ModifierKey::LeftControl);
        pressed.insert(ModifierKey::LeftAlt);
        assert_eq!(
            pressed.iter().collect::<Vec<_>>(),
            vec![
                ModifierKey::LeftControl,
                ModifierKey::LeftAlt,
                ModifierKey::RightShift
            ],
            "held modifiers read back in keyboard order"
        );
        assert_eq!(
            pressed.first(),
            Some(ModifierKey::LeftControl),
            "a chord of modifiers resolves to the leftmost key, not to whichever edge was seen first"
        );

        let all =
            ModifierKey::ALL
                .into_iter()
                .fold(PressedModifiers::NONE, |mut pressed, modifier| {
                    pressed.insert(modifier);
                    pressed
                });
        assert_eq!(all.iter().count(), ModifierKey::ALL.len());
        assert!(ModifierKey::ALL.iter().all(|modifier| all.holds(*modifier)));
    }

    /// The key-image vocabulary is a contract with model authors, so it has to
    /// satisfy the same rules the keyboard table does: every button named
    /// exactly once, no name carrying two meanings, and the name spelled the
    /// same way as the variant it belongs to.
    #[test]
    fn every_gamepad_button_has_exactly_one_canonical_image_name() {
        let mut names = BTreeSet::new();
        for button in GamepadButton::ALL {
            let name = button.key_image_name();
            assert_eq!(
                name,
                format!("{button:?}"),
                "the image stem must be the variant name"
            );
            assert!(
                names.insert(name),
                "{name} is claimed by more than one button"
            );
            assert!(
                name.bytes().all(|byte| byte.is_ascii_alphanumeric()),
                "{name} is not a portable file stem"
            );
        }
        assert_eq!(names.len(), GamepadButton::ALL.len());
    }

    /// `GamepadButton::ALL` is what the model store's vocabulary table, the
    /// binding table and the renderer's candidate list are all written against,
    /// so a button that is missing from it is a button the product cannot draw.
    #[test]
    fn all_is_exhaustive_against_the_public_vocabulary() {
        assert_eq!(GamepadButton::ALL.len(), 16);
        for button in [
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::West,
            GamepadButton::North,
            GamepadButton::LeftShoulder,
            GamepadButton::RightShoulder,
            GamepadButton::LeftTrigger,
            GamepadButton::RightTrigger,
            GamepadButton::Select,
            GamepadButton::Start,
            GamepadButton::LeftStick,
            GamepadButton::RightStick,
            GamepadButton::DpadUp,
            GamepadButton::DpadDown,
            GamepadButton::DpadLeft,
            GamepadButton::DpadRight,
        ] {
            assert!(
                GamepadButton::ALL.contains(&button),
                "{button:?} is missing from GamepadButton::ALL"
            );
        }
    }
}
