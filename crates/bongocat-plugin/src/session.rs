//! One plugin's process, and the conversation with it.
//!
//! This is the piece ADR-0079 is about in code. A plugin is a program: the host
//! starts it, writes it a line and reads a line back, and it lives in its own
//! address space the whole time. Nothing here loads a library, resolves a symbol or
//! calls through a function pointer — the entire interface is two pipes and a
//! protocol, which is why the host's crates stay `#![forbid(unsafe_code)]` and why a
//! plugin that faults costs the product nothing but a card that says so.
//!
//! # The three properties that make this workable
//!
//! **A crash is a state, not a catastrophe.** The child is a process; when it exits,
//! the reader sees EOF, the session records [`SessionState::Exited`] with whatever
//! exit code it had, and the panel is withdrawn. The other plugins keep running and
//! the model window keeps drawing. Recovering is a restart, which the store already
//! knows how to do because a restart is an install.
//!
//! **The host never blocks on a plugin.** Every write goes through a bounded queue
//! drained by the reader thread, and a full queue drops the message rather than
//! waiting. A panel that is a tick behind is a panel nobody notices; a host thread
//! waiting on a plugin that stopped reading is a model window that stopped.
//!
//! **The pipes are the trust boundary, and it is an honest one.** A plugin here is
//! not sandboxed: it can read files and open sockets with the user's own
//! privileges, because it is a program the user installed. What the protocol adds is
//! not a permission system but a *narrow, written-down* way for a plugin to ask the
//! product for something — a clock, an input feed, a motion by name — so what the
//! product supports is a list rather than whatever a plugin's author was clever
//! enough to reach.

use bongocat_plugin_protocol::{
    ConfigDocument, Hello, HostMessage, InputEvent, LogLevel, ModelOutcome, PanelUpdate,
    PluginError, PluginErrorCode, PluginId, PluginMessage, Subscription, write_message,
};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a plugin has to answer the handshake before it is given up on.
///
/// Generous, because a plugin is a program that may be loading a font, a config
/// file and a socket before it says anything, and the cost of waiting is nothing —
/// the host has other work. The cost of *not* waiting is a plugin process that lives
/// forever having contributed nothing.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// How many messages may be queued for a plugin before one is dropped.
///
/// Small because the queue holds things that are true whether or not they arrive: a
/// panel update supersedes the one before it, an input batch is superseded by the
/// next batch. A deep queue would mean a plugin that is not reading, and for that the
/// answer is to drop and count, not to buffer.
const OUTBOUND_CAPACITY: usize = 32;

/// How many bytes one line may be before it is refused.
///
/// The protocol's own bound, checked here too because this is the process boundary:
/// a plugin that writes an enormous line must not make the host allocate one.
const MAXIMUM_LINE_BYTES: usize = bongocat_plugin_protocol::MAXIMUM_MESSAGE_BYTES;

/// How long the writer waits for a message before looking at the stop flag.
///
/// Short enough that a session's writer is gone within a tenth of a second of the host
/// asking it to stop, which is what closes the plugin's end of the pipe; long enough that
/// a plugin receiving one message a second costs one wake-up rather than sixty-four.
const WRITE_POLL: Duration = Duration::from_millis(100);

/// The most restarts of one plugin in a run, after which it is left alone.
///
/// A plugin that crashes on start, is restarted, crashes again is a plugin that will
/// never work; restarting it forever is a fork bomb the user cannot see. Five is
/// enough for a genuine crash-and-retry (a file locked, a port in use) and few enough
/// that a permanently broken plugin costs five processes rather than five hundred.
pub const MAXIMUM_RESTARTS: u32 = 5;

/// What a session has to say about the plugin it is talking to.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFacts {
    pub id: PluginId,
    pub descriptor: Option<bongocat_plugin_protocol::PluginDescriptor>,
    /// Whether the process is alive and the handshake is done.
    pub running: bool,
    /// The plugin's own settings, as the host last received them.
    pub config: ConfigDocument,
    /// Why the plugin is not running, when it is not.
    pub failure: Option<PluginError>,
    /// How many times the process has been restarted in this run.
    pub restarts: u32,
    /// Whether the plugin wants each feed. A plugin that did not ask is not sent one.
    pub subscriptions: Vec<Subscription>,
    /// Whether the plugin asked for a position of its own in the model window.
    pub draws_panel: bool,
    /// The controls the plugin last asked the host to draw on its card.
    ///
    /// Empty for a plugin that offered none, and for one that has not run yet — the
    /// list is what the plugin *said*, so it is empty until it says something and
    /// again once the process is gone.
    pub actions: Vec<bongocat_plugin_protocol::PluginAction>,
}

impl SessionFacts {
    /// Whether this plugin asked for a feed.
    pub fn wants(&self, subscription: Subscription) -> bool {
        self.subscriptions.contains(&subscription)
    }
}

/// Where a session is in its own life.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    /// The process has been started and the handshake is in flight.
    Starting,
    /// The handshake is done and the plugin is running.
    Running,
    /// The plugin announced a failure and stopped.
    Failed,
    /// The process ended on its own.
    Exited,
    /// The host asked the plugin to stop.
    Stopped,
}

