use bongocat_input::{
    GamepadAxis, GamepadAxisKey, GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample,
    GamepadButton, GamepadButtonKey, GamepadConnection, GamepadConnectionError, InputControl,
    InputEdge, InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource,
    MonotonicMillis, PlatformInputDiagnostics,
};
use gilrs::{
    Axis, Button, EventType, Filter, GamepadId, Gilrs, GilrsBuilder,
    ev::filter::axis_dpad_to_button,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
};

const MAX_GAMEPADS: usize = 4;
const MAX_EVENTS_PER_DRAIN: usize = 256;
const AXIS_TO_BUTTON_PRESSED: f32 = 0.5 + f32::EPSILON;
const AXIS_TO_BUTTON_RELEASED: f32 = 0.5;

/// Owns the third-party gamepad context and translates it into BongoCat's
/// platform-neutral input protocol. No gilrs type crosses this module boundary.
pub(crate) struct GilrsGamepad {
    gilrs: Option<Gilrs>,
    backend_failure_reported: bool,
    recovery_requested: bool,
    producer: InputProducer,
    axis_producer: GamepadAxisProducer,
    connections: ConnectionTable,
    backend_ids: BTreeMap<usize, GamepadId>,
    pressed_triggers: BTreeSet<(usize, GamepadButton)>,
}

pub(crate) struct GilrsShutdown {
    pub(crate) disconnected: u64,
    pub(crate) backend_clean: bool,
}

impl GilrsGamepad {
    pub(crate) fn new(producer: InputProducer, axis_producer: GamepadAxisProducer) -> Self {
        // Backend construction failures must disable only gamepad; they must
        // not unwind the keyboard/mouse platform worker.
        let gilrs = catch_unwind(AssertUnwindSafe(|| {
            GilrsBuilder::new()
                .with_default_filters(false)
                .with_force_feedback(false)
                .add_included_mappings(true)
                .add_env_mappings(false)
                .set_update_state(false)
                .set_axis_to_btn(AXIS_TO_BUTTON_PRESSED, AXIS_TO_BUTTON_RELEASED)
                .build()
                .ok()
        }))
        .ok()
        .flatten();
        Self {
            gilrs,
            backend_failure_reported: false,
            recovery_requested: false,
            producer,
            axis_producer,
            connections: ConnectionTable::new(),
            backend_ids: BTreeMap::new(),
            pressed_triggers: BTreeSet::new(),
        }
    }

    pub(crate) fn drain(
        &mut self,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let events = {
            let Some(gilrs) = self.gilrs.as_mut() else {
                if !self.backend_failure_reported {
                    diagnostics.gamepad_backend_failures =
                        diagnostics.gamepad_backend_failures.saturating_add(1);
                    self.backend_failure_reported = true;
                }
                return Ok(());
            };
            let mut events = Vec::with_capacity(MAX_EVENTS_PER_DRAIN);
            for _ in 0..MAX_EVENTS_PER_DRAIN {
                let Some(event) = gilrs.next_event().filter_ev(&axis_dpad_to_button, gilrs) else {
                    break;
                };
                // State updates are explicit because gilrs' default filters are
                // disabled. The retained D-pad filter still consults this state to
                // release the previous direction when the hat jumps across center.
                gilrs.update(&event);
                events.push((event.id, event.event));
            }
            // gilrs' explicit-state mode expects the owner to advance its
            // event counter once after each processing batch.
            gilrs.inc();
            events
        };
        let mut events = events.into_iter();
        while let Some((gamepad_id, event_type)) = events.next() {
            if let Err(error) = self.process_event(gamepad_id, event_type, at, diagnostics) {
                let discarded = events.len().saturating_add(1) as u64;
                diagnostics.gamepad_event_discards =
                    diagnostics.gamepad_event_discards.saturating_add(discarded);
                return Err(error);
            }
        }
        self.reconcile_connected(at, diagnostics)?;
        if self.recovery_requested {
            self.recovery_requested = false;
            self.reseed_internal(false, at, diagnostics)?;
        }
        Ok(())
    }

