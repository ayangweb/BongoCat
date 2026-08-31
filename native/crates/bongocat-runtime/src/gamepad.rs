use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::{GamepadAxis, GamepadAxisKey, GamepadConnection, MonotonicMillis};

pub const DEFAULT_GAMEPAD_AXIS_CAPACITY: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamepadAxisSettings {
    pub stick_dead_zone: f32,
    pub trigger_dead_zone: f32,
}

impl Default for GamepadAxisSettings {
    fn default() -> Self {
        Self {
            stick_dead_zone: 0.15,
            trigger_dead_zone: 0.0,
        }
    }
}

impl GamepadAxisSettings {
    pub fn new(stick_dead_zone: f32, trigger_dead_zone: f32) -> Option<Self> {
        if (0.0..1.0).contains(&stick_dead_zone)
            && (0.0..1.0).contains(&trigger_dead_zone)
            && stick_dead_zone.is_finite()
            && trigger_dead_zone.is_finite()
        {
            Some(Self {
                stick_dead_zone,
                trigger_dead_zone,
            })
        } else {
            None
        }
    }

    pub fn apply(self, axis: GamepadAxis, value: f32) -> f32 {
        let dead_zone = if axis.is_trigger() {
            self.trigger_dead_zone
        } else {
            self.stick_dead_zone
        };
        if axis.is_trigger() {
            if value <= dead_zone {
                0.0
            } else {
                ((value - dead_zone) / (1.0 - dead_zone)).clamp(0.0, 1.0)
            }
        } else if value.abs() <= dead_zone {
            0.0
        } else {
            value.signum() * ((value.abs() - dead_zone) / (1.0 - dead_zone)).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamepadAxisSample {
    pub key: GamepadAxisKey,
    pub value: f32,
    pub at: MonotonicMillis,
}

impl GamepadAxisSample {
    pub fn new(
        key: GamepadAxisKey,
        value: f32,
        at: MonotonicMillis,
    ) -> Result<Self, GamepadAxisPublishError> {
        if !value.is_finite() {
            return Err(GamepadAxisPublishError::NonFinite(Self { key, value, at }));
        }
        let valid_range = if key.axis.is_trigger() {
            (0.0..=1.0).contains(&value)
        } else {
            (-1.0..=1.0).contains(&value)
        };
        if !valid_range {
            return Err(GamepadAxisPublishError::OutOfRange(Self { key, value, at }));
        }
        Ok(Self { key, value, at })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GamepadAxisPublishError {
    NonFinite(GamepadAxisSample),
    OutOfRange(GamepadAxisSample),
    NonMonotonic(GamepadAxisSample),
    CapacityExceeded(GamepadAxisSample),
    RuntimeStopped(GamepadAxisSample),
    StaleGeneration(GamepadAxisSample),
}

impl std::fmt::Display for GamepadAxisPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NonFinite(_) => "gamepad axis value is not finite",
            Self::OutOfRange(_) => "gamepad axis value is outside its normalized range",
            Self::NonMonotonic(_) => "gamepad axis sample time moved backwards",
            Self::CapacityExceeded(_) => "gamepad axis key capacity is exhausted",
            Self::RuntimeStopped(_) => "runtime is stopped",
            Self::StaleGeneration(_) => "gamepad axis sample belongs to a stale connection",
        })
    }
}

impl std::error::Error for GamepadAxisPublishError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadAxisTransportDiagnostics {
    pub published: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub discarded: u64,
    pub non_monotonic: u64,
    pub invalid_values: u64,
    pub capacity_rejections: u64,
    pub stale_generation_rejections: u64,
    pub rejected_after_stop: u64,
    pub pending: u64,
}

#[derive(Default)]
struct GamepadAxisSlotState {
    pending: BTreeMap<GamepadAxisKey, GamepadAxisSample>,
    last_published_at: BTreeMap<GamepadAxisKey, MonotonicMillis>,
    highest_generation: BTreeMap<u8, u64>,
    capacity: usize,
    stopped: bool,
    diagnostics: GamepadAxisTransportDiagnostics,
}

pub(crate) struct GamepadAxisSlot {
    state: Mutex<GamepadAxisSlotState>,
}

impl Default for GamepadAxisSlot {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_GAMEPAD_AXIS_CAPACITY)
    }
}

