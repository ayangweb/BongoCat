//! The plugin worker: one thread that owns the installed set and the panels.
//!
//! Everything a plugin does happens on this thread — fetching, installing, running
//! behaviors, rasterizing panels — and nothing it does can reach the render path.
//! It publishes a snapshot for the plugin centre and a set of layers for the model
//! window, both through bounded latest-wins channels, and it takes commands and
//! presses through one bounded command channel.
//!
//! # Why this is a worker and not a callback
//!
//! A panel changes on its own cadence and its work is milliseconds of
//! rasterization. Calling into it from the render thread would mean either
//! stalling a frame or copying a whole raster per frame; a worker means the render
//! thread only ever reads an already-published layer. The cost is that a panel lags
//! by up to one evaluation, which at the cadence a panel needs — once a second for
//! a countdown, not sixty times — is not perceptible.

use crate::LocalTimeCache;
use crate::engine::PluginInstance;
use crate::host::HostFacts;
use crate::store::PluginStore;
use bongocat_plugin_protocol::{
    BehaviorAction, BehaviorId, InstalledPlugin, PluginCatalogEntry, PluginError, PluginErrorCode,
    PluginId, PluginManifest, SceneNode, SpacerNode,
};
use bongocat_plugin_render::{
    DecodedImage, FontBook, ImageLibrary, RenderedPanel, TextMeasurer, render_contribution,
};
use bongocat_render::{OverlayLayer, OverlayLayerIds, OverlayLayerProducer};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The cadence a worker evaluates at when something needs the clock.
pub const EVALUATION_INTERVAL: Duration = Duration::from_millis(200);

/// The cadence a worker evaluates at when only presses can change anything.
///
/// A panel of nothing but counters is not re-evaluated sixty times a second to
/// produce sixty identical rasters, so a worker whose panels are all press-driven
/// sleeps on its command channel instead. A press wakes it, so the response is
/// still immediate.
pub const IDLE_EVALUATION_INTERVAL: Duration = Duration::from_secs(3600);

/// The most plugins that may be enabled at once.
///
/// A bound rather than a preference: every enabled plugin is a layer on the model
/// window and layers overlap, so past a handful the model is not visible — which
/// defeats the point of a panel that sits beside it.
pub const MAXIMUM_ENABLED_PLUGINS: usize = 4;

/// How many commands may be queued before a send is dropped.
const COMMAND_CAPACITY: usize = 32;

/// What the worker is doing right now.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginPhase {
    /// Working, with no operation in progress.
    Idle,
    RefreshingCatalog,
    Installing(PluginId),
    Removing(PluginId),
    Failed(PluginError),
}

impl PluginPhase {
    /// Whether the plugin centre should show a spinner.
    pub const fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// One plugin, as the plugin centre lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    pub manifest: PluginManifest,
    pub installed: bool,
    pub enabled: bool,
    /// The version the catalog offers, when it offers one for this host.
    pub available_version: Option<bongocat_plugin_protocol::PluginVersion>,
    /// Whether an installed version is older than the one on offer.
    pub update_available: bool,
    /// Why this plugin cannot be installed here, when it cannot.
    pub refusal: Option<PluginError>,
}

impl PluginEntry {
    /// Whether the centre's button for this plugin says "install".
    pub const fn is_installable(&self) -> bool {
        !self.installed
    }

    /// Whether it says "update".
    pub const fn is_updatable(&self) -> bool {
        self.update_available
    }
}

/// Everything the plugin centre renders from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginSnapshot {
    pub revision: u64,
    pub phase: Option<PluginPhase>,
    /// Every plugin the catalog offers, plus every installed plugin the catalog
    /// does not mention, in id order.
    pub entries: Vec<PluginEntry>,
    /// Plugins installed and enabled, as the overlay shows them.
    pub active: Vec<PluginId>,
    /// The last failure, kept until something replaces it so the centre shows it
    /// rather than clearing it on the next poll.
    pub last_error: Option<PluginError>,
}

impl PluginSnapshot {
    /// The entry for one plugin.
    pub fn entry(&self, id: &PluginId) -> Option<&PluginEntry> {
        self.entries.iter().find(|entry| &entry.manifest.id == id)
    }
}

