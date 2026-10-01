//! The plugin worker: one thread that owns the installed set and every session.
//!
//! Everything that happens to a plugin happens on this thread — fetching, installing,
//! starting processes, reading their messages, rasterizing their panels — and none
//! of it can reach the render path. It publishes a snapshot for the plugin center and
//! a set of layers for the model window, both through bounded latest-wins channels,
//! and it takes commands through one bounded command channel.
//!
//! # Why a worker and not a callback
//!
//! A panel changes on its own cadence and its work is milliseconds of rasterization
//! plus a pipe write. Calling into a plugin from the render thread would mean either
//! stalling a frame or copying a raster per frame; a worker means the render thread
//! only ever reads an already-published layer. The cost is that a panel lags by up to
//! one evaluation, which at the cadence a panel needs — once a second for a countdown,
//! never for a tally — is not perceptible.
//!
//! # The loop
//!
//! Four steps, in this order, every turn:
//!
//! 1. **Drain each session's messages.** Non-blocking, so a plugin that is silent
//!    costs nothing.
//! 2. **Publish the layers.** The only place a raster becomes a layer, so the panel
//!    channel is written from exactly one place.
//! 3. **Wait**, on the command channel, for a bounded interval.
//! 4. **Advance.** Ticks to plugins, the input feed, and the bubbles' lifetimes.
//!
//! The wait is the interesting part. A worker whose plugins have nothing to say does
//! not need to run at all, so the interval is short only while something is
//! clock-driven — and a press wakes it immediately, so the response is still
//! instant.

use crate::bubble::BubbleSet;
use crate::host::HostFacts;
use crate::input_feed::FeedSet;
use crate::local_time::LocalTimeCache;
use crate::model_request::ModelRequestRouter;
use crate::session::{HANDSHAKE_TIMEOUT, Incoming, Session, SessionOutcome, SessionState};
use crate::store::PluginStore;
use bongocat_audio::MotionAudioClient;
use bongocat_plugin_protocol::{
    ConfigDocument, InstalledPlugin, LogLevel, ModelRequest, PluginAnchor, PluginCatalogEntry,
    PluginError, PluginErrorCode, PluginId, PluginManifest, Subscription,
};
use bongocat_render::{OverlayLayer, OverlayLayerIds, OverlayLayerProducer};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The shortest interval a worker waits between evaluations.
///
/// Short enough that a countdown is not visibly behind and a press is answered
/// immediately, long enough that the loop is not a busy wait on a machine with four
/// idle plugins.
pub const EVALUATION_INTERVAL: Duration = Duration::from_millis(100);

/// The interval a worker waits when nothing it runs can change without a command.
///
/// A panel of nothing but a tally is not re-evaluated sixty times a second to produce
/// sixty identical rasters, so a worker whose panels are all press-driven sleeps on
/// its command channel instead. A press wakes it, so the response is still immediate.
pub const IDLE_EVALUATION_INTERVAL: Duration = Duration::from_secs(3600);

/// How many ticks a plugin may skip between two of them.
///
/// The host's own cadence is bounded at 240 Hz and a plugin does not need one message
/// per tick; this keeps the pipe from being the bottleneck on a fast machine while a
/// panel still gets a tick often enough to count seconds.
pub const MAXIMUM_TICK_INTERVAL: Duration = Duration::from_millis(250);

/// The most plugins that may be enabled at once.
///
/// The number of positions the model window has, read from the protocol rather than
/// written here, and that is the whole of why: **one plugin per position.** The bound
/// used to be four, a guess at how many panels a person could stand to look at, and it
/// refused the fifth plugin for a reason that had nothing to do with the window — two of
/// the plugins already installed could not run at the same time, which is not a limit
/// anybody would accept if it were a limit of the product's rather than of a number
/// somebody typed.
///
/// It is still a bound, and still needed: an enabled plugin is a process this worker is
/// responsible for, and nothing bounds that but this. What changed is that the number now
/// means something — it is what the model window can show without two panels landing on the
/// same corner — rather than what a designer guessed about legibility.
pub const MAXIMUM_ENABLED_PLUGINS: usize = crate::POSITIONS.len();

/// How many commands may be queued before a send is dropped.
const COMMAND_CAPACITY: usize = 32;

/// The raster scale panels are drawn at.
///
/// One device pixel per logical pixel. A plugin's panel is authored in logical pixels
/// so it is the same size relative to the window at every display scale, and the
/// renderer's own scale would double that work for a panel whose text is already
/// rasterized at the display's own resolution. Clamped by the renderer's bounds
/// rather than here, because those are the ones a texture has to satisfy.
pub const RASTER_SCALE: f32 = 1.0;

/// What the worker is doing right now.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginPhase {
    /// Working, with no operation in progress.
    Idle,
    /// Re-reading the catalog.
    RefreshingCatalog,
    /// Fetching and installing one plugin.
    Installing(PluginId),
    /// Removing one plugin and its files.
    Removing(PluginId),
    /// The last thing that happened went wrong.
    Failed(PluginError),
}

impl PluginPhase {
    /// Whether the plugin center should show a spinner.
    pub const fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// One plugin, as the plugin center lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    /// The id, and the version, as the archive the store holds or the catalog offers.
    pub manifest: PluginManifest,
    /// What the running plugin says about itself.
    ///
    /// [`None`] for a plugin that is not running — not installed, or installed and
    /// not yet started — and that is why the manifest is kept alongside it: the center
    /// can show a plugin whose process has not started yet.
    pub descriptor: Option<bongocat_plugin_protocol::PluginDescriptor>,
    pub installed: bool,
    pub enabled: bool,
    /// Whether the process is alive right now.
    pub running: bool,
    /// The plugin's own settings, as its file holds them.
    pub config: ConfigDocument,
    /// The version the catalog offers, when it offers one for this host.
    pub available_version: Option<bongocat_plugin_protocol::PluginVersion>,
    /// Whether an installed version is older than the one on offer.
    pub update_available: bool,
    /// Why this plugin cannot be installed here, when it cannot.
    pub refusal: Option<PluginError>,
    /// Why the plugin is not running, when it is not.
    pub failure: Option<PluginError>,
    /// How many times its process has been restarted this run.
    pub restarts: u32,
    /// The feeds it asked for.
    pub subscriptions: Vec<Subscription>,
    /// Where this plugin's panel is drawn, and whether the user put it there.
    ///
    /// `None` for a plugin that draws no panel at all, and for one that has not drawn one
    /// yet — a position needs a panel to be a position of, and offering a menu of nine for a
    /// sound plugin would be a control that changes nothing.
    pub position: Option<crate::Placed>,
    /// The positions this plugin may be moved to, its own first.
    ///
    /// Carried beside [`Self::position`] rather than derived from it, because a position
    /// alone cannot say which of the other eight are free: that is a fact about every other
    /// plugin, and a caller that had to work it out would be reimplementing the allocation.
    /// A position another plugin holds is *absent* from this list rather than marked
    /// unavailable, so a menu built from it offers nothing that would be refused.
    pub positions: Vec<PluginAnchor>,
    /// The controls it wants the host to draw on its card, newest list first.
    ///
    /// Empty for a plugin that offered none, for one that is not running, and for one
    /// that is installed but has not started — the list is what the plugin *said*, and
    /// a process that is not running has said nothing.
    pub actions: Vec<bongocat_plugin_protocol::PluginAction>,
    /// What this plugin has written this run, oldest first.
    ///
    /// Shown on the card rather than in the product's log file, because the product's
    /// log is a closed vocabulary of event codes and a plugin's arbitrary text would
    /// either need a code per plugin or an escape hatch, and both weaken a log the
    /// product reads by machine.
    pub log: Vec<crate::plugin_log::Line>,
}

