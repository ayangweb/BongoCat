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
use bongocat_plugin_manager::{
    AvailablePlugin, PluginManager, PluginManagerError, PluginSource, PluginState, RegistryError,
};
use bongocat_plugin_sdk::{ConfigValue, InputActivity, PluginOutput, UiNode};
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
    worker: Option<JoinHandle<Result<(), PluginManagerError>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginServiceStartError {
    #[error("the plugin store could not be opened: {0}")]
    Store(PluginManagerError),
    #[error("the plugin worker could not be started: {0}")]
    Spawn(std::io::Error),
    #[error("the plugin worker panicked")]
    WorkerPanicked,
    #[error("the plugin service could not shut down cleanly: {0}")]
    Shutdown(PluginManagerError),
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
        Self::start_with_source(storage_root, capacity, PluginSource::InstalledOnly, None)
    }

    pub fn start_with_source(
        storage_root: &Path,
        capacity: usize,
        source: PluginSource,
        input_activity: Option<Box<dyn Fn() -> InputActivity + Send>>,
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
                    source,
                    input_activity,
                )
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
            worker
                .join()
                .map_err(|_| PluginServiceStartError::WorkerPanicked)?
                .map_err(PluginServiceStartError::Shutdown)?;
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
    source: PluginSource,
    offers: Vec<AvailablePlugin>,
    job: Option<CatalogJob>,
    catalog_stop: Arc<AtomicBool>,
    input_activity: Option<Box<dyn Fn() -> InputActivity + Send>>,
    last_input_activity: InputActivity,
    endpoint: PluginServiceEndpoint,
    hud: OverlaySinks,
    compositor: Option<HudCompositor>,
    /// Store/catalog failures. Command refusals travel in their typed reply.
    error: Option<String>,
    interaction_failures: std::collections::BTreeMap<String, crate::PluginInteractionFailure>,
    /// The layer last published, kept only to decide whether a new one is needed.
    ///
    /// A panel that is not changing is the common case — a countdown changes once
    /// a second and a plugin with no model view never changes at all — so the
    /// comparison is what keeps an idle plugin from costing a rasterization per
    /// loop.
    last_signature: Option<LayerSignature>,
    started: Instant,
    last_tick: Duration,
    shutdown_reply: Option<PluginReply<()>>,
}

enum CatalogResult {
    List(Vec<AvailablePlugin>),
    Package {
        offer: Box<AvailablePlugin>,
        bytes: Vec<u8>,
    },
}
struct CatalogJob {
    result: Receiver<Result<CatalogResult, String>>,
    worker: JoinHandle<()>,
    package: Option<String>,
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
    source: PluginSource,
    input_activity: Option<Box<dyn Fn() -> InputActivity + Send>>,
) -> Result<(), PluginManagerError> {
    let Ok(mut manager) = PluginManager::new(directory) else {
        // The store could not be opened at all. The service still answers, so the
        // plugin center can say what happened instead of reporting a service that
        // never started.
        let mut unavailable = Unavailable {
            endpoint,
            error: Some(STORE_OPEN_FAILED.to_owned()),
        };
        while !stop.load(Ordering::Acquire) {
            if unavailable.drain() {
                break;
            }
            thread::sleep(LOOP_INTERVAL);
        }
        return Ok(());
    };
    let mut error = None;
    if let Err(failure) = manager.discover_plugins() {
        error = Some(failure.to_string());
    }
    // Resume failures are represented by each affected card's Failed state.
    let _ = manager.resume_enabled();
    let last_input_activity = input_activity
        .as_ref()
        .map_or(InputActivity::default(), |read| read());
    let mut worker = Worker {
        manager,
        source,
        offers: Vec::new(),
        job: None,
        catalog_stop: Arc::clone(&stop),
        input_activity,
        last_input_activity,
        endpoint,
        hud,
        compositor: load_platform_font().ok().map(HudCompositor::new),
        error,
        interaction_failures: Default::default(),
        last_signature: None,
        started: Instant::now(),
        last_tick: Duration::ZERO,
        shutdown_reply: None,
    };
    worker.initialize_running_input_activity();
    worker.run(stop)
}

const STORE_OPEN_FAILED: &str = "the plugin store could not be opened";
const COMMANDS_PER_TURN: usize = 16;
const INTERACTIONS_PER_TURN: usize = 64;

