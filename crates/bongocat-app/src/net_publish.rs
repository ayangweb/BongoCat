//! Wiring that serves a relay, mirrors local input into a room, and draws another
//! machine's input.
//!
//! This is the only place that knows about both the input pipeline and the network.
//! Keeping it here leaves `bongocat-net` ignorant of producers and `bongocat-input`
//! ignorant of rooms, which is what lets either side change without the other
//! noticing.

use std::collections::BTreeSet;
use std::io::Write;
use std::net::IpAddr;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use bongocat_config::{NetworkClientSource, NetworkConfig, NetworkListenMode};
use bongocat_input::{
    InputControl, InputEdge, InputEvent, InputProducer, InputSource, MonotonicMillis,
};
use bongocat_net::client::{ClientCommand, ClientConfig, ClientEvent, ClientWorker};
use bongocat_net::protocol::{DeviceInfo, InputFrame};
use bongocat_net::relay::{BindMode, RelayConfig, RunningRelay};
use bongocat_net::{RelayStatus, RoomState, RoomStatus, update_network_status};
use bongocat_overlay::TeeSubmitter;

use crate::product_options::RunOptions;

/// Depth of the queue between the input thread and the network thread.
///
/// Comfortably larger than the publisher's own queue, so a momentary stall in the
/// writer cannot make the tee drop a release edge — a dropped release leaves the
/// far end holding a key that nobody is pressing.
const SINK_CAPACITY: usize = 512;

/// How often a silent viewer checks whether it is holding keys nobody holds.
///
/// Short enough that the recovery is invisible, long enough that the check costs
/// nothing next to the frame rate.
const SILENCE_CHECK: Duration = Duration::from_millis(500);

/// How long a held key may go without any frame before it is released.
///
/// Any hop that drops a release frame leaves a key pressed forever otherwise, and
/// a held key is visible: the pet keeps a paw down. Nobody holds a key for five
/// seconds while typing, so the false-positive rate is acceptable where a shorter
/// window would not be.
const SILENCE_LIMIT: Duration = Duration::from_secs(5);

/// What one run does with the network, once the command line and the
/// configuration have been read together.
///
/// Built once and then read by everything that needs it, so the answer to "is this
/// process in a room" is one value rather than a flag re-derived at each call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NetworkPlan {
    /// The relay this run serves alongside the pet, if any.
    pub(crate) serve: Option<ServePlan>,
    /// The room this run joins, if any.
    pub(crate) join: Option<JoinPlan>,
    /// The main overlay can show local input without a room.
    pub(crate) primary_display_selected: bool,
}

/// One relay to serve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServePlan {
    pub(crate) bind: BindMode,
    pub(crate) port: u16,
    pub(crate) token: String,
}

/// One room to join.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JoinPlan {
    pub(crate) url: String,
    pub(crate) token: String,
    pub(crate) device_id: String,
    pub(crate) device_name: String,
    /// Whether this machine's own input is sent to the room.
    pub(crate) publish: bool,
    /// Whether this machine's pet draws its own input.
    ///
    /// Distinct from `publish`: a machine can push its keys into the room while
    /// its pet shows someone else's, and vice versa.
    pub(crate) show_self: bool,
    /// What this machine draws out of the room.
    pub(crate) draw: DrawMode,
    extra: Option<String>,
}

