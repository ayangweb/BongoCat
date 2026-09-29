//! The plugin host the product owns for its whole run.
//!
//! Three things have to be true at once and none of them is obvious from the
//! plugin crate alone:
//!
//! * The worker must exist **before** the overlay, because it owns the producer end
//!   of the layer channel the overlay consumes. That is why the host is started
//!   where it is and the consumer is handed to the overlay afterwards.
//! * The worker must be stopped **after** the frame source and **before** the
//!   renderer's GPU resources are released, or it is rasterizing into a device that
//!   is going away. [`ProductPluginHost::shutdown`] is joined at that point in
//!   [`crate::product_shutdown::ProductShutdown::finish`], and nowhere else.
//! * The local clock is read on the main thread, because `time` documents
//!   `current_local_offset` as sound only with one thread asking. The host owns the
//!   cache and the frame loop calls [`ProductPluginHost::refresh_local_time`].
//!
//! A failure to start is a degraded product, not a failed one: the user still gets
//! the cat, the plugin center says why, and nothing about the failure is allowed to
//! take the model window with it.

use bongocat_config::StorageLayout;
use bongocat_plugin::{
    LocalTimeCache, PluginWorkerEndpoint, PluginWorkerHandle, PluginWorkerReader,
    local_catalog_directory,
};
use bongocat_render::{OverlayLayerConsumer, OverlayPressSink};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the main thread re-reads the local clock.
///
/// A clock panel shows seconds, so a reading that is at most this stale is
/// indistinguishable from a live one — and the question itself is a process-wide
/// read that has no business running at the frame rate.
pub(crate) const LOCAL_TIME_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// How long shutdown waits for the worker before reporting that it did not stop.
///
/// Bounded because the worker may be inside a download, and shutdown must not block
/// on a network. Long enough for a normal stop, which is a channel receive.
pub(crate) const PLUGIN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ProductPluginHost {
    handle: PluginWorkerHandle,
    endpoint: PluginWorkerEndpoint,
    clock: Arc<LocalTimeCache>,
    /// The channel the overlay drains, and the sink it reports presses to.
    consumer: Option<OverlayLayerConsumer>,
    press_sink: Option<Arc<dyn OverlayPressSink>>,
    /// When the clock was last read, so the frame loop does not ask every frame.
    last_refresh: std::time::Instant,
}

impl ProductPluginHost {
    /// Start the worker, and hand back the two ends the overlay needs.
    ///
    /// The catalog directory is the storage layout's own, so a Development build
    /// reads a catalog a developer put there and a Production build reads one it
    /// fetched — with nothing in the executable to tell them apart.
    pub(crate) fn start(
        layout: &StorageLayout,
        runtime: Option<bongocat_runtime::RuntimeClient>,
    ) -> Result<Self, bongocat_plugin::PluginError> {
        let (producer, consumer) = bongocat_render::overlay_layer_channel();
        let clock = Arc::new(LocalTimeCache::new());
        let (handle, endpoint) = bongocat_plugin::start(
            bongocat_plugin::PluginStore::new(layout.plugins.clone()),
            local_catalog_directory(&layout.root),
            producer,
            Arc::clone(&clock),
            runtime,
        )?;
        let press_sink = Arc::new(endpoint.press_sink());
        Ok(Self {
            handle,
            endpoint,
            clock,
            consumer: Some(consumer),
            press_sink: Some(press_sink),
            last_refresh: std::time::Instant::now(),
        })
    }

    /// The channel the overlay drains for panels.
    ///
    /// Taken rather than borrowed, so the overlay is the only reader and a channel
    /// nobody reads is impossible.
    pub(crate) fn take_layer_consumer(&mut self) -> Option<OverlayLayerConsumer> {
        self.consumer.take()
    }

    /// The sink the overlay reports a press inside a panel to.
    pub(crate) fn press_sink(&self) -> Option<Arc<dyn OverlayPressSink>> {
        self.press_sink.clone()
    }

    /// A view of this worker another thread may read and command.
    pub(crate) fn reader(&self) -> PluginWorkerReader {
        self.handle.reader()
    }

    /// Apply the persisted preference to what the worker just loaded.
    ///
    /// The worker starts with every installed plugin switched on, because a plugin
    /// the user installed and never touched is on. This is where the stored
    /// preference is applied, and it only ever *switches off*: an id the
    /// configuration names that is not installed is ignored until it is, which is
    /// what makes an uninstalled plugin not a configuration error.
    pub(crate) fn apply_enabled_preference(&self, enabled: &[String]) {
        for entry in self
            .handle
            .snapshot()
            .entries
            .iter()
            .filter(|entry| entry.installed)
        {
            let id = &entry.manifest.id;
            if enabled.iter().any(|wanted| wanted == id.as_str()) {
                continue;
            }
            // A dropped command means the queue is full of work the user asked for
            // first; the panel stays on for this run and the next launch applies the
            // preference again. Failing startup over it would be worse than the
            // panel the user already turned off once.
            let _ = self
                .endpoint
                .send(bongocat_plugin::PluginCommand::SetEnabled {
                    id: id.clone(),
                    enabled: false,
                });
        }
    }

    /// Re-read the local clock if the interval has passed.
    ///
    /// Called from the frame loop, which runs on the main thread. The rate limit is
    /// here rather than in the loop because there are two frame loops — one per
    /// platform — and a clock that is read at the frame rate on one and not the
    /// other is a difference nobody would notice until they did.
    pub(crate) fn refresh_local_time(&mut self) {
        if self.last_refresh.elapsed() < LOCAL_TIME_REFRESH_INTERVAL {
            return;
        }
        self.last_refresh = std::time::Instant::now();
        self.clock.refresh();
    }

    /// Ask the worker to stop, and wait for it.
    ///
    /// The stop is explicit rather than "the handle went away" so it happens at the
    /// point in the shutdown order the product chose, and the join is bounded and
    /// reported rather than assumed: a worker that will not stop is a fact the
    /// shutdown log has to carry.
    pub(crate) fn shutdown(self) -> Result<(), String> {
        let mut stopper = self.handle.stopper(&self.endpoint);
        let queued = stopper.stop();
        self.handle
            .stop_and_join(PLUGIN_JOIN_TIMEOUT)
            .map_err(|error| error.to_string())?;
        if !queued {
            return Err("the plugin worker's stop request could not be queued".to_string());
        }
        Ok(())
    }
}

/// Stop the plugin worker if there is one, and record a worker that would not stop.
///
/// One helper because every caller is a startup-failure path with the same rule: by
/// the time any of them returns, nothing may still be publishing into a renderer
/// that is about to be released.
pub(crate) fn stop_plugin_worker(
    host: Option<ProductPluginHost>,
    failures: &Arc<Mutex<Vec<String>>>,
) {
    if let Some(host) = host
        && let Err(error) = host.shutdown()
    {
        crate::product_shutdown::record_failure(failures, error);
    }
}