impl GamepadAxisSlot {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "gamepad axis capacity must be non-zero");
        Self {
            state: Mutex::new(GamepadAxisSlotState {
                capacity,
                ..GamepadAxisSlotState::default()
            }),
        }
    }

    fn publish(&self, sample: GamepadAxisSample) -> Result<(), GamepadAxisPublishError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sample = match GamepadAxisSample::new(sample.key, sample.value, sample.at) {
            Ok(sample) => sample,
            Err(
                error @ (GamepadAxisPublishError::NonFinite(_)
                | GamepadAxisPublishError::OutOfRange(_)),
            ) => {
                state.diagnostics.invalid_values =
                    state.diagnostics.invalid_values.saturating_add(1);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        if state.stopped {
            state.diagnostics.rejected_after_stop =
                state.diagnostics.rejected_after_stop.saturating_add(1);
            return Err(GamepadAxisPublishError::RuntimeStopped(sample));
        }
        if state
            .highest_generation
            .get(&sample.key.connection.device_id)
            .is_some_and(|generation| sample.key.connection.generation < *generation)
        {
            state.diagnostics.stale_generation_rejections = state
                .diagnostics
                .stale_generation_rejections
                .saturating_add(1);
            return Err(GamepadAxisPublishError::StaleGeneration(sample));
        }
        if state
            .highest_generation
            .get(&sample.key.connection.device_id)
            .is_none_or(|generation| sample.key.connection.generation > *generation)
        {
            let before = state.pending.len();
            state
                .pending
                .retain(|key, _| key.connection.device_id != sample.key.connection.device_id);
            state.diagnostics.discarded = state
                .diagnostics
                .discarded
                .saturating_add((before - state.pending.len()) as u64);
            state
                .last_published_at
                .retain(|key, _| key.connection.device_id != sample.key.connection.device_id);
            state.highest_generation.insert(
                sample.key.connection.device_id,
                sample.key.connection.generation,
            );
        }
        if state
            .last_published_at
            .get(&sample.key)
            .is_some_and(|previous| sample.at < *previous)
        {
            state.diagnostics.non_monotonic = state.diagnostics.non_monotonic.saturating_add(1);
            return Err(GamepadAxisPublishError::NonMonotonic(sample));
        }
        if !state.pending.contains_key(&sample.key) && state.pending.len() >= state.capacity {
            state.diagnostics.capacity_rejections =
                state.diagnostics.capacity_rejections.saturating_add(1);
            return Err(GamepadAxisPublishError::CapacityExceeded(sample));
        }
        state.last_published_at.insert(sample.key, sample.at);
        state.diagnostics.published = state.diagnostics.published.saturating_add(1);
        if state.pending.insert(sample.key, sample).is_some() {
            state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
        }
        Ok(())
    }

    pub(crate) fn take(&self) -> Vec<GamepadAxisSample> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let samples = std::mem::take(&mut state.pending)
            .into_values()
            .collect::<Vec<_>>();
        state.diagnostics.consumed = state
            .diagnostics
            .consumed
            .saturating_add(samples.len() as u64);
        samples
    }

    pub(crate) fn clear_connection(&self, connection: GamepadConnection) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = state.pending.len();
        state.pending.retain(|key, _| key.connection != connection);
        state.diagnostics.discarded = state
            .diagnostics
            .discarded
            .saturating_add((before - state.pending.len()) as u64);
        state
            .last_published_at
            .retain(|key, _| key.connection != connection);
    }

    pub(crate) fn stop(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped = true;
    }

    pub(crate) fn diagnostics(&self) -> GamepadAxisTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        GamepadAxisTransportDiagnostics {
            pending: state.pending.len() as u64,
            ..state.diagnostics
        }
    }
}

#[derive(Clone)]
pub struct GamepadAxisProducer {
    slot: Arc<GamepadAxisSlot>,
}

