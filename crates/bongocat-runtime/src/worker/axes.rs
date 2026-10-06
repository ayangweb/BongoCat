//! The newest sample per gamepad axis, and the generation that makes a late
//! sample from a disconnected pad harmless.

use crate::*;

#[derive(Default)]
pub(crate) struct GamepadAxisValues {
    pub(crate) values: BTreeMap<GamepadAxisKey, GamepadAxisSample>,
}

impl GamepadAxisValues {
    pub(crate) fn consume(&mut self, producer: &GamepadAxisProducer) -> bool {
        let samples = producer.take();
        if samples.is_empty() {
            return false;
        }
        for sample in samples {
            self.values.retain(|key, _| {
                key.connection.device_id != sample.key.connection.device_id
                    || key.connection.generation == sample.key.connection.generation
            });
            self.values.insert(sample.key, sample);
        }
        true
    }

    pub(crate) fn activate_connection(
        &mut self,
        connection: GamepadConnection,
        connected_at: MonotonicMillis,
    ) {
        self.values
            .retain(|key, sample| key.connection != connection || sample.at >= connected_at);
    }

    /// The six stick and trigger values the model is driven with.
    ///
    /// Every connected pad contributes, and for each axis the newest sample is
    /// the one that reads. The pre-rewrite input layer merged every pad into one
    /// set of stick and trigger values under one name, so the newest write for a
    /// name won whichever pad made it; picking a single connection instead froze
    /// the model on whichever pad happened to connect first, so a second pad
    /// that the user then picked up moved nothing. One pad is unchanged either
    /// way — it is the only connection that can win.
    ///
    /// The dead zone is applied to the winning sample only, so a value that is
    /// projected as zero can never also decide a stick's visibility, and a
    /// connection the runtime has already released contributes nothing.
    pub(crate) fn project(
        &self,
        input_state: &InputState,
        settings: GamepadAxisSettings,
    ) -> [f32; 6] {
        let mut values = [0.0; 6];
        let mut newest = [None; 6];
        for (key, sample) in &self.values {
            if !input_state.is_gamepad_connected(key.connection) {
                continue;
            }
            let slot = key.axis as usize;
            if newest[slot].is_some_and(|seen| seen >= sample.at) {
                continue;
            }
            newest[slot] = Some(sample.at);
            values[slot] = settings.apply(key.axis, sample.value);
        }
        values
    }

    pub(crate) fn clear(&mut self) {
        self.values.clear();
    }

    pub(crate) fn clear_connection(&mut self, connection: GamepadConnection) {
        self.values.retain(|key, _| key.connection != connection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_state::InputState;

    const NO_DEAD_ZONE: GamepadAxisSettings = GamepadAxisSettings {
        stick_dead_zone: 0.0,
        trigger_dead_zone: 0.0,
    };

    fn connection(device_id: u8, generation: u64) -> GamepadConnection {
        GamepadConnection {
            device_id,
            generation,
        }
    }

    fn recorded(
        values: &mut GamepadAxisValues,
        connection: GamepadConnection,
        axis: GamepadAxis,
        value: f32,
        at: u64,
    ) {
        values.values.insert(
            GamepadAxisKey { connection, axis },
            GamepadAxisSample {
                key: GamepadAxisKey { connection, axis },
                value,
                at: MonotonicMillis::new(at),
            },
        );
    }

    fn state_with(connections: &[GamepadConnection]) -> InputState {
        let mut state = InputState::default();
        for connection in connections {
            state.active_gamepads.insert(*connection);
        }
        state
    }

    fn stick_left_x(values: &[f32]) -> f32 {
        values[GamepadAxis::LeftStickX as usize]
    }

    /// The pre-rewrite input layer merged every pad into one set of values under
    /// one name, so the newest write won. Choosing one connection instead meant a
    /// second pad the user picked up after the first was connected moved nothing
    /// at all, because the first pad's older sample kept every axis.
    #[test]
    fn the_newest_sample_of_an_axis_wins_across_every_connected_pad() {
        let first = connection(0, 1);
        let second = connection(1, 1);
        let mut values = GamepadAxisValues::default();
        recorded(&mut values, first, GamepadAxis::LeftStickX, -1.0, 10);
        recorded(&mut values, second, GamepadAxis::LeftStickX, 0.75, 20);
        let state = state_with(&[first, second]);
        assert_eq!(stick_left_x(&values.project(&state, NO_DEAD_ZONE)), 0.75);

        // The older pad moving again takes the axis back, and a pad returning to
        // center projects zero rather than freezing on the value it last held.
        recorded(&mut values, first, GamepadAxis::LeftStickX, -0.5, 30);
        assert_eq!(
            stick_left_x(&values.project(&state, NO_DEAD_ZONE)),
            -0.5,
            "the newest sample wins, whichever pad produced it"
        );
        recorded(&mut values, first, GamepadAxis::LeftStickX, 0.0, 40);
        assert_eq!(
            stick_left_x(&values.project(&state, NO_DEAD_ZONE)),
            0.0,
            "a stick returned to center projects zero, not its last non-zero value"
        );
    }

    /// A connection the runtime has already released must stop contributing the
    /// moment it is released, even though its samples are still the newest ones
    /// the store holds.
    #[test]
    fn a_released_connection_contributes_nothing_even_while_its_samples_are_newest() {
        let released = connection(0, 1);
        let live = connection(1, 1);
        let mut values = GamepadAxisValues::default();
        recorded(&mut values, released, GamepadAxis::LeftStickX, 1.0, 90);
        recorded(&mut values, live, GamepadAxis::LeftStickX, 0.25, 10);
        assert_eq!(
            stick_left_x(&values.project(&state_with(&[live]), NO_DEAD_ZONE)),
            0.25,
            "a released connection must not keep an axis alive"
        );
        assert_eq!(
            stick_left_x(&values.project(&state_with(&[]), NO_DEAD_ZONE)),
            0.0
        );
    }

    /// The dead zone is a projection, not a transport filter: the winning sample
    /// is chosen first and then shaped, so a stick inside its dead zone reads
    /// zero and cannot keep its artwork on screen.
    #[test]
    fn the_dead_zone_shapes_the_winning_sample_rather_than_the_transport() {
        let pad = connection(0, 1);
        let mut values = GamepadAxisValues::default();
        recorded(&mut values, pad, GamepadAxis::LeftStickY, 0.1, 5);
        let state = state_with(&[pad]);
        assert_eq!(
            values.project(&state, NO_DEAD_ZONE)[GamepadAxis::LeftStickY as usize],
            0.1
        );
        let shaped = GamepadAxisSettings {
            stick_dead_zone: 0.15,
            trigger_dead_zone: 0.0,
        };
        assert_eq!(
            values.project(&state, shaped)[GamepadAxis::LeftStickY as usize],
            0.0,
            "a stick inside its dead zone reads zero"
        );
    }
}