/// What the worker reports about itself, for a log line or a diagnostics export.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluginDiagnostics {
    pub evaluations: u64,
    pub layers_published: u64,
    pub raster_failures: u64,
    pub presses_ignored: u64,
    pub presses_handled: u64,
}

/// A request the worker takes.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginCommand {
    /// Re-read the catalog.
    RefreshCatalog,
    /// Fetch, verify and install one plugin.
    Install(PluginId),
    /// Remove one plugin and its files.
    Uninstall(PluginId),
    /// Turn a plugin's panel on or off.
    SetEnabled { id: PluginId, enabled: bool },
    /// A press inside a layer, in that layer's own raster pixels.
    ///
    /// Addressed by layer rather than by plugin id: the overlay's hit test knows
    /// which layer was hit and nothing else, and the layer ids are this worker's
    /// own allocation, so it is the only thing that can turn one back into a
    /// plugin. A layer id that names no loaded plugin is a press for a panel that
    /// has since been switched off, and is counted as ignored.
    Press { layer: u64, x: f32, y: f32 },
    /// Stop the worker.
    ///
    /// An explicit command rather than "the endpoint was dropped", because the
    /// product holds the endpoint for the whole run and the shutdown order needs
    /// the worker to stop *before* the renderer's GPU resources are released —
    /// a thread that stopped because its last handle went away would stop at an
    /// arbitrary point in that order.
    Shutdown,
}

/// The channel a worker takes commands on.
#[derive(Clone, Debug)]
pub struct PluginWorkerEndpoint {
    commands: mpsc::SyncSender<PluginCommand>,
}

impl PluginWorkerEndpoint {
    /// Queue a command, dropping it when the queue is full.
    ///
    /// `try_send` rather than a blocking send: the caller is the settings worker
    /// or the GPUI thread, and neither may block on a plugin that is downloading.
    /// A dropped command is a click that did not register, which is visible and
    /// recoverable; a blocked settings window is not.
    pub fn send(&self, command: PluginCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    /// This endpoint as the sink the model window reports presses to.
    pub fn press_sink(&self) -> PluginPressSink {
        PluginPressSink {
            endpoint: self.clone(),
        }
    }
}

/// The model window's presses, turned into worker commands.
///
/// The overlay holds one of these from the moment the overlay starts until it is
/// dropped, and the endpoint inside it keeps the worker's command channel open
/// for the whole run — so the sink going away is a window being replaced, not a
/// worker stopping. The worker stops on an explicit `Shutdown`.
#[derive(Clone, Debug)]
pub struct PluginPressSink {
    endpoint: PluginWorkerEndpoint,
}

impl bongocat_render::OverlayPressSink for PluginPressSink {
    fn press(&self, layer_id: u64, x: f32, y: f32) {
        self.endpoint.send(PluginCommand::Press {
            layer: layer_id,
            x,
            y,
        });
    }
}

/// Asks a worker to stop, once.
///
/// Held through the shutdown sequence so the stop happens at the point the product
/// chose rather than when a handle is dropped, and sent at most once so a second
/// drop does not queue a command nobody will read.
pub struct WorkerStopper {
    endpoint: PluginWorkerEndpoint,
    sent: bool,
}

impl WorkerStopper {
    /// Ask the worker to stop, reporting whether the request was queued.
    pub fn stop(&mut self) -> bool {
        if self.sent {
            return true;
        }
        self.sent = self.endpoint.send(PluginCommand::Shutdown);
        self.sent
    }
}

impl Drop for WorkerStopper {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Why a worker could not be joined.
#[derive(Debug, thiserror::Error)]
pub enum PluginWorkerJoinError {
    #[error("the plugin worker did not stop within its timeout")]
    Timeout,
    #[error("the plugin worker thread panicked: {0}")]
    Panicked(String),
}

/// One loaded plugin, with everything the worker needs to draw and press it.
struct Loaded {
    manifest: PluginManifest,
    instance: PluginInstance,
    enabled: bool,
    images: ImageLibrary,
    /// Stable while the plugin is loaded, so the overlay can keep one texture for
    /// it across content changes.
    layer_id: u64,
    /// The panel as last drawn, which is what a press is tested against.
    panel: Option<RenderedPanel>,
    /// The buttons this scene declared, in scene order.
    buttons: Vec<ButtonBinding>,
}

#[derive(Clone)]
struct ButtonBinding {
    id: String,
    target: Option<BehaviorId>,
    action: BehaviorAction,
}

/// State shared with the snapshot reader, so the plugin centre can read a revision
/// cheaply and take the full snapshot only when it moved.
#[derive(Debug)]
struct SharedState {
    snapshot: PluginSnapshot,
    stopped: bool,
}

/// A cloneable view of a worker, for a thread that reads its snapshot and sends it
/// commands.
///
/// Deliberately not the handle: joining the thread is a shutdown decision the
/// product makes at one point in its own order, and a reader that could also stop
/// the worker would let a settings-window button take the process's shutdown
/// hostage. Two objects, one owner each, is what keeps that impossible.
#[derive(Clone, Debug)]
pub struct PluginWorkerReader {
    endpoint: PluginWorkerEndpoint,
    state: Arc<Mutex<SharedState>>,
}

impl PluginWorkerReader {
    /// The current snapshot, for the plugin centre.
    pub fn snapshot(&self) -> PluginSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot
            .clone()
    }