impl GamepadAxisProducer {
    pub(crate) fn new(slot: Arc<GamepadAxisSlot>) -> Self {
        Self { slot }
    }

    pub fn publish(&self, sample: GamepadAxisSample) -> Result<(), GamepadAxisPublishError> {
        self.slot.publish(sample)
    }

    pub fn disconnect(&self, connection: GamepadConnection) {
        self.slot.clear_connection(connection);
    }

    pub fn diagnostics(&self) -> GamepadAxisTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn is_fully_accounted(&self) -> bool {
        let diagnostics = self.slot.diagnostics();
        diagnostics.published
            == diagnostics.coalesced
                + diagnostics.consumed
                + diagnostics.discarded
                + diagnostics.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTION: GamepadConnection = GamepadConnection {
        device_id: 0,
        generation: 1,
    };

    fn sample(axis: GamepadAxis, value: f32, at: u64) -> GamepadAxisSample {
        GamepadAxisSample {
            key: GamepadAxisKey {
                connection: CONNECTION,
                axis,
            },
            value,
            at: MonotonicMillis::new(at),
        }
    }

    #[test]
    fn axis_slot_coalesces_by_generation_key() {
        let slot = GamepadAxisSlot::with_capacity(6);
        for index in 0..10_000 {
            slot.publish(sample(
                GamepadAxis::LeftStickX,
                index as f32 / 10_000.0,
                index,
            ))
            .expect("axis accepted");
        }
        let samples = slot.take();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].value, 0.9999);
        assert_eq!(slot.diagnostics().coalesced, 9_999);
        assert_eq!(slot.diagnostics().discarded, 0);
        assert!(GamepadAxisProducer::new(Arc::new(slot)).is_fully_accounted());
    }

    #[test]
    fn stale_generation_is_rejected_and_reconnect_clears_old_pending_values() {
        let slot = GamepadAxisSlot::with_capacity(6);
        slot.publish(sample(GamepadAxis::LeftStickX, 0.5, 1))
            .expect("first generation accepted");
        let reconnect = GamepadAxisSample {
            key: GamepadAxisKey {
                connection: GamepadConnection {
                    device_id: 0,
                    generation: 2,
                },
                axis: GamepadAxis::RightStickX,
            },
            value: 0.25,
            at: MonotonicMillis::new(2),
        };
        slot.publish(reconnect).expect("reconnect accepted");
        assert_eq!(slot.take(), vec![reconnect]);
        assert_eq!(slot.diagnostics().discarded, 1);
        assert!(matches!(
            slot.publish(sample(GamepadAxis::LeftStickX, 0.75, 3)),
            Err(GamepadAxisPublishError::StaleGeneration(_))
        ));
    }

    #[test]
    fn axis_range_and_time_errors_are_explicit() {
        let slot = GamepadAxisSlot::with_capacity(6);
        assert!(matches!(
            slot.publish(sample(GamepadAxis::LeftStickX, 2.0, 0)),
            Err(GamepadAxisPublishError::OutOfRange(_))
        ));
        slot.publish(sample(GamepadAxis::LeftTrigger, 0.5, 2))
            .expect("trigger accepted");
        assert!(matches!(
            slot.publish(sample(GamepadAxis::LeftTrigger, 0.4, 1)),
            Err(GamepadAxisPublishError::NonMonotonic(_))
        ));
    }

    #[test]
    fn dead_zone_is_applied_only_by_runtime_settings() {
        let settings = GamepadAxisSettings::new(0.2, 0.1).expect("valid settings");
        assert_eq!(settings.apply(GamepadAxis::LeftStickX, 0.2), 0.0);
        assert!((settings.apply(GamepadAxis::LeftStickX, 0.6) - 0.5).abs() < f32::EPSILON);
        assert_eq!(settings.apply(GamepadAxis::LeftTrigger, 0.1), 0.0);
        assert!((settings.apply(GamepadAxis::LeftTrigger, 0.55) - 0.5).abs() < f32::EPSILON);
        assert!(GamepadAxisSettings::new(1.0, 0.0).is_none());
    }
}