impl SessionState {
    /// Whether the plugin is doing anything.
    pub const fn is_alive(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

/// One plugin's process, and the host's end of the conversation.
pub struct Session {
    id: PluginId,
    /// The directory the store unpacked this version into, which holds the
    /// executable, its assets and nothing else.
    directory: PathBuf,
    /// Where this plugin's own data goes, which the host creates and never writes
    /// inside.
    data_directory: PathBuf,
    state: SessionState,
    /// When this session's process was started, for the handshake's own bound.
    started_at: Instant,
    /// How many times this process has been restarted in this run.
    restarts: u32,
    descriptor: Option<bongocat_plugin_protocol::PluginDescriptor>,
    config: ConfigDocument,
    failure: Option<PluginError>,
    /// The panel the plugin last sent, waiting to be rasterized.
    panel: Option<PanelUpdate>,
    /// The images its scene named, decoded once.
    images: bongocat_plugin_render::ImageLibrary,
    /// The buttons the last panel declared, so a press can be checked against them.
    buttons: Vec<String>,
    /// The controls the plugin last offered for the host to draw on its card.
    ///
    /// Held so a press of one of them can be checked against the current list before
    /// it is delivered — the same reason [`Self::buttons`] is held, and the same rule:
    /// a press of a control the plugin is no longer offering is ignored rather than
    /// handed to a plugin that has forgotten what the id meant.
    actions: Vec<bongocat_plugin_protocol::PluginAction>,
    /// The rasterized panel, which is what a press is tested against.
    rendered: Option<bongocat_plugin_render::RenderedPanel>,
    layer_id: u64,
    outbound: SyncSender<HostMessage>,
    /// What the reader thread has handed back, drained by the worker.
    inbound: Receiver<Incoming>,
    /// The child's own end, closed when the host stops so the plugin sees EOF.
    child_stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    /// Told to stop, so the reader thread knows to end without waiting for the child.
    stopping: Arc<AtomicBool>,
    /// Counts what a session has done, for the diagnostics export.
    diagnostics: SessionDiagnostics,
}

/// What one session has cost and produced.
///
/// Counted rather than timed, because every number here answers a question a user
/// might ask after something went wrong — a plugin that is not sending panels, a
/// plugin the host is refusing to read, a panel that was sent but would not
/// rasterize — and a duration would answer none of them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionDiagnostics {
    pub messages_received: u64,
    pub messages_sent: u64,
    pub messages_dropped: u64,
    pub lines_refused: u64,
    pub panels_accepted: u64,
    pub panels_rejected: u64,
    pub raster_failures: u64,
    pub model_requests: u64,
    pub model_requests_refused: u64,
}

impl SessionDiagnostics {
    /// Every counter, summed, for the run's total.
    pub fn total(&self) -> u64 {
        self.messages_received
            + self.messages_sent
            + self.messages_dropped
            + self.lines_refused
            + self.panels_accepted
            + self.panels_rejected
            + self.raster_failures
            + self.model_requests
            + self.model_requests_refused
    }
}

impl Session {
    /// Start a plugin and begin its session.
    ///
    /// Everything that can fail about *starting* fails here and returns: a missing
    /// executable, a store directory that has gone, a path that is not the plain
    /// relative name the manifest declared. Everything after that is the child's
    /// problem and is reported through [`SessionFacts::failure`] instead, because a
    /// plugin that starts and then misbehaves is not a start failure.
    pub fn start(
        id: PluginId,
        directory: PathBuf,
        data_directory: PathBuf,
        app_version: String,
        locale: String,
        layer_id: u64,
    ) -> Result<Self, PluginError> {
        let manifest = read_manifest(&directory)?;
        let executable = manifest.executable_path(&directory).map_err(|error| {
            PluginError::with_detail(
                PluginErrorCode::PluginDirectoryUnreadable,
                error.to_string(),
            )
        })?;
        if !executable.is_file() {
            return Err(PluginError::with_detail(
                PluginErrorCode::PluginDirectoryUnreadable,
                format!(
                    "{} names an executable that is not there",
                    manifest.executable
                ),
            ));
        }
        bongocat_storage::create_private_dir_all(&data_directory)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::StoreWriteFailed, error))?;

        let (outbound, outbound_rx) = mpsc::sync_channel(OUTBOUND_CAPACITY);
        let (inbound_tx, inbound_rx) = mpsc::sync_channel(64);
        let stopping = Arc::new(AtomicBool::new(false));

        let (child, stdin) = spawn(&id, &executable, &directory, &data_directory, &stopping)?;
        let hello = Hello {
            protocol_version: bongocat_plugin_protocol::PROTOCOL_VERSION,
            app_version: app_version.clone(),
            id: id.clone(),
            version: manifest.version,
            plugin_dir: directory.display().to_string(),
            data_dir: data_directory.display().to_string(),
            locale: locale.clone(),
        };