    /// Whether a new snapshot is available since `last_seen`.
    pub fn changed_since(&self, last_seen: u64) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot
            .revision
            > last_seen
    }

    /// Queue a command, dropping it when the queue is full.
    pub fn send(&self, command: PluginCommand) -> bool {
        self.endpoint.send(command)
    }
}

/// The worker thread's handle, for joining at shutdown.
pub struct PluginWorkerHandle {
    handle: Option<std::thread::JoinHandle<()>>,
    state: Arc<Mutex<SharedState>>,
    endpoint: PluginWorkerEndpoint,
}

impl PluginWorkerHandle {
    /// The current snapshot, for the plugin centre.
    pub fn snapshot(&self) -> PluginSnapshot {
        self.snapshot_locked().clone()
    }

    /// A view of this worker another thread may read and command.
    pub fn reader(&self) -> PluginWorkerReader {
        PluginWorkerReader {
            endpoint: self.endpoint.clone(),
            state: Arc::clone(&self.state),
        }
    }

    /// A command channel that asks this worker to stop when it is dropped.
    ///
    /// The product's shutdown order needs the worker stopped *before* the
    /// renderer's GPU resources are released, and it holds the endpoint for the
    /// whole run — so stopping has to be something it asks for, at the point in the
    /// order where it wants it, rather than something that happens when the last
    /// handle goes away.
    pub fn stopper(&self, endpoint: &PluginWorkerEndpoint) -> WorkerStopper {
        WorkerStopper {
            endpoint: endpoint.clone(),
            sent: false,
        }
    }

    /// Whether a new snapshot is available since `last_seen`.
    pub fn changed_since(&self, last_seen: u64) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot
            .revision
            > last_seen
    }

    fn snapshot_locked(&self) -> PluginSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot
            .clone()
    }

    /// Stop the worker and wait for it, for a bounded time.
    ///
    /// The wait is bounded because the worker may be inside a download and
    /// shutdown may not block on a network. A timeout is reported rather than
    /// ignored: the caller decides whether an unjoined worker is acceptable at
    /// this point in the shutdown order, and it cannot if it is never told.
    pub fn stop_and_join(self, timeout: Duration) -> Result<(), PluginWorkerJoinError> {
        let deadline = Instant::now() + timeout;
        while !self.is_stopped() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !self.is_stopped() {
            return Err(PluginWorkerJoinError::Timeout);
        }
        match self.handle {
            Some(handle) => handle.join().map_err(|payload| {
                PluginWorkerJoinError::Panicked(payload.downcast_ref::<&str>().map_or_else(
                    || {
                        payload
                            .downcast_ref::<String>()
                            .cloned()
                            .unwrap_or_default()
                    },
                    |message| (*message).to_string(),
                ))
            }),
            None => Ok(()),
        }
    }

    fn is_stopped(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped
    }
}

/// Everything the worker needs to run.
struct Worker {
    store: PluginStore,
    layer_producer: OverlayLayerProducer,
    catalog_directory: PathBuf,
    /// The local time, published by the main thread. The worker reads it and
    /// never asks the operating system itself — see `local_time`.
    clock: Arc<LocalTimeCache>,
    /// The runtime, read once per evaluation for the facts a panel may show.
    /// Reading a snapshot is a lock and a copy; it is not on the render path, and
    /// a plugin that shows the active model needs it.
    runtime: Option<bongocat_runtime::RuntimeClient>,
    state: Arc<Mutex<SharedState>>,
    catalog: Option<crate::LoadedCatalog>,
    plugins: BTreeMap<PluginId, Loaded>,
    measured_at: Instant,
    diagnostics: PluginDiagnostics,
}