/// The worker when the plugin store itself could not be opened.
struct Unavailable {
    endpoint: PluginServiceEndpoint,
    error: Option<String>,
}

impl Unavailable {
    fn drain(&mut self) -> bool {
        for _ in 0..COMMANDS_PER_TURN {
            let Ok(Some(command)) = self.endpoint.try_recv() else {
                break;
            };
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
                | PluginCommand::InstallAvailable { reply, .. }
                | PluginCommand::Uninstall { reply, .. }
                | PluginCommand::ResetData { reply, .. }
                | PluginCommand::Action { reply, .. }
                | PluginCommand::Preview { reply, .. }
                | PluginCommand::Configure { reply, .. }
                | PluginCommand::SetLocale { reply, .. } => {
                    let _ = reply.respond(Err::<PluginCenterSnapshot, PluginCommandError>(
                        PluginCommandError::ServiceUnavailable,
                    ));
                }
                PluginCommand::Shutdown { reply } => {
                    self.endpoint.close();
                    let _ = reply.respond(());
                    return true;
                }
            }
        }
        false
    }
}

impl Worker {
    fn run(&mut self, stop: Arc<AtomicBool>) -> Result<(), PluginManagerError> {
        self.refresh_catalog();
        while !stop.load(Ordering::Acquire) {
            self.finish_catalog_job();
            self.drain_commands();
            if stop.load(Ordering::Acquire) {
                break;
            }
            self.dispatch_input_activity();
            self.drain_interactions();
            self.dispatch_tick();
            self.compose();
            thread::sleep(LOOP_INTERVAL);
        }
        self.endpoint.close();
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
        self.manager.shutdown()?;
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.respond(());
        }
        Ok(())
    }

    fn start_job(
        &mut self,
        package: Option<String>,
        operation: impl FnOnce() -> Result<CatalogResult, String> + Send + 'static,
    ) {
        let (sender, result) = sync_channel(1);
        match thread::Builder::new()
            .name("bongocat-plugin-catalog".into())
            .spawn(move || {
                let _ = sender.send(operation());
            }) {
            Ok(worker) => {
                self.job = Some(CatalogJob {
                    result,
                    worker,
                    package,
                })
            }
            Err(_) => self.error = Some("catalog worker unavailable".into()),
        }
    }

    fn refresh_catalog(&mut self) {
        if self.job.is_some() {
            return;
        }
        if matches!(self.source, PluginSource::InstalledOnly) {
            return;
        }
        let source = self.source.clone();
        let stop = Arc::clone(&self.catalog_stop);
        self.start_job(None, move || {
            source
                .discover(env!("CARGO_PKG_VERSION"), &stop)
                .map(CatalogResult::List)
        });
    }

    fn finish_catalog_job(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("catalog worker failed".into()),
        };
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
        match result {
            Ok(CatalogResult::List(offers)) => self.offers = offers,
            Ok(CatalogResult::Package { offer, bytes }) => {
                self.dispatch_input_activity();
                if let Err(error) = self.manager.install_verified(&bytes, Some(&offer.digest)) {
                    self.error = Some(error.to_string());
                }
                self.initialize_plugin_input_activity(&offer.metadata.id);
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// Hand the running plugins the device activity that happened since the last
    /// poll.
    ///
    /// The counters are absolute, so an unchanged read is what says nothing
    /// happened: an idle desktop costs one comparison per loop and no message at
    /// all, and a plugin that declared no activity capability is skipped inside
    /// the manager rather than here.
    fn dispatch_input_activity(&mut self) {
        let Some(read) = &self.input_activity else {
            return;
        };
        let current = read();
        if current.advanced_since(self.last_input_activity) {
            self.last_input_activity = current;
            self.manager.input_activity(self.now_ms(), current);
        }
    }

    /// Give newly started counter-based plugins the current totals without
    /// replaying input that happened before their process existed.
    fn initialize_running_input_activity(&mut self) {
        let ids = self
            .manager
            .list_plugins()
            .into_iter()
            .filter(|(_, state)| *state == PluginState::Enabled)
            .map(|(metadata, _)| metadata.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.initialize_plugin_input_activity(&id);
        }
    }

    fn initialize_plugin_input_activity(&mut self, id: &str) {
        let Some(read) = &self.input_activity else {
            return;
        };
        let current = read();
        self.manager
            .initialize_input_activity(id, self.now_ms(), current);
    }

    /// The elapsed monotonic clock a plugin counts with.
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    fn center(&self) -> PluginCenterSnapshot {
        let icon_urls = self
            .offers
            .iter()
            .map(|offer| (offer.metadata.id.clone(), offer.icon_url.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut cards: Vec<PluginCard> = self
            .manager
            .list_plugins()
            .into_iter()
            .map(|(metadata, state)| {
                let output = self.manager.plugin_output(&metadata.id);
                let version = self.manager.installed_version(&metadata.id);
                let available_version = self
                    .offers
                    .iter()
                    .find(|offer| offer.metadata.id == metadata.id)
                    .map(|offer| offer.metadata.version.clone());
                let update_available = version
                    .as_ref()
                    .zip(available_version.as_ref())
                    .is_some_and(|(installed, available)| {
                        bongocat_plugin_manager::is_newer(available, installed)
                    });
                PluginCard {
                    id: metadata.id.clone(),
                    name: metadata.name,
                    description: metadata.description,
                    localizations: metadata.localizations,
                    icon_url: icon_urls.get(&metadata.id).cloned(),
                    enabled: state == PluginState::Enabled,
                    failed: state == PluginState::Failed,
                    version,
                    available_version,
                    update_available,
                    actions: nodes(output, |output| &output.model.nodes),
                    settings: nodes(output, |output| &output.settings.nodes),
                    errors: nodes(output, |output| &output.errors),
                    interaction_failure: self.interaction_failures.get(&metadata.id).cloned(),
                }
            })
            .collect();
        for offer in &self.offers {
            if cards.iter().any(|card| card.id == offer.metadata.id) {
                continue;
            }
            // A bad installed manifest/session may prevent registration. Keep
            // its reset/uninstall controls reachable using verified catalog copy.
            let version = self.manager.installed_version(&offer.metadata.id);
            let update_available = version.as_ref().is_some_and(|installed| {
                bongocat_plugin_manager::is_newer(&offer.metadata.version, installed)
            });
            cards.push(PluginCard {
                id: offer.metadata.id.clone(),
                name: offer.metadata.name.clone(),
                description: offer.metadata.description.clone(),
                localizations: offer.metadata.localizations.clone(),
                icon_url: Some(offer.icon_url.clone()),
                enabled: false,
                failed: version.is_some(),
                version,
                available_version: Some(offer.metadata.version.clone()),
                update_available,
                actions: Vec::new(),
                settings: Vec::new(),
                errors: Vec::new(),
                interaction_failure: self.interaction_failures.get(&offer.metadata.id).cloned(),
            });
        }
        PluginCenterSnapshot {
            cards,
            error: self.error.clone(),
            loading: self.job.is_some(),
        }
    }

    fn drain_commands(&mut self) {
        for _ in 0..COMMANDS_PER_TURN {
            match self.endpoint.try_recv() {
                Ok(Some(command)) => {
                    self.apply(command);
                    if self.catalog_stop.load(Ordering::Acquire) {
                        break;
                    }
                }
                Ok(None) => return,
                Err(_) => return,
            }
        }
    }

    fn apply(&mut self, command: PluginCommand) {
        if let Some(package) = self.job.as_ref().and_then(|job| job.package.as_deref()) {
            match command {
                PluginCommand::SetEnabled { ref id, .. }
                | PluginCommand::Uninstall { ref id, .. }
                | PluginCommand::ResetData { ref id, .. }
                    if id == package =>
                {
                    let reply = match command {
                        PluginCommand::SetEnabled { reply, .. }
                        | PluginCommand::Uninstall { reply, .. }
                        | PluginCommand::ResetData { reply, .. } => reply,
                        _ => unreachable!(),
                    };
                    self.respond(reply, Err(PluginCommandError::Operation));
                    return;
                }
                PluginCommand::Install { reply, .. } => {
                    self.respond(reply, Err(PluginCommandError::Operation));
                    return;
                }
                _ => {}
            }
        }
        match command {
            PluginCommand::ReadCenter { reply } => {
                let _ = reply.respond(self.center());
            }
            PluginCommand::Refresh { reply } => {
                self.error = None;
                if let Err(error) = self.manager.discover_plugins() {
                    self.error = Some(error.to_string());
                }
                self.refresh_catalog();
                let _ = reply.respond(self.center());
            }
            PluginCommand::SetEnabled { id, enabled, reply } => {
                // Flush activity for plugins that are already running before a
                // newly enabled plugin receives its baseline.
                if enabled {
                    self.dispatch_input_activity();
                }
                let result = self.set_enabled(&id, enabled);
                if result.is_ok() {
                    self.interaction_failures.remove(&id);
                }
                if result.is_ok() && enabled {
                    self.initialize_plugin_input_activity(&id);
                }
                self.respond(reply, result);
            }
            PluginCommand::Install { bytes, reply } => {
                self.dispatch_input_activity();
                let id = bongocat_plugin_manager::install::inspect_package(&bytes)
                    .ok()
                    .map(|manifest| manifest.metadata.id);
                let result = if matches!(self.source, PluginSource::Production { .. }) {
                    Err(PluginCommandError::Operation)
                } else {
                    self.manager
                        .install(&bytes)
                        .map(|_| ())
                        .map_err(|error| operation(&error))
                };
                if let Some(id) = id {
                    self.initialize_plugin_input_activity(&id);
                }
                self.respond(reply, result);
            }
            PluginCommand::InstallAvailable { id, reply } => {
                if self.job.is_some() {
                    self.respond(reply, Err(PluginCommandError::Operation));
                    return;
                }
                let Some(offer) = self
                    .offers
                    .iter()
                    .find(|offer| offer.metadata.id == id)
                    .cloned()
                else {
                    self.respond(reply, Err(PluginCommandError::NotFound));
                    return;
                };
                if self
                    .manager
                    .installed_version(&id)
                    .is_some_and(|installed| {
                        !bongocat_plugin_manager::is_newer(&offer.metadata.version, &installed)
                    })
                {
                    self.respond(reply, Err(PluginCommandError::Operation));
                    return;
                }
                self.error = None;
                let stop = Arc::clone(&self.catalog_stop);
                self.start_job(Some(id), move || {
                    let bytes = offer.read(&stop)?;
                    Ok(CatalogResult::Package {
                        offer: Box::new(offer),
                        bytes,
                    })
                });
                self.respond(reply, Ok(()));
            }
            PluginCommand::Uninstall { id, reply } => {
                let result = self
                    .manager
                    .uninstall(&id)
                    .map(|_| ())
                    .map_err(|error| operation(&error));
                if result.is_ok() {
                    self.interaction_failures.remove(&id);
                }
                self.respond(reply, result);
            }
            PluginCommand::ResetData { id, reply } => {
                self.dispatch_input_activity();
                let result = self
                    .manager
                    .reset(&id)
                    .map(|_| ())
                    .map_err(|error| operation(&error));
                self.initialize_plugin_input_activity(&id);
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
                if result.is_ok() {
                    self.interaction_failures.remove(&plugin);
                }
                self.respond(reply, result);
            }
            PluginCommand::Preview {
                plugin,
                key,
                value,
                reply,
            } => {
                let result = self
                    .manager
                    .preview_plugin(&plugin, &key, &value)
                    .map_err(|error| operation(&error));
                if result.is_ok() {
                    self.interaction_failures.remove(&plugin);
                }
                self.respond(reply, result);
            }
            PluginCommand::Configure {
                plugin,
                key,
                value,
                reply,
            } => {
                let result = self.configure(&plugin, key, value);
                if result.is_ok() {
                    self.interaction_failures.remove(&plugin);
                }
                self.respond(reply, result);
            }
            PluginCommand::SetLocale { locale, reply } => {
                self.manager.set_locale(&locale);
                self.respond(reply, Ok(()));
            }
            PluginCommand::Shutdown { reply } => {
                self.catalog_stop.store(true, Ordering::Release);
                self.endpoint.close();
                // The loop acknowledges only after all workers stopped cleanly.
                self.shutdown_reply = Some(reply);
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
        for _ in 0..INTERACTIONS_PER_TURN {
            match self.hud.interactions.try_recv() {
                Ok(interaction) => {
                    if !self.manager.list_plugins().iter().any(|(metadata, state)| {
                        metadata.id == interaction.plugin && *state == PluginState::Enabled
                    }) {
                        continue;
                    }
                    let Some(output) = self.manager.plugin_output(&interaction.plugin) else {
                        continue;
                    };
                    let (result, failure) =
                        match output.model.nodes.iter().find(|node| match node {
                            UiNode::Button {
                                id, loading: false, ..
                            } => id == &interaction.id,
                            UiNode::Toggle { key, .. } => key == &interaction.id,
                            _ => false,
                        }) {
                            Some(UiNode::Button { .. }) => (
                                self.manager
                                    .action_plugin(&interaction.plugin, &interaction.id)
                                    .map_err(|error| operation(&error)),
                                crate::PluginInteractionFailure::Action {
                                    action: interaction.id.clone(),
                                    code: PluginCommandError::Operation,
                                },
                            ),
                            Some(UiNode::Toggle { key, value, .. }) => {
                                let key = key.clone();
                                (
                                    self.configure(
                                        &interaction.plugin,
                                        key.clone(),
                                        ConfigValue::Boolean(!value),
                                    ),
                                    crate::PluginInteractionFailure::Configure {
                                        key,
                                        code: PluginCommandError::Operation,
                                    },
                                )
                            }
                            _ => continue,
                        };
                    if let Err(error) = result {
                        let failure = match failure {
                            crate::PluginInteractionFailure::Action { action, .. } => {
                                crate::PluginInteractionFailure::Action {
                                    action,
                                    code: error,
                                }
                            }
                            crate::PluginInteractionFailure::Configure { key, .. } => {
                                crate::PluginInteractionFailure::Configure { key, code: error }
                            }
                        };
                        self.interaction_failures
                            .insert(interaction.plugin, failure);
                    } else {
                        self.interaction_failures.remove(&interaction.plugin);
                    }
                }
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

    /// Publish the plugins' current views only when their composed input changed.
    fn compose(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_downloads_serialize_conflicting_lifecycle_operations() {
        let store = tempfile::tempdir().unwrap();
        let (_, endpoint) = PluginServiceClient::bounded(4);
        let (hud_producer, _) = latest_hud_channel();
        let (_, viewport) = latest_viewport_channel();
        let (_, interactions) = sync_channel(4);
        let mut worker = Worker {
            manager: PluginManager::new(store.path().to_path_buf()).unwrap(),
            source: PluginSource::InstalledOnly,
            offers: Vec::new(),
            job: None,
            catalog_stop: Arc::default(),
            input_activity: None,
            last_input_activity: InputActivity::default(),
            endpoint,
            hud: OverlaySinks {
                hud_producer,
                viewport,
                interactions,
            },
            compositor: None,
            error: None,
            interaction_failures: Default::default(),
            last_signature: None,
            started: Instant::now(),
            last_tick: Duration::ZERO,
            shutdown_reply: None,
        };
        let id = "com.bongocat.pomodoro";
        let data = worker.manager.data_root(id, true).unwrap();
        std::fs::write(data.join("owned-data"), b"keep").unwrap();
        let (release, wait) = sync_channel(1);
        worker.start_job(Some(id.into()), move || {
            wait.recv().unwrap();
            Ok(CatalogResult::List(Vec::new()))
        });
        for operation in 0..4 {
            let (reply, response) = async_channel::bounded(1);
            let reply = PluginReply(reply);
            let command = match operation {
                0 => PluginCommand::SetEnabled {
                    id: id.into(),
                    enabled: false,
                    reply,
                },
                1 => PluginCommand::Uninstall {
                    id: id.into(),
                    reply,
                },
                2 => PluginCommand::ResetData {
                    id: id.into(),
                    reply,
                },
                _ => PluginCommand::Install {
                    bytes: Vec::new(),
                    reply,
                },
            };
            worker.apply(command);
            assert_eq!(
                response.recv_blocking().unwrap(),
                Err(PluginCommandError::Operation)
            );
        }
        assert_eq!(std::fs::read(data.join("owned-data")).unwrap(), b"keep");
        // An unrelated plugin's lifecycle is not blocked by this download.
        let (reply, response) = async_channel::bounded(1);
        worker.apply(PluginCommand::Uninstall {
            id: "com.bongocat.other".into(),
            reply: PluginReply(reply),
        });
        assert!(response.recv_blocking().unwrap().is_ok());
        release.send(()).unwrap();
        worker.job.take().unwrap().worker.join().unwrap();
    }
}