    fn process_event(
        &mut self,
        gamepad_id: GamepadId,
        event_type: EventType,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        match event_type {
            EventType::Connected => self.connect_backend(gamepad_id, at, diagnostics),
            EventType::Disconnected => self.disconnect_backend(gamepad_id, at, diagnostics),
            EventType::ButtonPressed(button, _) => {
                self.publish_button_event(gamepad_id, button, InputEdge::Down, at, diagnostics)
            }
            EventType::ButtonReleased(button, _) => {
                self.publish_button_event(gamepad_id, button, InputEdge::Up, at, diagnostics)
            }
            EventType::ButtonChanged(button, value, _) => {
                self.publish_button_value(gamepad_id, button, value, at, diagnostics)
            }
            EventType::AxisChanged(axis, value, _) => {
                let Some(axis) = map_axis(axis) else {
                    diagnostics.decode_errors = diagnostics.decode_errors.saturating_add(1);
                    return Ok(());
                };
                self.publish_axis(gamepad_id, axis, value, at, diagnostics)
            }
            EventType::BackendOverflow { dropped } => {
                diagnostics.gamepad_event_discards =
                    diagnostics.gamepad_event_discards.saturating_add(dropped);
                self.recovery_requested = true;
                Ok(())
            }
            EventType::ButtonRepeated(..)
            | EventType::Dropped
            | EventType::ForceFeedbackEffectCompleted => Ok(()),
            _ => Ok(()),
        }
    }

    pub(crate) fn reseed(
        &mut self,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        self.reseed_internal(true, at, diagnostics)
    }

