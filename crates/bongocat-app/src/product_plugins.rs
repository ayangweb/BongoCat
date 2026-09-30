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
//!   cache and the frame loop calls [`ProductPluginHost::refresh_host_facts`].
//!
//! A failure to start is a degraded product, not a failed one: the user still gets
//! the cat, the plugin center says why, and nothing about the failure is allowed to
//! take the model window with it.

use bongocat_config::StorageLayout;
use bongocat_plugin::PLUGIN_CATALOG_FILE_NAME;
use bongocat_plugin::{
    CatalogMode, LocalTimeCache, PluginWorkerEndpoint, PluginWorkerHandle, PluginWorkerReader,
    local_catalog_directory,
};
use bongocat_render::{OverlayLayerConsumer, OverlayPressSink};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the main thread re-reads the process-wide facts.
///
/// The clock is the reason for the number: a clock panel shows seconds, so a reading that is
/// at most this stale is indistinguishable from a live one. The input method changes only
/// when a person switches keyboards, so it would tolerate a much longer interval — it comes
/// along because asking it here costs nothing next to the clock read that is happening
/// anyway.
pub(crate) const HOST_FACTS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// How long shutdown waits for the worker before reporting that it did not stop.
///
/// Bounded because the worker may be inside a download, and shutdown must not block
/// on a network. Long enough for a normal stop, which is a channel receive.
pub(crate) const PLUGIN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ProductPluginHost {
    handle: PluginWorkerHandle,
    endpoint: PluginWorkerEndpoint,
    clock: Arc<LocalTimeCache>,
    /// The keyboard input method, read by the same main-thread pass as the clock.
    input_method: Arc<bongocat_plugin::InputMethodCache>,
    /// The channel the overlay drains, and the sink it reports presses to.
    consumer: Option<OverlayLayerConsumer>,
    press_sink: Option<Arc<dyn OverlayPressSink>>,
    /// When the clock was last read, so the frame loop does not ask every frame.
    last_refresh: std::time::Instant,
}

impl ProductPluginHost {
    /// Start the worker, and hand back the two ends the overlay needs.
    ///
    /// The catalog directory is resolved by [`catalog_directory`], which is the
    /// only place that decides between the data root and the repository.
    /// `locale` is the user's language as a locale tag, handed to every plugin so a
    /// plugin can pick its own copy before it draws its first panel. It is a starting
    /// value rather than the only one: [`Self::apply_locale`] republishes it when the
    /// user changes language, and a plugin sees the new value on its next tick.
    pub(crate) fn start(
        layout: &StorageLayout,
        runtime: Option<bongocat_runtime::RuntimeClient>,
        locale: &str,
    ) -> Result<Self, bongocat_plugin::PluginError> {
        let (producer, consumer) = bongocat_render::overlay_layer_channel();
        let clock = Arc::new(LocalTimeCache::new());
        let input_method = Arc::new(bongocat_plugin::InputMethodCache::new());
        let (handle, endpoint) = bongocat_plugin::start(
            bongocat_plugin::PluginStore::new(layout.plugins.clone()),
            catalog_directory(layout),
            // Only the product knows the build environment, and the two differ in
            // exactly one way that matters here: a Development build has a catalog
            // directory an author can write into, and a Production build has an
            // empty one and has to ask the network.
            catalog_mode(),
            producer,
            Arc::clone(&clock),
            Arc::clone(&input_method),
            runtime,
            // A plugin's own state lives beside the plugin store rather than inside
            // it, so an update that replaces a version directory cannot replace what
            // the plugin remembered.
            layout.plugin_data.clone(),
            bongocat_app::PRODUCT_VERSION.to_string(),
            locale.to_string(),
        )?;
        let press_sink = Arc::new(endpoint.press_sink());
        Ok(Self {
            handle,
            endpoint,
            clock,
            input_method,
            consumer: Some(consumer),
            press_sink: Some(press_sink),
            last_refresh: std::time::Instant::now(),
        })
    }