/// Start a worker on a new thread.
///
/// The thread is named so a hang or a crash names the thing that caused it in a
/// stack dump or a task manager, which is the only clue available afterwards.
pub fn start(
    store: PluginStore,
    catalog_directory: PathBuf,
    layer_producer: OverlayLayerProducer,
    clock: Arc<LocalTimeCache>,
    runtime: Option<bongocat_runtime::RuntimeClient>,
) -> Result<(PluginWorkerHandle, PluginWorkerEndpoint), PluginError> {
    store.create()?;
    let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
    let state = Arc::new(Mutex::new(SharedState {
        snapshot: PluginSnapshot::default(),
        stopped: false,
    }));
    let thread_state = Arc::clone(&state);
    let thread_store = store.clone();
    let handle = std::thread::Builder::new()
        .name("bongocat-plugins".to_string())
        .spawn(move || {
            let layer_ids = OverlayLayerIds::new();
            let mut worker = Worker {
                store: thread_store,
                layer_producer,
                catalog_directory,
                clock,
                runtime,
                state: thread_state,
                catalog: None,
                plugins: BTreeMap::new(),
                measured_at: Instant::now(),
                diagnostics: PluginDiagnostics::default(),
            };
            worker.run(receiver, &layer_ids);
        })
        .map_err(|error| PluginError::with_detail(PluginErrorCode::StoreWriteFailed, error))?;
    Ok((
        PluginWorkerHandle {
            handle: Some(handle),
            state,
            endpoint: PluginWorkerEndpoint {
                commands: commands.clone(),
            },
        },
        PluginWorkerEndpoint { commands },
    ))
}