        let mut session = Self {
            id,
            directory,
            data_directory,
            state: SessionState::Starting,
            started_at: Instant::now(),
            restarts: 0,
            descriptor: None,
            config: ConfigDocument::default(),
            failure: None,
            panel: None,
            images: bongocat_plugin_render::ImageLibrary::new(),
            buttons: Vec::new(),
            actions: Vec::new(),
            rendered: None,
            layer_id,
            outbound,
            inbound: inbound_rx,
            child_stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            stopping: Arc::clone(&stopping),
            diagnostics: SessionDiagnostics::default(),
        };
        // The hello goes through the queue rather than straight to the pipe, so the
        // writer has exactly one door. A queue this new is empty, so it cannot drop.
        session.queue(HostMessage::Hello(hello));
        session.start_reader(outbound_rx, inbound_tx);
        Ok(session)
    }

    /// Take the next thing the reader thread heard, if there is one.
    ///
    /// Non-blocking, and separate from the loop that acts on it: a plugin that is
    /// silent costs one `try_recv` per turn and nothing else, and the worker never
    /// waits on a plugin.
    pub fn take_message(&mut self) -> Option<Incoming> {
        match self.inbound.try_recv() {
            Ok(Incoming::Message(message)) => Some(Incoming::Message(message)),
            Ok(Incoming::Stderr(line)) => Some(Incoming::Stderr(line)),
            Ok(Incoming::Exited(code)) => Some(Incoming::Exited(code)),
            Ok(Incoming::Refused(error)) => Some(Incoming::Refused(error)),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Hand the plugin a press on one of the buttons its panel declared.
    pub fn press_button(&self, id: &str) -> bool {
        self.queue(HostMessage::Press { id: id.to_string() })
    }

    /// Hand the plugin a press of one of the controls it offered for its card.
    ///
    /// The same message [`Self::press_button`] sends, and refused under the same rule:
    /// only an id the plugin is *currently* offering is delivered. The window builds its
    /// card from a snapshot, so a button can outlive the list it came from by however
    /// long a poll takes — and a press of a control the plugin has since withdrawn would
    /// otherwise be handed to a plugin that has forgotten what the id meant, which is
    /// the one thing a press must never be.
    ///
    /// Returns whether the press was queued, so the caller can count the rest as
    /// ignored rather than delivered.
    pub fn press_action(&self, id: &str) -> bool {
        if !self.actions.iter().any(|action| action.id == id) {
            return false;
        }
        self.press_button(id)
    }

    /// The controls this plugin is currently offering.
    pub fn actions(&self) -> &[bongocat_plugin_protocol::PluginAction] {
        &self.actions
    }

    /// A session with the host's end of the conversation but no process behind it.
    ///
    /// For the parts of a session that are about *talking* rather than about running: the
    /// offered controls, the press routing, the outcome each message means. Those are the
    /// decisions worth testing, and testing them through a real process would mean
    /// spawning a binary to observe a list, which tests the spawn and not the list.
    #[cfg(test)]
    fn without_a_process(id: PluginId) -> Self {
        let (outbound, outbound_rx) = mpsc::sync_channel(OUTBOUND_CAPACITY);
        let (_inbound_tx, inbound_rx) = mpsc::sync_channel(1);
        // Both receivers are deliberately forgotten rather than stored. A live session's
        // writer thread holds `outbound_rx` for the session's whole life; here there is
        // no thread, so a receiver that was dropped would make every send report a
        // disconnected channel — and these tests assert *whether a press was queued*, so
        // they would be asserting on the test's own plumbing instead of on the routing.
        // Leaking one channel per test process is a cost worth paying for that.
        std::mem::forget(outbound_rx);
        Self {
            id,
            directory: PathBuf::new(),
            data_directory: PathBuf::new(),
            state: SessionState::Running,
            started_at: Instant::now(),
            restarts: 0,
            descriptor: None,
            config: ConfigDocument::default(),
            failure: None,
            panel: None,
            images: bongocat_plugin_render::ImageLibrary::new(),
            buttons: Vec::new(),
            actions: Vec::new(),
            rendered: None,
            layer_id: 0,
            outbound,
            inbound: inbound_rx,
            child_stdin: Mutex::new(None),
            child: Mutex::new(None),
            stopping: Arc::new(AtomicBool::new(false)),
            diagnostics: SessionDiagnostics::default(),
        }
    }

    /// Where this session is in its own life.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// When this session's process was started.
    pub fn started_at(&self) -> Instant {
        self.started_at
    }

    /// How many times this session's process has been restarted in this run.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Record how many times this session's process has been restarted.
    pub fn set_restarts(&mut self, restarts: u32) {
        self.restarts = restarts;
    }

    /// Give up on a process that never announced itself.
    ///
    /// Reported rather than merely stopped, because a plugin that starts and says
    /// nothing is a plugin the user needs to hear about, and "it did not answer" is the
    /// only fact that explains a card that stays empty.
    pub fn give_up_handshake(&mut self) {
        self.state = SessionState::Failed;
        self.failure = Some(PluginError::with_detail(
            PluginErrorCode::PluginHandshakeFailed,
            "the plugin did not announce itself",
        ));
        self.stop();
        self.panel = None;
        self.rendered = None;
    }

    /// This session's id.
    pub fn id(&self) -> &PluginId {
        &self.id
    }

    /// The layer this session's panel is drawn on.
    ///
    /// Allocated once and held, so the overlay keeps one texture across content
    /// changes — and so a press can be turned back into the plugin it belongs to.
    pub fn layer_id(&self) -> u64 {
        self.layer_id
    }

    /// This session's directory in the store.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Where this plugin's own data goes.
    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    /// Whether the plugin is running.
    pub fn is_running(&self) -> bool {
        self.state.is_alive()
    }

    /// Whether this plugin asked for a position of its own in the model window.
    ///
    /// False before the handshake, which is deliberate: a plugin is placed from the moment it
    /// is enabled rather than from the moment it says hello, so a panel that arrives first
    /// still gets a corner nobody else holds. And a panel a plugin draws without asking
    /// still sits where its own anchor puts it, because this decides only whether the *user*
    /// is offered a position to move.
    pub fn draws_panel(&self) -> bool {
        self.descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.draws_panel)
    }

    /// What this session has to say about its plugin.
    pub fn facts(&self) -> SessionFacts {
        SessionFacts {
            id: self.id.clone(),
            descriptor: self.descriptor.clone(),
            running: self.is_running(),
            config: self.config.clone(),
            failure: self.failure.clone(),
            restarts: self.restarts,
            draws_panel: self
                .descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.draws_panel),
            subscriptions: self
                .descriptor
                .as_ref()
                .map(|descriptor| descriptor.subscriptions.clone())
                .unwrap_or_default(),
            actions: self.actions.clone(),
        }
    }

    /// What this session has cost and produced.
    pub fn diagnostics(&self) -> SessionDiagnostics {
        self.diagnostics
    }

    /// The panel the plugin last sent, if it is still the current one.
    pub fn panel(&self) -> Option<&PanelUpdate> {
        self.panel.as_ref()
    }

    /// The rasterized panel, which is what a press is tested against.
    pub fn rendered(&self) -> Option<&bongocat_plugin_render::RenderedPanel> {
        self.rendered.as_ref()
    }

    /// The images this session has decoded for the plugin's scenes.
    pub fn images(&self) -> &bongocat_plugin_render::ImageLibrary {
        &self.images
    }

    /// Hand the child's output to the worker, and its input to the plugin.
    ///
    /// One thread for each direction, plus one per stream, because the three cannot share
    /// a thread and the reason is worth writing down because getting it wrong is silent:
    ///
    /// * **The writer runs on its own, and runs first.** A plugin that has not been sent
    ///   its `hello` has no identity, no data directory and no protocol version, so it
    ///   cannot say anything — and it will never say anything, because it is waiting for a
    ///   message that is queued behind a write that has not happened. An earlier shape
    ///   started writing only after both readers had ended, which meant no plugin ever
    ///   received a handshake and every session sat in `Starting` until the host gave up.
    /// * **The readers are separate from each other.** A plugin that fills its stderr
    ///   pipe while the host reads only stdout would deadlock, and a plugin that logs is
    ///   exactly the case where that happens.
    ///
    /// The writer waits with a timeout rather than blocking forever on the queue, so it
    /// notices a stop: a thread that outlives the session would be a thread nothing joins
    /// at shutdown.
    fn start_reader(
        &mut self,
        outbound_rx: Receiver<HostMessage>,
        inbound_tx: SyncSender<Incoming>,
    ) {
        let stdout = take_stdout(&self.child);
        let stderr = take_stderr(&self.child);
        let stdin = self
            .child_stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let stopping = Arc::clone(&self.stopping);
        let name = format!("bongocat-plugin-{}", self.id);
        // A named thread so a hang or a crash names the plugin that caused it, which
        // is the only clue available afterwards.
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            let mut readers = Vec::new();
            if let Some(stdout) = stdout {
                let inbound = inbound_tx.clone();
                readers.push(std::thread::spawn(move || {
                    read_lines(stdout, LineKind::Protocol, &inbound)
                }));
            }
            if let Some(stderr) = stderr {
                let inbound = inbound_tx.clone();
                readers.push(std::thread::spawn(move || {
                    read_lines(stderr, LineKind::Diagnostic, &inbound)
                }));
            }
            drop(inbound_tx);

            let mut writer: Option<ChildStdin> = stdin;
            loop {
                match outbound_rx.recv_timeout(WRITE_POLL) {
                    Ok(message) => {
                        if !write_line(&mut writer, &message) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // A stop is a message *and* a close, and the message is queued
                        // just before `stopping` is set — so it has already gone out, and
                        // this is where the writer ends. Dropping the pipe is what tells
                        // the plugin the session is over, so a writer that waited for
                        // another message would wait forever.
                        if stopping.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            for reader in readers {
                let _ = reader.join();
            }
        });
    }

    /// Queue one message for the plugin, dropping it when the queue is full.
    ///
    /// `try_send` rather than a blocking send, because the caller is the worker and
    /// it may be rasterizing another plugin's panel. A dropped message is a tick
    /// behind; a blocked worker is a model window that stopped.
    pub fn queue(&self, message: HostMessage) -> bool {
        match self.outbound.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Tell the plugin time passed, and publish the host's facts.
    ///
    /// One message rather than two, because a plugin that wants seconds gets them and
    /// a plugin that wants the model name gets a fresh answer without the host having
    /// to know which one it asked about.
    /// Tell the plugin time passed.
    ///
    /// One message carrying three things: the monotonic elapsed time, the host's
    /// facts, and the wall clock. The clock rides along rather than being fetched,
    /// because reading the local UTC offset is only sound when one thread at a time
    /// asks and the worker's thread is not that one — so the main thread reads it and
    /// the plugin is handed the answer.
    pub fn tick(
        &self,
        elapsed_ms: u64,
        state: &bongocat_plugin_protocol::HostState,
        clock: crate::WallClock,
    ) -> bool {
        self.queue(HostMessage::Tick {
            elapsed_ms,
            state: state.clone(),
            clock: bongocat_plugin_protocol::WallClock::new(clock.hour, clock.minute, clock.second),
        })
    }

    /// Hand the plugin the input events it asked for.
    pub fn send_input(&self, events: Vec<InputEvent>) -> bool {
        if events.is_empty() {
            return true;
        }
        self.queue(HostMessage::Input { events })
    }

    /// Hand the plugin a press on one of its own buttons.
    ///
    /// Tested against the panel that was published rather than a freshly laid out
    /// one: a press lands on what the user could see, and a panel that moved since
    /// the last evaluation would otherwise make a button pressable somewhere it is
    /// not drawn.
    pub fn press(&self, x: f32, y: f32) -> Option<String> {
        let button = self.rendered.as_ref()?.hit_test(x, y)?.to_string();
        if !self.buttons.contains(&button) {
            return None;
        }
        Some(button)
    }

    /// Hand the plugin the whole configuration document.
    ///
    /// The whole document rather than a patch, because the plugin writes its own file
    /// atomically and a patch would have to be merged by a side that does not own the
    /// file.
    pub fn send_config(&self, config: ConfigDocument) -> bool {
        self.queue(HostMessage::ConfigChanged { config })
    }

    /// Ask the plugin to stop, and close its pipes.
    ///
    /// The stop is a message *and* a close: a plugin that reads the message gets to
    /// flush its state, and one that ignores it still sees EOF when the pipes go, so
    /// there is no way to be left with a process the host has to kill.
    pub fn stop(&self) {
        self.queue(HostMessage::Shutdown);
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut stdin) = self.child_stdin.lock() {
            *stdin = None;
        }
    }

    /// Reap the child, if it has ended.
    ///
    /// Non-blocking, and called from the worker's own loop rather than from a second
    /// thread: `try_wait` is a single syscall on every platform this ships, and a
    /// thread per plugin that existed only to wait would be one more thing to join at
    /// shutdown for no gain.
    pub fn reap(&self) {
        let Ok(mut guard) = self.child.lock() else {
            return;
        };
        let Some(child) = guard.as_mut() else {
            return;
        };
        // The exit itself is reported by the reader's EOF, which carries the code;
        // reaping here only stops the handle from being kept for a process that has
        // already gone.
        if matches!(child.try_wait(), Ok(Some(_))) {
            *guard = None;
        }
    }

    /// Act on one thing the plugin said.
    ///
    /// Returns what the worker should do about it, so the decision is made once and
    /// in one place rather than by each caller guessing. A panel is rasterized here
    /// rather than returned, because the raster belongs to the session that owns the
    /// panel and the textures the overlay holds.
    pub fn on_child_message(
        &mut self,
        message: PluginMessage,
        fonts: &mut bongocat_plugin_render::TextMeasurer,
        scale: f32,
    ) -> SessionOutcome {
        self.diagnostics.messages_received = self.diagnostics.messages_received.saturating_add(1);
        match message {
            PluginMessage::Ready { descriptor } => self.on_ready(*descriptor),
            PluginMessage::Panel(update) => {
                if self.accept_panel(*update, fonts, scale) {
                    SessionOutcome::PanelChanged
                } else {
                    SessionOutcome::Ignored
                }
            }
            PluginMessage::HidePanel => {
                self.panel = None;
                self.rendered = None;
                SessionOutcome::PanelWithdrawn
            }
            PluginMessage::ConfigChanged { config } => {
                self.config = config;
                SessionOutcome::ConfigChanged
            }
            PluginMessage::Actions { actions } => {
                // Replaced rather than merged, and this is where that rule is enforced:
                // a plugin that stops wanting a control must be able to make it go away,
                // and a press of an id that is no longer in the list has to be ignored
                // rather than delivered to a plugin that has forgotten what it meant.
                if bongocat_plugin_protocol::check_actions(&actions).is_err() {
                    // A list the host cannot draw is refused rather than partially
                    // applied. Half a card's buttons — some renamed, some missing — is a
                    // state a user cannot act on and a plugin author cannot debug from
                    // the card, so the previous list stands and the refusal is logged.
                    self.diagnostics.lines_refused =
                        self.diagnostics.lines_refused.saturating_add(1);
                    crate::plugin_log::record(
                        &self.id,
                        LogLevel::Warn,
                        "a list of actions was refused, so the previous one still stands",
                    );
                    return SessionOutcome::Ignored;
                }
                if self.actions == actions {
                    // Nothing changed, so nothing to tell the worker. A plugin that
                    // re-sends an unchanged list every tick then costs one comparison
                    // rather than a snapshot revision and a card rebuild.
                    return SessionOutcome::Ignored;
                }
                self.actions = actions;
                SessionOutcome::ActionsChanged
            }
            PluginMessage::Log { level, message } => {
                crate::plugin_log::record(&self.id, level, &message);
                SessionOutcome::Ignored
            }
            PluginMessage::Failed { error } => {
                self.failure = Some(error.clone());
                self.state = SessionState::Failed;
                self.panel = None;
                self.rendered = None;
                SessionOutcome::Failed(error)
            }
            PluginMessage::Shutdown => {
                self.state = SessionState::Stopped;
                self.panel = None;
                self.rendered = None;
                SessionOutcome::Ended
            }
            PluginMessage::Request { id, request } => {
                // The only message a plugin sends that asks for something. It is
                // handed back rather than answered here, because answering needs the
                // worker's model router and its bubble set — neither of which belongs
                // to a session.
                SessionOutcome::ModelRequested {
                    id,
                    request: *request,
                }
            }
            PluginMessage::Answer(_) => {
                // Travels the other way. A plugin echoing one of ours back is a plugin
                // built against a different protocol, and answering it would be
                // inventing a conversation it is not having.
                self.diagnostics.lines_refused = self.diagnostics.lines_refused.saturating_add(1);
                SessionOutcome::Ignored
            }
        }
    }

    fn on_ready(
        &mut self,
        descriptor: bongocat_plugin_protocol::PluginDescriptor,
    ) -> SessionOutcome {
        if let Err(error) = descriptor.validate() {
            self.failure = Some(error.clone());
            self.state = SessionState::Failed;
            return SessionOutcome::Failed(error);
        }
        self.descriptor = Some(descriptor);
        self.state = SessionState::Running;
        // The panel's very first rasterization has no images loaded, so this is
        // where the plugin's own assets are read — once, by the paths its first scene
        // named. A later scene naming a new one is a miss the draw pass handles.
        self.diagnostics.panels_accepted = self.diagnostics.panels_accepted.saturating_add(0);
        SessionOutcome::Ready
    }

    /// Check and rasterize a panel, keeping it only if it is drawable.
    fn accept_panel(
        &mut self,
        update: PanelUpdate,
        fonts: &mut bongocat_plugin_render::TextMeasurer,
        scale: f32,
    ) -> bool {
        if let Err(error) = update.validate() {
            self.diagnostics.panels_rejected = self.diagnostics.panels_rejected.saturating_add(1);
            crate::plugin_log::record(
                &self.id,
                LogLevel::Warn,
                &format!("a panel was refused: {error}"),
            );
            return false;
        }
        self.load_images(&update);
        self.buttons = declared_buttons(&update);
        match bongocat_plugin_render::render_update(&update, scale, fonts, &self.images) {
            Ok(panel) => {
                if panel.pixels.is_empty() {
                    // An empty raster is not an error — it is a panel whose scene
                    // drew nothing — but it is also not worth publishing, and
                    // publishing one would make the overlay hold an empty texture.
                    self.panel = Some(update);
                    self.rendered = Some(panel);
                    self.diagnostics.panels_accepted =
                        self.diagnostics.panels_accepted.saturating_add(1);
                    return true;
                }
                self.panel = Some(update);
                self.rendered = Some(panel);
                self.diagnostics.panels_accepted =
                    self.diagnostics.panels_accepted.saturating_add(1);
                true
            }
            Err(error) => {
                self.diagnostics.raster_failures =
                    self.diagnostics.raster_failures.saturating_add(1);
                crate::plugin_log::record(
                    &self.id,
                    LogLevel::Warn,
                    &format!("a panel could not be drawn: {error}"),
                );
                false
            }
        }
    }

    /// Decode any image the scene named that this session has not read.
    ///
    /// Once per asset rather than once per panel: a panel is redrawn on every tick,
    /// and re-reading a PNG for a countdown that ticks once a second would be the
    /// most expensive thing the worker does.
    fn load_images(&mut self, update: &PanelUpdate) {
        let mut inspector = bongocat_plugin_protocol::scene::inspect::Inspector::new();
        if bongocat_plugin_protocol::scene::inspect::walk(&update.scene, 1, &mut inspector).is_err()
        {
            return;
        }
        for asset in inspector.assets {
            if self.images.get(&asset).is_some() {
                continue;
            }
            let Ok(path) = update_asset_path(&self.directory, &asset) else {
                continue;
            };
            // A missing image leaves its node empty at draw time, which the render
            // pass handles. Reading it here is only so the bytes are not re-read.
            if let Ok(image) = bongocat_plugin_render::DecodedImage::read_png(&path) {
                self.images.insert(asset, image);
            }
        }
    }

    /// Record that the process ended, and withdraw its panel.
    pub fn on_exit(&mut self, code: Option<i32>) {
        if self.state == SessionState::Stopped {
            return;
        }
        self.state = SessionState::Exited;
        self.panel = None;
        self.rendered = None;
        self.failure = Some(PluginError::with_detail(
            PluginErrorCode::PluginExited,
            match code {
                Some(code) => format!("the plugin's process exited with code {code}"),
                None => "the plugin's process ended".to_string(),
            },
        ));
    }

    /// Record that a line could not be read, and count it.
    pub fn on_refused(&mut self, error: PluginError) {
        self.diagnostics.lines_refused = self.diagnostics.lines_refused.saturating_add(1);
        crate::plugin_log::record(
            &self.id,
            LogLevel::Warn,
            &format!("a line was refused: {error}"),
        );
    }

    /// Whether this plugin wants a feed.
    pub fn wants(&self, subscription: Subscription) -> bool {
        self.descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.wants(subscription))
    }

    /// Write one answer to the plugin's pipe.
    ///
    /// A separate door from [`Self::queue`] because an answer is the one message the
    /// host produces *in reply*, and a reply that sat behind the plugin's own outbound
    /// queue behind a panel update would be a reply the plugin learned something from
    /// after it had already given up waiting.
    pub(crate) fn write_answer(&self, id: u64, outcome: ModelOutcome) -> bool {
        let Some(line) = write_message(&HostMessage::ModelAnswer { id, outcome }) else {
            return false;
        };
        let mut framed = line;
        framed.push(b'\n');
        let Ok(mut guard) = self.child_stdin.lock() else {
            return false;
        };
        let Some(writer) = guard.as_mut() else {
            return false;
        };
        matches!(writer.write_all(&framed), Ok(())) && matches!(writer.flush(), Ok(()))
    }
}