    /// The endpoint, for the input forwarder and anything else that is not a snapshot.
    pub(crate) fn endpoint(&self) -> &PluginWorkerEndpoint {
        &self.endpoint
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
    /// The worker starts with every installed plugin switched on, because a plugin the
    /// user installed and never touched is on. This is where the stored preference is
    /// applied, and it only ever *switches off*: an id the configuration names that is
    /// not installed is ignored until it is, which is what makes an uninstalled plugin
    /// not a configuration error.
    ///
    /// The list is the ids the user switched **off**, not the ones they switched on, and
    /// the difference is the whole of this function: an on-list that is empty means the
    /// same thing as "I have decided about nothing" and would switch off every installed
    /// plugin on a fresh configuration — so a plugin somebody had just installed would
    /// draw nothing and say nothing.
    pub(crate) fn apply_disabled_preference(&self, disabled: &[String]) {
        for entry in self
            .handle
            .snapshot()
            .entries
            .iter()
            .filter(|entry| entry.installed)
        {
            let id = &entry.manifest.id;
            if !disabled.iter().any(|unwanted| unwanted == id.as_str()) {
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

    /// Re-read the process-wide facts if the interval has passed.
    ///
    /// Called from the frame loop, which runs on the main thread, and it reads two things:
    /// the local clock and the keyboard input method. Both are process-wide questions that
    /// only one thread may ask — the offset database because the documentation says so, the
    /// input source because two threads calling it abort inside Core Foundation — so they
    /// are asked together, here, by the thread that is allowed to.
    ///
    /// The rate limit is here rather than in the loop because there are two frame loops —
    /// one per platform — and a fact read at the frame rate on one and not the other is a
    /// difference nobody would notice until they did. Half a second is short enough that a
    /// clock panel is not visibly behind and long enough that neither read is a per-frame
    /// cost.
    pub(crate) fn refresh_host_facts(&mut self) {
        if self.last_refresh.elapsed() < HOST_FACTS_REFRESH_INTERVAL {
            return;
        }
        self.last_refresh = std::time::Instant::now();
        self.clock.refresh();
        // Translated here rather than in the plugin host, because this is the crate that
        // owns the platform: the framework's answer comes out as the platform crate's type
        // and goes in as the protocol's.
        self.input_method
            .publish(
                bongocat_platform::input_method::current_input_method().map(|method| {
                    bongocat_plugin::InputMethod {
                        id: method.id,
                        name: method.name,
                        ascii_capable: method.ascii_capable,
                    }
                }),
            );
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

/// The directory this build reads its plugin catalog from.
///
/// The data root is the answer for both environments, because a Production build
/// downloads its catalog and writes it nowhere a Development build would read it.
///
/// A **Development** run whose data root holds no catalog falls back to the
/// repository's own `plugins/` directory, which is what makes the authoring loop
/// work with no setup at all: clone, run, and the reference plugin is on the
/// model window. Without this the developer would have to know a directory path,
/// copy two files into it, and re-run — which is not a loop, it is a setup step.
///
/// The fallback is guarded twice on purpose. It only applies to a Development
/// build, decided at compile time, so a Production binary cannot take this path
/// even if it was built on a machine that has the repository; and it only applies
/// when the repository directory actually holds a catalog, so a developer who has
/// deleted theirs gets an empty catalog rather than a build-time path from
/// someone else's machine.
///
/// The data root still wins when it has a catalog, so a developer testing their
/// own archive overrides the shipped example without deleting anything.
pub(crate) fn catalog_directory(layout: &StorageLayout) -> PathBuf {
    let data = local_catalog_directory(&layout.root);
    if !data.join(PLUGIN_CATALOG_FILE_NAME).is_file()
        && matches!(
            bongocat_app::BUILD_ENVIRONMENT,
            bongocat_config::BuildEnvironment::Development
        )
        && let Some(repository) = repository_plugin_catalog()
        && repository.join(PLUGIN_CATALOG_FILE_NAME).is_file()
    {
        return repository;
    }
    data
}

/// The repository's own `plugins/` directory, when this binary was built in one.
///
/// Resolved from the executable's own recorded manifest directory rather than the
/// working directory, for the same reason the preset models are: the product must
/// behave the same however it was launched.
pub(crate) fn repository_plugin_catalog() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2)?;
    Some(root.join("plugins"))
}

/// Where this build reads its plugin catalog from.
///
/// The one thing that differs between the two environments, and it is decided here
/// because this is the only place that knows the build environment. Both read the
/// same schema, validate the same way, and show the same page; a Development build
/// just has a directory to read so the author loop needs no network.
pub(crate) fn catalog_mode() -> CatalogMode {
    match bongocat_app::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => CatalogMode::Directory,
        bongocat_config::BuildEnvironment::Production => CatalogMode::Network,
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
