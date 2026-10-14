//! The plugin worker: one thread that owns the manager and drives everything.
//!
//! The loop is a fixed cadence with three jobs. It applies every command the
//! windows sent, it hands the running plugins their tick and collects what they
//! published, and it composes the panels into the one layer the model window
//! draws. Nothing here blocks on a window: a window that is closed is simply not
//! sending commands, and the plugins keep running.

use crate::command::{
    PluginCard, PluginCenterSnapshot, PluginCommand, PluginCommandError, PluginReply,
    PluginServiceClient, PluginServiceEndpoint,
};
use crate::{LOOP_INTERVAL, TICK_INTERVAL};
use bongocat_plugin_hud::{
    HudCompositor, HudConsumer, HudInteraction, HudLayer, HudProducer, HudViewport,
    HudViewportReader, HudViewportReporter, PluginHudFrame, latest_hud_channel,
    latest_viewport_channel, load_platform_font,
};
use bongocat_plugin_manager::{PluginManager, PluginManagerError, PluginState, RegistryError};
use bongocat_plugin_sdk::{ConfigValue, PluginOutput};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// The store root a build's plugins are installed under.
///
/// Development and Production never share one: the storage root already separates
/// them, and a Development build that scanned Production's plugins would run
/// packages the user installed in the other environment. The manager owns the
/// layout below this (`packages/`, `data/`), so this is only the root.
pub fn plugin_directory(storage_root: &Path) -> PathBuf {
    storage_root.join("plugins")
}

/// The channel halves the model window owns and the service writes into.
///
/// They are handed over once, at start, because the window is what knows its own
/// size and is what draws the layer. Until the window reports a size the service
/// publishes empty layers, which is what a product with no model window yet
/// looks like.
pub struct PluginOverlayHandles {
    /// The newest composed panel layer, taken once per overlay tick.
    pub hud_consumer: HudConsumer,
    /// Where the window reports its own size and backing scale.
    pub viewport_reporter: HudViewportReporter,
    /// Where the window reports a click that landed on a plugin's control.
    pub interaction_sender: SyncSender<HudInteraction>,
}

/// The service's own end of the overlay channels, plus its command client.
pub struct PluginService {
    client: PluginServiceClient,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginServiceStartError {
    #[error("the plugin store could not be opened: {0}")]
    Store(PluginManagerError),
    #[error("the plugin worker could not be started: {0}")]
    Spawn(std::io::Error),
}

impl PluginService {
    /// Start the worker and its manager.
    ///
    /// The two channel pairs are created here rather than taken as arguments, so
    /// the layer a window draws and the viewport the service composes against are
    /// always the two ends of the same conversation. The manager is opened on a
    /// background thread, so a slow or unreadable store delays nothing the user
    /// can see.
    pub fn start(
        storage_root: &Path,
        capacity: usize,
    ) -> Result<(Self, PluginOverlayHandles), PluginServiceStartError> {
        let directory = plugin_directory(storage_root);
        let (client, endpoint) = PluginServiceClient::bounded(capacity);
        let (hud_producer, hud_consumer) = latest_hud_channel();
        let (viewport_reporter, viewport) = latest_viewport_channel();
        let (interaction_sender, interactions) = sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("bongocat-plugin-service".to_owned())
            .spawn(move || {
                run(
                    directory,
                    endpoint,
                    OverlaySinks {
                        hud_producer,
                        viewport,
                        interactions,
                    },
                    worker_stop,
                );
            })
            .map_err(PluginServiceStartError::Spawn)?;
        Ok((
            Self {
                client,
                stop,
                worker: Some(worker),
            },
            PluginOverlayHandles {
                hud_consumer,
                viewport_reporter,
                interaction_sender,
            },
        ))
    }

    /// The window end of the command channel.
    pub fn client(&self) -> PluginServiceClient {
        self.client.clone()
    }

