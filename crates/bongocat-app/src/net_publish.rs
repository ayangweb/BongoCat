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
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

use bongocat_config::{NetworkClientSource, NetworkConfig, NetworkListenMode};
use bongocat_input::{
    InputControl, InputEdge, InputEvent, InputProducer, InputSource, MonotonicMillis,
};
use bongocat_net::client::{ClientCommand, ClientConfig, ClientEvent};
use bongocat_net::protocol::{DeviceInfo, InputFrame};
use bongocat_net::relay::{BindMode, RelayConfig};
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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NetworkPlan {
    /// The relay this run serves alongside the pet, if any.
    pub(crate) serve: Option<ServePlan>,
    /// The room this run joins, if any.
    pub(crate) join: Option<JoinPlan>,
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

impl DrawMode {
    pub(crate) const fn draws(&self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

impl NetworkPlan {
    /// Whether this run takes input from the room rather than from this machine.
    pub(crate) fn watching(&self) -> bool {
        self.join
            .as_ref()
            .is_some_and(|join| join.draw.draws() && !join.publish)
    }

    /// Whether this run is in a room yet has nothing for the pet to draw.
    ///
    /// "This machine" is off and no peer is listed: the joined socket exists,
    /// but a pet dancing to nothing reads as the switch being broken, so the
    /// window hides until the tray or a restart brings it back.
    pub(crate) fn in_room_without_pet(&self) -> bool {
        self.join
            .as_ref()
            .is_some_and(|join| !join.show_self && !join.draw.draws())
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
        NetworkClientSource::FollowLocal => config.server.token.clone(),
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
    let show_self = config
        .client
        .display
        .iter()
        .any(|entry| entry == bongocat_config::NETWORK_SELF_DEVICE);
    let draw = if !enabled {
        DrawMode::Nothing
    } else if options.watch {
        DrawMode::Any
    } else {
        config
            .client
            .display
            .iter()
            .find(|entry| entry.as_str() != bongocat_config::NETWORK_SELF_DEVICE)
            .map_or(DrawMode::Nothing, |device| DrawMode::Device(device.clone()))
    };

    let join = match (enabled, url, token) {
        (true, Some(url), Some(token)) if !token.is_empty() => Some(JoinPlan {
            url,
            token,
            device_id,
            device_name,
            publish,
            show_self,
            draw,
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

    NetworkPlan { serve, join }
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
    plan: &NetworkPlan,
    base: InputProducer,
    cursor: bongocat_input::CursorProducer,
) -> Result<InputProducer, String> {
    if let Some(serve) = &plan.serve {
        start_relay(serve)?;
    }
    let Some(join) = &plan.join else {
        return Ok(base);
    };
    let config = ClientConfig {
        url: join.url.clone(),
        token: join.token.clone(),
        device: DeviceInfo {
            id: join.device_id.clone(),
            name: join.device_name.clone(),
            publish: join.publish,
        },
    };
    let (events, commands) =
        bongocat_net::client::spawn(config).map_err(|error| error.to_string())?;
    publish_room(RoomState::Connecting, None, &[]);

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
    let suppress_local = Arc::new(std::sync::atomic::AtomicBool::new(!join.show_self));
    let sink = if join.publish {
        let (sink, queue) = sync_channel::<InputFrame>(SINK_CAPACITY);
        let commands = commands.clone();
        let device_id = join.device_id.clone();
        std::thread::Builder::new()
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
        Some(sink)
    } else {
        None
    };

    // Draining the event side is not optional: the client only `try_send`s, so an
    // unread channel would silently discard every status change and every input
    // frame coming back from the room.
    let producer = base.clone();
    let draw = join.draw.clone();
    std::thread::Builder::new()
        .name("bongocat-room".to_owned())
        .spawn(move || {
            // The client reads a closed command channel as "the owner is gone" and
            // stops. A machine that only draws sends no commands, so this sender
            // exists purely to keep the session alive for as long as the thread runs.
            let _commands = commands;
            room_loop(&events, &producer, &draw, &cursor);
        })
        .map_err(|error| format!("failed to start the room thread: {error}"))?;

    // Every joined run goes through the tee: it is the gate that keeps this
    // machine's own input away from a pet the device list has pointed at
    // someone else, publisher or not.
    Ok(base.with_submitter(Arc::new(TeeSubmitter::new(
        base.submitter(),
        sink,
        suppress_local,
    ))))
}

/// Serves a relay on its own thread.
///
/// `run` blocks for the lifetime of the process, so a run that is also a pet has to
/// give it a thread of its own; the relay is otherwise the same server the
/// `--relay` mode starts.
fn start_relay(plan: &ServePlan) -> Result<(), String> {
    let config = RelayConfig {
        bind: plan.bind,
        port: plan.port,
        token: plan.token.clone(),
    };
    let url = format!("ws://{}", plan.bind.addr(plan.port));
    std::thread::Builder::new()
        .name("bongocat-relay".to_owned())
        .spawn(move || {
            if let Err(error) = bongocat_net::relay::run(config) {
                let _ = writeln!(std::io::stderr(), "relay: {error}");
            }
            update_network_status(|status| status.relay = None);
        })
        .map_err(|error| format!("failed to start the relay: {error}"))?;
    update_network_status(|status| status.relay = Some(RelayStatus { url }));
    Ok(())
}

/// Turns a room's events into local input, and into the status the settings window
/// shows.
fn room_loop(
    events: &Receiver<ClientEvent>,
    producer: &InputProducer,
    draw: &DrawMode,
    cursor: &bongocat_input::CursorProducer,
) {
    let mut room_peers: Vec<bongocat_net::protocol::PeerInfo> = Vec::new();
    let mut peers: u16 = 0;
    let mut bound: Option<String> = None;
    // What this viewer believes is held down on the far machine. A release frame
    // dropped anywhere in the chain would leave the pet pressing a key forever;
    // this set is the only thing that can recover from it.
    let mut pressed: BTreeSet<InputControl> = BTreeSet::new();
    let mut last_frame = Instant::now();

    loop {
        let event = match events.recv_timeout(SILENCE_CHECK) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if !pressed.is_empty() && last_frame.elapsed() >= SILENCE_LIMIT {
                    release_held(producer, &mut pressed);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

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
                    DrawMode::Any => true,
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
                if let Ok(sample) = bongocat_input::CursorSample::new(
                    bongocat_input::CursorPosition {
                        x: frame.x,
                        y: frame.y,
                    },
                    bongocat_input::CursorViewport {
                        origin: bongocat_input::CursorPosition { x: 0.0, y: 0.0 },
                        width: 1.0,
                        height: 1.0,
                    },
                    MonotonicMillis::new(now_millis().value()),
                ) {
                    let _ = cursor.publish(sample);
                }
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
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(std::time::Instant::now).elapsed();
    MonotonicMillis::new(elapsed.as_millis() as u64)
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

    /// A machine that switched nothing on opens no socket and serves no relay,
    /// so an ordinary local pet costs nothing for a feature it does not use.
    #[test]
    fn a_configuration_that_switches_nothing_on_does_nothing() {
        let plan = plan(&options(), &NetworkConfig::default());
        assert_eq!(plan, NetworkPlan::default());
        assert!(!plan.watching());
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

    /// The first entry of the display list that is not this machine is the one
    /// pet this machine draws.
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
        assert!(plan.watching());
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