    fn reseed_internal(
        &mut self,
        reset_backend: bool,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        if reset_backend {
            let reset_failed = match self.gilrs.as_mut() {
                Some(gilrs) => gilrs.reset().is_err(),
                None => false,
            };
            if reset_failed {
                diagnostics.gamepad_backend_failures =
                    diagnostics.gamepad_backend_failures.saturating_add(1);
                self.gilrs = None;
                self.backend_failure_reported = true;
                self.disconnect_all();
                return Ok(());
            }
        }

        // Reset clears runtime pressed state; do not let a cached trigger set
        // suppress the first valid held-state edge during the replay.
        self.pressed_triggers.clear();
        let connected = self.backend_ids.values().copied().collect::<Vec<_>>();
        let connections = connected
            .iter()
            .filter_map(|gamepad_id| self.connections.get(gamepad_id.into_inner()))
            .collect::<Vec<_>>();
        reseed_connections(&self.producer, connections, at)?;
        for gamepad_id in connected {
            if self.connections.get(gamepad_id.into_inner()).is_some() {
                self.publish_snapshot(gamepad_id, at, diagnostics)?;
            }
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> GilrsShutdown {
        let disconnected = self.disconnect_all();
        let backend_clean = match self.gilrs.take() {
            Some(gilrs) => catch_unwind(AssertUnwindSafe(|| gilrs.shutdown()))
                .ok()
                .and_then(|result| result.ok())
                .is_some(),
            None => true,
        };
        GilrsShutdown {
            disconnected,
            backend_clean,
        }
    }

    fn connect_backend(
        &mut self,
        gamepad_id: GamepadId,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let backend_id = gamepad_id.into_inner();
        let Some((backend_id, connection)) =
            self.connections
                .allocate(backend_id, &self.axis_producer, at, diagnostics)?
        else {
            return Ok(());
        };
        if let Err(error) = self
            .producer
            .publish(InputEvent::GamepadConnected { connection, at })
        {
            self.connections
                .rollback(backend_id, connection, &self.axis_producer);
            return Err(error);
        }
        self.backend_ids.insert(backend_id, gamepad_id);
        diagnostics.gamepad_connections = diagnostics.gamepad_connections.saturating_add(1);
        self.publish_snapshot(gamepad_id, at, diagnostics)
    }

    fn disconnect_backend(
        &mut self,
        gamepad_id: GamepadId,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let backend_id = gamepad_id.into_inner();
        let Some((_, connection)) = self.connections.remove(backend_id) else {
            return Ok(());
        };
        self.backend_ids.remove(&backend_id);
        self.pressed_triggers
            .retain(|(candidate, _)| *candidate != backend_id);
        self.axis_producer.disconnect(connection);
        diagnostics.gamepad_disconnections = diagnostics.gamepad_disconnections.saturating_add(1);
        self.producer
            .publish(InputEvent::GamepadDisconnected { connection, at })
            .map(|_| ())
    }

    fn reconcile_connected(
        &mut self,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(gilrs) = self.gilrs.as_ref() else {
            return Ok(());
        };
        let connected = gilrs
            .gamepads()
            .map(|(id, _)| (id.into_inner(), id))
            .collect::<BTreeMap<_, _>>();
        for gamepad_id in connected.values() {
            self.connect_backend(*gamepad_id, at, diagnostics)?;
        }
        let stale = self
            .connections
            .backend_ids()
            .filter(|backend_id| !connected.contains_key(backend_id))
            .collect::<Vec<_>>();
        for backend_id in stale {
            let Some(gamepad_id) = self.backend_ids.get(&backend_id).copied() else {
                continue;
            };
            self.disconnect_backend(gamepad_id, at, diagnostics)?;
        }
        Ok(())
    }

    fn publish_button_event(
        &self,
        gamepad_id: GamepadId,
        button: Button,
        edge: InputEdge,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        // Axis-like triggers use their continuous ButtonChanged values and the
        // product-owned exact 0.5 threshold below. Their synthesized
        // ButtonPressed/ButtonReleased events are therefore not duplicated.
        if map_button_axis(button).is_some() {
            return Ok(());
        }
        let Some(button) = map_button(button) else {
            diagnostics.unsupported_buttons = diagnostics.unsupported_buttons.saturating_add(1);
            return Ok(());
        };
        self.publish_button_edge(gamepad_id, button, edge, at, diagnostics)
    }

    fn publish_button_value(
        &mut self,
        gamepad_id: GamepadId,
        button: Button,
        value: f32,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(axis) = map_button_axis(button) else {
            return Ok(());
        };
        let Some(product_button) = map_button(button) else {
            return Ok(());
        };
        let backend_id = gamepad_id.into_inner();
        let trigger_key = (backend_id, product_button);
        let was_pressed = self.pressed_triggers.contains(&trigger_key);
        let edge = match trigger_edge(was_pressed, value) {
            Ok(edge) => edge,
            Err(()) => {
                record_invalid_axis_value(diagnostics);
                return Ok(());
            }
        };
        if let Some(edge) = edge {
            self.publish_button_edge(gamepad_id, product_button, edge, at, diagnostics)?;
            if edge == InputEdge::Down {
                self.pressed_triggers.insert(trigger_key);
            } else {
                self.pressed_triggers.remove(&trigger_key);
            }
        }
        self.publish_axis(gamepad_id, axis, value, at, diagnostics)
    }

    fn publish_button_edge(
        &self,
        gamepad_id: GamepadId,
        button: GamepadButton,
        edge: InputEdge,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(connection) = self.connections.get(gamepad_id.into_inner()) else {
            return Ok(());
        };
        self.producer
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge,
                source: InputSource::Capture,
                at,
            })
            .map(|_| ())?;
        diagnostics.gamepad_button_edges = diagnostics.gamepad_button_edges.saturating_add(1);
        Ok(())
    }

