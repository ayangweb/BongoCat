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

    pub(crate) fn project(
        &self,
        input_state: &InputState,
        settings: GamepadAxisSettings,
    ) -> [f32; 6] {
        let Some(connection) = self
            .values
            .keys()
            .filter(|key| input_state.is_gamepad_connected(key.connection))
            .map(|key| key.connection)
            .min()
        else {
            return [0.0; 6];
        };
        let mut values = [0.0; 6];
        for (key, value) in self.values.iter().filter(|(key, _)| {
            key.connection == connection && input_state.is_gamepad_connected(key.connection)
        }) {
            values[key.axis as usize] = settings.apply(key.axis, value.value);
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