impl Worker {
    fn run(&mut self, receiver: mpsc::Receiver<PluginCommand>, layer_ids: &OverlayLayerIds) {
        self.reload(layer_ids);
        let mut fonts = TextMeasurer::new(FontBook::load_system());
        // One evaluation before the first wait, so a panel whose behaviors are all
        // press-driven still appears at start-up. Without it a stopped timer would
        // not be drawn until something woke the worker, and a press-driven panel is
        // never woken by a tick.
        self.evaluate(&mut fonts);
        loop {
            let interval = if self.needs_clock() {
                EVALUATION_INTERVAL
            } else {
                IDLE_EVALUATION_INTERVAL
            };
            match receiver.recv_timeout(interval) {
                Ok(PluginCommand::Shutdown) => break,
                Ok(command) => self.handle(command, &mut fonts, layer_ids),
                Err(mpsc::RecvTimeoutError::Timeout) => self.evaluate(&mut fonts),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Nothing is published after this point: the render thread may already be
        // releasing the GPU.
        self.layer_producer.close();
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped = true;
    }

    /// Whether any panel can change without a press.
    fn needs_clock(&self) -> bool {
        self.plugins
            .values()
            .any(|plugin| plugin.enabled && plugin.instance.is_clock_driven())
    }

    fn handle(
        &mut self,
        command: PluginCommand,
        fonts: &mut TextMeasurer,
        layer_ids: &OverlayLayerIds,
    ) {
        match command {
            PluginCommand::RefreshCatalog => self.refresh_catalog(),
            PluginCommand::Install(id) => self.install(&id, layer_ids),
            PluginCommand::Uninstall(id) => self.uninstall(&id),
            PluginCommand::SetEnabled { id, enabled } => {
                self.set_enabled(&id, enabled, layer_ids);
                self.evaluate(fonts);
            }
            PluginCommand::Press { layer, x, y } => {
                if self.press(layer, x, y) {
                    self.evaluate(fonts);
                }
            }
            // Handled in `run`, which is where the loop breaks. Reaching it here
            // would mean a command was queued after the loop had already read a
            // `Shutdown`, which cannot happen; the arm exists so a future variant
            // is a compile error here rather than a silently ignored command.
            PluginCommand::Shutdown => {}
        }
    }

    /// Publish a new snapshot, with a phase and a failure to report.
    fn publish(&mut self, phase: Option<PluginPhase>, last_error: Option<PluginError>) {
        let entries = self.entries();
        let active = self
            .plugins
            .values()
            .filter(|plugin| plugin.enabled)
            .map(|plugin| plugin.manifest.id.clone())
            .collect();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if phase.is_some() {
            state.snapshot.phase = phase;
        }
        if last_error.is_some() {
            state.snapshot.last_error = last_error;
        }
        state.snapshot.entries = entries;
        state.snapshot.active = active;
        state.snapshot.revision = state.snapshot.revision.saturating_add(1);
    }

    /// The plugin centre's list: what the catalog offers, plus what is installed.
    fn entries(&self) -> Vec<PluginEntry> {
        let mut entries: BTreeMap<PluginId, PluginEntry> = BTreeMap::new();
        for record in self.store.installed() {
            let Some(manifest) = self
                .plugins
                .get(&record.id)
                .map(|plugin| plugin.manifest.clone())
                .or_else(|| self.store.manifest(&record).ok())
            else {
                // A record whose manifest will not parse is not listed. The
                // failure was reported when it was loaded; listing it here would
                // show a row with nothing in it.
                continue;
            };
            entries.insert(
                record.id.clone(),
                PluginEntry {
                    enabled: self
                        .plugins
                        .get(&record.id)
                        .is_some_and(|plugin| plugin.enabled),
                    installed: true,
                    manifest,
                    available_version: None,
                    update_available: false,
                    refusal: None,
                },
            );
        }
        if let Some(catalog) = &self.catalog {
            for offer in &catalog.catalog.plugins {
                let installed_entry = entries.remove(&offer.id);
                let downloadable = offer.download_for(crate::host_platform());
                entries.insert(
                    offer.id.clone(),
                    PluginEntry {
                        manifest: installed_entry.as_ref().map_or_else(
                            || manifest_from_catalog(offer),
                            |entry| entry.manifest.clone(),
                        ),
                        installed: installed_entry.is_some(),
                        enabled: self
                            .plugins
                            .get(&offer.id)
                            .is_some_and(|plugin| plugin.enabled),
                        available_version: downloadable
                            .as_ref()
                            .ok()
                            .map(|_| offer.version.clone()),
                        update_available: installed_entry.as_ref().is_some_and(|entry| {
                            downloadable.is_ok() && offer.version > entry.manifest.version
                        }),
                        refusal: downloadable.as_ref().err().cloned(),
                    },
                );
            }
        }
        entries.into_values().collect()
    }

    /// Read the installed set from disk and load every plugin.
    fn reload(&mut self, layer_ids: &OverlayLayerIds) {
        self.plugins.clear();
        let mut first_error = None;
        for record in self.store.installed() {
            match self.load(&record, layer_ids) {
                Ok(loaded) => {
                    self.plugins.insert(record.id.clone(), loaded);
                }
                Err(error) => {
                    // A plugin that will not load is reported and skipped. The
                    // others still run: one broken plugin must not empty the model
                    // window of every other panel.
                    first_error.get_or_insert(error);
                }
            }
        }
        self.publish(Some(PluginPhase::Idle), first_error);
    }

    fn load(
        &self,
        record: &InstalledPlugin,
        layer_ids: &OverlayLayerIds,
    ) -> Result<Loaded, PluginError> {
        let manifest = self.store.manifest(record)?;
        let instance = PluginInstance::new(manifest.id.clone(), &manifest.overlay.behaviors)?;
        let mut inspector = bongocat_plugin_protocol::scene::inspect::Inspector::new();
        bongocat_plugin_protocol::scene::inspect::walk(&manifest.overlay.scene, 1, &mut inspector)?;
        crate::validate_bindings(&manifest.overlay.behaviors, &inspector.bindings)?;
        for binding in &inspector.bindings {
            let Some((source, _)) = binding.split_once('.') else {
                continue;
            };
            if source == crate::HOST_PREFIX
                && !bongocat_plugin_protocol::HOST_BINDING_PATHS.contains(&binding.as_str())
            {
                return Err(PluginError::with_detail(
                    PluginErrorCode::UnknownBinding,
                    binding.clone(),
                ));
            }
        }
        let mut images = ImageLibrary::new();
        for asset in &inspector.assets {
            if images.get(asset).is_some() {
                continue;
            }
            if let Ok(path) = manifest.asset_path(&record.directory, asset)
                && let Ok(image) = DecodedImage::read_png(&path)
            {
                // A missing image leaves its node empty at draw time, which the
                // render pass handles. Reading it here is only so the bytes are not
                // re-read on every evaluation.
                images.insert(asset.clone(), image);
            }
        }
        let buttons = inspector
            .actions
            .iter()
            .map(|action| ButtonBinding {
                id: action.button.clone(),
                target: action.target.clone(),
                // A button's action is declared, so it is used as declared. A
                // button with no target is a decoration: pressing it counts as
                // ignored rather than changing anything.
                action: action.action,
            })
            .collect();
        Ok(Loaded {
            layer_id: layer_ids.allocate(),
            manifest,
            instance,
            enabled: true,
            images,
            panel: None,
            buttons,
        })
    }

    /// Publish a failure, as the phase and as the last error.
    ///
    /// One helper because the phase and the error carry the same value, and every
    /// call site would otherwise have to clone one of them by hand.
    fn fail(&mut self, error: PluginError) {
        self.publish(Some(PluginPhase::Failed(error.clone())), Some(error));
    }

    fn refresh_catalog(&mut self) {
        self.publish(Some(PluginPhase::RefreshingCatalog), None);
        match crate::load_local(&self.catalog_directory) {
            Ok(catalog) => {
                self.catalog = Some(catalog);
                self.publish(Some(PluginPhase::Idle), None);
            }
            Err(error) => self.fail(error),
        }
    }

    fn install(&mut self, id: &PluginId, layer_ids: &OverlayLayerIds) {
        // The catalog is *taken* rather than borrowed: the phases below publish
        // through `&mut self`, and an install that spans a publish would otherwise
        // need the catalog cloned — a document that can be half a megabyte, cloned
        // for the sake of a borrow.
        let Some(catalog) = self.catalog.take() else {
            let error = PluginError::new(PluginErrorCode::PluginNotPublished);
            self.publish(Some(PluginPhase::Failed(error.clone())), Some(error));
            return;
        };
        let resolved = catalog
            .entry_for_host(id)
            .map(|entry| (entry.clone(), catalog.clone()));
        self.catalog = Some(catalog);
        let (entry, _) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                self.publish(Some(PluginPhase::Failed(error.clone())), Some(error));
                return;
            }
        };
        self.publish(Some(PluginPhase::Installing(id.clone())), None);
        let outcome = self.fetch_and_unpack(&entry);
        match outcome {
            Ok(()) => match self.store.set_current(id, &entry.version) {
                Ok(()) => {
                    self.reload(layer_ids);
                    self.publish(Some(PluginPhase::Idle), None);
                }
                Err(error) => {
                    let code = error;
                    self.publish(Some(PluginPhase::Failed(code.clone())), Some(code))
                }
            },
            Err(error) => {
                let code = error;
                self.publish(Some(PluginPhase::Failed(code.clone())), Some(code))
            }
        }
    }