    /// Ask the worker to stop and wait for it to have stopped.
    ///
    /// Every plugin process is joined before this returns, which is what makes
    /// the plugin service a step in the product's shutdown order rather than a
    /// thread that outlives the window it drew into.
    pub fn shutdown(mut self) -> Result<(), PluginServiceStartError> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

impl Drop for PluginService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The service's end of the overlay channels.
struct OverlaySinks {
    hud_producer: HudProducer,
    viewport: HudViewportReader,
    interactions: Receiver<HudInteraction>,
}

/// The worker's whole state.
struct Worker {
    manager: PluginManager,
    endpoint: PluginServiceEndpoint,
    hud: OverlaySinks,
    compositor: Option<HudCompositor>,
    /// The last refusal reported to a window, kept so the center's retry row can
    /// say what happened rather than only that something did.
    error: Option<String>,
    /// The layer last published, kept only to decide whether a new one is needed.
    ///
    /// A panel that is not changing is the common case — a countdown changes once
    /// a second and a plugin with no model view never changes at all — so the
    /// comparison is what keeps an idle plugin from costing a rasterization per
    /// loop.
    last_signature: Option<LayerSignature>,
    /// Clicks the model window resolved to a plugin control, in arrival order.
    ///
    /// A click is applied to the plugin that drew the control and the plugin
    /// decides what it means: the service never learns what a button does.
    pending_interactions: Vec<HudInteraction>,
    started: Instant,
    last_tick: Duration,
}

/// What a composed layer is a function of, for skipping an unchanged one.
#[derive(Clone, PartialEq)]
struct LayerSignature {
    viewport: HudViewport,
    frames: Vec<PluginHudFrame>,
}

fn run(
    directory: PathBuf,
    endpoint: PluginServiceEndpoint,
    hud: OverlaySinks,
    stop: Arc<AtomicBool>,
) {
    let Ok(mut manager) = PluginManager::new(directory) else {
        // The store could not be opened at all. The service still answers, so the
        // plugin center can say what happened instead of reporting a service that
        // never started.
        let mut unavailable = Unavailable {
            endpoint,
            error: Some(STORE_OPEN_FAILED.to_owned()),
        };
        while !stop.load(Ordering::Acquire) {
            unavailable.drain();
            thread::sleep(LOOP_INTERVAL);
        }
        return;
    };
    let mut error = None;
    if let Err(failure) = manager.discover_plugins() {
        error = Some(failure.to_string());
    }
    let mut worker = Worker {
        manager,
        endpoint,
        hud,
        compositor: load_platform_font().ok().map(HudCompositor::new),
        error,
        last_signature: None,
        pending_interactions: Vec::new(),
        started: Instant::now(),
        last_tick: Duration::ZERO,
    };
    worker.run(stop);
}

const STORE_OPEN_FAILED: &str = "the plugin store could not be opened";

/// The worker when the plugin store itself could not be opened.
struct Unavailable {
    endpoint: PluginServiceEndpoint,
    error: Option<String>,
}

impl Unavailable {
    fn drain(&mut self) {
        while let Ok(Some(command)) = self.endpoint.try_recv() {
            let center = PluginCenterSnapshot {
                cards: Vec::new(),
                error: self.error.clone(),
                loading: false,
            };
            match command {
                PluginCommand::ReadCenter { reply } | PluginCommand::Refresh { reply } => {
                    let _ = reply.respond(center);
                }
                PluginCommand::SetEnabled { reply, .. }
                | PluginCommand::Install { reply, .. }
                | PluginCommand::Uninstall { reply, .. }
                | PluginCommand::ResetData { reply, .. }
                | PluginCommand::Action { reply, .. }
                | PluginCommand::Configure { reply, .. } => {
                    let _ = reply.respond(Err::<PluginCenterSnapshot, PluginCommandError>(
                        PluginCommandError::ServiceUnavailable,
                    ));
                }
                PluginCommand::Shutdown { reply } => {
                    let _ = reply.respond(());
                }
            }
        }
    }
}

impl Worker {
    fn run(&mut self, stop: Arc<AtomicBool>) {
        while !stop.load(Ordering::Acquire) {
            self.drain_commands();
            self.drain_interactions();
            self.dispatch_tick();
            self.compose();
            thread::sleep(LOOP_INTERVAL);
        }
        self.manager.shutdown();
    }

    /// The elapsed monotonic clock a plugin counts with.
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    fn center(&self) -> PluginCenterSnapshot {
        PluginCenterSnapshot {
            cards: self
                .manager
                .list_plugins()
                .into_iter()
                .map(|(metadata, state)| {
                    let output = self.manager.plugin_output(&metadata.id);
                    PluginCard {
                        id: metadata.id.clone(),
                        name: metadata.name,
                        description: metadata.description,
                        icon: metadata.icon,
                        enabled: state == PluginState::Enabled,
                        failed: state == PluginState::Failed,
                        version: self.manager.installed_version(&metadata.id),
                        actions: nodes(output, |output| &output.model.nodes),
                        settings: nodes(output, |output| &output.settings.nodes),
                    }
                })
                .collect(),
            error: self.error.clone(),
            loading: false,
        }
    }

    fn drain_commands(&mut self) {
        loop {
            match self.endpoint.try_recv() {
                Ok(Some(command)) => self.apply(command),
                Ok(None) => return,
                Err(_) => return,
            }
        }
    }