impl PluginEntry {
    /// Whether the center's button for this plugin says "install".
    pub const fn is_installable(&self) -> bool {
        !self.installed
    }

    /// Whether it says "update".
    pub const fn is_updatable(&self) -> bool {
        self.update_available
    }

    /// The icon to show on this plugin's card.
    ///
    /// The running plugin's own icon when it is running, because a plugin may improve
    /// the emoji it ships in a later version, and the archive's otherwise — which is
    /// what a plugin the user has not installed has.
    pub fn icon(&self) -> bongocat_plugin_protocol::PluginIcon {
        self.descriptor
            .as_ref()
            .map(|descriptor| descriptor.icon.clone())
            .unwrap_or_else(|| self.manifest.display_icon())
    }
}

/// Everything the plugin center renders from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginSnapshot {
    pub revision: u64,
    pub phase: Option<PluginPhase>,
    /// Every plugin the catalog offers, plus every installed plugin the catalog
    /// does not mention, in id order.
    pub entries: Vec<PluginEntry>,
    /// Plugins installed and enabled, as the overlay shows them.
    pub active: Vec<PluginId>,
    /// The last failure, kept until something replaces it so the center shows it
    /// rather than clearing it on the next poll.
    pub last_error: Option<PluginError>,
    /// Whether a catalog has been read yet, successfully or not.
    ///
    /// Without this the center cannot tell "not looked yet" from "looked and there
    /// is nothing", and a worker that has not read one — because a read is pending,
    /// or because the first read has not finished — would render a loading state
    /// forever. An empty list is an answer, and only an unread catalog is a question.
    pub catalog_read: bool,
}

impl PluginSnapshot {
    /// The entry for one plugin.
    pub fn entry(&self, id: &PluginId) -> Option<&PluginEntry> {
        self.entries.iter().find(|entry| &entry.manifest.id == id)
    }

    /// The settings of one plugin, for the configuration panel.
    ///
    /// `None` for a plugin with no settings at all, which is the panel's own empty
    /// state — a panel that is not shown because there is nothing to change in it.
    pub fn settings_of(&self, id: &PluginId) -> Option<&bongocat_plugin_protocol::ConfigSchema> {
        self.entry(id)
            .and_then(|entry| entry.descriptor.as_ref())
            .map(|descriptor| &descriptor.config)
            .filter(|schema| !schema.fields.is_empty())
    }
}

/// What the worker reports about itself, for a log line or a diagnostics export.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluginDiagnostics {
    pub evaluations: u64,
    pub layers_published: u64,
    pub sessions_started: u64,
    pub session_failures: u64,
    pub raster_failures: u64,
    pub presses_ignored: u64,
    pub presses_handled: u64,
    pub input_dropped: u64,
    pub model_requests: u64,
    /// Model requests the product could not carry out, so a plugin that keeps asking
    /// for a motion this model does not have is visible rather than merely absent.
    pub model_requests_refused: u64,
    /// Audio files a plugin asked for that the product played.
    ///
    /// Its own counter rather than a fold into `model_requests`, because a sound is not a
    /// model reaction and a diagnostic that conflated them could not answer the question a
    /// user actually asks of it: "is the typing-sound plugin working?"
    pub sounds_played: u64,
    pub bubbles_shown: u64,
}

impl PluginDiagnostics {
    /// Every counter, summed, for the run's total.
    pub fn total(&self) -> u64 {
        self.evaluations
            + self.layers_published
            + self.sessions_started
            + self.session_failures
            + self.raster_failures
            + self.presses_ignored
            + self.presses_handled
            + self.input_dropped
            + self.model_requests
            + self.model_requests_refused
            + self.sounds_played
            + self.bubbles_shown
    }
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
    /// A press of one of the controls a plugin offered for the host to draw.
    ///
    /// Addressed by plugin id and control id rather than by layer, which is what makes
    /// this different from [`Self::Press`]: a press on the model window arrives with a
    /// layer because the overlay's hit test knows which layer was hit and nothing else,
    /// whereas this arrives from the settings window, which knows the plugin by name and
    /// bypasses the hit test entirely. Both end at the same
    /// [`bongocat_plugin_protocol::HostMessage::Press`].
    ///
    /// The control id is checked against what the plugin is currently offering before
    /// anything is sent, for the same reason a panel press is checked against the panel
    /// that declared it: a button can outlive the list it was drawn from by however long
    /// a snapshot takes to arrive, and a press must never reach a plugin that has
    /// forgotten what the id meant.
    PressAction { id: PluginId, action: String },
    /// A press inside a layer, in that layer's own raster pixels.
    ///
    /// Addressed by layer rather than by plugin id: the overlay's hit test knows
    /// which layer was hit and nothing else, and the layer ids are this worker's own
    /// allocation, so it is the only thing that can turn one back into a plugin. A
    /// layer id that names no loaded plugin is a press for a panel that has since
    /// been switched off, and is counted as ignored.
    Press { layer: u64, x: f32, y: f32 },
    /// Input events, for the plugins that asked for the feed.
    ///
    /// Not addressed by plugin, because a subscription is not a per-plugin channel: one
    /// publish fans out to every feed, and a plugin that did not ask is never sent
    /// anything. The alternative — the product asking which plugins want input and
    /// building a message per plugin — would make the host's cost depend on the number of
    /// plugins, and would mean the same keystroke is queued N times.
    Input {
        events: Vec<bongocat_plugin_protocol::InputEvent>,
    },
    /// The user's language changed.
    ///
    /// Republished on every tick rather than only at the handshake, because a plugin
    /// resolves its own strings against the locale it was handed and a user who
    /// switches language expects a panel that is in it to switch with them. A plugin
    /// running when this arrives sees the new value on its next tick.
    SetLocale { locale: String },
    /// The user moved one plugin's panel.
    ///
    /// A preference rather than a command to draw: it is applied when the next layer is
    /// published, and it does not ask the plugin to redraw, because a corner is the model's
    /// arrangement and the panel's own pixels do not change when it moves.
    SetPosition {
        id: PluginId,
        anchor: bongocat_plugin_protocol::PluginAnchor,
    },
    /// The user changed one of a plugin's settings.
    ///
    /// The whole document rather than one field, because the plugin writes its own
    /// file atomically and a patch would have to be merged by a side that does not
    /// own the file.
    SetConfig {
        id: PluginId,
        config: ConfigDocument,
    },
    /// Stop the worker.
    ///
    /// An explicit command rather than "the endpoint was dropped", because the
    /// product holds the endpoint for the whole run and the shutdown order needs the
    /// worker to stop *before* the renderer's GPU resources are released — a thread
    /// that stopped because its last handle went away would stop at an arbitrary
    /// point in that order.
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