    /// Fetch, check and unpack, without making anything live.
    ///
    /// Splitting this from `install` is what makes an install atomic: every step
    /// that can fail happens before the version is marked current, so a failure
    /// leaves the previously installed version exactly where it was.
    fn fetch_and_unpack(&self, entry: &PluginCatalogEntry) -> Result<(), PluginError> {
        let download = entry.download_for(crate::host_platform())?;
        let bytes = match &download.path {
            Some(path) => self.read_local_archive(path)?,
            None => {
                let agent = crate::agent()?;
                let through = proxy_used_for_catalog();
                crate::fetch_archive(download, through, |url, timeout| {
                    crate::download_with(url, timeout, &agent)
                })?
            }
        };
        // Digest first, then signature: a digest mismatch means the bytes are not
        // what the catalog announced, and there is no point asking whether a
        // substituted archive is signed. A development catalog may carry neither,
        // which is the whole of what "no publish step" means; a published one is
        // refused at parse time unless it carries both.
        if let Some(sha256) = &download.sha256
            && !crate::digest_matches(sha256, &bytes)
        {
            return Err(PluginError::new(PluginErrorCode::ChecksumMismatch));
        }
        if let Some(signature) = &download.signature {
            crate::verify_signature(bongocat_update::RELEASE_SIGNING_KEY, &bytes, signature)?;
        }
        self.store
            .unpack(&entry.id, &entry.version, &bytes)
            .map(|_| ())
    }