/// What input from the room this machine draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DrawMode {
    /// Nothing: the pet answers this machine, or stays off screen entirely.
    Nothing,
    /// Whichever device publishes first. The room can hold several publishers and
    /// one pet draws one of them, so an unspecified choice has to be made once and
    /// then kept rather than interleaving two machines into one cat.
    Any,
    /// This device id, and no other.
    Device(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DisplaySelection {
    pub(crate) local: bool,
    draw: DrawMode,
    extra: Option<String>,
}

impl DisplaySelection {
    pub(crate) fn from_devices(devices: &[String], room_available: bool) -> Self {
        let mut peers = devices
            .iter()
            .filter(|id| room_available && id.as_str() != bongocat_config::NETWORK_SELF_DEVICE);
        let first = peers.next();
        Self {
            local: devices
                .iter()
                .any(|id| id == bongocat_config::NETWORK_SELF_DEVICE),
            draw: first.map_or(DrawMode::Nothing, |id| DrawMode::Device(id.clone())),
            extra: peers.find(|id| Some(*id) != first).cloned(),
        }
    }

    pub(crate) fn extra_device(&self) -> Option<&str> {
        self.extra.as_deref()
    }

    pub(crate) fn extra_available(&self, room: Option<&RoomStatus>) -> bool {
        self.extra.as_ref().is_some_and(|id| {
            room.is_some_and(|room| {
                matches!(room.state, RoomState::Connected { .. })
                    && room.peers.iter().any(|peer| peer.publish && peer.id == *id)
            })
        })
    }

    pub(crate) fn first_device(&self) -> Option<&str> {
        match &self.draw {
            DrawMode::Device(id) => Some(id),
            DrawMode::Nothing | DrawMode::Any => None,
        }
    }

    pub(crate) fn primary(&self) -> bool {
        self.local || self.draw.draws()
    }

    pub(crate) fn both(&self) -> bool {
        self.local && self.draw.draws()
    }

    pub(crate) fn remote_available(&self, room: Option<&RoomStatus>) -> bool {
        room.is_some_and(|room| {
            matches!(room.state, RoomState::Connected { .. })
                && room.peers.iter().any(|peer| {
                    peer.publish
                        && match &self.draw {
                            DrawMode::Nothing => false,
                            DrawMode::Any => true,
                            DrawMode::Device(device) => *device == peer.id,
                        }
                })
        })
    }
}

pub(crate) struct DisplayRouting {
    selection: Arc<Mutex<DisplaySelection>>,
    suppress_local: Arc<AtomicBool>,
    primary: InputProducer,
    cursor: bongocat_input::CursorProducer,
}

pub(crate) struct NetworkSession {
    pub(crate) input: InputProducer,
    pub(crate) local_input: Arc<TeeSubmitter>,
    pub(crate) display: DisplayRouting,
    pub(crate) control: TransportController,
}

type PublicationSink = Arc<Mutex<Option<std::sync::mpsc::SyncSender<InputFrame>>>>;

pub(crate) struct TransportController {
    options: RunOptions,
    relay: Option<RunningRelay>,
    serve: Option<ServePlan>,
    room: Option<RoomTransport>,
    join: Option<JoinPlan>,
    primary: InputProducer,
    cursor: bongocat_input::CursorProducer,
    secondary: Option<bongocat_runtime::RuntimeClient>,
    extra: Option<bongocat_runtime::RuntimeClient>,
    selection: Arc<Mutex<DisplaySelection>>,
    sink: PublicationSink,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NetworkApplyError {
    Relay,
    Room,
    Restore,
}

impl TransportController {
    pub(crate) fn connected_room_configured(&self) -> bool {
        self.join.is_some()
    }

    pub(crate) fn publishing_configured(&self) -> bool {
        self.join.as_ref().is_some_and(|join| join.publish)
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(room) = self.room.take() {
            let _ = room.stop(Duration::from_millis(500));
        }
        self.relay.take();
        update_network_status(|status| {
            status.room = None;
            status.relay = None;
        });
    }

    pub(crate) fn apply(&mut self, config: &NetworkConfig) -> Result<(), NetworkApplyError> {
        let desired = plan(&self.options, config);
        if config.server.enabled && desired.serve.is_none() {
            return Err(NetworkApplyError::Relay);
        }
        if config.client.enabled
            && config.client.source == NetworkClientSource::Manual
            && desired.join.is_none()
        {
            return Err(NetworkApplyError::Room);
        }
        let room_changed = match (&self.join, &desired.join) {
            (Some(old), Some(new)) => {
                old.url != new.url
                    || old.token != new.token
                    || old.device_id != new.device_id
                    || old.device_name != new.device_name
                    || old.publish != new.publish
            }
            (None, None) => false,
            _ => true,
        };
        let relay_changed =
            self.serve != desired.serve || (desired.serve.is_some() && self.relay.is_none());
        let follows_local = self.options.publish_url.is_none()
            && config.client.source == NetworkClientSource::FollowLocal;
        let room_changed = room_changed
            || (desired.join.is_some()
                && (self.room.is_none() || (relay_changed && follows_local)));
        let same_port = relay_changed
            && self
                .serve
                .as_ref()
                .zip(desired.serve.as_ref())
                .is_some_and(|(old, new)| old.port == new.port);

        if same_port && let Some(relay) = self.relay.take() {
            relay.stop().map_err(|_| NetworkApplyError::Relay)?;
        }
        let candidates = (|| {
            let relay = if relay_changed {
                desired
                    .serve
                    .as_ref()
                    .map(bind_relay)
                    .transpose()
                    .map_err(|_| NetworkApplyError::Relay)?
            } else {
                None
            };
            let room = if room_changed {
                desired
                    .join
                    .as_ref()
                    .map(|join| {
                        bongocat_net::client::prepare_room(
                            join.client_config(),
                            Duration::from_secs(2),
                        )
                        .map_err(|_| NetworkApplyError::Room)
                    })
                    .transpose()?
            } else {
                None
            };
            Ok::<_, NetworkApplyError>((relay, room))
        })();

        let (relay, room) = match candidates {
            Ok(prepared) => prepared,
            Err(error) => {
                if same_port && let Some(old) = &self.serve {
                    self.relay = bind_relay(old).ok();
                    if self.relay.is_none() {
                        update_network_status(|status| status.relay = None);
                        return Err(NetworkApplyError::Restore);
                    }
                }
                return Err(error);
            }
        };

        if room_changed {
            if let Some(old) = self.room.take() {
                let _ = old.stop(Duration::from_millis(500));
            }
            let next = room
                .map(|candidate| {
                    self.attach(
                        desired.join.as_ref().expect("admitted room has a plan"),
                        candidate.events,
                        candidate.commands,
                        candidate.worker,
                        Some(candidate.initial_roster),
                    )
                })
                .transpose();
            match next {
                Ok(next) => self.room = next,
                Err(_) => {
                    drop(relay);
                    if same_port && let Some(old) = &self.serve {
                        self.relay = bind_relay(old).ok();
                    }
                    if let Some(old) = &self.join {
                        self.room = self.open(old).ok();
                    }
                    if (same_port && self.relay.is_none())
                        || (self.join.is_some() && self.room.is_none())
                    {
                        update_network_status(|status| {
                            if self.relay.is_none() {
                                status.relay = None;
                            }
                            if self.room.is_none() {
                                status.room = None;
                            }
                        });
                        return Err(NetworkApplyError::Restore);
                    }
                    return Err(NetworkApplyError::Room);
                }
            }
            if self.room.is_none() {
                update_network_status(|status| status.room = None);
            }
        }
        if relay_changed {
            self.relay = relay;
            self.serve = desired.serve;
            update_network_status(|status| {
                status.relay = self.serve.as_ref().map(|serve| RelayStatus {
                    url: format!("ws://{}", serve.bind.addr(serve.port)),
                });
            });
        }
        self.join = desired.join;
        Ok(())
    }

    fn open(&self, join: &JoinPlan) -> Result<RoomTransport, String> {
        let (events, commands, client) = bongocat_net::client::spawn_managed(join.client_config())
            .map_err(|error| error.to_string())?;
        publish_room(RoomState::Connecting, None, &[]);
        self.attach(join, events, commands, client, None)
    }
}

impl JoinPlan {
    fn client_config(&self) -> ClientConfig {
        ClientConfig {
            url: self.url.clone(),
            token: self.token.clone(),
            device: DeviceInfo {
                id: self.device_id.clone(),
                name: self.device_name.clone(),
                publish: self.publish,
            },
        }
    }
}

pub(crate) struct RoomTransport {
    client: ClientWorker,
    bridge: std::thread::JoinHandle<()>,
    publish: Option<std::thread::JoinHandle<()>>,
    sink: Arc<Mutex<Option<std::sync::mpsc::SyncSender<InputFrame>>>>,
    cursor: bongocat_input::CursorProducer,
}

impl RoomTransport {
    pub(crate) fn stop(self, timeout: Duration) -> bool {
        self.cursor.set_observer(None);
        self.sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let stopped = self.client.stop(timeout);
        // The outbound bridge owns only a queue receiver and the inbound
        // bridge owns only the client's event receiver. Once the client
        // exits and the outbound sender is dropped, both return.
        let publish_stopped = self.publish.is_none_or(|worker| worker.join().is_ok());
        stopped && self.bridge.join().is_ok() && publish_stopped
    }
}

impl DisplayRouting {
    pub(crate) fn select(&self, next: DisplaySelection) {
        let mut selection = self
            .selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if selection.local != next.local {
            self.suppress_local.store(true, Ordering::Release);
            self.cursor.set_local_suppressed(true);
            let _ = self.primary.recover(
                bongocat_input::InputResetReason::ServiceRestart,
                now_millis(),
            );
            center_cursor(&self.cursor);
            self.suppress_local.store(!next.local, Ordering::Release);
            self.cursor.set_local_suppressed(!next.local);
        }
        *selection = next;
    }
}

fn center_cursor(cursor: &bongocat_input::CursorProducer) {
    use bongocat_input::{CursorPosition, CursorSample, CursorViewport};
    let sample = CursorSample::new(
        CursorPosition { x: 0.5, y: 0.5 },
        CursorViewport {
            origin: CursorPosition { x: 0.0, y: 0.0 },
            width: 1.0,
            height: 1.0,
        },
        now_millis(),
    )
    .expect("unit viewport and centered cursor");
    let _ = cursor.publish_remote(sample);
}

impl DrawMode {
    pub(crate) const fn draws(&self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

impl NetworkPlan {
    pub(crate) fn initial_display(&self) -> DisplaySelection {
        self.join.as_ref().map_or(
            DisplaySelection {
                local: self.primary_display_selected,
                draw: DrawMode::Nothing,
                extra: None,
            },
            |join| DisplaySelection {
                local: join.show_self,
                draw: join.draw.clone(),
                extra: join.extra.clone(),
            },
        )
    }
    pub(crate) fn shows_both(&self) -> bool {
        self.join
            .as_ref()
            .is_some_and(|join| join.show_self && join.draw.draws())
    }

    /// Whether this run takes input from the room rather than from this machine.
    pub(crate) fn watching(&self) -> bool {
        self.join
            .as_ref()
            .is_some_and(|join| join.draw.draws() && !join.publish && !join.show_self)
    }
}

/// Reads the command line over the configuration.
///
/// A flag wins where it says anything: it is a decision made for one run, while a
/// configuration value is the default every run starts from. The room key is the
/// exception — a client following the local server has to present that server's
/// key, or the two halves of one document would describe rooms that cannot meet.
pub(crate) fn plan(options: &RunOptions, config: &NetworkConfig) -> NetworkPlan {
    let device_id = options
        .device_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| non_empty(&config.device.id).unwrap_or_else(host_name));
    let device_name = non_empty(&config.device.name).unwrap_or_else(host_name);

    let serve = serve_plan(config);
    let port = serve
        .as_ref()
        .map(|serve| serve.port)
        .unwrap_or(config.server.port);

    let manual = options.publish_url.is_some();
    let source = if manual {
        NetworkClientSource::Manual
    } else {
        config.client.source
    };
    let url = match source {
        NetworkClientSource::FollowLocal => Some(format!("ws://127.0.0.1:{port}")),
        NetworkClientSource::Manual => options
            .publish_url
            .clone()
            .or_else(|| config.client.url.clone())
            .filter(|url| !url.trim().is_empty()),
    };
    let token = match source {
        NetworkClientSource::FollowLocal => serve.as_ref().map(|relay| relay.token.clone()),
        NetworkClientSource::Manual => options
            .publish_token
            .clone()
            .or_else(|| config.client.token.clone()),
    };

    // The room is joined only when the section says so, or when a flag asks for
    // it: serving a relay must not drag this machine into its own room, and a
    // machine that switched the client off must open no socket at all.
    let enabled = config.client.enabled || manual || options.watch;
    let publish = enabled && !options.watch && config.client.publish;
    // "This machine" is an entry in the same list that names the peers: it is
    // what keeps the pet dancing to this machine's own keys, and taking it out
    // is the switch the page presents as "follow my own input".
    let show_self = !options.watch
        && config
            .client
            .display
            .iter()
            .any(|entry| entry == bongocat_config::NETWORK_SELF_DEVICE);
    let display = DisplaySelection::from_devices(&config.client.display, enabled);
    let draw = if options.watch && enabled {
        DrawMode::Any
    } else {
        display.draw
    };
    let extra = if options.watch { None } else { display.extra };

    let join = match (enabled, url, token) {
        (true, Some(url), Some(token)) if !token.is_empty() => Some(JoinPlan {
            url,
            token,
            device_id,
            device_name,
            publish,
            show_self,
            draw,
            extra,
        }),
        _ => None,
    };
    // An enabled section that cannot name its address or room key would
    // otherwise sit "Off" forever with nothing on screen to say why.
    if enabled && join.is_none() {
        let _ = writeln!(
            std::io::stderr(),
            "net: not joining; the section is enabled but the address or the room key is missing"
        );
    }

    let primary_display_selected = show_self || join.as_ref().is_some_and(|join| join.draw.draws());
    NetworkPlan {
        serve,
        join,
        primary_display_selected,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn serve_plan(config: &NetworkConfig) -> Option<ServePlan> {
    if !config.server.enabled {
        return None;
    }
    let token = config.server.token.as_deref().map(str::trim)?;
    if token.is_empty() {
        return None;
    }
    let bind = match config.server.listen {
        NetworkListenMode::LocalOnly => Some(BindMode::LocalOnly),
        NetworkListenMode::Ipv4 => Some(BindMode::Ipv4Only),
        NetworkListenMode::DualStack => Some(BindMode::DualStack),
        NetworkListenMode::Custom => config
            .server
            .address
            .as_deref()
            .and_then(|address| address.trim().parse::<IpAddr>().ok())
            .map(BindMode::Custom),
    };
    // A section that cannot be served is reported and skipped rather than
    // failing the run: the pet is the product, and a mistyped interface should
    // not be what keeps it from starting.
    let Some(bind) = bind else {
        let _ = writeln!(
            std::io::stderr(),
            "relay: not serving; --listen custom needs an interface address"
        );
        return None;
    };
    Some(ServePlan {
        bind,
        port: config.server.port,
        token: token.to_owned(),
    })
}

/// Starts everything the plan asks for and returns the producer the pet should
/// read from.
///
/// A publishing run gets a tee that mirrors every local edge to the room; every
/// other run keeps the producer it already had, so a machine that is not in a
/// room opens no socket and starts no thread.
pub(crate) fn start(
    options: RunOptions,
    plan: &NetworkPlan,
    base: InputProducer,
    cursor: bongocat_input::CursorProducer,
    peer: Option<bongocat_runtime::RuntimeClient>,
    extra: Option<bongocat_runtime::RuntimeClient>,
) -> Result<NetworkSession, String> {
    let relay = plan.serve.as_ref().map(bind_relay).transpose()?;
    update_network_status(|status| {
        status.relay = plan.serve.as_ref().map(|serve| RelayStatus {
            url: format!("ws://{}", serve.bind.addr(serve.port)),
        });
    });
    let initial = plan.initial_display();
    let selection = Arc::new(Mutex::new(initial.clone()));
    let suppress_local = Arc::new(AtomicBool::new(!initial.local));
    cursor.set_local_suppressed(!initial.local);
    let routing = DisplayRouting {
        selection: Arc::clone(&selection),
        suppress_local: Arc::clone(&suppress_local),
        primary: base.clone(),
        cursor: cursor.clone(),
    };
    let sink = Arc::new(Mutex::new(None));
    let local_input = Arc::new(TeeSubmitter::with_switchable_sink(
        base.clone(),
        Arc::clone(&sink),
        suppress_local,
    ));
    let input = InputProducer::new(local_input.clone());
    let mut control = TransportController {
        options,
        relay,
        serve: plan.serve.clone(),
        room: None,
        join: plan.join.clone(),
        primary: base,
        cursor,
        secondary: peer,
        extra,
        selection,
        sink,
    };
    control.room = plan
        .join
        .as_ref()
        .map(|join| control.open(join))
        .transpose()?;
    Ok(NetworkSession {
        input,
        local_input,
        display: routing,
        control,
    })
}

impl TransportController {
    fn attach(
        &self,
        join: &JoinPlan,
        events: Receiver<ClientEvent>,
        commands: std::sync::mpsc::Sender<ClientCommand>,
        client: ClientWorker,
        initial_roster: Option<ClientEvent>,
    ) -> Result<RoomTransport, String> {
        let cursor = self.cursor.clone();
        let sink = Arc::clone(&self.sink);
        // The pointer mirror samples the cursor slot from inside publish, so it
        // never races the renderer's take, and it rate-limits itself to well under
        // the room's input budget: a pointer position superseded by the next one is
        // worthless, but sixty of them a second is still sixty frames the relay
        // forwards for nothing.
        if join.publish {
            let pointer_commands = commands.clone();
            let pointer_device = join.device_id.clone();
            let last_pointer_sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let pointer_sequence = Arc::new(std::sync::atomic::AtomicU64::new(0));
            cursor.set_observer(Some(Arc::new(move |sample| {
                const POINTER_MIN_INTERVAL_MS: u64 = 16;
                let now = now_millis().value();
                // The baseline only moves when a frame is actually sent: swapping
                // on every sample would push the deadline forward forever, and a
                // 10 ms capture against a 16 ms budget would starve itself to a
                // trickle — the jerky trail this replaces.
                let last = last_pointer_sent.load(std::sync::atomic::Ordering::Relaxed);
                if now.saturating_sub(last) < POINTER_MIN_INTERVAL_MS {
                    return;
                }
                last_pointer_sent.store(now, std::sync::atomic::Ordering::Relaxed);
                let frame = bongocat_net::protocol::PointerFrame {
                    src: pointer_device.clone(),
                    seq: pointer_sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    t: now,
                    x: sample.position.x,
                    y: sample.position.y,
                };
                let _ = pointer_commands.send(ClientCommand::PublishPointer(frame));
            })));
        }

        // The bridge only moves frames. The waiting happens here rather than on the
        // input thread, which is why the tee is allowed to drop instead of block.
        // The pet draws this machine's own input only when "this machine" is still
        // in the device list; a publisher that turned it off sends to the room but
        // does not dance, and a viewer that turned it off feeds the pet nothing at
        // all. The flag is shared so the tee (which owns the submit path) reads the
        // same decision the plan already made — and the tee exists for every joined
        // run, not only for publishers: without it a viewer's own input would reach
        // the pet beside whatever the room is sending.
        let publish = if join.publish {
            let routing_sink = &sink;
            let (sink, queue) = sync_channel::<InputFrame>(SINK_CAPACITY);
            let commands = commands.clone();
            let device_id = join.device_id.clone();
            let worker = std::thread::Builder::new()
                .name("bongocat-publish".to_owned())
                .spawn(move || {
                    while let Ok(mut frame) = queue.recv() {
                        // The tee cannot know which device it runs on, and the relay
                        // forwards frames verbatim to save a parse and a build per
                        // frame, so the sender is the one place the source can be
                        // named. Without it the far end's device list can never
                        // match a frame, and its "following" row reads an empty id.
                        frame.src = device_id.clone();
                        if commands.send(ClientCommand::Publish(frame)).is_err() {
                            break;
                        }
                    }
                })
                .map_err(|error| format!("failed to start the publish bridge: {error}"))?;
            *routing_sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
            Some(worker)
        } else {
            None
        };

        // Draining the event side is not optional: the client only `try_send`s, so an
        // unread channel would silently discard every status change and every input
        // frame coming back from the room.
        let room_base = self.primary.clone();
        let room_cursor = cursor.clone();
        let selection = Arc::clone(&self.selection);
        let peer = self.secondary.clone();
        let extra = self.extra.clone();
        let bridge = std::thread::Builder::new()
            .name("bongocat-room".to_owned())
            .spawn(move || {
                let extra = extra.map(|peer| (peer.input_producer(), peer.cursor_producer()));
                let secondary = peer.map(|peer| (peer.input_producer(), peer.cursor_producer()));
                run_room_loop(
                    &events,
                    &room_base,
                    &room_cursor,
                    &selection,
                    secondary.as_ref(),
                    extra.as_ref(),
                    initial_roster,
                    true,
                );
            })
            .map_err(|error| format!("failed to start the room thread: {error}"))?;

        Ok(RoomTransport {
            client,
            bridge,
            publish,
            sink,
            cursor,
        })
    }
}

fn bind_relay(plan: &ServePlan) -> Result<RunningRelay, String> {
    let config = RelayConfig {
        bind: plan.bind,
        port: plan.port,
        token: plan.token.clone(),
    };
    bongocat_net::relay::BoundRelay::bind(config)
        .and_then(bongocat_net::relay::BoundRelay::spawn)
        .map_err(|error| error.to_string())
}

/// Turns a room's events into local input, and into the status the settings window
/// shows.
#[cfg(test)]
fn room_loop(
    events: &Receiver<ClientEvent>,
    producer: &InputProducer,
    draw: &DrawMode,
    cursor: &bongocat_input::CursorProducer,
) {
    let selection = Mutex::new(DisplaySelection {
        local: false,
        draw: draw.clone(),
        extra: None,
    });
    run_room_loop(
        events, producer, cursor, &selection, None, None, None, false,
    );
}

#[allow(clippy::too_many_arguments)]
fn run_room_loop(
    events: &Receiver<ClientEvent>,
    primary: &InputProducer,
    primary_cursor: &bongocat_input::CursorProducer,
    selection: &Mutex<DisplaySelection>,
    secondary: Option<&(InputProducer, bongocat_input::CursorProducer)>,
    extra: Option<&(InputProducer, bongocat_input::CursorProducer)>,
    mut initial_roster: Option<ClientEvent>,
    reset_on_disconnect: bool,
) {
    let mut room_peers: Vec<bongocat_net::protocol::PeerInfo> = Vec::new();
    let mut peers: u16 = 0;
    let mut bound: Option<String> = None;
    // What this viewer believes is held down on the far machine. A release frame
    // dropped anywhere in the chain would leave the pet pressing a key forever;
    // this set is the only thing that can recover from it.
    let mut pressed: BTreeSet<InputControl> = BTreeSet::new();
    let mut last_frame = Instant::now();
    let mut extra_pressed = BTreeSet::new();
    let mut extra_last_frame = Instant::now();
    let mut active = selection
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut producer = primary;
    let mut cursor = primary_cursor;
    if active.both()
        && let Some((inputs, pointer)) = secondary
    {
        (producer, cursor) = (inputs, pointer);
    }

    loop {
        let pending = initial_roster
            .take()
            .map_or_else(|| events.recv_timeout(SILENCE_CHECK), Ok);
        let selected = selection.lock().unwrap_or_else(PoisonError::into_inner);
        if active.extra != selected.extra
            && let Some((producer, cursor)) = extra
        {
            release_held(producer, &mut extra_pressed);
            center_cursor(cursor);
        }
        if selected.local != active.local || selected.draw != active.draw {
            if !active.local && selected.local {
                // The main thread already reset the primary before allowing local input.
                pressed.clear();
            } else {
                release_held(producer, &mut pressed);
                if active.draw.draws() {
                    center_cursor(cursor);
                }
            }
            bound = None;
            update_network_status(|status| {
                if let Some(room) = &mut status.room {
                    room.bound = None;
                }
            });
            active.local = selected.local;
            active.draw.clone_from(&selected.draw);
            (producer, cursor) = (primary, primary_cursor);
            if active.both()
                && let Some((inputs, pointer)) = secondary
            {
                (producer, cursor) = (inputs, pointer);
            }
        }
        active.extra.clone_from(&selected.extra);
        if let Some((producer, _)) = extra
            && !extra_pressed.is_empty()
            && extra_last_frame.elapsed() >= SILENCE_LIMIT
        {
            release_held(producer, &mut extra_pressed);
        }
        let draw = &active.draw;
        let event = match pending {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if let Some((producer, _)) = extra
                    && extra_last_frame.elapsed() >= SILENCE_LIMIT
                {
                    release_held(producer, &mut extra_pressed);
                }
                if !pressed.is_empty() && last_frame.elapsed() >= SILENCE_LIMIT {
                    release_held(producer, &mut pressed);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                if reset_on_disconnect && let Some((producer, cursor)) = extra {
                    release_held(producer, &mut extra_pressed);
                    center_cursor(cursor);
                }
                if reset_on_disconnect {
                    release_held(producer, &mut pressed);
                    if bound.is_some() {
                        center_cursor(cursor);
                    }
                }
                break;
            }
        };

        if let Some((producer, cursor)) = extra {
            match &event {
                ClientEvent::Input(frame) if active.extra.as_ref() == Some(&frame.src) => {
                    match frame.edge {
                        InputEdge::Down => {
                            extra_pressed.insert(frame.control);
                        }
                        InputEdge::Up => {
                            extra_pressed.remove(&frame.control);
                        }
                    }
                    extra_last_frame = Instant::now();
                    let _ = producer.publish(InputEvent::Edge {
                        control: frame.control,
                        edge: frame.edge,
                        source: InputSource::Capture,
                        at: now_millis(),
                    });
                }
                ClientEvent::Pointer(frame) if active.extra.as_ref() == Some(&frame.src) => {
                    publish_pointer(cursor, frame.x, frame.y);
                }
                ClientEvent::Joined { .. }
                | ClientEvent::Reconnecting { .. }
                | ClientEvent::Failed(_) => {
                    release_held(producer, &mut extra_pressed);
                    center_cursor(cursor);
                }
                ClientEvent::PeerLeft(id) if active.extra.as_ref() == Some(id) => {
                    release_held(producer, &mut extra_pressed);
                    center_cursor(cursor);
                }
                ClientEvent::PeerJoined(peer) if active.extra.as_ref() == Some(&peer.id) => {
                    release_held(producer, &mut extra_pressed);
                    center_cursor(cursor);
                }
                _ => {}
            }
        }

        match event {
            ClientEvent::Input(frame) => {
                match draw {
                    DrawMode::Nothing => continue,
                    DrawMode::Any => {
                        if bound.as_deref() != Some(frame.src.as_str()) {
                            if bound.is_some() {
                                continue;
                            }
                            let _ = writeln!(
                                std::io::stderr(),
                                "watch: drawing {} from the room",
                                frame.src
                            );
                            bound = Some(frame.src.clone());
                            publish_room(
                                RoomState::Connected { peers },
                                bound.as_deref(),
                                &room_peers,
                            );
                        }
                    }
                    DrawMode::Device(device) => {
                        if frame.src != *device {
                            continue;
                        }
                        if bound.as_deref() != Some(device.as_str()) {
                            let _ = writeln!(
                                std::io::stderr(),
                                "watch: drawing {device} from the room"
                            );
                            bound = Some(device.clone());
                            publish_room(
                                RoomState::Connected { peers },
                                bound.as_deref(),
                                &room_peers,
                            );
                        }
                    }
                }
                match frame.edge {
                    InputEdge::Down => {
                        pressed.insert(frame.control);
                    }
                    InputEdge::Up => {
                        pressed.remove(&frame.control);
                    }
                }
                last_frame = Instant::now();
                let input = InputEvent::Edge {
                    control: frame.control,
                    edge: frame.edge,
                    source: InputSource::Capture,
                    // The sender's clock starts at its own boot, so replaying it
                    // here would place events in this machine's past. The arrival
                    // time is the only comparable stamp.
                    at: now_millis(),
                };
                if let Err(error) = producer.publish(input) {
                    let _ = writeln!(std::io::stderr(), "watch: publish failed: {error}");
                }
            }
            ClientEvent::Pointer(frame) => {
                // The same display filter as the edge frames: a pointer position
                // from a device this pet is not drawing is ignored outright.
                let draws_this = match draw {
                    DrawMode::Nothing => false,
                    DrawMode::Any => bound.as_deref().is_none_or(|id| id == frame.src),
                    DrawMode::Device(device) => frame.src == *device,
                };
                if !draws_this {
                    continue;
                }
                if bound.is_none() {
                    bound = Some(frame.src.clone());
                    publish_room(
                        RoomState::Connected { peers },
                        bound.as_deref(),
                        &room_peers,
                    );
                }
                // The frame already carries normalized 0..1 coordinates; the
                // unit viewport reproduces them exactly on this machine's
                // screen, so no geometry needs to travel for the common case.
                publish_pointer(cursor, frame.x, frame.y);
            }
            ClientEvent::Joined { peers: room, .. } => {
                room_peers = room;
                peers = room_peers.len().min(u16::MAX as usize) as u16;
                let _ = writeln!(
                    std::io::stderr(),
                    "watch: in the room, {} peer(s) publishing",
                    room_peers.iter().filter(|peer| peer.publish).count()
                );
                // A reconnect invalidates the binding: the far end may have
                // restarted with a different id, and holding a stale one would
                // leave the pet frozen. Whatever we believe is held came from a
                // session that no longer exists.
                release_held(producer, &mut pressed);
                bound = None;
                publish_room(RoomState::Connected { peers }, None, &room_peers);
            }
            ClientEvent::Latency(latency_ms) => {
                update_network_status(|status| {
                    if let Some(room) = status.room.as_mut() {
                        room.latency_ms = Some(latency_ms);
                    }
                });
            }
            ClientEvent::PeerJoined(peer) => {
                // A replacement with the same ID retires its predecessor
                // without a separate departure notification.
                if bound.as_deref() == Some(peer.id.as_str())
                    && room_peers.iter().any(|old| old.id == peer.id)
                {
                    release_held(producer, &mut pressed);
                    center_cursor(cursor);
                }
                room_peers.retain(|old| old.id != peer.id);
                room_peers.push(peer);
                peers = room_peers.len().min(u16::MAX as usize) as u16;
                publish_room(
                    RoomState::Connected { peers },
                    bound.as_deref(),
                    &room_peers,
                );
            }
            ClientEvent::PeerLeft(id) => {
                room_peers.retain(|peer| peer.id != id);
                peers = room_peers.len().min(u16::MAX as usize) as u16;
                let _ = writeln!(std::io::stderr(), "watch: {id} left");
                if bound.as_deref() == Some(id.as_str()) {
                    release_held(producer, &mut pressed);
                    bound = None;
                }
                publish_room(
                    RoomState::Connected { peers },
                    bound.as_deref(),
                    &room_peers,
                );
            }
            ClientEvent::Reconnecting { attempt, reason } => {
                let _ = writeln!(
                    std::io::stderr(),
                    "watch: reconnecting (attempt {attempt}): {reason}"
                );
                release_held(producer, &mut pressed);
                bound = None;
                room_peers.clear();
                publish_room(RoomState::Reconnecting, None, &[]);
            }
            ClientEvent::Failed(reason) => {
                let _ = writeln!(std::io::stderr(), "watch: stopped: {reason}");
                release_held(producer, &mut pressed);
                room_peers.clear();
                publish_room(RoomState::Failed, None, &[]);
                break;
            }
        }
    }
}

fn publish_pointer(cursor: &bongocat_input::CursorProducer, x: f64, y: f64) {
    if let Ok(sample) = bongocat_input::CursorSample::new(
        bongocat_input::CursorPosition { x, y },
        bongocat_input::CursorViewport {
            origin: bongocat_input::CursorPosition { x: 0.0, y: 0.0 },
            width: 1.0,
            height: 1.0,
        },
        now_millis(),
    ) {
        let _ = cursor.publish_remote(sample);
    }
}

/// Writes what the room is doing where the settings window can read it.
fn publish_room(
    state: RoomState,
    bound: Option<&str>,
    room_peers: &[bongocat_net::protocol::PeerInfo],
) {
    update_network_status(|status| {
        // The latency is sampled on its own cadence, so a state change must not
        // wipe the last sample the way it wipes the roster it is rebuilding.
        let latency_ms = status.room.as_ref().and_then(|room| room.latency_ms);
        status.room = Some(RoomStatus {
            state,
            bound: bound.map(str::to_owned),
            peers: room_peers.to_vec(),
            latency_ms,
        });
    });
}

/// Releases every control this viewer believes is held.
///
/// Publishing the release matters more than dropping the state: the render side
/// keeps its own pressed set and has no other way to learn that the key came up.
fn release_held(producer: &InputProducer, pressed: &mut BTreeSet<InputControl>) {
    let held = std::mem::take(pressed);
    if held.is_empty() {
        return;
    }
    let count = held.len();
    for control in held {
        let _ = producer.publish(InputEvent::Edge {
            control,
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: now_millis(),
        });
    }
    let _ = writeln!(
        std::io::stderr(),
        "watch: released {count} key(s) the far end never released"
    );
}

fn host_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|text| text.trim().to_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "bongocat".to_owned())
}

/// This process's monotonic clock, in the unit the input pipeline expects.
fn now_millis() -> MonotonicMillis {
    MonotonicMillis::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser;

    fn options() -> RunOptions {
        RunOptions::parse_from(["bongocat-app"])
    }

    fn serving(token: &str, port: u16) -> NetworkConfig {
        let mut config = NetworkConfig::default();
        config.server.enabled = true;
        config.server.token = Some(token.to_owned());
        config.server.port = port;
        config
    }

    #[test]
    fn two_remote_pointers_are_routed_to_different_consumers() {
        let main = bongocat_runtime::RuntimeOwner::start(true, 8);
        let first_owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let second_owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let first = (first_owner.input_producer(), first_owner.cursor_producer());
        let second = (
            second_owner.input_producer(),
            second_owner.cursor_producer(),
        );
        let selection = Mutex::new(DisplaySelection::from_devices(
            &["self".into(), "first".into(), "second".into()],
            true,
        ));
        let (sender, events) = std::sync::mpsc::channel();
        for (src, x) in [("first", 0.25), ("second", 0.75)] {
            sender
                .send(ClientEvent::Pointer(bongocat_net::protocol::PointerFrame {
                    src: src.to_owned(),
                    seq: 0,
                    t: 0,
                    x,
                    y: 0.5,
                }))
                .unwrap();
        }
        drop(sender);
        run_room_loop(
            &events,
            &main.input_producer(),
            &main.cursor_producer(),
            &selection,
            Some(&first),
            Some(&second),
            None,
            false,
        );
        assert!(main.cursor_producer().take().is_none());
        assert_eq!(first.1.take().unwrap().position.x, 0.25);
        assert_eq!(second.1.take().unwrap().position.x, 0.75);
    }

    #[test]
    fn followed_pointer_ignores_other_peers_and_is_not_republished() {
        let owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let cursor = bongocat_input::CursorProducer::new();
        cursor.set_local_suppressed(true);
        let observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_local = Arc::clone(&observed);
        cursor.set_observer(Some(Arc::new(move |_| {
            observed_local.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })));
        let (sender, receiver) = std::sync::mpsc::channel();
        for (source, x) in [("other", 0.2), ("pc", 0.8)] {
            sender
                .send(ClientEvent::Pointer(bongocat_net::protocol::PointerFrame {
                    src: source.to_owned(),
                    seq: 0,
                    t: 0,
                    x,
                    y: 0.4,
                }))
                .unwrap();
        }
        drop(sender);
        room_loop(
            &receiver,
            &owner.input_producer(),
            &DrawMode::Device("pc".into()),
            &cursor,
        );
        let received = cursor.take().expect("selected peer's pointer");
        assert_eq!(received.position.x, 0.8);
        assert_eq!(received.position.y, 0.4);
        assert_eq!(
            observed.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "remote pointer must not be echoed to the network observer"
        );
        owner.shutdown(std::time::Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn local_and_remote_pointer_share_one_monotonic_clock() {
        let owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let cursor = bongocat_input::CursorProducer::new();
        let viewport = bongocat_input::CursorViewport {
            origin: bongocat_input::CursorPosition { x: 0.0, y: 0.0 },
            width: 1.0,
            height: 1.0,
        };
        cursor
            .publish(
                bongocat_input::CursorSample::new(
                    bongocat_input::CursorPosition { x: 0.1, y: 0.1 },
                    viewport,
                    MonotonicMillis::now(),
                )
                .unwrap(),
            )
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(ClientEvent::Pointer(bongocat_net::protocol::PointerFrame {
                src: "pc".into(),
                seq: 0,
                t: 0,
                x: 0.8,
                y: 0.4,
            }))
            .unwrap();
        drop(sender);
        room_loop(
            &receiver,
            &owner.input_producer(),
            &DrawMode::Device("pc".into()),
            &cursor,
        );
        assert_eq!(
            cursor.take().expect("remote sample is accepted").position.x,
            0.8
        );
        assert_eq!(cursor.diagnostics().non_monotonic, 0);
        owner.shutdown(std::time::Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn suppressed_local_key_does_not_reset_remote_hold() {
        use bongocat_input::PhysicalKey;
        let owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let client = owner.client();
        let timeout = Duration::from_secs(2);
        client.wait_for_revision(1, timeout).unwrap();
        let remote = owner.input_producer();
        let local = InputProducer::new(Arc::new(TeeSubmitter::new(
            remote.clone(),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
        )));
        let edge = |usage, edge| InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::from_hid_usage(usage)),
            edge,
            source: InputSource::Capture,
            at: MonotonicMillis::now(),
        };
        let first = remote.publish(edge(0x04, InputEdge::Down)).unwrap();
        client.wait_for_input_sequence(first, timeout).unwrap();
        local.publish(edge(0x05, InputEdge::Down)).unwrap();
        let second = remote.publish(edge(0x06, InputEdge::Down)).unwrap();
        let state = client.wait_for_input_sequence(second, timeout).unwrap();
        assert_eq!(state.input.pressed_key_count, 2);
        assert_eq!(state.input.diagnostics.sequence_gap_count, 0);
        assert_eq!(state.input.diagnostics.non_monotonic_time_count, 0);
        let release_a = remote.publish(edge(0x04, InputEdge::Up)).unwrap();
        client.wait_for_input_sequence(release_a, timeout).unwrap();
        let release_c = remote.publish(edge(0x06, InputEdge::Up)).unwrap();
        let released = client.wait_for_input_sequence(release_c, timeout).unwrap();
        assert_eq!(released.input.pressed_key_count, 0);
        owner.shutdown(timeout).unwrap();
    }

    #[test]
    fn local_and_remote_keys_use_comparable_timestamps() {
        use bongocat_input::PhysicalKey;
        let owner = bongocat_runtime::RuntimeOwner::start(true, 8);
        let client = owner.client();
        let timeout = Duration::from_secs(2);
        client.wait_for_revision(1, timeout).unwrap();
        let producer = owner.input_producer();
        let local = producer
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::now(),
            })
            .unwrap();
        client.wait_for_input_sequence(local, timeout).unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(ClientEvent::Input(InputFrame {
                src: "pc".into(),
                seq: 0,
                t: 0,
                control: InputControl::Key(PhysicalKey::from_hid_usage(0x05)),
                edge: InputEdge::Down,
                bounds: None,
            }))
            .unwrap();
        drop(sender);
        room_loop(
            &receiver,
            &producer,
            &DrawMode::Device("pc".into()),
            &bongocat_input::CursorProducer::new(),
        );
        let state = client.wait_for_input_sequence(local + 1, timeout).unwrap();
        assert_eq!(state.input.pressed_key_count, 2);
        assert_eq!(state.input.diagnostics.non_monotonic_time_count, 0);
        assert_eq!(state.input.diagnostics.sequence_gap_count, 0);
        owner.shutdown(timeout).unwrap();
    }

    /// A machine that switched nothing on opens no socket and serves no relay,
    /// so an ordinary local pet costs nothing for a feature it does not use.
    #[test]
    fn a_configuration_that_switches_nothing_on_does_nothing() {
        let plan = plan(&options(), &NetworkConfig::default());
        assert!(plan.serve.is_none());
        assert!(plan.join.is_none());
        assert!(!plan.watching());
        assert!(plan.primary_display_selected);
    }

    #[test]
    fn empty_room_display_has_no_primary_source_even_with_publishing_enabled() {
        let mut config = serving("room-key", 8787);
        config.client.enabled = true;
        config.client.display.clear();
        let with_room = plan(&options(), &config);
        assert!(!with_room.primary_display_selected);
        assert!(with_room.join.unwrap().publish);
        config.client.enabled = false;
        let without_room = plan(&options(), &config);
        assert!(without_room.join.is_none());
        assert!(!without_room.primary_display_selected);
    }

    #[test]
    fn selected_remote_waits_for_the_configured_peer_to_publish() {
        let selection = DisplaySelection::from_devices(&["pc".into()], true);
        let mut room = RoomStatus {
            state: RoomState::Connected { peers: 2 },
            peers: vec![
                bongocat_net::protocol::PeerInfo {
                    id: "other".into(),
                    name: "Other".into(),
                    publish: true,
                },
                bongocat_net::protocol::PeerInfo {
                    id: "pc".into(),
                    name: "PC".into(),
                    publish: false,
                },
            ],
            ..RoomStatus::default()
        };
        assert!(!selection.remote_available(Some(&room)));
        room.peers[1].publish = true;
        assert!(selection.remote_available(Some(&room)));
        room.state = RoomState::Reconnecting;
        assert!(!selection.remote_available(Some(&room)));
        assert!(!selection.remote_available(None));
    }

    #[test]
    fn local_and_peer_keys_have_independent_pressed_state() {
        use bongocat_input::PhysicalKey;
        let local = bongocat_runtime::RuntimeOwner::start(true, 8);
        let peer = bongocat_runtime::RuntimeOwner::start(true, 8);
        let timeout = Duration::from_secs(2);
        let control = InputControl::Key(PhysicalKey::KEY_A);
        let local_sequence = local
            .input_producer()
            .publish(InputEvent::Edge {
                control,
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: now_millis(),
            })
            .unwrap();
        let (sender, events) = std::sync::mpsc::channel();
        for (seq, edge) in [InputEdge::Down, InputEdge::Up].into_iter().enumerate() {
            sender
                .send(ClientEvent::Input(InputFrame {
                    src: "peer".into(),
                    seq: seq as u64,
                    t: 0,
                    control,
                    edge,
                    bounds: None,
                }))
                .unwrap();
        }
        drop(sender);
        let peer_input = peer.input_producer();
        room_loop(
            &events,
            &peer_input,
            &DrawMode::Device("peer".into()),
            &peer.cursor_producer(),
        );
        let state = local
            .client()
            .wait_for_input_sequence(local_sequence, timeout)
            .unwrap();
        assert_eq!(state.input.pressed_key_count, 1);
        let state = peer
            .client()
            .wait_for_input_sequence(local_sequence + 1, timeout)
            .unwrap();
        assert_eq!(state.input.pressed_key_count, 0);
        assert_eq!(state.input.diagnostics.sequence_gap_count, 0);
        local.shutdown(timeout).unwrap();
        peer.shutdown(timeout).unwrap();
    }

    #[test]
    fn switching_remote_devices_releases_the_old_source_without_resetting_local_input() {
        use bongocat_input::PhysicalKey;
        let local = bongocat_runtime::RuntimeOwner::start(true, 8);
        let peer = bongocat_runtime::RuntimeOwner::start(true, 8);
        let timeout = Duration::from_secs(2);
        let local_input = local.input_producer();
        let local_cursor = local.cursor_producer();
        let peer_input = (peer.input_producer(), peer.cursor_producer());
        let select = |id: &str| DisplaySelection::from_devices(&["self".into(), id.into()], true);
        let selection = Mutex::new(select("first"));
        std::thread::scope(|scope| {
            let (sender, events) = std::sync::mpsc::channel();
            let send = |id: &str, control, edge| {
                sender
                    .send(ClientEvent::Input(InputFrame {
                        src: id.into(),
                        seq: 0,
                        t: 0,
                        control,
                        edge,
                        bounds: None,
                    }))
                    .unwrap()
            };
            let room_input = &local_input;
            let room_selection = &selection;
            scope.spawn(move || {
                run_room_loop(
                    &events,
                    room_input,
                    &local_cursor,
                    room_selection,
                    Some(&peer_input),
                    None,
                    None,
                    false,
                )
            });
            let key = InputControl::Key(PhysicalKey::KEY_A);
            let local_seq = local_input
                .publish(InputEvent::Edge {
                    control: key,
                    edge: InputEdge::Down,
                    source: InputSource::Capture,
                    at: now_millis(),
                })
                .unwrap();
            send("first", key, InputEdge::Down);
            assert_eq!(
                peer.client()
                    .wait_for_input_sequence(local_seq, timeout)
                    .unwrap()
                    .input
                    .pressed_key_count,
                1
            );

            *selection.lock().unwrap() = select("second");
            send("first", key, InputEdge::Down);
            let cleared = peer
                .client()
                .wait_for_input_sequence(local_seq + 1, timeout)
                .unwrap();
            assert_eq!(cleared.input.pressed_key_count, 0);
            send("second", key, InputEdge::Down);
            let received = peer
                .client()
                .wait_for_input_sequence(local_seq + 2, timeout)
                .unwrap();
            assert_eq!(received.input.pressed_key_count, 1);
            assert_eq!(
                local
                    .client()
                    .wait_for_input_sequence(local_seq, timeout)
                    .unwrap()
                    .input
                    .pressed_key_count,
                1
            );
            drop(sender);
        });
        local.shutdown(timeout).unwrap();
        peer.shutdown(timeout).unwrap();
    }

    /// Serving a relay must not drag this machine into its own room: the two are
    /// separate switches and the client one stays off.
    #[test]
    fn serving_a_relay_does_not_join_the_room_it_serves() {
        let plan = plan(&options(), &serving("room-key", 8787));
        assert!(plan.serve.is_some());
        assert!(plan.join.is_none());
    }

    /// "Follow the local server" is only coherent if the address and the room key
    /// come from the server half of the same document.
    #[test]
    fn following_the_local_server_takes_its_port_and_its_key() {
        let mut config = serving("room-key", 9001);
        config.client.enabled = true;
        let plan = plan(&options(), &config);
        let join = plan.join.clone().expect("an enabled client joins");
        assert_eq!(join.url, "ws://127.0.0.1:9001");
        assert_eq!(join.token, "room-key");
        // `display` holds only `self`, which is this machine's own input: there is
        // nothing to draw out of the room.
        assert_eq!(join.draw, DrawMode::Nothing);
        assert!(join.publish);
    }

    #[test]
    fn changing_local_relay_port_rejoins_and_an_occupied_port_keeps_the_room() {
        use std::net::{TcpListener, TcpStream};

        let free_port = || {
            TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let old_port = free_port();
        let mut config = serving("room-key", old_port);
        config.server.listen = NetworkListenMode::LocalOnly;
        config.client.enabled = true;
        let options = options();
        let original = plan(&options, &config);
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 8);
        let mut network = start(
            options,
            &original,
            runtime.input_producer(),
            runtime.cursor_producer(),
            None,
            None,
        )
        .unwrap();

        // Wait for a live session before replacing the room key on the same
        // port. This exercises accepted sockets as well as the listener.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(
            bongocat_net::network_status()
                .room
                .as_ref()
                .map(|room| &room.state),
            Some(RoomState::Connected { .. })
        ) {
            assert!(Instant::now() < deadline, "original room did not join");
            std::thread::sleep(Duration::from_millis(5));
        }
        config.server.token = Some("replacement-key".into());
        network.control.apply(&config).unwrap();
        assert_eq!(
            network.control.join.as_ref().unwrap().token,
            "replacement-key"
        );
        let mut rejected = config.clone();
        rejected.client.source = NetworkClientSource::Manual;
        rejected.client.url = Some(format!("ws://127.0.0.1:{old_port}"));
        rejected.client.token = Some("wrong-key".into());
        assert!(network.control.apply(&rejected).is_err());
        assert_eq!(
            network.control.join.as_ref().unwrap().token,
            "replacement-key"
        );

        config.client.publish = false;
        network.control.apply(&config).unwrap();
        assert!(!network.control.publishing_configured());

        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        config.server.port = occupied.local_addr().unwrap().port();
        assert!(network.control.apply(&config).is_err());
        assert_eq!(network.control.serve.as_ref().unwrap().port, old_port);
        assert!(TcpStream::connect(("127.0.0.1", old_port)).is_ok());

        let new_port = free_port();
        config.server.port = new_port;
        network.control.apply(&config).unwrap();
        assert_eq!(
            network.control.join.as_ref().unwrap().url,
            format!("ws://127.0.0.1:{new_port}")
        );
        assert!(TcpStream::connect(("127.0.0.1", old_port)).is_err());
        assert!(TcpStream::connect(("127.0.0.1", new_port)).is_ok());

        config.server.enabled = false;
        network.control.apply(&config).unwrap();
        assert!(!network.control.connected_room_configured());
        assert!(TcpStream::connect(("127.0.0.1", new_port)).is_err());
        network.control.shutdown();
        runtime.shutdown(Duration::from_secs(2)).unwrap();
    }

    /// A selected peer gets its own display while the local display is enabled.
    #[test]
    fn a_peer_in_the_display_list_is_what_this_machine_draws() {
        let mut config = NetworkConfig::default();
        config.client.enabled = true;
        config.client.source = NetworkClientSource::Manual;
        config.client.url = Some("ws://192.168.1.9:8787".to_owned());
        config.client.token = Some("room-key".to_owned());
        config.client.publish = false;
        config.client.display = vec!["self".to_owned(), "pc-02".to_owned()];
        let plan = plan(&options(), &config);
        let join = plan.join.clone().expect("a manual client with a url joins");
        assert_eq!(join.draw, DrawMode::Device("pc-02".to_owned()));
        assert!(!join.publish);
        assert!(plan.shows_both());
        assert!(plan.primary_display_selected);
        assert!(!plan.watching());
    }

    /// A flag is a decision made for one run, so it wins over the document.
    #[test]
    fn a_command_line_flag_beats_the_configuration() {
        let mut config = NetworkConfig::default();
        config.client.enabled = false;
        config.client.source = NetworkClientSource::Manual;
        config.client.url = Some("ws://ignored.invalid:1".to_owned());
        config.client.token = Some("ignored".to_owned());
        let mut options = options();
        options.watch = true;
        options.publish_url = Some("ws://192.168.1.9:8787".to_owned());
        options.publish_token = Some("room-key".to_owned());
        let plan = plan(&options, &config);
        let join = plan
            .join
            .clone()
            .expect("a flag joins even when the section is off");
        assert_eq!(join.url, "ws://192.168.1.9:8787");
        assert_eq!(join.token, "room-key");
        assert_eq!(join.draw, DrawMode::Any);
        // A viewer captures nothing locally, so it never publishes.
        assert!(!join.publish);
        assert!(plan.watching());
    }

    /// A custom bind with no interface is skipped rather than becoming a relay
    /// that cannot start.
    #[test]
    fn a_custom_bind_without_an_interface_is_not_served() {
        let mut config = serving("room-key", 8787);
        config.server.listen = NetworkListenMode::Custom;
        config.server.address = None;
        assert!(plan(&options(), &config).serve.is_none());

        config.server.address = Some("192.168.1.9".to_owned());
        let plan = plan(&options(), &config);
        let serve = plan.serve.expect("a named interface is served");
        assert_eq!(serve.bind.addr(serve.port).to_string(), "192.168.1.9:8787");
    }
}