    fn apply(&mut self, command: PluginCommand) {
        match command {
            PluginCommand::ReadCenter { reply } => {
                let _ = reply.respond(self.center());
            }
            PluginCommand::Refresh { reply } => {
                self.error = None;
                if let Err(error) = self.manager.discover_plugins() {
                    self.error = Some(error.to_string());
                }
                let _ = reply.respond(self.center());
            }
            PluginCommand::SetEnabled { id, enabled, reply } => {
                let result = self.set_enabled(&id, enabled);
                self.respond(reply, result);
            }
            PluginCommand::Install { bytes, reply } => {
                let result = self
                    .manager
                    .install(&bytes)
                    .map(|_| ())
                    .map_err(|error| operation(&error));
                self.respond(reply, result);
            }
            PluginCommand::Uninstall { id, reply } => {
                let result = self
                    .manager
                    .uninstall(&id)
                    .map(|_| ())
                    .map_err(|error| operation(&error));
                self.respond(reply, result);
            }
            PluginCommand::ResetData { id, reply } => {
                let result = self
                    .manager
                    .reset(&id)
                    .map(|_| ())
                    .map_err(|error| operation(&error));
                self.respond(reply, result);
            }
            PluginCommand::Action {
                plugin,
                action,
                reply,
            } => {
                let result = self
                    .manager
                    .action_plugin(&plugin, &action)
                    .map_err(|error| operation(&error));
                self.respond(reply, result);
            }
            PluginCommand::Configure {
                plugin,
                key,
                value,
                reply,
            } => {
                let result = self.configure(&plugin, key, value);
                self.respond(reply, result);
            }
            PluginCommand::Shutdown { reply } => {
                self.manager.shutdown();
                let _ = reply.respond(());
            }
        }
    }

    fn respond(
        &mut self,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
        result: Result<(), PluginCommandError>,
    ) {
        let center = result.map(|()| self.center());
        let _ = reply.respond(center);
    }

    fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), PluginCommandError> {
        if !self
            .manager
            .list_plugins()
            .iter()
            .any(|(metadata, _)| metadata.id == id)
        {
            return Err(PluginCommandError::NotFound);
        }
        let result = if enabled {
            self.manager.enable_plugin(id)
        } else {
            self.manager.disable_plugin(id)
        };
        result.map_err(|error| operation(&error))
    }

    /// Merge one declared key into a running plugin's configuration.
    ///
    /// The plugin owns the schema, so the value is merged with what the plugin
    /// last published rather than replacing it: a control that edits one key must
    /// not clear the others, and a key the plugin never declared is refused by the
    /// plugin itself.
    fn configure(
        &mut self,
        plugin: &str,
        key: String,
        value: ConfigValue,
    ) -> Result<(), PluginCommandError> {
        let mut config = self
            .manager
            .plugin_output(plugin)
            .map(|output| output.config.clone())
            .ok_or(PluginCommandError::NotFound)?;
        config.insert(key, value);
        self.manager
            .configure_plugin(plugin, config)
            .map_err(|error| operation(&error))
    }

    fn drain_interactions(&mut self) {
        loop {
            match self.hud.interactions.try_recv() {
                Ok(interaction) => self.pending_interactions.push(interaction),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }

    fn dispatch_tick(&mut self) {
        let elapsed = self.started.elapsed();
        if elapsed.saturating_sub(self.last_tick) < TICK_INTERVAL {
            return;
        }
        self.last_tick = elapsed;
        let now = self.now_ms();
        self.manager.tick(now);
    }

    /// Apply every pending click, then publish what the plugins now publish.
    fn compose(&mut self) {
        for interaction in std::mem::take(&mut self.pending_interactions) {
            if let Err(error) = self
                .manager
                .action_plugin(&interaction.plugin, &interaction.id)
            {
                self.error = Some(error.to_string());
            }
        }
        let Some(compositor) = self.compositor.as_ref() else {
            return;
        };
        let viewport = self.hud.viewport.latest();
        let frames = if viewport.is_usable() {
            self.running_frames()
        } else {
            Vec::new()
        };
        let signature = LayerSignature {
            viewport,
            frames: frames.clone(),
        };
        if self.last_signature.as_ref() == Some(&signature) {
            return;
        }
        let layer = if viewport.is_usable() {
            compositor
                .compose(viewport, &frames)
                .unwrap_or_else(|_| HudLayer::empty())
        } else {
            HudLayer::empty()
        };
        self.last_signature = Some(signature);
        self.hud.hud_producer.publish(layer);
    }

    /// The panel each enabled plugin currently publishes.
    ///
    /// A plugin that is enabled but has published nothing yet is skipped rather
    /// than drawn as an empty panel: its first frame arrives on the tick that
    /// follows its start.
    fn running_frames(&self) -> Vec<PluginHudFrame> {
        self.manager
            .list_plugins()
            .into_iter()
            .filter(|(_, state)| *state == PluginState::Enabled)
            .filter_map(|(metadata, _)| {
                let output = self.manager.plugin_output(&metadata.id)?;
                Some(PluginHudFrame::new(metadata.id, output.model.clone()))
            })
            .collect()
    }
}

fn nodes<T: Clone>(
    output: Option<&PluginOutput>,
    select: impl FnOnce(&PluginOutput) -> &Vec<T>,
) -> Vec<T> {
    output.map(select).cloned().unwrap_or_else(Vec::new)
}

fn operation(error: &PluginManagerError) -> PluginCommandError {
    match error {
        PluginManagerError::Registry(RegistryError::NotFound(_)) => PluginCommandError::NotFound,
        PluginManagerError::Registry(RegistryError::NotRunning(_)) => {
            PluginCommandError::NotRunning
        }
        _ => PluginCommandError::Operation,
    }
}