/// One thing that arrived from a plugin's process.
pub enum Incoming {
    /// A message the plugin sent.
    Message(PluginMessage),
    /// A line from the plugin's own stderr.
    Stderr(String),
    /// The process ended.
    Exited(Option<i32>),
    /// A line the host refused.
    Refused(PluginError),
}

/// What the worker should do about one thing a plugin said.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionOutcome {
    /// The handshake is done and the plugin is running.
    Ready,
    /// A new panel is on the model window.
    PanelChanged,
    /// The plugin took its panel down.
    PanelWithdrawn,
    /// The plugin's settings changed.
    ConfigChanged,
    /// The controls the plugin wants drawn on its card changed.
    ///
    /// Its own outcome rather than folded into [`Self::Ignored`] because it moves
    /// something the settings window is drawing: without a worker republish here, a
    /// timer that renamed its button from "Start" to "Pause" would leave the card
    /// showing the old word until something else happened to move the snapshot.
    ActionsChanged,
    /// The plugin failed, and said why.
    Failed(PluginError),
    /// The plugin is finished.
    Ended,
    /// The plugin asked the product to do something.
    ModelRequested {
        id: u64,
        request: bongocat_plugin_protocol::ModelRequest,
    },
    /// Nothing to do.
    Ignored,
}