    fn publish_snapshot(
        &mut self,
        gamepad_id: GamepadId,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(gilrs) = self.gilrs.as_ref() else {
            return Ok(());
        };
        let gamepad = gilrs.gamepad(gamepad_id);
        let backend_id = gamepad_id.into_inner();
        for (gilrs_button, product_button) in BUTTON_MAP {
            let trigger_axis = map_button_axis(gilrs_button);
            let Some(data) = gamepad.button_data(gilrs_button) else {
                if trigger_axis.is_some() {
                    self.pressed_triggers.remove(&(backend_id, product_button));
                }
                continue;
            };
            if trigger_axis.is_some() && !valid_trigger_value(data.value()) {
                self.pressed_triggers.remove(&(backend_id, product_button));
                continue;
            }
            let pressed = if trigger_axis.is_some() {
                trigger_is_pressed(data.value())
            } else {
                data.is_pressed()
            };
            if map_button_axis(gilrs_button).is_some() {
                if pressed {
                    self.pressed_triggers.insert((backend_id, product_button));
                } else {
                    self.pressed_triggers.remove(&(backend_id, product_button));
                }
            }
            if pressed {
                self.publish_button_edge(
                    gamepad_id,
                    product_button,
                    InputEdge::Down,
                    at,
                    diagnostics,
                )?;
            }
        }
        for axis in GamepadAxis::ALL {
            let value = match axis {
                GamepadAxis::LeftTrigger => gamepad
                    .button_data(Button::LeftTrigger2)
                    .map_or(0.0, |data| data.value()),
                GamepadAxis::RightTrigger => gamepad
                    .button_data(Button::RightTrigger2)
                    .map_or(0.0, |data| data.value()),
                GamepadAxis::LeftStickX => gamepad
                    .axis_data(Axis::LeftStickX)
                    .map_or(0.0, |data| data.value()),
                GamepadAxis::LeftStickY => gamepad
                    .axis_data(Axis::LeftStickY)
                    .map_or(0.0, |data| data.value()),
                GamepadAxis::RightStickX => gamepad
                    .axis_data(Axis::RightStickX)
                    .map_or(0.0, |data| data.value()),
                GamepadAxis::RightStickY => gamepad
                    .axis_data(Axis::RightStickY)
                    .map_or(0.0, |data| data.value()),
            };
            self.publish_axis(gamepad_id, axis, value, at, diagnostics)?;
        }
        Ok(())
    }

    fn publish_axis(
        &self,
        gamepad_id: GamepadId,
        axis: GamepadAxis,
        value: f32,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<(), InputPublishError> {
        let Some(connection) = self.connections.get(gamepad_id.into_inner()) else {
            return Ok(());
        };
        match self.axis_producer.publish(GamepadAxisSample {
            key: GamepadAxisKey { connection, axis },
            value,
            at,
        }) {
            Ok(()) => {
                diagnostics.gamepad_axis_samples =
                    diagnostics.gamepad_axis_samples.saturating_add(1);
                Ok(())
            }
            Err(GamepadAxisPublishError::RuntimeStopped(_)) => Err(runtime_stopped(at)),
            Err(error) => {
                if matches!(
                    error,
                    GamepadAxisPublishError::NonFinite(_) | GamepadAxisPublishError::OutOfRange(_)
                ) {
                    record_invalid_axis_value(diagnostics);
                } else {
                    diagnostics.gamepad_axis_publish_rejections = diagnostics
                        .gamepad_axis_publish_rejections
                        .saturating_add(1);
                }
                Ok(())
            }
        }
    }

    fn disconnect_all(&mut self) -> u64 {
        let disconnected = self.connections.len() as u64;
        for connection in self.connections.values() {
            self.axis_producer.disconnect(connection);
        }
        self.connections.clear();
        self.backend_ids.clear();
        self.pressed_triggers.clear();
        disconnected
    }
}

impl Drop for GilrsGamepad {
    fn drop(&mut self) {
        self.disconnect_all();
    }
}

struct ConnectionTable {
    active: BTreeMap<usize, GamepadConnection>,
    free_device_ids: BTreeSet<u8>,
}

impl ConnectionTable {
    fn new() -> Self {
        Self {
            active: BTreeMap::new(),
            free_device_ids: (0..MAX_GAMEPADS as u8).collect(),
        }
    }

    fn allocate(
        &mut self,
        backend_id: usize,
        axis_producer: &GamepadAxisProducer,
        at: MonotonicMillis,
        diagnostics: &mut PlatformInputDiagnostics,
    ) -> Result<Option<(usize, GamepadConnection)>, InputPublishError> {
        if self.active.contains_key(&backend_id) {
            return Ok(None);
        }
        let Some(device_id) = self.free_device_ids.pop_first() else {
            diagnostics.gamepad_connection_rejections =
                diagnostics.gamepad_connection_rejections.saturating_add(1);
            return Ok(None);
        };
        let connection = match axis_producer.connect(device_id) {
            Ok(connection) => connection,
            Err(GamepadConnectionError::RuntimeStopped) => {
                self.free_device_ids.insert(device_id);
                return Err(runtime_stopped(at));
            }
            Err(GamepadConnectionError::GenerationExhausted) => {
                self.free_device_ids.insert(device_id);
                diagnostics.gamepad_connection_rejections =
                    diagnostics.gamepad_connection_rejections.saturating_add(1);
                diagnostics.gamepad_axis_publish_rejections = diagnostics
                    .gamepad_axis_publish_rejections
                    .saturating_add(1);
                return Ok(None);
            }
        };
        self.active.insert(backend_id, connection);
        Ok(Some((backend_id, connection)))
    }