    /// Read an archive the development catalog names, relative to itself.
    ///
    /// The only path a plugin archive is ever read from outside the store, and only
    /// a catalog that came out of a directory can name one. The size cap is the one
    /// a network download is held to, so a local path is not a way to make the
    /// worker allocate without bound.
    fn read_local_archive(&self, path: &str) -> Result<Vec<u8>, PluginError> {
        let bytes = std::fs::read(self.catalog_directory.join(path)).map_err(|error| {
            PluginError::with_detail(PluginErrorCode::DownloadFailed, format!("{path}: {error}"))
        })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > crate::MAXIMUM_ARCHIVE_BYTES {
            return Err(PluginError::new(PluginErrorCode::StoreWriteFailed));
        }
        Ok(bytes)
    }

    fn uninstall(&mut self, id: &PluginId) {
        self.publish(Some(PluginPhase::Removing(id.clone())), None);
        // The running instance goes first, so a failed delete leaves a plugin that
        // is no longer drawn rather than one drawn from files on their way out.
        self.plugins.remove(id);
        match self.store.uninstall(id) {
            Ok(()) => self.publish(Some(PluginPhase::Idle), None),
            Err(error) => {
                let code = error;
                self.publish(Some(PluginPhase::Failed(code.clone())), Some(code))
            }
        }
    }

    fn set_enabled(&mut self, id: &PluginId, enabled: bool, layer_ids: &OverlayLayerIds) {
        if !enabled {
            // A disabled plugin is unloaded rather than flagged, so its images and
            // its manifest are released rather than held for a panel nobody sees.
            self.plugins.remove(id);
            return;
        }
        if self.plugins.contains_key(id) {
            return;
        }
        let enabled_count = self.plugins.len();
        if enabled_count >= MAXIMUM_ENABLED_PLUGINS {
            // Refused rather than admitted: the bound is about the model staying
            // visible, and quietly exceeding it would make every panel smaller than
            // it declared.
            let error = PluginError::new(PluginErrorCode::TooManyEnabled);
            self.publish(Some(PluginPhase::Failed(error.clone())), Some(error));
            return;
        }
        let Some(record) = self
            .store
            .installed()
            .into_iter()
            .find(|record| &record.id == id)
        else {
            return;
        };
        match self.load(&record, layer_ids) {
            Ok(mut loaded) => {
                loaded.enabled = true;
                self.plugins.insert(id.clone(), loaded);
            }
            Err(error) => {
                let code = error;
                self.publish(Some(PluginPhase::Failed(code.clone())), Some(code))
            }
        }
    }

    /// Resolve a press to a button and run the action it names.
    ///
    /// Reports whether anything changed, so a press that hit nothing does not
    /// force a re-rasterization of every other panel.
    fn press(&mut self, layer: u64, x: f32, y: f32) -> bool {
        let Some(id) = self
            .plugins
            .iter()
            .find(|(_, plugin)| plugin.layer_id == layer)
            .map(|(id, _)| id.clone())
        else {
            // A press for a layer that is not loaded: the panel was switched off
            // between the click and the command being read, or the overlay's hit
            // test answered a layer whose plugin has been uninstalled.
            return false;
        };
        let Some(plugin) = self.plugins.get_mut(&id) else {
            return false;
        };
        if !plugin.enabled {
            self.diagnostics.presses_ignored = self.diagnostics.presses_ignored.saturating_add(1);
            return false;
        }
        // Tested against the panel that was published, not a freshly laid out one:
        // a press lands on what the user could see, and a layout that moved since
        // the last evaluation would otherwise make a button pressable somewhere it
        // is not drawn.
        let Some(button) = plugin
            .panel
            .as_ref()
            .and_then(|panel| panel.hit_test(x, y))
            .map(str::to_string)
        else {
            self.diagnostics.presses_ignored = self.diagnostics.presses_ignored.saturating_add(1);
            return false;
        };
        let Some(binding) = plugin
            .buttons
            .iter()
            .find(|candidate| candidate.id == button)
            .cloned()
        else {
            return false;
        };
        let Some(target) = binding.target.clone() else {
            // A button with no target draws and does nothing. Refusing it at load
            // would be stricter, but a panel that shows a pressable-looking
            // placeholder is a legitimate thing for a plugin to want.
            self.diagnostics.presses_ignored = self.diagnostics.presses_ignored.saturating_add(1);
            return false;
        };
        plugin.instance.apply(&target, binding.action);
        self.diagnostics.presses_handled = self.diagnostics.presses_handled.saturating_add(1);
        true
    }