impl SessionOutcome {
    /// Whether the panel channel needs publishing after this.
    pub fn changed_the_panel(self) -> bool {
        matches!(self, Self::PanelChanged | Self::PanelWithdrawn)
    }

    /// Whether this outcome is a failure the user should see.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// The buttons a scene declared, in scene order.
fn declared_buttons(update: &PanelUpdate) -> Vec<String> {
    let mut inspector = bongocat_plugin_protocol::scene::inspect::Inspector::new();
    if bongocat_plugin_protocol::scene::inspect::walk(&update.scene, 1, &mut inspector).is_err() {
        return Vec::new();
    }
    inspector.buttons
}

/// An image the scene named, resolved inside the plugin's own directory.
fn update_asset_path(directory: &Path, asset: &str) -> Result<PathBuf, PluginError> {
    bongocat_plugin_protocol::validate_relative_asset_path(asset)?;
    Ok(directory.join(asset))
}

/// Read and check the manifest beside an executable.
fn read_manifest(
    directory: &Path,
) -> Result<bongocat_plugin_protocol::PluginManifest, PluginError> {
    let path = directory.join(bongocat_plugin_protocol::PLUGIN_MANIFEST_FILE_NAME);
    let bytes = std::fs::read(&path).map_err(|error| {
        PluginError::with_detail(PluginErrorCode::PluginDirectoryUnreadable, error)
    })?;
    bongocat_plugin_protocol::PluginManifest::parse(&bytes)
}

/// Start the process, with its pipes.
///
/// Two details that are load-bearing rather than incidental. The child's **current
/// directory** is its own, so a plugin that opens a relative path reads its own
/// assets rather than whatever directory the app happened to be launched from. And
/// its **environment** is the host's, unmodified: a plugin that shells out to `git`
/// or `node` needs a real `PATH`, and rewriting one would be the host deciding what
/// a plugin may run.
///
/// The two variables the host *does* set are facts about this plugin, and they are passed
/// in rather than derived from the directory. `BONGOCAT_PLUGIN_ID` used to be the
/// directory's own file name, which is a shortcut that is true for a directory named after
/// its plugin and false for a real installation: the store lays plugins out as
/// `plugins/<id>/<version>/`, so a shipped plugin was told it was called `1.0.0`. Nothing
/// in the product read the variable, so no symptom showed it — but it is named as an id
/// and a plugin author is entitled to believe it is one, and the `hello` on this session's
/// stdin says the same thing correctly.
fn spawn(
    id: &PluginId,
    executable: &Path,
    directory: &Path,
    data_directory: &Path,
    stopping: &Arc<AtomicBool>,
) -> Result<(Child, ChildStdin), PluginError> {
    let mut command = Command::new(executable);
    command
        .current_dir(directory)
        .env("BONGOCAT_PLUGIN_ID", id.as_str())
        .env("BONGOCAT_PLUGIN_DATA", data_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| PluginError::with_detail(PluginErrorCode::PluginSpawnFailed, error))?;
    let stdin = child.stdin.take().ok_or_else(|| {
        PluginError::with_detail(
            PluginErrorCode::PluginSpawnFailed,
            "the plugin's standard input could not be opened",
        )
    })?;
    let _ = stopping;
    Ok((child, stdin))
}

fn take_stdout(child: &Mutex<Option<Child>>) -> Option<std::process::ChildStdout> {
    child
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
        .and_then(|child| child.stdout.take())
}

fn take_stderr(child: &Mutex<Option<Child>>) -> Option<std::process::ChildStderr> {
    child
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
        .and_then(|child| child.stderr.take())
}

/// Write one line to the plugin, if its end is still open.
fn write_line(writer: &mut Option<ChildStdin>, message: &HostMessage) -> bool {
    let Some(writer) = writer.as_mut() else {
        return false;
    };
    let Some(mut line) = write_message(message) else {
        return false;
    };
    line.push(b'\n');
    matches!(writer.write_all(&line), Ok(())) && matches!(writer.flush(), Ok(()))
}

/// Read one of the child's streams until it ends.
///
/// `Protocol` lines are length-checked and parsed; `Diagnostic` lines are forwarded as
/// they are. The two are not the same kind of thing — one is the wire and one is the
/// plugin talking to itself — and treating them alike is either how a plugin's own log
/// line becomes a refused protocol line, or how a protocol line becomes a log entry.
fn read_lines<R: std::io::Read>(stream: R, kind: LineKind, inbound: &SyncSender<Incoming>) {
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        let event = match kind {
            LineKind::Protocol if line.len() > MAXIMUM_LINE_BYTES => {
                Incoming::Refused(PluginError::with_detail(
                    PluginErrorCode::ProtocolInvalid,
                    format!("a protocol line is longer than {MAXIMUM_LINE_BYTES} bytes"),
                ))
            }
            LineKind::Protocol => {
                match bongocat_plugin_protocol::parse_plugin_message(line.as_bytes()) {
                    Ok(message) => Incoming::Message(message),
                    Err(error) => Incoming::Refused(error),
                }
            }
            // Forwarded rather than parsed: stderr is the plugin's own log, and the
            // host's answer is to write it down, not to interpret it.
            LineKind::Diagnostic => Incoming::Stderr(line),
        };
        if inbound.send(event).is_err() {
            break;
        }
    }
    if matches!(kind, LineKind::Protocol) {
        // The end of a plugin's stdout is the end of the plugin, and it is reported once
        // however many lines came before it. The exit code arrives separately, from the
        // worker's own `reap`.
        let _ = inbound.send(Incoming::Exited(None));
    }
}