    /// This endpoint as the sink the runtime's input stream is published through.
    pub fn input_sink(&self) -> PluginInputSink {
        PluginInputSink {
            endpoint: self.clone(),
        }
    }
}

/// The model window's presses, turned into worker commands.
///
/// The overlay holds one of these from the moment the overlay starts until it is
/// dropped, and the endpoint inside it keeps the worker's command channel open for
/// the whole run — so the sink going away is a window being replaced, not a worker
/// stopping. The worker stops on an explicit `Shutdown`.
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

/// The product's way of handing input to the plugins that asked for it.
///
/// A sink rather than a method on the endpoint, for the same reason presses are: the thing
/// that produces input is the runtime, on the platform layer's thread, and it must not
/// know what a plugin is. A sink is the whole of what it needs.
#[derive(Clone, Debug)]
pub struct PluginInputSink {
    endpoint: PluginWorkerEndpoint,
}

impl PluginInputSink {
    /// Publish events to every plugin that asked for the feed.
    ///
    /// Returns whether the batch was queued. A dropped batch is counted by the feed rather
    /// than here: the events are real edges the platform layer has already accepted, and
    /// the count that matters is the per-plugin one a plugin's own diagnostics report.
    pub fn publish(&self, events: Vec<bongocat_plugin_protocol::InputEvent>) -> bool {
        if events.is_empty() {
            return true;
        }
        self.endpoint.send(PluginCommand::Input { events })
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

/// State shared with the snapshot reader, so the plugin center can read a revision
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
/// product makes at one point in its own order, and a reader that could also stop the
/// worker would let a settings-window button take the process's shutdown hostage.
/// Two objects, one owner each, is what keeps that impossible.
#[derive(Clone, Debug)]
pub struct PluginWorkerReader {
    endpoint: PluginWorkerEndpoint,
    state: Arc<Mutex<SharedState>>,
}

impl PluginWorkerReader {
    /// The current snapshot, for the plugin center.
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
    /// The current snapshot, for the plugin center.
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
    /// The product's shutdown order needs the worker stopped *before* the renderer's
    /// GPU resources are released, and it holds the endpoint for the whole run — so
    /// stopping has to be something it asks for, at the point in the order where it
    /// wants it, rather than something that happens when the last handle goes away.
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

/// Where a worker reads its catalog from.
///
/// Decided once, by the product, because only it knows the build environment — and
/// the two environments have no other reason to differ. A Development build has a
/// directory an author can put a catalog in; a Production build has an empty one
/// and has to ask the network.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogMode {
    /// A directory beside the application's data root.
    ///
    /// What makes the author → install → see-it loop work with no network and no
    /// publish step. A directory with no catalog in it is an empty catalog, not a
    /// failure.
    Directory,
    /// The published catalog, fetched through the updater's mirrors in their order.
    Network,
}

/// Start a worker on a new thread.
///
/// The thread is named so a hang or a crash names the thing that caused it in a
/// stack dump or a task manager, which is the only clue available afterwards.
/// Start a worker on a new thread.
///
/// Every argument is a fact only the product knows — where its storage is, which
/// environment it was built for, what version it is, which language the user reads —
/// which is the point: the worker decides *how* plugins run and the product decides
/// *for whom*.
#[allow(clippy::too_many_arguments)]
pub fn start(
    store: PluginStore,
    catalog_directory: PathBuf,
    catalog_mode: CatalogMode,
    layer_producer: OverlayLayerProducer,
    clock: Arc<LocalTimeCache>,
    runtime: Option<bongocat_runtime::RuntimeClient>,
    // Where plugins keep the state they wrote, what version this build is, which language
    // the user reads, and which voice a plugin's sound request goes through — all four
    // facts only the product holds, and all four handed to plugins rather than invented
    // here. The audio client is [`MotionAudioClient::unavailable`] on a build with no audio
    // service, so this worker needs no knowledge of whether there is one.
    plugin_data: PathBuf,
    app_version: String,
    locale: String,
    audio: MotionAudioClient,
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
                catalog_mode,
                catalog_read: false,
                clock,
                router: ModelRequestRouter::new(runtime.clone()).with_audio(audio),
                runtime,
                plugin_data,
                app_version,
                state: thread_state,
                catalog: None,
                sessions: BTreeMap::new(),
                feeds: FeedSet::new(),
                bubbles: BubbleSet::new(),
                start_failures: BTreeMap::new(),
                positions: BTreeMap::new(),
                started: Instant::now(),
                last_tick: Instant::now(),
                locale,
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

/// Everything the worker needs to run.
struct Worker {
    store: PluginStore,
    layer_producer: OverlayLayerProducer,
    catalog_directory: PathBuf,
    /// The local time, published by the main thread. The worker reads it and never
    /// asks the operating system itself — see `local_time`.
    clock: Arc<LocalTimeCache>,
    /// The runtime, read once per evaluation for the facts a plugin may show.
    runtime: Option<bongocat_runtime::RuntimeClient>,
    /// Where plugins keep the state they wrote.
    ///
    /// A directory the host creates per plugin and never writes inside: everything in
    /// it belongs to the plugin, which is what keeps an update from replacing it.
    plugin_data: PathBuf,
    /// Where a plugin's model requests go.
    router: ModelRequestRouter,
    /// The application version, sent in each plugin's handshake.
    app_version: String,
    /// The user's language, republished to plugins on every tick.
    locale: String,
    state: Arc<Mutex<SharedState>>,
    catalog: Option<crate::LoadedCatalog>,
    /// Where a catalog is read from. Decided by the product, which knows the build
    /// environment, and read only here.
    catalog_mode: CatalogMode,
    /// Whether a catalog has been read. Mirrors the published snapshot and is kept
    /// here because a read is the worker's own fact, not something a caller reports.
    catalog_read: bool,
    /// The running plugins, by id.
    sessions: BTreeMap<PluginId, Session>,
    /// Each plugin's input queue, for the plugins that asked for one.
    feeds: FeedSet,
    /// The bubbles currently showing, by layer.
    bubbles: BubbleSet,
    /// Why each plugin could not be started, by id.
    ///
    /// A plugin that failed to start has no session, and a session is where
    /// [`PluginEntry::failure`] comes from — so without this the card of a plugin whose
    /// manifest would not parse, or whose process never said hello, would show no reason at
    /// all. The page-level error says *something* went wrong without saying which plugin,
    /// and one slot for every plugin means the second failure replaces the first.
    ///
    /// Cleared as soon as the plugin starts, because a start that worked is the end of the
    /// story and a stale reason beside a working plugin is its own kind of lie.
    start_failures: BTreeMap<PluginId, PluginError>,
    /// Where the user put each plugin's panel, by id.
    ///
    /// A preference rather than a reservation: a position named here for a plugin that is
    /// not drawing anything right now is not held, so switching a plugin off frees its
    /// corner for somebody else without the file being edited.
    positions: BTreeMap<PluginId, bongocat_plugin_protocol::PluginAnchor>,

    /// When this worker started, which is what a plugin's `elapsed_ms` counts from.
    started: Instant,
    /// When a tick was last sent, so the interval is bounded without depending on how
    /// often the loop happens to run.
    last_tick: Instant,
    diagnostics: PluginDiagnostics,
}

impl Worker {
    fn run(&mut self, receiver: mpsc::Receiver<PluginCommand>, layer_ids: &OverlayLayerIds) {
        let mut fonts = bongocat_plugin_render::TextMeasurer::new(
            bongocat_plugin_render::FontBook::load_system(),
        );
        self.reload(layer_ids);
        // The catalog is read after the installed set is loaded, not before it. A
        // Production build's read is a network request across several mirrors, and an
        // installed panel must not wait for it to appear — so the panels are already
        // on the model window while the page still says it is reading.
        //
        // The read is the worker's own thread, so a press that arrives while it is in
        // flight waits in the command channel rather than being lost; the channel is
        // bounded and this happens once per run.
        self.refresh_catalog();
        loop {
            let outcome = self.turn(&mut fonts, layer_ids);
            match outcome {
                Turn::Wait(duration) => match receiver.recv_timeout(duration) {
                    Ok(PluginCommand::Shutdown) => break,
                    Ok(command) => self.handle(command, &mut fonts, layer_ids),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
            }
        }
        // Every plugin is stopped before this thread ends, so nothing can still be
        // writing to a renderer that is about to be released.
        for session in self.sessions.values() {
            session.stop();
        }
        self.sessions.clear();
        self.layer_producer.close();
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped = true;
    }

    /// One pass: drain, publish, decide whether to keep waiting.
    fn turn(
        &mut self,
        fonts: &mut bongocat_plugin_render::TextMeasurer,
        layer_ids: &OverlayLayerIds,
    ) -> Turn {
        self.drain(fonts, layer_ids);
        self.publish_layers(layer_ids);
        if self.needs_clock() {
            self.advance(layer_ids);
            self.drain(fonts, layer_ids);
            self.publish_layers(layer_ids);
            return Turn::Wait(EVALUATION_INTERVAL);
        }
        // Nothing a plugin is running can change without a command. A press, a
        // configuration change or a catalog refresh all arrive on the command channel
        // and wake this, so the long wait costs nothing but a tick the panel did not
        // need.
        Turn::Wait(IDLE_EVALUATION_INTERVAL)
    }

    /// Whether anything a plugin is running can change without a command.
    fn needs_clock(&self) -> bool {
        self.sessions.values().any(|session| session.is_running())
    }

    /// Read every session's messages and act on them.
    ///
    /// Two passes, because the things acted on — the feeds, the bubbles, the model
    /// router — belong to the worker while the messages belong to a session. Taking
    /// everything first and deciding afterwards is what keeps one borrow from
    /// excluding the other, and it has the useful side effect that the order in which
    /// sessions are read does not depend on which of them had something to say.
    fn drain(
        &mut self,
        fonts: &mut bongocat_plugin_render::TextMeasurer,
        layer_ids: &OverlayLayerIds,
    ) {
        let mut pending: Vec<(PluginId, Incoming)> = Vec::new();
        for session in self.sessions.values_mut() {
            session.reap();
            while let Some(message) = session.take_message() {
                pending.push((session.id().clone(), message));
            }
        }
        for (id, message) in pending {
            let Some(session) = self.sessions.get_mut(&id) else {
                // The plugin was uninstalled between taking the message and acting on
                // it, which can only happen for a message read on the same turn as an
                // uninstall — and a message for a plugin that is gone has nowhere to go.
                continue;
            };
            match message {
                Incoming::Message(message) => {
                    let outcome = session.on_child_message(message, fonts, RASTER_SCALE);
                    self.on_outcome(&id, outcome, layer_ids);
                }
                Incoming::Stderr(line) => {
                    // Recorded on the worker's own thread rather than the reader's,
                    // because a plugin's log ring is thread-local and this is the
                    // thread the center reads it back from.
                    crate::plugin_log::record(&id, LogLevel::Info, &line);
                }
                Incoming::Exited(code) => {
                    session.on_exit(code);
                    self.diagnostics.session_failures =
                        self.diagnostics.session_failures.saturating_add(1);
                    // Its queue belongs to a process that no longer exists, and
                    // carrying it forward would show a tally that includes events from
                    // before the plugin restarted.
                    self.feeds.feed_mut(&id).clear();
                    // Published here, and this used to be missing. The exit changes what
                    // the center says about this plugin — its panel is gone and it is no
                    // longer running — and a snapshot that is only refreshed by *other*
                    // plugins' activity is a snapshot that goes on claiming a dead plugin is
                    // alive until something unrelated happens. A plugin that crashes
                    // quietly on a machine running one plugin would sit there marked
                    // running for the rest of the session, with no panel and no reason.
                    //
                    // Restarting publishes again a moment later, so this is a moment where
                    // the center is honest about a plugin being down rather than a state a
                    // user can see for long. Two publishes for one crash is the right price
                    // for a card that never lies about its own process.
                    self.publish(None, None);
                }
                Incoming::Refused(error) => session.on_refused(error),
            }
        }
    }

    /// Apply what one message from a plugin meant.
    fn on_outcome(
        &mut self,
        plugin: &PluginId,
        outcome: SessionOutcome,
        layer_ids: &OverlayLayerIds,
    ) {
        let id = plugin;
        match outcome {
            SessionOutcome::Ready => {
                self.diagnostics.sessions_started =
                    self.diagnostics.sessions_started.saturating_add(1);
                if self
                    .sessions
                    .get(id)
                    .is_some_and(|session| session.wants(Subscription::Input))
                {
                    // A feed exists only for a plugin that asked, which is what makes
                    // asking a statement of intent rather than a hint.
                    let _ = self.feeds.feed_mut(id);
                }
                self.publish(None, None);
            }
            SessionOutcome::ModelRequested { id, request } => {
                self.route_model_request(plugin, id, &request, layer_ids);
            }
            // `ActionsChanged` is here rather than left to the next thing that happens
            // to publish: the controls on a card are what the user reads to decide what
            // to press, so a timer that renamed its button from "Start" to "Pause" has to
            // repaint the moment it says so rather than whenever a panel next changes.
            SessionOutcome::ConfigChanged
            | SessionOutcome::ActionsChanged
            | SessionOutcome::Failed(_) => {
                self.publish(None, None);
            }
            _ => {}
        }
    }

    /// Advance time: tick every plugin, deliver input, expire bubbles.
    fn advance(&mut self, layer_ids: &OverlayLayerIds) {
        let now = Instant::now();
        let elapsed_ms = now
            .saturating_duration_since(self.started)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let state = self.facts().to_host_state(&self.locale, &self.app_version);
        let ticked = now.saturating_duration_since(self.last_tick) >= MAXIMUM_TICK_INTERVAL;
        if ticked {
            self.last_tick = now;
        }

        let mut running: Vec<PluginId> = self
            .sessions
            .values()
            .filter(|session| session.is_running())
            .map(|session| session.id().clone())
            .collect();
        let expired = self.bubbles.take_expired(now);
        if !expired.is_empty() {
            // A bubble that has been up long enough is withdrawn, and the channel is
            // the only way to say so — republishing without it is that withdrawal.
            self.publish_layers(layer_ids);
        }
        // A plugin that has not said hello in time is stopped rather than waited on
        // for ever: it is a process the host is responsible for, and a session that
        // never completes is a plugin that will never contribute anything.
        let mut abandoned: Vec<PluginId> = self
            .sessions
            .iter()
            .filter(|(_, session)| {
                session.state() == SessionState::Starting
                    && now.saturating_duration_since(session.started_at()) >= HANDSHAKE_TIMEOUT
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in abandoned.drain(..) {
            if let Some(session) = self.sessions.get_mut(&id) {
                session.give_up_handshake();
            }
            self.publish(
                None,
                Some(PluginError::with_detail(
                    PluginErrorCode::PluginHandshakeFailed,
                    format!(
                        "the plugin did not announce itself within {} seconds",
                        HANDSHAKE_TIMEOUT.as_secs()
                    ),
                )),
            );
        }
        // A plugin that ended is started again, up to a budget: a genuine crash should
        // not need the user to restart the app, and a plugin that cannot start at all
        // must not become a loop of processes.
        let mut restartable: Vec<PluginId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.state() == SessionState::Exited)
            .filter(|(_, session)| crate::restart_is_allowed(session.restarts()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in restartable.drain(..) {
            self.restart(&id, layer_ids);
        }
        for id in running.drain(..) {
            let wants_input = self
                .sessions
                .get(&id)
                .is_some_and(|session| session.wants(Subscription::Input));
            let batch = if wants_input {
                self.feeds.feed_mut(&id).drain()
            } else {
                Vec::new()
            };
            if let Some(session) = self.sessions.get(&id) {
                if ticked {
                    session.tick(elapsed_ms, &state, self.clock.read());
                }
                if !batch.is_empty() {
                    session.send_input(batch);
                }
            }
        }
    }

    /// Start one plugin's process again after it ended.
    ///
    /// The previous one is stopped and dropped first, so a plugin that leaked a
    /// process of its own is not joined to a second copy of itself: the host reaps the
    /// old handle and starts a new one, and the budget is what stops that becoming a
    /// loop.
    fn restart(&mut self, id: &PluginId, layer_ids: &OverlayLayerIds) {
        let Some(record) = self
            .store
            .installed()
            .into_iter()
            .find(|record| &record.id == id)
        else {
            // It was uninstalled while it was running; nothing to restart.
            self.sessions.remove(id);
            return;
        };
        let previous = self.sessions.remove(id);
        let restarts = previous
            .as_ref()
            .map_or(0, |session| session.restarts().saturating_add(1));
        if let Some(previous) = previous {
            previous.stop();
        }
        match self.start_session(&record, layer_ids) {
            Ok(mut session) => {
                session.set_restarts(restarts);
                self.start_failures.remove(id);
                self.sessions.insert(id.clone(), session);
                self.publish(None, None);
            }
            Err(error) => {
                crate::plugin_log::record(
                    id,
                    bongocat_plugin_protocol::LogLevel::Warn,
                    &format!("it could not be started again: {error}"),
                );
                self.publish(None, Some(error));
            }
        }
    }

    /// Carry out one plugin's request, or answer why not.
    ///
    /// Two owners, decided here rather than inside either: a bubble is chrome drawn on
    /// the layer channel and belongs to this worker, and everything else is the model
    /// itself and belongs to the runtime through the router. The split is by *system*
    /// rather than by request kind, which is why it is one test rather than two
    /// code paths — and it is checked in the same place the request arrived, so a
    /// plugin's own id is the id its answer carries.
    fn route_model_request(
        &mut self,
        plugin: &PluginId,
        request_id: u64,
        request: &ModelRequest,
        layer_ids: &OverlayLayerIds,
    ) {
        let Some(session) = self.sessions.get(plugin) else {
            return;
        };
        if crate::bubble::is_bubble_request(request) {
            self.show_bubble(plugin, request, request_id, layer_ids);
            return;
        }
        let subscribed = session.wants(Subscription::ModelReaction);
        if crate::sound::is_sound_request(request) {
            // A sound is the audio device rather than the model, so it gets its own path
            // through here even though both are "a request the router answers": the sound
            // has a file to check and a queue to publish to, and neither is a thing the
            // runtime's snapshot has an opinion about.
            self.play_sound(plugin, request, request_id, subscribed);
            return;
        }
        if !crate::ModelRequestRouter::routes_here(request) {
            return;
        }
        let answer = self.router.answer(request_id, request, subscribed);
        self.diagnostics.model_requests = self.diagnostics.model_requests.saturating_add(1);
        if !matches!(answer.outcome, bongocat_plugin_protocol::ModelOutcome::Done) {
            self.diagnostics.model_requests_refused =
                self.diagnostics.model_requests_refused.saturating_add(1);
        }
        if let Some(session) = self.sessions.get_mut(plugin) {
            session.write_answer(answer.id, answer.outcome);
        }
    }

    /// Play the audio file a plugin named, and answer the request.
    ///
    /// The refusal is recorded on the plugin's own log rather than the product's, for the
    /// reason the plugin log exists at all: "the file you chose is not there" is a fact
    /// about one plugin's configuration, and it is the only thing that tells a user why
    /// their click made no noise. The product's log is a closed vocabulary of event codes
    /// and this would need a code per reason.
    fn play_sound(
        &mut self,
        plugin: &PluginId,
        request: &ModelRequest,
        request_id: u64,
        subscribed: bool,
    ) {
        let outcome = self.router.play_sound(request, subscribed);
        self.diagnostics.model_requests = self.diagnostics.model_requests.saturating_add(1);
        match &outcome {
            crate::SoundOutcome::Queued => {
                self.diagnostics.sounds_played = self.diagnostics.sounds_played.saturating_add(1);
            }
            crate::SoundOutcome::Refused(refusal) => {
                self.diagnostics.model_requests_refused =
                    self.diagnostics.model_requests_refused.saturating_add(1);
                crate::plugin_log::record(
                    plugin,
                    bongocat_plugin_protocol::LogLevel::Warn,
                    &format!("it could not play a sound: {}", refusal.as_str()),
                );
            }
            crate::SoundOutcome::NotSubscribed => {
                self.diagnostics.model_requests_refused =
                    self.diagnostics.model_requests_refused.saturating_add(1);
            }
        }
        if let Some(session) = self.sessions.get_mut(plugin) {
            session.write_answer(request_id, outcome.outcome());
        }
    }

    /// Show one bubble on a plugin's own layer, and answer the request.
    ///
    /// The layer is the plugin's, so two plugins can each have a bubble without either
    /// silencing the other, and a bubble outlives no plugin: taking it down is the
    /// layer going away, not a plugin being told to stop.
    fn show_bubble(
        &mut self,
        plugin: &PluginId,
        request: &ModelRequest,
        request_id: u64,
        layer_ids: &OverlayLayerIds,
    ) {
        let Some(session) = self.sessions.get(plugin) else {
            return;
        };
        let layer = session.layer_id();
        let outcome = match crate::bubble::bubble_from(request, &self.locale, Instant::now()) {
            Some(bubble) => {
                self.bubbles.show(layer, bubble);
                self.diagnostics.bubbles_shown = self.diagnostics.bubbles_shown.saturating_add(1);
                bongocat_plugin_protocol::ModelOutcome::Done
            }
            // A hide, or anything this worker cannot make a bubble from. Answered
            // rather than dropped: a plugin that asked and got nothing would have to
            // time out to learn the answer.
            None => {
                self.bubbles.hide(layer);
                bongocat_plugin_protocol::ModelOutcome::Done
            }
        };
        if let Some(session) = self.sessions.get_mut(plugin) {
            session.write_answer(request_id, outcome);
        }
        self.publish_layers(layer_ids);
    }

    /// Publish one layer per panel, plus one per bubble.
    fn publish_layers(&mut self, layer_ids: &OverlayLayerIds) {
        let mut layers: Vec<OverlayLayer> = Vec::new();
        let mut used: Vec<u64> = Vec::new();
        let placements = self.placements();
        for session in self.sessions.values() {
            let layer_id = session.layer_id();
            if let Some(rendered) = session.rendered() {
                let mut placement = rendered.to_placement();
                // The host owns where a panel goes: the plugin said which corner it would
                // prefer, the user may have moved it, and one plugin holds each position —
                // so the anchor is read here rather than taken from the panel. Everything
                // else in the placement stays the plugin's, because a panel's size and
                // opacity are its business and a corner is the model's.
                if let Some(placed) = placements.of(session.id()) {
                    placement.anchor = placed.anchor.to_overlay_anchor();
                }
                layers.push(OverlayLayer {
                    id: layer_id,
                    placement,
                    raster: rendered.to_raster(),
                });
                used.push(layer_id);
            }
        }
        for layer in self.bubbles.layer_ids() {
            used.push(layer);
        }
        if self.layer_producer.publish_checked(layers).is_ok() {
            self.diagnostics.layers_published = self.diagnostics.layers_published.saturating_add(1);
        }
        let _ = layer_ids;
    }

    fn handle(
        &mut self,
        command: PluginCommand,
        fonts: &mut bongocat_plugin_render::TextMeasurer,
        layer_ids: &OverlayLayerIds,
    ) {
        match command {
            PluginCommand::RefreshCatalog => self.refresh_catalog(),
            PluginCommand::Install(id) => self.install(&id, layer_ids),
            PluginCommand::Uninstall(id) => self.uninstall(&id),
            PluginCommand::SetEnabled { id, enabled } => {
                self.set_enabled(&id, enabled, layer_ids);
                self.drain(fonts, layer_ids);
            }
            PluginCommand::Press { layer, x, y } => {
                if self.press(layer, x, y) {
                    self.drain(fonts, layer_ids);
                    self.publish_layers(layer_ids);
                }
            }
            PluginCommand::PressAction { id, action } => {
                // The drain is there because the answer matters: a press sent from a
                // card changes what the plugin draws and says, and the settings window
                // is also drawing a button whose label depends on it. Skipping it would
                // mean the card kept saying "Start" until the next unrelated publish.
                if self.press_action(&id, &action) {
                    self.drain(fonts, layer_ids);
                } else {
                    // A press for a control the plugin is not offering. Counted as
                    // ignored, exactly as a press outside every button on a panel is:
                    // it is a click that did not register, which is visible and
                    // recoverable, rather than a press handed to a plugin that has
                    // forgotten what the id meant.
                    self.diagnostics.presses_ignored =
                        self.diagnostics.presses_ignored.saturating_add(1);
                }
            }
            PluginCommand::SetPosition { id, anchor } => {
                self.positions.insert(id, anchor);
                // Published because a card reads the position and the settings form offers
                // it; publishing without republishing the layers would move the panel a
                // frame later than the control that moved it.
                self.publish_layers(layer_ids);
                self.publish(None, None);
            }
            PluginCommand::SetConfig { id, config } => self.set_config(&id, config),
            PluginCommand::Input { events } => {
                for feed in self.feeds.feeds_mut() {
                    for event in &events {
                        if !feed.offer(event.clone()) {
                            // An edge that did not fit is a keystroke a plugin will never
                            // see, and the feed counts it. The product's own answer is not
                            // to grow the queue: a plugin that generates thousands of edges
                            // a second is not displaying them, and a host thread that
                            // blocked on one would be a model window that stopped.
                            self.diagnostics.input_dropped =
                                self.diagnostics.input_dropped.saturating_add(1);
                        }
                    }
                }
            }
            PluginCommand::SetLocale { locale } => {
                self.locale = locale;
                self.publish(None, None);
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
            .sessions
            .values()
            .filter(|session| session.is_running())
            .map(|session| session.id().clone())
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
        state.snapshot.catalog_read = self.catalog_read;
        state.snapshot.revision = state.snapshot.revision.saturating_add(1);
    }

    /// The plugin center's list: what the catalog offers, plus what is installed.
    fn entries(&self) -> Vec<PluginEntry> {
        let mut entries: BTreeMap<PluginId, PluginEntry> = BTreeMap::new();
        for record in self.store.installed() {
            let Ok(manifest) = self.store.manifest(&record) else {
                // A record whose manifest will not parse is not listed. The failure was
                // reported when it was read; listing it here would show a row with
                // nothing in it.
                continue;
            };
            let session = self.sessions.get(&record.id);
            let facts = session.map(|session| session.facts());
            entries.insert(
                record.id.clone(),
                PluginEntry {
                    enabled: session.is_some(),
                    installed: true,
                    manifest,
                    descriptor: facts.as_ref().and_then(|facts| facts.descriptor.clone()),
                    running: facts.as_ref().is_some_and(|facts| facts.running),
                    config: facts
                        .as_ref()
                        .map(|facts| facts.config.clone())
                        .unwrap_or_default(),
                    available_version: None,
                    update_available: false,
                    refusal: None,
                    // A session's own failure first, then a start that never produced one.
                    // The order matters: a plugin that ran and then died has a session to
                    // say so, and the reason it is down is more use than why it could not
                    // start last time.
                    failure: facts
                        .as_ref()
                        .and_then(|facts| facts.failure.clone())
                        .or_else(|| self.start_failures.get(&record.id).cloned()),
                    restarts: facts.as_ref().map_or(0, |facts| facts.restarts),
                    subscriptions: facts
                        .as_ref()
                        .map(|facts| facts.subscriptions.clone())
                        .unwrap_or_default(),
                    position: self.position_of(&record.id),
                    positions: self.positions_for(&record.id),
                    actions: facts
                        .as_ref()
                        .map(|facts| facts.actions.clone())
                        .unwrap_or_default(),
                    log: crate::plugin_log::lines_for(&record.id),
                },
            );
        }
        if let Some(catalog) = &self.catalog {
            for offer in &catalog.catalog.plugins {
                let installed = entries.remove(&offer.id);
                let downloadable = offer.download_for(crate::host_platform());
                let installed_version = installed.as_ref().map(|entry| &entry.manifest.version);
                entries.insert(
                    offer.id.clone(),
                    PluginEntry {
                        manifest: installed.as_ref().map_or_else(
                            || manifest_from_catalog(offer),
                            |entry| entry.manifest.clone(),
                        ),
                        installed: installed.is_some(),
                        enabled: installed.as_ref().is_some_and(|entry| entry.enabled),
                        descriptor: installed
                            .as_ref()
                            .and_then(|entry| entry.descriptor.clone()),
                        running: installed.as_ref().is_some_and(|entry| entry.running),
                        config: installed
                            .as_ref()
                            .map(|entry| entry.config.clone())
                            .unwrap_or_default(),
                        available_version: downloadable.as_ref().ok().map(|_| offer.version),
                        update_available: installed_version.is_some_and(|version| {
                            downloadable.is_ok() && offer.version > *version
                        }),
                        refusal: downloadable.as_ref().err().cloned(),
                        failure: installed.as_ref().and_then(|entry| entry.failure.clone()),
                        restarts: installed.as_ref().map_or(0, |entry| entry.restarts),
                        subscriptions: installed
                            .as_ref()
                            .map(|entry| entry.subscriptions.clone())
                            .unwrap_or_default(),
                        position: self.position_of(&offer.id),
                        positions: self.positions_for(&offer.id),
                        actions: installed
                            .as_ref()
                            .map(|entry| entry.actions.clone())
                            .unwrap_or_default(),
                        log: installed.map(|entry| entry.log).unwrap_or_default(),
                    },
                );
            }
        }
        entries.into_values().collect()
    }

    /// Read the installed set from disk and start every plugin.
    fn reload(&mut self, layer_ids: &OverlayLayerIds) {
        for session in self.sessions.values() {
            session.stop();
        }
        self.sessions.clear();
        self.feeds = FeedSet::new();
        self.bubbles.clear();
        let mut first_error = None;
        for record in self.store.installed() {
            match self.start_session(&record, layer_ids) {
                Ok(session) => {
                    self.sessions.insert(record.id.clone(), session);
                }
                Err(error) => {
                    // A plugin that will not start is reported and skipped. The others
                    // still run: one broken plugin must not empty the model window of
                    // every other panel.
                    first_error.get_or_insert(error);
                }
            }
        }
        self.publish(Some(PluginPhase::Idle), first_error);
    }

    /// Start one plugin's process.
    fn start_session(
        &self,
        record: &InstalledPlugin,
        layer_ids: &OverlayLayerIds,
    ) -> Result<Session, PluginError> {
        let data_directory = self.plugin_data.join(record.id.as_str());
        Session::start(
            record.id.clone(),
            record.directory.clone(),
            data_directory,
            self.app_version.clone(),
            self.locale.clone(),
            layer_ids.allocate(),
        )
    }

    /// Publish a failure, as the phase and as the last error.
    ///
    /// One helper because the phase and the error carry the same value, and every
    /// call site would otherwise have to clone one of them by hand.
    fn fail(&mut self, error: PluginError) {
        self.publish(Some(PluginPhase::Failed(error.clone())), Some(error));
    }

    /// Read the catalog from wherever this build reads it, and publish the result.
    ///
    /// Publishes `catalog_read` on every path, including a failure. A read that
    /// failed *was* a read: the page's answer is "the catalog could not be read,
    /// here is why, try again", not "still reading", and a page that cannot tell
    /// those apart is a page that lies.
    fn refresh_catalog(&mut self) {
        self.publish(Some(PluginPhase::RefreshingCatalog), None);
        let read = match self.catalog_mode {
            CatalogMode::Directory => crate::load_local(&self.catalog_directory),
            CatalogMode::Network => self.fetch_catalog(),
        };
        self.catalog_read = true;
        match read {
            Ok(catalog) => {
                self.catalog = Some(catalog);
                self.publish(Some(PluginPhase::Idle), None);
            }
            Err(error) => self.fail(error),
        }
    }

    /// The published catalog, through the updater's mirrors in their order.
    ///
    /// The agent is built here rather than kept, because one is only worth holding
    /// for the length of one read and this worker may live for days. A failure to
    /// build one is a failure to read, and is reported as one — a center that
    /// silently showed an empty catalog would look like a catalog with nothing in it.
    fn fetch_catalog(&self) -> Result<crate::LoadedCatalog, PluginError> {
        let agent = crate::agent()?;
        crate::fetch_catalog(|url, timeout| crate::download_with(url, timeout, &agent))
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
        // The running process goes first, so a failed delete leaves a plugin that is
        // no longer drawn rather than one drawn from files on their way out.
        if let Some(session) = self.sessions.remove(id) {
            session.stop();
        }
        self.feeds.remove(id);
        crate::plugin_log::forget(id);
        match self.store.uninstall(id) {
            Ok(()) => self.publish(Some(PluginPhase::Idle), None),
            Err(error) => {
                let code = error;
                self.publish(Some(PluginPhase::Failed(code.clone())), Some(code))
            }
        }
    }

    /// Turn one plugin's panel on or off, and publish what changed.
    ///
    /// The two directions reach the published snapshot by different routes, and only
    /// one of them is a route. Switching **on** starts a process, and a process that
    /// starts says hello — so the handshake's own [`Self::on_outcome`] publishes, with
    /// the descriptor and the schema the plugin declared, whether it got that far or
    /// gave up. Switching **off** has no such voice: the session is removed and the
    /// process is told to stop, so nothing is left that will ever publish on this
    /// plugin's behalf.
    ///
    /// That asymmetry is the whole bug this comment exists for. An entry's `enabled` is
    /// read out of [`Self::sessions`] when the snapshot is built, so a stop that does
    /// not publish leaves the plugin center describing a plugin that is no longer
    /// there, and leaves the revision where it was — which is what the settings
    /// window's poll watches. The switch on the card therefore stayed on after the
    /// press that turned it off, and stayed on until some unrelated command happened
    /// to publish and the page redrew itself around a state the user had already
    /// reached.
    ///
    /// Published only when a session was actually there. A press for a plugin that was
    /// already off changed nothing, and a revision that moves for nothing costs the
    /// settings window a full snapshot — model catalog scan included — for a page that
    /// is not going to look different.
    fn set_enabled(&mut self, id: &PluginId, enabled: bool, layer_ids: &OverlayLayerIds) {
        if !enabled {
            // A disabled plugin's process is stopped rather than flagged, so its memory
            // and its files are released rather than held for a plugin nobody sees.
            let stopped = match self.sessions.remove(id) {
                Some(session) => {
                    session.stop();
                    true
                }
                None => false,
            };
            self.feeds.remove(id);
            // The reason it could not be started belongs to being asked for it. Leaving it
            // behind would put a start failure on a card for a plugin the user has just
            // switched off, which is not what happened.
            //
            // And the publish counts the clearing, not just the stop. A plugin that could
            // not start has no session, so switching it off stops nothing — and a snapshot
            // published only on a stop would leave the reason on the card for as long as the
            // settings window was open, which is the same "stale until something else
            // happens" this loop has now had to be corrected for twice.
            let cleared = self.start_failures.remove(id).is_some();
            if stopped || cleared {
                self.publish(None, None);
            }
            return;
        }
        if self.sessions.contains_key(id) {
            return;
        }
        if self.sessions.len() >= MAXIMUM_ENABLED_PLUGINS {
            // Refused rather than admitted. The bound is the number of positions the model
            // window has, so exceeding it would mean a panel with nowhere to go: two of
            // them would be drawn on one corner, which is the thing the whole placement
            // rule exists to prevent — and admitting one quietly would turn a limit the
            // user can see into an overlap they cannot.
            let error = PluginError::with_detail(
                PluginErrorCode::TooManyEnabled,
                format!(
                    "the model window has {MAXIMUM_ENABLED_PLUGINS} places for a panel, and \
                     they are all in use"
                ),
            );
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
        match self.start_session(&record, layer_ids) {
            Ok(session) => {
                self.start_failures.remove(id);
                self.sessions.insert(id.clone(), session);
            }
            Err(error) => {
                self.start_failures.insert(id.clone(), error.clone());
                self.publish(Some(PluginPhase::Failed(error.clone())), Some(error))
            }
        }
    }

    /// Hand a plugin the settings the user just changed.
    ///
    /// The whole document, and the plugin writes it — which is what keeps a plugin's
    /// configuration a thing its own author owns rather than a section of the
    /// application's.
    fn set_config(&mut self, id: &PluginId, config: ConfigDocument) {
        let Some(session) = self.sessions.get(id) else {
            return;
        };
        session.send_config(config);
    }

    /// Resolve a press to a button and hand it to the plugin that declared it.
    ///
    /// Reports whether anything was delivered, so a press that hit nothing does not
    /// force a re-rasterization of every other panel.
    fn press(&mut self, layer: u64, x: f32, y: f32) -> bool {
        let Some(id) = self
            .sessions
            .values()
            .find(|session| session.layer_id() == layer)
            .map(|session| session.id().clone())
        else {
            // A press for a layer that is not loaded: the panel was switched off
            // between the click and the command being read, or the overlay's hit
            // test answered a layer whose plugin has since been uninstalled.
            self.diagnostics.presses_ignored = self.diagnostics.presses_ignored.saturating_add(1);
            return false;
        };
        let Some(session) = self.sessions.get(&id) else {
            return false;
        };
        let Some(button) = session.press(x, y) else {
            self.diagnostics.presses_ignored = self.diagnostics.presses_ignored.saturating_add(1);
            return false;
        };
        session.press_button(&button);
        self.diagnostics.presses_handled = self.diagnostics.presses_handled.saturating_add(1);
        true
    }

    /// Hand a plugin a press of a control it offered for the host to draw.
    ///
    /// The counterpart to [`Self::press`], and it differs in exactly one place that
    /// matters: there is no hit test. The settings window knows which plugin it is
    /// pressing and which control, so the worker's only job is to check the control is
    /// one the plugin is *currently* offering — which [`Session::press_action`] does,
    /// against the same rule a panel press is checked against.
    ///
    /// Reports whether anything was delivered, so the caller can count the rest as
    /// ignored and know whether the drain that follows has anything to read.
    fn press_action(&mut self, id: &PluginId, action: &str) -> bool {
        let Some(session) = self.sessions.get(id) else {
            // The plugin was uninstalled between the click and the command being read.
            return false;
        };
        if !session.press_action(action) {
            return false;
        }
        self.diagnostics.presses_handled = self.diagnostics.presses_handled.saturating_add(1);
        true
    }

    /// Where every plugin's panel is drawn, right now.
    ///
    /// Computed on demand rather than cached, so there is no map to forget to update when a
    /// plugin starts, stops, draws its first panel or changes the corner it would prefer.
    /// The cost is a handful of comparisons per published snapshot, against a bug class —
    /// a card, a form and a layer disagreeing about where a panel is — that no test of the
    /// allocator alone would find.
    fn placements(&self) -> crate::Placements {
        let wanted: Vec<(PluginId, bongocat_plugin_protocol::PluginAnchor)> = self
            .sessions
            .values()
            .filter(|session| session.is_running() && session.draws_panel())
            .map(|session| {
                let preferred = session
                    .panel()
                    .map(|panel| panel.placement.anchor)
                    .unwrap_or_default();
                (session.id().clone(), preferred)
            })
            .collect();
        crate::Placements::allocate(&wanted, &self.positions)
    }

    /// One plugin's position, for the snapshot.
    fn position_of(&self, id: &PluginId) -> Option<crate::Placed> {
        self.placements().of(id)
    }

    /// The positions one plugin may be moved to, for the snapshot.
    ///
    /// Empty for a plugin with no place, and that is the only correct answer for it. A
    /// plugin that did not declare a panel has no position, so it appears in no allocation
    /// and holds none — and offering it the free positions would be a menu where every
    /// choice does the same thing, which is none. The choice would be recorded in the
    /// preferences and then ignored, because the allocation only ever considers plugins that
    /// asked for a place: a control that takes a setting and never applies it.
    fn positions_for(&self, id: &PluginId) -> Vec<PluginAnchor> {
        let placements = self.placements();
        if placements.of(id).is_none() {
            return Vec::new();
        }
        placements.available_for(id)
    }

    /// What the runtime currently says, for the facts a plugin may show.
    ///
    /// A worker with no runtime client — one started before the runtime was up, or
    /// in a test — reads nothing, so every plugin sees the hidden-window answer
    /// rather than a panic. That is the same picture a runtime in `Starting`
    /// produces, which is the honest answer for a panel shown before the model is.
    fn facts(&self) -> HostFacts {
        self.runtime
            .as_ref()
            .map_or_else(HostFacts::default, |client| {
                HostFacts::from_runtime(&client.snapshot())
            })
    }
}

/// What one turn of the loop decided to do next.
enum Turn {
    /// Keep waiting this long.
    Wait(Duration),
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
/// center can show a name, a description and an icon for it. It has no executable
/// and cannot be started — its `executable` is the id, which is not a file — so a
/// synthesized entry can never be mistaken for an installed one.
///
/// The name and description are the catalog's, in every language the catalog has
/// them in, because they are the only copy a card has before the plugin has ever
/// run: the running process's own [`bongocat_plugin_protocol::PluginDescriptor`]
/// does not exist yet, and a synthesized manifest is exactly the fallback the
/// projection reaches for. The icon comes along for the same reason — a catalog
/// entry with no icon would show the user a letter for a plugin that has one.
fn manifest_from_catalog(entry: &PluginCatalogEntry) -> PluginManifest {
    PluginManifest {
        schema_version: bongocat_plugin_protocol::PLUGIN_SCHEMA_VERSION,
        api_version: entry.api_version,
        id: entry.id.clone(),
        name: entry.name.clone(),
        version: entry.version,
        min_app_version: entry.min_app_version,
        author: entry.author.clone(),
        description: entry.description.clone(),
        icon: entry.icon.display_icon(),
        executable: entry.id.as_str().to_string(),
    }
}