    /// What the runtime currently says, for the facts a panel may show.
    ///
    /// A worker with no runtime client — one started before the runtime was up, or
    /// in a test — reads nothing, so every `host.` binding falls back to the empty
    /// reading rather than panicking. That is the same picture a runtime in
    /// `Starting` produces, which is the honest answer for a panel shown before the
    /// model is.
    fn facts(&self) -> HostFacts {
        self.runtime
            .as_ref()
            .map_or_else(HostFacts::default, |client| {
                let snapshot = client.snapshot();
                HostFacts::from_runtime(&snapshot)
            })
    }

    /// Advance every plugin and publish the layers.
    fn evaluate(&mut self, fonts: &mut TextMeasurer) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.measured_at);
        self.measured_at = now;
        let clock = self.clock.read();
        let facts = self.facts();
        let mut layers = Vec::new();
        for plugin in self.plugins.values_mut() {
            if !plugin.enabled {
                continue;
            }
            let mut table = plugin.instance.evaluate(elapsed, clock);
            facts.write_into(&mut table);
            match render_contribution(&plugin.manifest.overlay, &table, 1.0, fonts, &plugin.images)
            {
                Ok(panel) => {
                    if !panel.pixels.is_empty() {
                        layers.push(OverlayLayer {
                            id: plugin.layer_id,
                            placement: panel.to_placement(),
                            raster: panel.to_raster(),
                        });
                    }
                    plugin.panel = Some(panel);
                }
                Err(_) => {
                    self.diagnostics.raster_failures =
                        self.diagnostics.raster_failures.saturating_add(1);
                }
            }
        }
        self.diagnostics.evaluations = self.diagnostics.evaluations.saturating_add(1);
        if self.layer_producer.publish_checked(layers).is_ok() {
            self.diagnostics.layers_published = self.diagnostics.layers_published.saturating_add(1);
        }
        self.publish(None, None);
    }
}

/// The proxy prefix a network catalog was fetched through, if any.
///
/// A catalog that came from the official endpoint has none, and an archive is
/// then fetched from its own URL directly. This is the same choice the updater
/// makes: a run that could reach GitHub through a proxy should not then fail to
/// reach the file GitHub pointed at.
fn proxy_used_for_catalog() -> Option<&'static str> {
    // The catalog does not record which source served it, so this cannot know. A
    // Development build reads its catalog from a directory and reaches archives
    // through the same directory, so it has no proxy at all. A Production build
    // that succeeded reached the catalog through one of the prefixes, and reusing
    // the first is harmless when that guess is wrong: the archive fetch falls back
    // to the official URL, and the archive is checked against the catalog's digest
    // either way.
    bongocat_update::GITHUB_PROXY_PREFIXES.first().copied()
}

/// A manifest carrying only a catalog entry's identity.
///
/// Used for a plugin the catalog offers and this build has not installed, so the
/// centre can show a name and a description for it. It has no panel and cannot be
/// loaded — its `size` is one pixel, which the manifest bounds would refuse if it
/// were ever read from disk, so a synthesized entry can never be mistaken for an
/// installed one.
fn manifest_from_catalog(entry: &PluginCatalogEntry) -> PluginManifest {
    PluginManifest {
        schema_version: bongocat_plugin_protocol::PLUGIN_SCHEMA_VERSION,
        api_version: entry.api_version,
        id: entry.id.clone(),
        name: entry.name.clone(),
        version: entry.version.clone(),
        author: entry.author.clone(),
        description: entry.description.clone(),
        min_app_version: entry.min_app_version.clone(),
        capabilities: Vec::new(),
        icon: None,
        overlay: bongocat_plugin_protocol::OverlayContribution {
            anchor: bongocat_plugin_protocol::PluginAnchor::BottomLeft,
            margin: [0.02, 0.02],
            width_fraction: 0.72,
            opacity: 1.0,
            size: [1, 1],
            behaviors: Vec::new(),
            scene: SceneNode::Spacer(SpacerNode { grow: 1.0 }),
        },
    }
}