/// Which of a plugin's two output streams a line came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LineKind {
    /// stdout: the protocol, and the only stream whose end ends the session.
    Protocol,
    /// stderr: the plugin's own log, forwarded and never interpreted.
    Diagnostic,
}

/// Whether a restart is still allowed, and why not when it is not.
pub const fn restart_is_allowed(restarts: u32) -> bool {
    restarts < MAXIMUM_RESTARTS
}

/// The answer to a model request, before it is queued.
#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_protocol::{PluginDescriptor, PluginVersion};

    fn a_descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("key-stats").expect("valid"),
            name: "Key stats".into(),
            version: PluginVersion::new(1, 0, 0),
            author: String::new(),
            description: Default::default(),
            icon: Default::default(),
            config: Default::default(),
            draws_panel: true,
            subscriptions: vec![Subscription::Input],
        }
    }

    #[test]
    fn a_restart_budget_is_a_bound_and_not_a_preference() {
        assert!(restart_is_allowed(0));
        assert!(restart_is_allowed(MAXIMUM_RESTARTS - 1));
        assert!(
            !restart_is_allowed(MAXIMUM_RESTARTS),
            "a plugin that crashes on start five times is one that will never work, and restarting \
             it forever is a fork bomb the user cannot see"
        );
    }

    #[test]
    fn an_outcome_says_whether_the_panel_channel_needs_publishing() {
        assert!(SessionOutcome::PanelChanged.changed_the_panel());
        assert!(SessionOutcome::PanelWithdrawn.changed_the_panel());
        assert!(!SessionOutcome::Ready.changed_the_panel());
        assert!(!SessionOutcome::Ignored.changed_the_panel());
    }

    #[test]
    fn an_outcome_says_whether_it_is_a_failure_the_user_should_see() {
        assert!(
            SessionOutcome::Failed(PluginError::new(PluginErrorCode::PluginExited)).is_failure()
        );
        assert!(!SessionOutcome::Ended.is_failure());
        assert!(!SessionOutcome::PanelChanged.is_failure());
    }

    /// A font book, for the one argument `on_child_message` needs and these tests do not
    /// exercise.
    ///
    /// Built per call rather than shared, because `&mut` is what the method takes and a
    /// shared measurer would be borrowed across two of them.
    fn measurer() -> bongocat_plugin_render::TextMeasurer {
        // No fonts: nothing in these tests sends a panel, so the measurer is only an
        // argument the signature asks for. `load_from` with two empty paths is the
        // book that says "no font found" without touching the system's font directories.
        bongocat_plugin_render::TextMeasurer::new(bongocat_plugin_render::FontBook::load_from(
            &[],
            &[],
        ))
    }

    #[test]
    fn a_new_actions_outcome_is_not_a_panel_change_because_it_only_moves_the_card() {
        // The distinction that keeps this from re-rasterizing every other plugin's panel
        // on the model window. A control the host draws on its own card is not on the
        // model window at all, so it is exactly the outcome the panel-channel test above
        // must keep returning false for.
        assert!(!SessionOutcome::ActionsChanged.changed_the_panel());
        assert!(!SessionOutcome::ActionsChanged.is_failure());
    }

    /// The controls a plugin offers, as the protocol's own type.
    fn an_action(id: &str, label: &str) -> bongocat_plugin_protocol::PluginAction {
        bongocat_plugin_protocol::PluginAction {
            id: id.to_string(),
            label: bongocat_plugin_protocol::LocalizedText::from(label),
            glyph: bongocat_plugin_protocol::ActionGlyph::Play,
            disabled: false,
        }
    }

    #[test]
    fn an_offered_control_is_delivered_and_one_that_was_withdrawn_is_not() {
        // The reason the session holds the list rather than trusting the press. A card is
        // rebuilt from a snapshot, so its button can outlive the offer it was drawn from
        // by however long a poll takes — and a press handed to a plugin that has forgotten
        // what the id meant is the one thing a press must never be.
        let mut session = Session::without_a_process(PluginId::new("pomodoro").expect("valid"));

        assert!(
            !session.press_action("toggle"),
            "nothing has been offered yet, so there is nothing a press can mean"
        );

        let offered = bongocat_plugin_protocol::PluginMessage::Actions {
            actions: vec![an_action("toggle", "Pause")],
        };
        assert_eq!(
            session.on_child_message(offered, &mut measurer(), 1.0),
            SessionOutcome::ActionsChanged,
            "and offering is something the worker has to hear about, because the card draws it"
        );
        assert_eq!(session.actions().len(), 1);
        assert!(
            session.press_action("toggle"),
            "so the control the card is drawing is pressable"
        );
        assert!(
            !session.press_action("something-else"),
            "and an id the plugin never offered is refused rather than delivered"
        );

        // Withdrawn: an empty list replaces the old one rather than merging with it, so a
        // plugin that stops wanting a control makes it go away.
        let withdrawn = bongocat_plugin_protocol::PluginMessage::Actions { actions: vec![] };
        assert_eq!(
            session.on_child_message(withdrawn, &mut measurer(), 1.0),
            SessionOutcome::ActionsChanged
        );
        assert!(session.actions().is_empty());
        assert!(
            !session.press_action("toggle"),
            "so the button on a card that has not caught up yet is a click that does nothing, \
             rather than a press the plugin cannot interpret"
        );
    }

    #[test]
    fn re_offering_the_same_control_is_not_a_change() {
        // A plugin may call this every tick. Without this, sixty offers a second would be
        // sixty snapshot revisions and sixty card rebuilds to say the same word — and the
        // whole cost of the mechanism is that it is cheap to leave switched on.
        let mut session = Session::without_a_process(PluginId::new("pomodoro").expect("valid"));
        let offered = bongocat_plugin_protocol::PluginMessage::Actions {
            actions: vec![an_action("toggle", "Pause")],
        };
        assert_eq!(
            session.on_child_message(offered.clone(), &mut measurer(), 1.0),
            SessionOutcome::ActionsChanged
        );
        assert_eq!(
            session.on_child_message(offered, &mut measurer(), 1.0),
            SessionOutcome::Ignored,
            "and the second offer is the same list, so there is nothing to republish"
        );
    }

    #[test]
    fn a_list_the_host_cannot_draw_keeps_the_previous_one_rather_than_part_of_it() {
        // Refused whole, not partly applied. Half a card's buttons — some renamed, some
        // missing — is a state a user cannot act on and a plugin author cannot debug from
        // the card, so the last good list stands and the refusal is counted.
        let mut session = Session::without_a_process(PluginId::new("pomodoro").expect("valid"));
        let good = bongocat_plugin_protocol::PluginMessage::Actions {
            actions: vec![an_action("toggle", "Pause")],
        };
        session.on_child_message(good, &mut measurer(), 1.0);

        let two_with_one_id = bongocat_plugin_protocol::PluginMessage::Actions {
            actions: vec![an_action("toggle", "Pause"), an_action("toggle", "Start")],
        };
        assert_eq!(
            session.on_child_message(two_with_one_id, &mut measurer(), 1.0),
            SessionOutcome::Ignored
        );
        assert_eq!(
            session.actions().len(),
            1,
            "so the card still shows the control it had, rather than two that answer to one id"
        );
    }

    #[test]
    fn a_descriptor_validates_before_a_session_accepts_it() {
        let mut descriptor = a_descriptor();
        descriptor.name = "  ".into();
        assert_eq!(
            descriptor.validate().unwrap_err().code(),
            PluginErrorCode::InvalidPluginName
        );
        assert!(a_descriptor().validate().is_ok());
    }

    #[test]
    fn a_line_longer_than_the_bound_is_refused_before_it_is_parsed() {
        // The reader checks the bytes, so a plugin cannot make the host build a
        // document of any size by writing one enormous line.
        const { assert!(MAXIMUM_LINE_BYTES == bongocat_plugin_protocol::MAXIMUM_MESSAGE_BYTES) };
        assert!(
            bongocat_plugin_protocol::check_line_length(&vec![b'a'; MAXIMUM_LINE_BYTES + 1])
                .is_err()
        );
    }

    #[test]
    fn a_missing_executable_is_a_start_failure_rather_than_a_hanging_session() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            directory
                .path()
                .join(bongocat_plugin_protocol::PLUGIN_MANIFEST_FILE_NAME),
            br#"{"schema_version":1,"api_version":1,"id":"gone","name":"Gone",
                "version":"1.0.0","executable":"not-here"}"#,
        )
        .expect("writes a manifest");
        let data = tempfile::tempdir().expect("a temporary directory");
        let error = Session::start(
            PluginId::new("gone").expect("valid"),
            directory.path().to_path_buf(),
            data.path().to_path_buf(),
            "2.0.1".to_string(),
            "en-US".to_string(),
            1,
        )
        .err()
        .expect("an executable that is not there cannot be started");
        assert_eq!(error.code(), PluginErrorCode::PluginDirectoryUnreadable);
    }

    #[test]
    fn a_directory_with_no_manifest_at_all_is_refused_before_anything_is_executed() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let data = tempfile::tempdir().expect("a temporary directory");
        let error = Session::start(
            PluginId::new("gone").expect("valid"),
            directory.path().to_path_buf(),
            data.path().to_path_buf(),
            "2.0.1".to_string(),
            "en-US".to_string(),
            1,
        )
        .err()
        .expect("no manifest, no plugin");
        assert_eq!(error.code(), PluginErrorCode::PluginDirectoryUnreadable);
    }

    #[test]
    fn a_manifest_naming_an_executable_outside_its_own_directory_is_refused() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            directory
                .path()
                .join(bongocat_plugin_protocol::PLUGIN_MANIFEST_FILE_NAME),
            br#"{"schema_version":1,"api_version":1,"id":"evil","name":"Evil",
                "version":"1.0.0","executable":"../../bin/sh"}"#,
        )
        .expect("writes a manifest");
        let data = tempfile::tempdir().expect("a temporary directory");
        let error = Session::start(
            PluginId::new("evil").expect("valid"),
            directory.path().to_path_buf(),
            data.path().to_path_buf(),
            "2.0.1".to_string(),
            "en-US".to_string(),
            1,
        )
        .err()
        .expect("a path out of the plugin's own directory is not an executable");
        assert_eq!(error.code(), PluginErrorCode::InvalidAssetPath);
    }
}