    fn rollback(
        &mut self,
        backend_id: usize,
        connection: GamepadConnection,
        axis_producer: &GamepadAxisProducer,
    ) {
        if self.active.remove(&backend_id) == Some(connection) {
            axis_producer.disconnect(connection);
            self.free_device_ids.insert(connection.device_id);
        }
    }

    fn remove(&mut self, backend_id: usize) -> Option<(usize, GamepadConnection)> {
        let connection = self.active.remove(&backend_id)?;
        self.free_device_ids.insert(connection.device_id);
        Some((backend_id, connection))
    }

    fn get(&self, backend_id: usize) -> Option<GamepadConnection> {
        self.active.get(&backend_id).copied()
    }

    fn values(&self) -> impl Iterator<Item = GamepadConnection> + '_ {
        self.active.values().copied()
    }

    fn backend_ids(&self) -> impl Iterator<Item = usize> + '_ {
        self.active.keys().copied()
    }

    fn len(&self) -> usize {
        self.active.len()
    }

    fn clear(&mut self) {
        self.active.clear();
        self.free_device_ids = (0..MAX_GAMEPADS as u8).collect();
    }
}

const fn trigger_is_pressed(value: f32) -> bool {
    value >= 0.5
}

fn valid_trigger_value(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn trigger_edge(was_pressed: bool, value: f32) -> Result<Option<InputEdge>, ()> {
    if !valid_trigger_value(value) {
        return Err(());
    }
    let is_pressed = trigger_is_pressed(value);
    Ok((was_pressed != is_pressed).then_some(if is_pressed {
        InputEdge::Down
    } else {
        InputEdge::Up
    }))
}

fn record_invalid_axis_value(diagnostics: &mut PlatformInputDiagnostics) {
    diagnostics.decode_errors = diagnostics.decode_errors.saturating_add(1);
    diagnostics.gamepad_axis_publish_rejections = diagnostics
        .gamepad_axis_publish_rejections
        .saturating_add(1);
}

fn reseed_connections(
    producer: &InputProducer,
    connections: impl IntoIterator<Item = GamepadConnection>,
    at: MonotonicMillis,
) -> Result<(), InputPublishError> {
    for connection in connections {
        producer.publish(InputEvent::GamepadConnected { connection, at })?;
    }
    Ok(())
}

fn runtime_stopped(at: MonotonicMillis) -> InputPublishError {
    InputPublishError::RuntimeStopped(InputEvent::Reset {
        reason: InputResetReason::ServiceRestart,
        at,
    })
}

const BUTTON_MAP: [(Button, GamepadButton); 16] = [
    (Button::South, GamepadButton::South),
    (Button::East, GamepadButton::East),
    (Button::West, GamepadButton::West),
    (Button::North, GamepadButton::North),
    (Button::LeftTrigger, GamepadButton::LeftShoulder),
    (Button::RightTrigger, GamepadButton::RightShoulder),
    (Button::LeftTrigger2, GamepadButton::LeftTrigger),
    (Button::RightTrigger2, GamepadButton::RightTrigger),
    (Button::Select, GamepadButton::Select),
    (Button::Start, GamepadButton::Start),
    (Button::LeftThumb, GamepadButton::LeftStick),
    (Button::RightThumb, GamepadButton::RightStick),
    (Button::DPadUp, GamepadButton::DpadUp),
    (Button::DPadDown, GamepadButton::DpadDown),
    (Button::DPadLeft, GamepadButton::DpadLeft),
    (Button::DPadRight, GamepadButton::DpadRight),
];

const fn map_button(button: Button) -> Option<GamepadButton> {
    Some(match button {
        Button::South => GamepadButton::South,
        Button::East => GamepadButton::East,
        Button::West => GamepadButton::West,
        Button::North => GamepadButton::North,
        Button::LeftTrigger => GamepadButton::LeftShoulder,
        Button::RightTrigger => GamepadButton::RightShoulder,
        Button::LeftTrigger2 => GamepadButton::LeftTrigger,
        Button::RightTrigger2 => GamepadButton::RightTrigger,
        Button::Select => GamepadButton::Select,
        Button::Start => GamepadButton::Start,
        Button::LeftThumb => GamepadButton::LeftStick,
        Button::RightThumb => GamepadButton::RightStick,
        Button::DPadUp => GamepadButton::DpadUp,
        Button::DPadDown => GamepadButton::DpadDown,
        Button::DPadLeft => GamepadButton::DpadLeft,
        Button::DPadRight => GamepadButton::DpadRight,
        Button::C | Button::Z | Button::Mode | Button::Unknown => return None,
    })
}

const fn map_button_axis(button: Button) -> Option<GamepadAxis> {
    if matches!(button, Button::LeftTrigger2) {
        Some(GamepadAxis::LeftTrigger)
    } else if matches!(button, Button::RightTrigger2) {
        Some(GamepadAxis::RightTrigger)
    } else {
        None
    }
}

const fn map_axis(axis: Axis) -> Option<GamepadAxis> {
    Some(match axis {
        Axis::LeftStickX => GamepadAxis::LeftStickX,
        Axis::LeftStickY => GamepadAxis::LeftStickY,
        Axis::RightStickX => GamepadAxis::RightStickX,
        Axis::RightStickY => GamepadAxis::RightStickY,
        Axis::DPadX | Axis::DPadY | Axis::LeftZ | Axis::RightZ | Axis::Unknown => {
            return None;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_runtime::RuntimeOwner;
    use std::time::Duration;

    #[test]
    fn canonical_gilrs_controls_map_to_the_complete_product_vocabulary() {
        for (gilrs_button, product_button) in BUTTON_MAP {
            assert_eq!(map_button(gilrs_button), Some(product_button));
        }
        assert_eq!(map_button(Button::C), None);
        assert_eq!(map_button(Button::Z), None);
        assert_eq!(map_button(Button::Mode), None);
        assert_eq!(map_button(Button::Unknown), None);

        for (gilrs_axis, product_axis) in [
            (Axis::LeftStickX, GamepadAxis::LeftStickX),
            (Axis::LeftStickY, GamepadAxis::LeftStickY),
            (Axis::RightStickX, GamepadAxis::RightStickX),
            (Axis::RightStickY, GamepadAxis::RightStickY),
        ] {
            assert_eq!(map_axis(gilrs_axis), Some(product_axis));
        }
        assert_eq!(map_axis(Axis::DPadX), None);
        assert_eq!(map_axis(Axis::DPadY), None);
        assert_eq!(
            map_button_axis(Button::LeftTrigger2),
            Some(GamepadAxis::LeftTrigger)
        );
        assert_eq!(
            map_button_axis(Button::RightTrigger2),
            Some(GamepadAxis::RightTrigger)
        );
        assert_eq!(map_button_axis(Button::South), None);
        assert!(!trigger_is_pressed(0.499_999_94));
        assert!(trigger_is_pressed(0.5));
        assert!(trigger_is_pressed(1.0));
        assert!(valid_trigger_value(0.0));
        assert!(valid_trigger_value(1.0));
        assert!(!valid_trigger_value(-0.01));
        assert!(!valid_trigger_value(1.01));
        assert!(!valid_trigger_value(f32::NAN));
        assert!(!valid_trigger_value(f32::INFINITY));
        assert_eq!(trigger_edge(false, 0.5), Ok(Some(InputEdge::Down)));
        assert_eq!(trigger_edge(true, 0.5), Ok(None));
        assert_eq!(trigger_edge(true, 0.49), Ok(Some(InputEdge::Up)));
        assert!(trigger_edge(false, f32::NAN).is_err());
        assert!(trigger_edge(false, -0.1).is_err());
        assert!(trigger_edge(false, 1.1).is_err());
    }

    #[test]
    fn connection_table_is_bounded_and_reuses_slots_with_a_new_generation() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 8);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let axes = runtime.gamepad_axis_producer();
        let mut diagnostics = PlatformInputDiagnostics::default();
        let mut table = ConnectionTable::new();

        for backend_id in 0..MAX_GAMEPADS {
            let (allocated_id, _) = table
                .allocate(backend_id, &axes, MonotonicMillis::new(1), &mut diagnostics)
                .expect("connection allocation")
                .expect("free slot");
            assert_eq!(allocated_id, backend_id);
        }
        assert!(
            table
                .allocate(
                    MAX_GAMEPADS,
                    &axes,
                    MonotonicMillis::new(1),
                    &mut diagnostics,
                )
                .expect("capacity rejection")
                .is_none()
        );
        assert_eq!(diagnostics.gamepad_connection_rejections, 1);

        let first = table.remove(2).expect("existing connection").1;
        axes.disconnect(first);
        let (_, replacement) = table
            .allocate(2, &axes, MonotonicMillis::new(2), &mut diagnostics)
            .expect("replacement allocation")
            .expect("released slot");
        assert_eq!(replacement.device_id, first.device_id);
        assert!(replacement.generation > first.generation);

        for connection in table.values() {
            axes.disconnect(connection);
        }
        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[test]
    fn reset_reseed_republishes_the_same_connections_without_new_generations() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let producer = runtime.input_producer();
        let axes = runtime.gamepad_axis_producer();
        let mut table = ConnectionTable::new();
        let mut diagnostics = PlatformInputDiagnostics::default();
        for backend_id in 0..2 {
            table
                .allocate(backend_id, &axes, MonotonicMillis::new(1), &mut diagnostics)
                .expect("connection allocation")
                .expect("free slot");
        }
        let connections = table.values().collect::<Vec<_>>();
        reseed_connections(
            &producer,
            connections.iter().copied(),
            MonotonicMillis::new(2),
        )
        .expect("initial connections");
        client
            .wait_for_input_sequence(1, TIMEOUT)
            .expect("initial gamepads");

        let reset_sequence = producer
            .recover(InputResetReason::ServiceRestart, MonotonicMillis::new(3))
            .expect("runtime reset");
        client
            .wait_for_input_sequence(reset_sequence, TIMEOUT)
            .expect("reset consumed");
        assert_eq!(client.snapshot().input.connected_gamepad_count, 0);

        reseed_connections(
            &producer,
            connections.iter().copied(),
            MonotonicMillis::new(4),
        )
        .expect("reseeded connections");
        client
            .wait_for_input_sequence(reset_sequence + 2, TIMEOUT)
            .expect("reseeded gamepads");
        assert_eq!(client.snapshot().input.connected_gamepad_count, 2);
        assert_eq!(axes.diagnostics().connections, 2);
        assert_eq!(axes.diagnostics().disconnections, 0);

        for connection in connections {
            axes.disconnect(connection);
        }
        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn gamepad_backend_failure_does_not_stop_the_input_service() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 8);
        let mut gamepad =
            GilrsGamepad::new(runtime.input_producer(), runtime.gamepad_axis_producer());
        gamepad.gilrs = None;
        gamepad.backend_failure_reported = false;
        let mut diagnostics = PlatformInputDiagnostics::default();
        gamepad
            .drain(MonotonicMillis::new(1), &mut diagnostics)
            .expect("keyboard service remains operational");
        gamepad
            .drain(MonotonicMillis::new(2), &mut diagnostics)
            .expect("keyboard service remains operational");
        assert_eq!(diagnostics.gamepad_backend_failures, 1);
        runtime.shutdown(TIMEOUT).expect("runtime stop");
    }

    #[test]
    fn runtime_stop_is_not_misreported_as_a_capacity_rejection() {
        const TIMEOUT: Duration = Duration::from_secs(2);
        let runtime = RuntimeOwner::start(true, 8);
        let axes = runtime.gamepad_axis_producer();
        runtime.shutdown(TIMEOUT).expect("runtime stop");
        let mut table = ConnectionTable::new();
        let error = table
            .allocate(
                0,
                &axes,
                MonotonicMillis::new(1),
                &mut PlatformInputDiagnostics::default(),
            )
            .expect_err("stopped axis transport");
        assert!(matches!(error, InputPublishError::RuntimeStopped(_)));
        assert_eq!(table.len(), 0);
        assert_eq!(table.free_device_ids.len(), MAX_GAMEPADS);
    }
}
