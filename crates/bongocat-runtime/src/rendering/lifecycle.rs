//! Starting, stopping, and the channel in between.
//!
//! Closing is the part worth reading: the renderer has to stop accepting frames
//! before the runtime tears down the model those frames point into, or a frame in
//! flight arrives after the model it names is gone.

use super::*;

impl RuntimeRenderer {
    pub(crate) fn channel() -> (RuntimeRenderBootstrap, RenderConsumer) {
        let (producer, consumer) = latest_render_channel();
        (RuntimeRenderBootstrap { producer }, consumer)
    }
}

impl RuntimeRenderer {
    pub(crate) fn start(bootstrap: RuntimeRenderBootstrap) -> Self {
        Self {
            producer: bootstrap.producer,
            model_settings: ModelSettings::default(),
            next_model_generation: 0,
            next_transport_sequence: 0,
            active: None,
            pending: None,
        }
    }
}

impl RuntimeRenderer {
    pub(crate) fn close(&self) {
        self.producer.close();
    }
}
