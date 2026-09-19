use bongocat_input_queue_spike::{
    LatestValues, LatestValuesDiagnostics, LatestValuesErrorKind, QueueErrorKind, ReliableQueue,
};
use std::collections::{BTreeMap, BTreeSet};

const BUTTON_PRESS_THRESHOLD: f32 = 0.5;
const AXES_PER_CONTROLLER: usize = GamepadAxis::ALL.len();

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

    fn is_trigger(self) -> bool {
        matches!(self, Self::LeftTrigger | Self::RightTrigger)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GamepadAxisKey {
    pub connection: GamepadConnection,
    pub axis: GamepadAxis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MacExtendedGamepadSnapshot {
    buttons: [f32; GamepadButton::ALL.len()],
    axes: [f32; AXES_PER_CONTROLLER],
}

impl Default for MacExtendedGamepadSnapshot {
    fn default() -> Self {
        Self {
            buttons: [0.0; GamepadButton::ALL.len()],
            axes: [0.0; AXES_PER_CONTROLLER],
        }
    }
}

impl MacExtendedGamepadSnapshot {
    pub fn with_button(mut self, button: GamepadButton, value: f32) -> Self {
        self.buttons[button as usize] = value;
        self
    }

    pub fn with_axis(mut self, axis: GamepadAxis, value: f32) -> Self {
        self.axes[axis as usize] = value;
        self
    }

    fn button_value(&self, button: GamepadButton) -> f32 {
        self.buttons[button as usize]
    }

    fn axis_value(&self, axis: GamepadAxis) -> f32 {
        self.axes[axis as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamepadResetReason {
    QueueOverflow,
    ServiceStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamepadEvent {
    DeviceConnected {
        connection: GamepadConnection,
    },
    DeviceDisconnected {
        connection: GamepadConnection,
    },
    ButtonDown {
        connection: GamepadConnection,
        button: GamepadButton,
    },
    ButtonUp {
        connection: GamepadConnection,
        button: GamepadButton,
    },
    Reset {
        reason: GamepadResetReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequencedGamepadEvent {
    pub sequence: u64,
    pub event: GamepadEvent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadProducerDiagnostics {
    pub connections: u64,
    pub disconnections: u64,
    pub button_down: u64,
    pub button_up: u64,
    pub stale_callbacks: u64,
    pub invalid_values: u64,
    pub reliable_overflows: u64,
    pub reliable_discarded: u64,
    pub rejected_after_close: u64,
}

#[derive(Clone, Debug, Default)]
struct ActiveGamepad {
    pressed: BTreeSet<GamepadButton>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GamepadProducerError {
    AlreadyConnected,
    CapacityExceeded,
    UnknownOrStaleConnection,
    Closed,
}

#[derive(Debug)]
pub struct MacGamepadProducer {
    max_controllers: usize,
    reliable: ReliableQueue<SequencedGamepadEvent>,
    axes: LatestValues<GamepadAxisKey, f32>,
    active: BTreeMap<GamepadConnection, ActiveGamepad>,
    next_generation: u64,
    next_sequence: u64,
    diagnostics: GamepadProducerDiagnostics,
    closed: bool,
}

impl MacGamepadProducer {
    pub fn new(max_controllers: usize, reliable_capacity: usize) -> Self {
        assert!(max_controllers > 0, "gamepad capacity must be positive");
        assert!(
            reliable_capacity >= 2,
            "reliable capacity must hold reset and recovered event"
        );
        Self {
            max_controllers,
            reliable: ReliableQueue::with_capacity(reliable_capacity),
            axes: LatestValues::with_capacity(max_controllers * AXES_PER_CONTROLLER),
            active: BTreeMap::new(),
            next_generation: 1,
            next_sequence: 0,
            diagnostics: GamepadProducerDiagnostics::default(),
            closed: false,
        }
    }

    pub fn connect(&mut self, device_id: u8) -> Result<GamepadConnection, GamepadProducerError> {
        if self.closed {
            self.diagnostics.rejected_after_close += 1;
            return Err(GamepadProducerError::Closed);
        }
        if self.active.keys().any(|key| key.device_id == device_id) {
            return Err(GamepadProducerError::AlreadyConnected);
        }
        if self.active.len() == self.max_controllers {
            return Err(GamepadProducerError::CapacityExceeded);
        }
        let connection = GamepadConnection {
            device_id,
            generation: self.next_generation,
        };
        self.next_generation = self.next_generation.saturating_add(1);
        self.active.insert(connection, ActiveGamepad::default());
        self.enqueue(GamepadEvent::DeviceConnected { connection })?;
        self.diagnostics.connections += 1;
        Ok(connection)
    }

    pub fn disconnect(
        &mut self,
        connection: GamepadConnection,
    ) -> Result<(), GamepadProducerError> {
        if self.closed {
            self.diagnostics.rejected_after_close += 1;
            return Err(GamepadProducerError::Closed);
        }
        if self.active.remove(&connection).is_none() {
            self.diagnostics.stale_callbacks += 1;
            return Err(GamepadProducerError::UnknownOrStaleConnection);
        }
        self.axes.discard_where(|key| key.connection == connection);
        self.enqueue(GamepadEvent::DeviceDisconnected { connection })?;
        self.diagnostics.disconnections += 1;
        Ok(())
    }

    pub fn publish_snapshot(
        &mut self,
        connection: GamepadConnection,
        snapshot: MacExtendedGamepadSnapshot,
    ) -> Result<(), GamepadProducerError> {
        if self.closed {
            self.diagnostics.rejected_after_close += 1;
            return Err(GamepadProducerError::Closed);
        }
        let Some(old_pressed) = self
            .active
            .get(&connection)
            .map(|active| active.pressed.clone())
        else {
            self.diagnostics.stale_callbacks += 1;
            return Err(GamepadProducerError::UnknownOrStaleConnection);
        };

        let mut pressed = BTreeSet::new();
        for button in GamepadButton::ALL {
            let value = self.normalize(snapshot.button_value(button), false);
            if value >= BUTTON_PRESS_THRESHOLD {
                pressed.insert(button);
            }
        }
        for button in old_pressed
            .difference(&pressed)
            .copied()
            .collect::<Vec<_>>()
        {
            self.enqueue(GamepadEvent::ButtonUp { connection, button })?;
            self.diagnostics.button_up += 1;
        }
        for button in pressed
            .difference(&old_pressed)
            .copied()
            .collect::<Vec<_>>()
        {
            self.enqueue(GamepadEvent::ButtonDown { connection, button })?;
            self.diagnostics.button_down += 1;
        }
        if let Some(active) = self.active.get_mut(&connection) {
            active.pressed = pressed;
        }

        for axis in GamepadAxis::ALL {
            let value = self.normalize(snapshot.axis_value(axis), axis.is_trigger());
            let key = GamepadAxisKey { connection, axis };
            if let Err(error) = self.axes.replace(key, value) {
                match error.kind {
                    LatestValuesErrorKind::Closed => {
                        self.diagnostics.rejected_after_close += 1;
                        return Err(GamepadProducerError::Closed);
                    }
                    LatestValuesErrorKind::CapacityExceeded => {
                        return Err(GamepadProducerError::CapacityExceeded);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn drain_events(&mut self) -> Vec<SequencedGamepadEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.reliable.pop() {
            events.push(event);
        }
        events
    }

    pub fn drain_axes(&mut self) -> Vec<(GamepadAxisKey, f32)> {
        self.axes.drain()
    }

    pub fn active_connections(&self) -> Vec<GamepadConnection> {
        self.active.keys().copied().collect()
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        if !self.active.is_empty() {
            let _ = self.enqueue(GamepadEvent::Reset {
                reason: GamepadResetReason::ServiceStop,
            });
            self.active.clear();
        }
        self.reliable.close();
        self.axes.close();
        self.closed = true;
    }

    pub fn diagnostics(&self) -> GamepadProducerDiagnostics {
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

    fn normalize(&mut self, value: f32, trigger: bool) -> f32 {
        if !value.is_finite() {
            self.diagnostics.invalid_values += 1;
            return 0.0;
        }
        if trigger {
            value.clamp(0.0, 1.0)
        } else {
            value.clamp(-1.0, 1.0)
        }
    }

    fn enqueue(&mut self, event: GamepadEvent) -> Result<(), GamepadProducerError> {
        let sequence = self.take_sequence();
        let item = SequencedGamepadEvent { sequence, event };
        let reset = SequencedGamepadEvent {
            sequence,
            event: GamepadEvent::Reset {
                reason: GamepadResetReason::QueueOverflow,
            },
        };
        match self.reliable.push_with_overflow_reset(item, reset) {
            Ok(()) => Ok(()),
            Err(error) if error.kind == QueueErrorKind::Full => {
                for active in self.active.values_mut() {
                    active.pressed.clear();
                }
                let recovered = SequencedGamepadEvent {
                    sequence: self.take_sequence(),
                    event: error.item.event,
                };
                self.reliable
                    .push(recovered)
                    .map_err(|_| GamepadProducerError::Closed)
            }
            Err(_) => {
                self.diagnostics.rejected_after_close += 1;
                Err(GamepadProducerError::Closed)
            }
        }
    }

    fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacGameControllerProbeReport {
    pub started: bool,
    pub background_monitoring_enabled: bool,
    pub background_monitoring_restored: bool,
    pub enumerations: u64,
    pub observed_controllers: u64,
    pub unsupported_profiles: u64,
    pub callback_panics: u64,
    pub reliable_events: u64,
    pub axis_samples: u64,
    pub clean_shutdown: bool,
    pub producer: GamepadProducerDiagnostics,
    pub axes: LatestValuesDiagnostics,
}

#[cfg(target_os = "macos")]
struct AttachedController {
    connection: GamepadConnection,
    profile: objc2::rc::Retained<objc2_game_controller::GCExtendedGamepad>,
}

#[cfg(target_os = "macos")]
impl AttachedController {
    fn clear_handler(&self) {
        // SAFETY: the profile is retained by this owner, and null removes the
        // copied Objective-C block before the pure Rust sink can be closed.
        unsafe { self.profile.setValueChangedHandler(std::ptr::null_mut()) };
    }
}

#[cfg(target_os = "macos")]
pub fn run_gamecontroller_probe(duration: std::time::Duration) -> MacGameControllerProbeReport {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_game_controller::{GCController, GCControllerElement, GCExtendedGamepad};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ptr::NonNull;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    use std::time::{Duration, Instant};

    let producer = Arc::new(Mutex::new(MacGamepadProducer::new(4, 256)));
    // SAFETY: these class methods only read and update GameController's
    // process-global delivery policy. This owner restores the original value
    // after all handlers are removed.
    let original_background_monitoring = unsafe { GCController::shouldMonitorBackgroundEvents() };
    unsafe { GCController::setShouldMonitorBackgroundEvents(true) };
    let background_monitoring_enabled = unsafe { GCController::shouldMonitorBackgroundEvents() };
    let callback_panics = Arc::new(AtomicU64::new(0));
    let mut attached = BTreeMap::<usize, AttachedController>::new();
    let mut free_slots = BTreeSet::from([0u8, 1, 2, 3]);
    let mut enumerations = 0u64;
    let mut observed_controllers = 0u64;
    let mut unsupported_profiles = 0u64;
    let mut reliable_events = 0u64;
    let mut axis_samples = 0u64;
    let deadline = Instant::now() + duration;

    while Instant::now() < deadline {
        enumerations += 1;
        // SAFETY: GameController returns an owned immutable NSArray. The
        // retained controllers remain valid through this reconciliation pass.
        let controllers = unsafe { GCController::controllers() }.to_vec();
        observed_controllers = observed_controllers.max(controllers.len() as u64);
        let mut present = BTreeSet::new();
        for controller in controllers {
            let identity = Retained::as_ptr(&controller) as usize;
            present.insert(identity);
            if attached.contains_key(&identity) {
                continue;
            }
            // SAFETY: the retained controller is valid and the method returns
            // an independently retained profile when supported.
            let Some(profile) = (unsafe { controller.extendedGamepad() }) else {
                unsupported_profiles += 1;
                continue;
            };
            let Some(device_id) = free_slots.pop_first() else {
                unsupported_profiles += 1;
                continue;
            };
            let connection = match producer
                .lock()
                .expect("gamepad producer poisoned")
                .connect(device_id)
            {
                Ok(connection) => connection,
                Err(_) => {
                    free_slots.insert(device_id);
                    continue;
                }
            };
            let callback_producer = Arc::clone(&producer);
            let callback_panics_counter = Arc::clone(&callback_panics);
            let block: RcBlock<dyn Fn(NonNull<GCExtendedGamepad>, NonNull<GCControllerElement>)> =
                RcBlock::new(
                    move |profile: NonNull<GCExtendedGamepad>,
                          _element: NonNull<GCControllerElement>| {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            // SAFETY: GameController supplies both callback pointers
                            // for the duration of this copied block invocation.
                            let snapshot = unsafe { snapshot_extended_gamepad(profile.as_ref()) };
                            let _ = callback_producer
                                .lock()
                                .expect("gamepad producer poisoned")
                                .publish_snapshot(connection, snapshot);
                        }));
                        if result.is_err() {
                            callback_panics_counter.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                );
            // SAFETY: the profile copies the block. Its closure captures only
            // a pure Rust Arc sink and a value connection key, not ObjC owners.
            unsafe { profile.setValueChangedHandler(RcBlock::as_ptr(&block)) };
            // Seed current state so buttons already held at attach time are not
            // missed while waiting for a value-change callback.
            let snapshot = unsafe { snapshot_extended_gamepad(&profile) };
            let _ = producer
                .lock()
                .expect("gamepad producer poisoned")
                .publish_snapshot(connection, snapshot);
            attached.insert(
                identity,
                AttachedController {
                    connection,
                    profile,
                },
            );
        }

        let removed = attached
            .keys()
            .copied()
            .filter(|identity| !present.contains(identity))
            .collect::<Vec<_>>();
        for identity in removed {
            if let Some(controller) = attached.remove(&identity) {
                controller.clear_handler();
                let _ = producer
                    .lock()
                    .expect("gamepad producer poisoned")
                    .disconnect(controller.connection);
                free_slots.insert(controller.connection.device_id);
            }
        }

        use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
        let remaining = deadline.saturating_duration_since(Instant::now());
        // SAFETY: the default mode is a process-owned CoreFoundation constant.
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            remaining.min(Duration::from_millis(20)),
            true,
        );
        let mut producer = producer.lock().expect("gamepad producer poisoned");
        reliable_events += producer.drain_events().len() as u64;
        axis_samples += producer.drain_axes().len() as u64;
    }

    for (_, controller) in attached {
        controller.clear_handler();
        let _ = producer
            .lock()
            .expect("gamepad producer poisoned")
            .disconnect(controller.connection);
    }
    // SAFETY: all value-change handlers have been cleared, so restoring the
    // process-global delivery policy cannot race a live producer callback.
    unsafe {
        GCController::setShouldMonitorBackgroundEvents(original_background_monitoring);
    }
    let background_monitoring_restored =
        unsafe { GCController::shouldMonitorBackgroundEvents() } == original_background_monitoring;
    let mut producer = producer.lock().expect("gamepad producer poisoned");
    producer.close();
    reliable_events += producer.drain_events().len() as u64;
    axis_samples += producer.drain_axes().len() as u64;
    let producer_diagnostics = producer.diagnostics();
    let axis_diagnostics = producer.axis_diagnostics();
    let clean_shutdown = producer.active_connections().is_empty()
        && producer.axes_fully_accounted()
        && producer_diagnostics.rejected_after_close == 0
        && background_monitoring_restored;
    MacGameControllerProbeReport {
        started: true,
        background_monitoring_enabled,
        background_monitoring_restored,
        enumerations,
        observed_controllers,
        unsupported_profiles,
        callback_panics: callback_panics.load(Ordering::Relaxed),
        reliable_events,
        axis_samples,
        clean_shutdown,
        producer: producer_diagnostics,
        axes: axis_diagnostics,
    }
}

#[cfg(target_os = "macos")]
unsafe fn snapshot_extended_gamepad(
    profile: &objc2_game_controller::GCExtendedGamepad,
) -> MacExtendedGamepadSnapshot {
    use objc2_game_controller::{GCControllerButtonInput, GCExtendedGamepad};

    unsafe fn button(
        profile: &GCExtendedGamepad,
        getter: unsafe fn(&GCExtendedGamepad) -> objc2::rc::Retained<GCControllerButtonInput>,
    ) -> f32 {
        // SAFETY: the caller supplies a getter for this retained profile.
        unsafe { getter(profile).value() }
    }

    // SAFETY: all getters belong to a retained extended profile. Returned
    // elements are retained for each immediate scalar read.
    unsafe {
        let left = profile.leftThumbstick();
        let right = profile.rightThumbstick();
        let dpad = profile.dpad();
        let left_trigger = profile.leftTrigger().value();
        let right_trigger = profile.rightTrigger().value();
        let mut snapshot = MacExtendedGamepadSnapshot::default()
            .with_button(
                GamepadButton::South,
                button(profile, GCExtendedGamepad::buttonA),
            )
            .with_button(
                GamepadButton::East,
                button(profile, GCExtendedGamepad::buttonB),
            )
            .with_button(
                GamepadButton::West,
                button(profile, GCExtendedGamepad::buttonX),
            )
            .with_button(
                GamepadButton::North,
                button(profile, GCExtendedGamepad::buttonY),
            )
            .with_button(
                GamepadButton::LeftShoulder,
                button(profile, GCExtendedGamepad::leftShoulder),
            )
            .with_button(
                GamepadButton::RightShoulder,
                button(profile, GCExtendedGamepad::rightShoulder),
            )
            .with_button(GamepadButton::LeftTrigger, left_trigger)
            .with_button(GamepadButton::RightTrigger, right_trigger)
            .with_button(GamepadButton::Start, profile.buttonMenu().value())
            .with_button(GamepadButton::DpadUp, dpad.up().value())
            .with_button(GamepadButton::DpadDown, dpad.down().value())
            .with_button(GamepadButton::DpadLeft, dpad.left().value())
            .with_button(GamepadButton::DpadRight, dpad.right().value())
            .with_axis(GamepadAxis::LeftStickX, left.xAxis().value())
            .with_axis(GamepadAxis::LeftStickY, left.yAxis().value())
            .with_axis(GamepadAxis::RightStickX, right.xAxis().value())
            .with_axis(GamepadAxis::RightStickY, right.yAxis().value())
            .with_axis(GamepadAxis::LeftTrigger, left_trigger)
            .with_axis(GamepadAxis::RightTrigger, right_trigger);
        if let Some(button) = profile.buttonOptions() {
            snapshot = snapshot.with_button(GamepadButton::Select, button.value());
        }
        if let Some(button) = profile.leftThumbstickButton() {
            snapshot = snapshot.with_button(GamepadButton::LeftStick, button.value());
        }
        if let Some(button) = profile.rightThumbstickButton() {
            snapshot = snapshot.with_button(GamepadButton::RightStick, button.value());
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_snapshot_and_release_use_reliable_edges_and_latest_axes() {
        let mut producer = MacGamepadProducer::new(1, 16);
        let connection = producer.connect(0).unwrap();
        producer
            .publish_snapshot(
                connection,
                MacExtendedGamepadSnapshot::default()
                    .with_button(GamepadButton::South, 0.5)
                    .with_axis(GamepadAxis::LeftStickX, 0.25),
            )
            .unwrap();
        producer
            .publish_snapshot(
                connection,
                MacExtendedGamepadSnapshot::default()
                    .with_button(GamepadButton::South, 0.49)
                    .with_axis(GamepadAxis::LeftStickX, 0.75),
            )
            .unwrap();

        let events = producer.drain_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[0].event,
            GamepadEvent::DeviceConnected { .. }
        ));
        assert!(matches!(events[1].event, GamepadEvent::ButtonDown { .. }));
        assert!(matches!(events[2].event, GamepadEvent::ButtonUp { .. }));
        assert_eq!(
            events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let axes = producer.drain_axes();
        assert_eq!(axes.len(), AXES_PER_CONTROLLER);
        assert_eq!(
            axes.iter()
                .find(|(key, _)| key.axis == GamepadAxis::LeftStickX)
                .map(|(_, value)| *value),
            Some(0.75)
        );
        assert_eq!(producer.axis_diagnostics().captured, 12);
        assert_eq!(producer.axis_diagnostics().coalesced, 6);
        assert!(producer.axes_fully_accounted());
    }

    #[test]
    fn disconnect_discards_pending_axes_and_reused_slot_gets_new_generation() {
        let mut producer = MacGamepadProducer::new(1, 16);
        let first = producer.connect(0).unwrap();
        producer
            .publish_snapshot(first, MacExtendedGamepadSnapshot::default())
            .unwrap();
        producer.disconnect(first).unwrap();
        assert!(producer.drain_axes().is_empty());
        let second = producer.connect(0).unwrap();
        assert!(second.generation > first.generation);
        assert_eq!(
            producer.publish_snapshot(first, MacExtendedGamepadSnapshot::default()),
            Err(GamepadProducerError::UnknownOrStaleConnection)
        );
        assert_eq!(producer.diagnostics().stale_callbacks, 1);
    }

    #[test]
    fn axis_flood_never_uses_reliable_button_capacity() {
        let mut producer = MacGamepadProducer::new(1, 4);
        let connection = producer.connect(0).unwrap();
        for value in 0..10_000 {
            producer
                .publish_snapshot(
                    connection,
                    MacExtendedGamepadSnapshot::default()
                        .with_axis(GamepadAxis::LeftStickX, value as f32 / 10_000.0),
                )
                .unwrap();
        }
        producer
            .publish_snapshot(
                connection,
                MacExtendedGamepadSnapshot::default().with_button(GamepadButton::South, 1.0),
            )
            .unwrap();
        producer
            .publish_snapshot(connection, MacExtendedGamepadSnapshot::default())
            .unwrap();
        assert_eq!(producer.diagnostics().reliable_overflows, 0);
        let events = producer.drain_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2].event, GamepadEvent::ButtonUp { .. }));
    }

    #[test]
    fn reliable_overflow_inserts_reset_then_retries_rejected_edge() {
        let mut producer = MacGamepadProducer::new(1, 2);
        let connection = producer.connect(0).unwrap();
        producer
            .publish_snapshot(
                connection,
                MacExtendedGamepadSnapshot::default().with_button(GamepadButton::South, 1.0),
            )
            .unwrap();
        producer
            .publish_snapshot(connection, MacExtendedGamepadSnapshot::default())
            .unwrap();
        let events = producer.drain_events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].event,
            GamepadEvent::Reset {
                reason: GamepadResetReason::QueueOverflow
            }
        ));
        assert!(matches!(events[1].event, GamepadEvent::ButtonUp { .. }));
        assert_eq!(events[0].sequence + 1, events[1].sequence);
        assert_eq!(producer.diagnostics().reliable_overflows, 1);
        assert_eq!(producer.diagnostics().reliable_discarded, 2);
    }

    #[test]
    fn invalid_values_are_sanitized_and_trigger_cannot_be_negative() {
        let mut producer = MacGamepadProducer::new(1, 8);
        let connection = producer.connect(0).unwrap();
        producer
            .publish_snapshot(
                connection,
                MacExtendedGamepadSnapshot::default()
                    .with_button(GamepadButton::South, f32::NAN)
                    .with_axis(GamepadAxis::LeftTrigger, -1.0)
                    .with_axis(GamepadAxis::RightStickX, 2.0),
            )
            .unwrap();
        let axes = producer.drain_axes();
        assert_eq!(producer.diagnostics().invalid_values, 1);
        assert_eq!(
            axes.iter()
                .find(|(key, _)| key.axis == GamepadAxis::LeftTrigger)
                .map(|(_, value)| *value),
            Some(0.0)
        );
        assert_eq!(
            axes.iter()
                .find(|(key, _)| key.axis == GamepadAxis::RightStickX)
                .map(|(_, value)| *value),
            Some(1.0)
        );
    }

    #[test]
    fn shutdown_resets_active_state_flushes_axes_and_rejects_late_callbacks() {
        let mut producer = MacGamepadProducer::new(1, 8);
        let connection = producer.connect(0).unwrap();
        producer
            .publish_snapshot(connection, MacExtendedGamepadSnapshot::default())
            .unwrap();
        producer.close();
        assert!(producer.active_connections().is_empty());
        assert_eq!(producer.drain_axes().len(), AXES_PER_CONTROLLER);
        assert!(producer.axes_fully_accounted());
        assert_eq!(
            producer.publish_snapshot(connection, MacExtendedGamepadSnapshot::default()),
            Err(GamepadProducerError::Closed)
        );
        assert_eq!(producer.diagnostics().rejected_after_close, 1);
    }

    #[test]
    fn controller_capacity_is_bounded_before_axis_keys_are_created() {
        let mut producer = MacGamepadProducer::new(1, 8);
        producer.connect(0).unwrap();
        assert_eq!(
            producer.connect(1),
            Err(GamepadProducerError::CapacityExceeded)
        );
        assert!(producer.drain_axes().is_empty());
    }
}
