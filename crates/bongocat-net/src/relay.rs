//! The relay: a star hub that fans input frames out to a room.
//!
//! Deliberately stateless with respect to history — it forwards and forgets.
//! The only state it keeps is who is currently connected under which token.
//!
//! The relay is synchronous on purpose: one blocking thread per connection is
//! plenty for a handful of displays, and it keeps the input path free of an
//! async runtime that would otherwise have to be justified for a LAN hop.
//!
//! Threading model, per connection:
//!
//! - one thread owns the socket and is the only thing that ever writes to it; it
//!   also does the reading, using a short read timeout so it wakes up often
//!   enough to drain its outbound queue;
//! - every other thread reaches that connection only through a peer link, which
//!   is a bounded queue plus a socket handle that can be shut down from another
//!   thread. Enqueuing never blocks, so a slow peer cannot stall the sender that
//!   is fanning frames out.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tungstenite::protocol::WebSocket;
use tungstenite::{Error as WsError, Message};

use crate::protocol::{
    DeviceInfo, Envelope, ErrorCode, Heartbeat, Hello, InputFrame, Joined, PeerInfo, PeerLeft,
    PointerFrame, ProtocolError, kind,
};

/// How long a connection thread may sit in `read` before it drains its outbound
/// queue. This is also the worst-case added latency for a frame waiting to be
/// written to a quiet connection.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A peer that cannot deliver `hello` within this window is dropped, so a stalled
/// TCP connection cannot pin a thread forever.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// A write that cannot drain for this long means the peer stopped reading. The
/// connection is dropped instead of blocking this thread forever on a full send
/// buffer.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a retirement notice may take to reach the connection it is ending.
///
/// Much shorter than `WRITE_TIMEOUT` on purpose: that connection is going away
/// either way, and its replacement must not wait behind a peer that has stopped
/// reading.
const RETIRE_WRITE_TIMEOUT: Duration = Duration::from_millis(250);

/// Outbound queue depth per connection. About two seconds of maximum-rate input:
/// a peer that fills it has stopped reading, and is disconnected rather than
/// buffered without bound.
const OUTBOX_CAPACITY: usize = 1024;

/// The largest message one connection may hand the relay.
///
/// Input frames are under a kilobyte; a peer sending megabytes is either broken
/// or hostile, and tungstenite's default 64 MiB ceiling would let either one
/// pin a buffer against this thread for as long as the frame takes to arrive.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Burst and sustained rate for discrete input (keys, buttons, clicks).
const DISCRETE_BURST: f64 = 500.0;
const DISCRETE_PER_SECOND: f64 = 500.0;

/// Burst and sustained rate for continuous input (pointer geometry). Two frames
/// of slack absorb normal scheduling jitter around the 120 Hz ceiling.
const CONTINUOUS_BURST: f64 = 2.0;
const CONTINUOUS_PER_SECOND: f64 = 120.0;

/// At most one `rate_limited` error per second per connection: a peer that keeps
/// flooding should not be answered with a flood of its own.
const RATE_NOTICE_INTERVAL: Duration = Duration::from_secs(1);

/// Where the relay listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindMode {
    /// Loopback only: this machine's own clients.
    LocalOnly,
    /// All IPv4 interfaces.
    Ipv4Only,
    /// All interfaces, accepting both families on one socket.
    DualStack,
    /// Exactly one interface, named by its address.
    Custom(IpAddr),
}

impl BindMode {
    /// The address to listen on for this mode.
    pub fn addr(&self, port: u16) -> SocketAddr {
        match self {
            Self::LocalOnly => SocketAddr::from(([127, 0, 0, 1], port)),
            Self::Ipv4Only => SocketAddr::from(([0, 0, 0, 0], port)),
            Self::DualStack => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
            Self::Custom(address) => SocketAddr::new(*address, port),
        }
    }
}

/// Everything the relay needs to start.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub bind: BindMode,
    pub port: u16,
    /// The room key. Every client presenting this token shares one room.
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("failed to bind {0}: {1}")]
    Bind(SocketAddr, std::io::Error),
    #[error("relay stopped: {0}")]
    Io(#[from] std::io::Error),
}

/// Accepts connections until the listener fails; never returns on success.
///
/// Every accepted connection is served on its own thread, so one slow or
/// malicious peer cannot stop the relay from accepting the next one.
pub fn run(config: RelayConfig) -> Result<(), RelayError> {
    let listener = bind_listener(&config)?;
    let token: Arc<str> = Arc::from(config.token.as_str());
    let hub: Arc<Mutex<Hub>> = Arc::new(Mutex::new(Hub::default()));

    for accepted in listener.incoming() {
        let Ok(stream) = accepted else {
            // A single failed `accept` (fd exhaustion, a client that vanished
            // mid-handshake) must not take the listener down.
            continue;
        };
        let token = Arc::clone(&token);
        let hub = Arc::clone(&hub);
        let _ = thread::Builder::new()
            .name("bongocat-relay-conn".to_owned())
            .spawn(move || serve_connection(stream, token, hub));
    }

    Ok(())
}

/// Binds the configured address, falling back to IPv4 when a dual-stack listen
/// is not available on this host.
fn bind_listener(config: &RelayConfig) -> Result<TcpListener, RelayError> {
    let addr = config.bind.addr(config.port);
    match TcpListener::bind(addr) {
        Ok(listener) => Ok(listener),
        Err(error) => {
            if config.bind == BindMode::DualStack
                && let Ok(listener) = TcpListener::bind(BindMode::Ipv4Only.addr(config.port))
            {
                return Ok(listener);
            }
            Err(RelayError::Bind(addr, error))
        }
    }
}

/// Serves one connection from the handshake to the last cleanup step.
fn serve_connection(stream: TcpStream, token: Arc<str>, hub: Arc<Mutex<Hub>>) {
    // Input frames are small and latency-sensitive; Nagle would batch them.
    let _ = stream.set_nodelay(true);
    // Bounded so a client that opens a socket and says nothing releases the
    // thread after `HANDSHAKE_TIMEOUT` instead of holding it forever.
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));

    let mut socket_config = tungstenite::protocol::WebSocketConfig::default();
    socket_config.max_message_size = Some(MAX_MESSAGE_BYTES);
    socket_config.max_frame_size = Some(MAX_MESSAGE_BYTES);
    let mut socket = match tungstenite::accept_with_config(stream, Some(socket_config)) {
        Ok(socket) => socket,
        Err(_) => return,
    };
    // From here on the thread must wake up regularly to drain the outbound queue.
    let _ = socket.get_ref().set_read_timeout(Some(POLL_INTERVAL));
    let _ = socket.get_ref().set_write_timeout(Some(WRITE_TIMEOUT));

    // A second handle on the same socket, kept only so other threads can ask
    // this connection to end (a device id taken over) or cut it off when it
    // cannot be reached at all.
    let Ok(shutdown) = socket.get_ref().try_clone() else {
        return;
    };

    let hello = match handshake(&mut socket, &token) {
        Handshake::Accepted(hello) => hello,
        Handshake::Rejected(error) => {
            // Answer before closing: the client can then tell "bad token" from
            // "relay unreachable" instead of retrying a lost cause.
            if let Some(frame) = encode(kind::ERROR, &error) {
                let _ = socket.send(Message::text(frame.as_ref()));
            }
            let _ = socket.close(None);
            let _ = socket.flush();
            return;
        }
        Handshake::Abandoned => return,
    };

    let (outbox, inbox) = mpsc::sync_channel::<Outbox>(OUTBOX_CAPACITY);
    let link: Arc<dyn PeerLink> = Arc::new(SocketLink {
        outbox: Mutex::new(outbox),
        shutdown,
    });

    let (conn, peers, notify) = {
        let mut hub = lock(&hub);
        let outcome = hub.join(&token, &hello.device, Arc::clone(&link));
        (outcome.conn, outcome.peers, outcome.notify)
    };

    let joined = Joined {
        peer_id: hello.device.id.clone(),
        peers,
    };
    if let Some(frame) = encode(kind::JOINED, &joined) {
        link.deliver(frame);
    }
    announce(&notify, kind::PEER_JOINED, &peer_info(&hello.device));

    let mut limiter = RateLimiter::new(Instant::now());
    loop {
        // Write first, then wait: `joined` is already queued by this point and
        // must not sit behind a read timeout.
        if !pump(&mut socket, &inbox) {
            break;
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if !handle_text(text.as_str(), &hub, &token, conn, &link, &mut limiter) {
                    break;
                }
            }
            // The peer closed the session.
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                // A read timeout is the normal case: it is how this thread wakes
                // up to write. Anything else means the socket is unusable.
                if !is_timeout(&error) {
                    break;
                }
            }
        }
    }

    // Whatever ended the session — clean close, write failure, or the retire
    // marker another thread queued — the room has to stop listing this peer.
    let departure = {
        let mut hub = lock(&hub);
        hub.leave(&token, conn)
    };
    if let Some(left) = departure.left {
        announce(&departure.notify, kind::PEER_LEFT, &PeerLeft { id: left });
    }

    // A last flush for whatever the loop queued on its way out: the error that
    // ended the session, or the notice that retired it. The socket is still
    // writable here, and this is the only chance the peer gets to learn why it
    // is going instead of seeing a bare disconnect.
    let _ = pump(&mut socket, &inbox);

    let _ = socket.close(None);
    let _ = socket.flush();
}

enum Handshake {
    Accepted(Hello),
    Rejected(ProtocolError),
    /// The peer went away, or never sent a frame we could read.
    Abandoned,
}

/// Reads the first frame and decides whether this connection may join.
fn handshake(socket: &mut WebSocket<TcpStream>, token: &str) -> Handshake {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;

    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let Ok(envelope) = serde_json::from_str::<Envelope>(&text) else {
                    return Handshake::Rejected(protocol_violation(
                        "first frame is not a protocol envelope",
                    ));
                };
                return match authorize(&envelope, token) {
                    Ok(hello) => Handshake::Accepted(hello),
                    Err(error) => Handshake::Rejected(error),
                };
            }
            Ok(Message::Close(_)) => return Handshake::Abandoned,
            // Ping/pong/binary before the handshake are noise, not a decision.
            Ok(_) => {}
            Err(error) => {
                if !is_timeout(&error) {
                    return Handshake::Abandoned;
                }
            }
        }
    }

    Handshake::Abandoned
}

/// The handshake rules, kept free of sockets so they can be tested directly.
///
/// Order matters: a version mismatch is reported before anything else, so an
/// incompatible build is told to upgrade rather than to check its token. The
/// token is checked before the device id, and the answer for a bad token never
/// describes what this relay would have accepted.
fn authorize(envelope: &Envelope, token: &str) -> Result<Hello, ProtocolError> {
    envelope.check_version()?;

    if envelope.kind != kind::HELLO {
        return Err(protocol_violation(&format!(
            "expected a `{}` frame first, got `{}`",
            kind::HELLO,
            envelope.kind
        )));
    }

    let hello = envelope
        .decode::<Hello>()
        .map_err(|_| protocol_violation("hello payload is malformed"))?;

    if hello.token != token {
        return Err(bad_token("token rejected"));
    }

    if hello.device.id.trim().is_empty() {
        return Err(bad_token("hello carried an empty device id"));
    }

    Ok(hello)
}

fn bad_token(message: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::BadToken,
        message: message.to_owned(),
    }
}

/// The peer is speaking a dialect this build cannot answer.
fn protocol_violation(message: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::Protocol,
        message: message.to_owned(),
    }
}

/// This device id was taken over by a newer connection.
fn conflict() -> ProtocolError {
    ProtocolError {
        code: ErrorCode::Conflict,
        message: "device id claimed by a newer connection".to_owned(),
    }
}

/// Handles one client frame. Returns `false` when the connection must close.
fn handle_text(
    text: &str,
    hub: &Arc<Mutex<Hub>>,
    token: &str,
    conn: u64,
    link: &Arc<dyn PeerLink>,
    limiter: &mut RateLimiter,
) -> bool {
    let Ok(envelope) = serde_json::from_str::<Envelope>(text) else {
        // A frame that is not even an envelope means the peer speaks a different
        // dialect, not that it is merely noisy. Answering once and closing is
        // bounded; ignoring garbage forever would look like silence from the far
        // end, which is the hardest failure to diagnose.
        send_error(
            link,
            &protocol_violation("frame is not a protocol envelope"),
        );
        return false;
    };

    if let Err(error) = envelope.check_version() {
        send_error(link, &error);
        return false;
    }

    match envelope.kind.as_str() {
        kind::INPUT => {
            let Ok(frame) = envelope.decode::<InputFrame>() else {
                // Undecodable after a successful handshake means the peer speaks a
                // different dialect. Staying connected would look like silence from
                // the far end, which is the hardest failure to diagnose.
                send_error(
                    link,
                    &protocol_violation("input frame did not match the agreed shape"),
                );
                return false;
            };
            match limiter.admit(&frame, Instant::now()) {
                Admission::Allow => {}
                // A dropped continuous frame is expected at high polling rates;
                // the next one supersedes it.
                Admission::Drop => return true,
                Admission::Limited => {
                    send_error(
                        link,
                        &ProtocolError {
                            code: ErrorCode::RateLimited,
                            message: "input rate limit exceeded".to_owned(),
                        },
                    );
                    return true;
                }
            }

            // Forwarded verbatim: re-encoding would cost a parse and a build per
            // frame, and would drop any field this build does not know about.
            let wire: Arc<str> = Arc::from(text);
            let targets = {
                let hub = lock(hub);
                hub.broadcast_targets(token, conn)
            };
            for target in targets {
                target.deliver(Arc::clone(&wire));
            }
        }
        kind::POINTER => {
            let Ok(frame) = envelope.decode::<PointerFrame>() else {
                send_error(
                    link,
                    &protocol_violation("pointer frame did not match the agreed shape"),
                );
                return false;
            };
            // Dropped when the sender's viewport disagrees with the receiver's
            // would be a courtesy; normalized coordinates travel instead, so the
            // frame is forwarded as-is and the receiver maps it onto its own
            // viewport at draw time.
            let _ = frame;
            let wire: Arc<str> = Arc::from(text);
            let targets = {
                let hub = lock(hub);
                hub.broadcast_targets(token, conn)
            };
            for target in targets {
                target.deliver(Arc::clone(&wire));
            }
        }
        kind::PING => {
            let Ok(beat) = envelope.decode::<Heartbeat>() else {
                send_error(
                    link,
                    &protocol_violation("heartbeat did not match the agreed shape"),
                );
                return false;
            };
            if let Some(frame) = encode(kind::PONG, &beat) {
                link.deliver(frame);
            }
        }
        // Legitimate but uninteresting: the relay never pings, so a late pong is
        // harmless and needs no answer.
        kind::PONG => {}
        _ => {
            // A second hello, a server-to-client kind, or something this version
            // does not define. Staying connected would hide a peer that thinks it
            // is talking to a different relay build.
            send_error(
                link,
                &protocol_violation("frame kind is not valid from a client"),
            );
            return false;
        }
    }

    true
}

/// Writes everything queued for this connection and flushes.
///
/// Returns `false` when the connection must end: either the socket can no longer
/// be written to, or a retirement notice was reached and has been flushed.
fn pump(socket: &mut WebSocket<TcpStream>, inbox: &Receiver<Outbox>) -> bool {
    loop {
        match inbox.try_recv() {
            Ok(Outbox::Frame(frame)) => {
                if socket.write(Message::text(frame.as_ref())).is_err() {
                    return false;
                }
            }
            // Always the last thing queued, so the frame in front of it — the
            // reason this connection is ending — has already been written.
            // Flushing here rather than returning `true` is what gets the
            // notice onto the wire before the close that follows.
            Ok(Outbox::Retire) => {
                let _ = socket.flush();
                return false;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return false,
        }
    }

    socket.flush().is_ok()
}

fn announce<T: Serialize>(targets: &[Arc<dyn PeerLink>], kind_name: &str, payload: &T) {
    let Some(frame) = encode(kind_name, payload) else {
        return;
    };
    for target in targets {
        target.deliver(Arc::clone(&frame));
    }
}

fn send_error(link: &Arc<dyn PeerLink>, error: &ProtocolError) {
    if let Some(frame) = encode(kind::ERROR, error) {
        link.deliver(frame);
    }
}

fn encode<T: Serialize>(kind_name: &str, payload: &T) -> Option<Arc<str>> {
    let envelope = Envelope::of(kind_name, payload).ok()?;
    let wire = serde_json::to_string(&envelope).ok()?;
    Some(Arc::from(wire))
}

fn peer_info(device: &DeviceInfo) -> PeerInfo {
    PeerInfo {
        id: device.id.clone(),
        name: device.name.clone(),
        publish: device.publish,
    }
}

/// Locks a mutex, recovering from poisoning: relay state is a set of connection
/// handles, so one panicking connection thread must not take the relay down.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What another thread may do to a live connection.
///
/// Every operation is non-blocking, so a broadcast never waits on a slow peer
/// and never runs while the room lock is held.
trait PeerLink: Send + Sync {
    /// Queues an already-serialized frame.
    fn deliver(&self, frame: Arc<str>);
    /// Ends the connection politely: `last_words` is written if that can be done
    /// promptly, and the connection is closed right after it.
    ///
    /// This is how a connection that is being replaced says goodbye. A hard
    /// shutdown would discard that farewell, and the peer would read it as a
    /// network failure — reconnect, reclaim its id, and kick this connection in
    /// turn. A peer that has stopped reading gets nothing instead of a hung
    /// socket: the write deadline is short, and the fallback is a hard close.
    fn retire(&self, last_words: Arc<str>);
    /// Drops the connection immediately. For peers that have stopped reading, so
    /// there is nothing left worth delivering.
    fn disconnect(&self);
}

/// What a connection thread may be asked to write.
enum Outbox {
    /// A frame to forward.
    Frame(Arc<str>),
    /// Nothing is queued behind this: flush what has been written and end the
    /// connection. Sharing one queue with the frames is what keeps a farewell
    /// from being overtaken by — or dropped for — the close that follows it.
    Retire,
}

struct SocketLink {
    outbox: Mutex<SyncSender<Outbox>>,
    shutdown: TcpStream,
}

impl PeerLink for SocketLink {
    fn deliver(&self, frame: Arc<str>) {
        let outbox = lock(&self.outbox);
        match outbox.try_send(Outbox::Frame(frame)) {
            Ok(()) => {}
            // A full queue means this peer has stopped draining: its receive
            // window is shut, so this thread would have to buffer without bound.
            // Dropping the frame instead would corrupt input state silently —
            // a lost release leaves a remote key stuck down, and the receiver
            // has no way to tell — so the connection is dropped and the peer
            // resyncs from scratch when it reconnects.
            Err(TrySendError::Full(_)) => self.disconnect(),
            // The connection thread is already gone; its own cleanup announces
            // the departure.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    fn retire(&self, last_words: Arc<str>) {
        // The connection is ending regardless, so the farewell gets a short
        // deadline: a peer that stopped reading must not hold the socket — and
        // the thread that owns it — for the whole `WRITE_TIMEOUT`. This is the
        // `TcpStream` the `WebSocket` reads and writes through, so the deadline
        // binds the thread that actually performs the write.
        let _ = self.shutdown.set_write_timeout(Some(RETIRE_WRITE_TIMEOUT));

        let outbox = lock(&self.outbox);
        let queued = outbox.try_send(Outbox::Frame(last_words)).is_ok()
            && outbox.try_send(Outbox::Retire).is_ok();
        drop(outbox);

        if !queued {
            // Either the outbox is full, which means the peer is not draining
            // and there is no way to tell it anything, or its thread is already
            // gone. Cutting the socket is all that is left.
            self.disconnect();
        }
    }

    fn disconnect(&self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
    }
}

/// Connection numbering, room membership and eviction, with no socket in sight.
///
/// Rooms are keyed by token. The relay is configured with a single token today,
/// but nothing here assumes that: a second token is a second room.
#[derive(Default)]
struct Hub {
    rooms: HashMap<String, Room>,
    next_conn: u64,
}

#[derive(Default)]
struct Room {
    peers: HashMap<u64, Peer>,
}

impl Room {
    /// Everyone the room contains, as far as anybody else is concerned.
    ///
    /// A retiring connection is on its way out: listing it would show the
    /// newcomer a second copy of its own device, telling it about the newcomer
    /// would compete with the notice that is ending it, and fanning input out to
    /// it would race the same close.
    fn active(&self) -> impl Iterator<Item = (u64, &Peer)> {
        self.peers
            .iter()
            .filter(|(_, peer)| !peer.retiring)
            .map(|(conn, peer)| (*conn, peer))
    }
}

struct Peer {
    device: DeviceInfo,
    link: Arc<dyn PeerLink>,
    /// Its device id was claimed by a newer connection. The peer stays in the
    /// room until its own thread notices and leaves — that is what lets the
    /// notice be written to it — but it is invisible to everyone else in the
    /// meantime.
    retiring: bool,
}

impl Peer {
    fn info(&self) -> PeerInfo {
        peer_info(&self.device)
    }
}

struct JoinOutcome {
    conn: u64,
    /// Peers already in the room, as shown to the newcomer.
    peers: Vec<PeerInfo>,
    /// Peers that must be told about the newcomer.
    notify: Vec<Arc<dyn PeerLink>>,
}

#[derive(Default)]
struct Departure {
    left: Option<String>,
    notify: Vec<Arc<dyn PeerLink>>,
}

impl Hub {
    /// Adds a connection to its room.
    ///
    /// A second connection claiming an existing device id replaces the old one:
    /// the same display reconnecting must not appear twice. The stale connection
    /// is retired rather than cut off, and told why — two machines sharing one
    /// device id would otherwise kick each other forever, each mistaking the
    /// other for a flaky network.
    fn join(&mut self, token: &str, device: &DeviceInfo, link: Arc<dyn PeerLink>) -> JoinOutcome {
        self.next_conn = self.next_conn.wrapping_add(1);
        let conn = self.next_conn;

        let room = self.rooms.entry(token.to_owned()).or_default();

        // Retirement is a mark, not a removal: the connection has to stay
        // reachable long enough for its own thread to write the notice and
        // close. Everything that reads the roster goes through `active`, so for
        // everyone else the room behaves as if it were already gone.
        let mut stale = Vec::new();
        for peer in room.peers.values_mut() {
            if !peer.retiring && peer.device.id == device.id {
                peer.retiring = true;
                stale.push(Arc::clone(&peer.link));
            }
        }
        // The notice is queued before the marker that ends the connection, so
        // the close cannot overtake it.
        let notice = encode(kind::ERROR, &conflict());
        for link in stale {
            match &notice {
                Some(frame) => link.retire(Arc::clone(frame)),
                // Unreachable with a payload this simple, but a connection left
                // marked retiring without being told would never close.
                None => link.disconnect(),
            }
        }

        let peers = room.active().map(|(_, peer)| peer.info()).collect();
        let notify = room
            .active()
            .map(|(_, peer)| Arc::clone(&peer.link))
            .collect();
        room.peers.insert(
            conn,
            Peer {
                device: device.clone(),
                link,
                retiring: false,
            },
        );

        JoinOutcome {
            conn,
            peers,
            notify,
        }
    }

    /// Removes a connection. Idempotent: a connection that already left, or
    /// whose room is gone, produces nothing to announce.
    fn leave(&mut self, token: &str, conn: u64) -> Departure {
        let Some(room) = self.rooms.get_mut(token) else {
            return Departure::default();
        };
        let Some(peer) = room.peers.remove(&conn) else {
            return Departure::default();
        };

        let notify = room
            .active()
            .map(|(_, peer)| Arc::clone(&peer.link))
            .collect();

        // A retired connection leaving is not the device leaving: a newer
        // connection holds that device id by now, so announcing `peer_left`
        // would tell the room that a display which is still here has gone.
        let still_live = room
            .active()
            .any(|(_, other)| other.device.id == peer.device.id);
        let left = (!still_live).then_some(peer.device.id);

        // An empty room is indistinguishable from one that never existed, so it
        // is dropped: tokens are disposable and rooms must not accumulate.
        let empty = room.peers.is_empty();
        if empty {
            self.rooms.remove(token);
        }

        Departure { left, notify }
    }

    /// Everyone in the room except `except`, collected under the lock so the
    /// caller can fan out after releasing it.
    fn broadcast_targets(&self, token: &str, except: u64) -> Vec<Arc<dyn PeerLink>> {
        self.rooms
            .get(token)
            .map(|room| {
                room.active()
                    .filter(|(conn, _)| *conn != except)
                    .map(|(_, peer)| Arc::clone(&peer.link))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameClass {
    /// Keys, buttons, clicks: every frame is a distinct state change and losing
    /// one loses information.
    Discrete,
    /// Pointer geometry: frames describe a position, so a newer one always makes
    /// an older one worthless.
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    Allow,
    /// Over the ceiling for a stream that supersedes itself: drop silently.
    Drop,
    /// Over the ceiling for discrete events: the peer is doing something wrong.
    Limited,
}

/// Which bucket an input frame belongs to.
///
/// The wire format distinguishes the two by geometry: a frame that carries the
/// sender's display bounds is positioning the pointer, while a frame without them
/// is a state change on a named control.
fn classify(frame: &InputFrame) -> FrameClass {
    if frame.bounds.is_some() {
        FrameClass::Continuous
    } else {
        FrameClass::Discrete
    }
}

#[derive(Debug, Clone, Copy)]
struct TokenBucket {
    capacity: f64,
    per_second: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    fn full(capacity: f64, per_second: f64, now: Instant) -> Self {
        Self {
            capacity,
            per_second,
            tokens: capacity,
            last: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Per-connection admission control.
///
/// Two buckets, because the two traffic shapes need opposite treatment: a burst
/// of real key presses is legitimate up to a high ceiling, while pointer samples
/// are throttled to a display-friendly rate and simply thinned out.
struct RateLimiter {
    discrete: TokenBucket,
    continuous: TokenBucket,
    last_notice: Option<Instant>,
}

impl RateLimiter {
    fn new(now: Instant) -> Self {
        Self {
            discrete: TokenBucket::full(DISCRETE_BURST, DISCRETE_PER_SECOND, now),
            continuous: TokenBucket::full(CONTINUOUS_BURST, CONTINUOUS_PER_SECOND, now),
            last_notice: None,
        }
    }

    fn admit(&mut self, frame: &InputFrame, now: Instant) -> Admission {
        match classify(frame) {
            FrameClass::Continuous => {
                if self.continuous.take(now) {
                    Admission::Allow
                } else {
                    Admission::Drop
                }
            }
            FrameClass::Discrete => {
                if self.discrete.take(now) {
                    return Admission::Allow;
                }
                // One notice per second: enough for the client to know it is the
                // problem, not enough to become the problem.
                let notify = self
                    .last_notice
                    .is_none_or(|last| now.saturating_duration_since(last) >= RATE_NOTICE_INTERVAL);
                if notify {
                    self.last_notice = Some(now);
                    Admission::Limited
                } else {
                    Admission::Drop
                }
            }
        }
    }
}

fn is_timeout(error: &WsError) -> bool {
    matches!(
        error,
        WsError::Io(io)
            if matches!(
                io.kind(),
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::protocol::{Bounds, PROTOCOL_VERSION};

    #[derive(Default)]
    struct TestLink {
        frames: Mutex<Vec<Arc<str>>>,
        retired: AtomicBool,
        closed: AtomicBool,
    }

    impl TestLink {
        fn frames(&self) -> Vec<Arc<str>> {
            lock(&self.frames).clone()
        }

        fn retired(&self) -> bool {
            self.retired.load(Ordering::SeqCst)
        }

        fn closed(&self) -> bool {
            self.closed.load(Ordering::SeqCst)
        }
    }

    impl PeerLink for TestLink {
        fn deliver(&self, frame: Arc<str>) {
            lock(&self.frames).push(frame);
        }

        fn retire(&self, last_words: Arc<str>) {
            // Kept with the delivered frames: a real connection writes this one
            // through the same path, right before it closes.
            lock(&self.frames).push(last_words);
            self.retired.store(true, Ordering::SeqCst);
        }

        fn disconnect(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    fn device(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            name: format!("{id} display"),
            publish: true,
        }
    }

    fn hello(token: &str, id: &str) -> Envelope {
        Envelope::of(
            kind::HELLO,
            &Hello {
                token: token.to_owned(),
                device: device(id),
            },
        )
        .expect("encode hello")
    }

    fn input_frame(bounds: Option<Bounds>) -> InputFrame {
        // Built through JSON so the test also pins the control's wire shape.
        let control: bongocat_input::InputControl =
            serde_json::from_value(serde_json::json!({ "Key": 30 })).expect("control");
        InputFrame {
            src: "a".to_owned(),
            seq: 1,
            t: 0,
            control,
            edge: bongocat_input::InputEdge::Down,
            bounds,
        }
    }

    #[test]
    fn input_reaches_the_room_but_never_the_sender() {
        let mut hub = Hub::default();
        let a = Arc::new(TestLink::default());
        let b = Arc::new(TestLink::default());

        // `a.clone()` returns `Arc<TestLink>` and is then unsized to
        // `Arc<dyn PeerLink>`; `Arc::clone(&a)` would instead demand a
        // `&Arc<dyn PeerLink>` argument, which a `&Arc<TestLink>` is not.
        let a_link: Arc<dyn PeerLink> = a.clone();
        let b_link: Arc<dyn PeerLink> = b.clone();

        let a_join = hub.join("room", &device("a"), a_link);
        assert!(a_join.peers.is_empty(), "the room was empty before A");
        assert!(a_join.notify.is_empty());

        let b_join = hub.join("room", &device("b"), b_link);
        assert_eq!(b_join.peers.len(), 1, "B is told that A is already here");
        assert_eq!(b_join.peers[0].id, "a");
        assert_eq!(b_join.notify.len(), 1, "A is told that B arrived");

        let wire = Arc::<str>::from(r#"{"v":1,"type":"input","payload":{}}"#);
        let targets = hub.broadcast_targets("room", a_join.conn);
        assert_eq!(targets.len(), 1, "only B is a target");
        for target in &targets {
            target.deliver(Arc::clone(&wire));
        }

        assert!(
            a.frames().is_empty(),
            "a frame is never echoed to its sender"
        );
        assert_eq!(b.frames().len(), 1);
        assert_eq!(b.frames()[0].as_ref(), wire.as_ref());
    }

    #[test]
    fn handshake_rejects_bad_versions_tokens_and_device_ids() {
        let accepted = authorize(&hello("s3cret", "a"), "s3cret").expect("matching token");
        assert_eq!(accepted.device.id, "a");

        let wrong_token = authorize(&hello("guess", "a"), "s3cret").expect_err("wrong token");
        assert_eq!(wrong_token.code, ErrorCode::BadToken);

        let mut stale = hello("s3cret", "a");
        stale.v = PROTOCOL_VERSION + 1;
        assert_eq!(
            authorize(&stale, "s3cret").expect_err("old build").code,
            ErrorCode::BadVersion,
            "a version mismatch is reported before the token is even looked at"
        );

        assert_eq!(
            authorize(&hello("s3cret", "  "), "s3cret")
                .expect_err("blank device id")
                .code,
            ErrorCode::BadToken
        );

        // A frame of the wrong kind is a protocol violation, not a credential
        // problem: the peer is speaking something this relay cannot answer.
        let not_hello = Envelope::of(kind::PING, &Heartbeat { t: 1 }).expect("encode ping");
        assert_eq!(
            authorize(&not_hello, "s3cret").expect_err("not hello").code,
            ErrorCode::Protocol
        );
    }

    #[test]
    fn reconnecting_with_the_same_device_id_evicts_the_old_connection() {
        let mut hub = Hub::default();
        let first = Arc::new(TestLink::default());
        let second = Arc::new(TestLink::default());

        let first_link: Arc<dyn PeerLink> = first.clone();
        let second_link: Arc<dyn PeerLink> = second.clone();

        let old = hub.join("room", &device("a"), first_link);
        let new = hub.join("room", &device("a"), second_link);

        assert!(
            first.retired(),
            "the stale connection is retired, not cut off"
        );
        assert!(
            !first.closed(),
            "a soft retire leaves the socket writable: the explanation is written \
             by the connection's own thread, not by the evicting one"
        );
        assert!(!second.retired(), "the newcomer stays connected");

        let notice = first.frames();
        assert_eq!(notice.len(), 1, "the evicted peer gets exactly one reason");
        assert!(
            notice[0].contains("\"type\":\"error\""),
            "{}",
            notice[0].as_ref()
        );
        assert!(
            notice[0].contains("\"code\":\"conflict\""),
            "two machines sharing an id must learn why, instead of retrying: {}",
            notice[0].as_ref()
        );

        assert!(
            new.peers.is_empty(),
            "the room holds one peer: the newcomer"
        );
        assert!(
            hub.broadcast_targets("room", new.conn).is_empty(),
            "no input is fanned out to the evicted connection"
        );
        assert!(
            hub.leave("room", old.conn).left.is_none(),
            "a retired connection going is not a departure: its device id is \
             still in the room, so announcing one would evict the newcomer"
        );
    }

    #[test]
    fn a_retiring_connection_is_invisible_to_the_rest_of_the_room() {
        let mut hub = Hub::default();
        let bystander = Arc::new(TestLink::default());
        let first = Arc::new(TestLink::default());
        let second = Arc::new(TestLink::default());
        let bystander_link: Arc<dyn PeerLink> = bystander.clone();
        let first_link: Arc<dyn PeerLink> = first.clone();
        let second_link: Arc<dyn PeerLink> = second.clone();

        hub.join("room", &device("b"), bystander_link);
        let old = hub.join("room", &device("a"), first_link);
        let new = hub.join("room", &device("a"), second_link);

        assert_eq!(
            new.peers
                .iter()
                .map(|peer| peer.id.as_str())
                .collect::<Vec<_>>(),
            ["b"],
            "the newcomer is not shown the connection it just retired"
        );
        assert_eq!(
            new.notify.len(),
            1,
            "only the bystander is told about the newcomer; the retiring \
             connection is busy being told something else"
        );
        assert_eq!(
            hub.broadcast_targets("room", new.conn).len(),
            1,
            "input goes to the live peer, never to a retiring connection"
        );
        assert_eq!(
            first.frames().len(),
            1,
            "the retiring connection hears exactly one thing: why it is going"
        );

        // Its own disconnect path still runs — but the device is still here, so
        // there is no departure to announce.
        let departure = hub.leave("room", old.conn);
        assert!(departure.left.is_none());
        assert_eq!(departure.notify.len(), 2, "the two live connections remain");
        assert_eq!(
            hub.broadcast_targets("room", new.conn).len(),
            1,
            "the room is unchanged by a retired connection leaving"
        );
    }

    #[test]
    fn leaving_announces_the_departure_and_forgets_an_empty_room() {
        let mut hub = Hub::default();
        let a = Arc::new(TestLink::default());
        let b = Arc::new(TestLink::default());
        let a_link: Arc<dyn PeerLink> = a.clone();
        let b_link: Arc<dyn PeerLink> = b.clone();
        let a_join = hub.join("room", &device("a"), a_link);
        let b_join = hub.join("room", &device("b"), b_link);

        let departure = hub.leave("room", a_join.conn);
        assert_eq!(departure.left.as_deref(), Some("a"));
        assert_eq!(departure.notify.len(), 1, "B is told that A left");

        assert!(hub.leave("room", a_join.conn).left.is_none(), "idempotent");
        assert!(
            hub.leave("room", 9_999).left.is_none(),
            "unknown connection"
        );

        assert!(hub.leave("room", b_join.conn).left.is_some());
        assert!(hub.rooms.is_empty(), "an empty room is dropped");
    }

    #[test]
    fn discrete_bursts_are_capped_and_pointer_frames_are_thinned() {
        let start = Instant::now();
        let key = input_frame(None);
        let pointer = input_frame(Some(Bounds {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }));

        let mut limiter = RateLimiter::new(start);
        for _ in 0..500 {
            assert_eq!(limiter.admit(&key, start), Admission::Allow);
        }
        assert_eq!(
            limiter.admit(&key, start),
            Admission::Limited,
            "the 501st key event in the same instant is over budget"
        );
        assert_eq!(
            limiter.admit(&key, start),
            Admission::Drop,
            "further violations are silent within the notice interval"
        );
        assert_eq!(
            limiter.admit(&key, start + Duration::from_secs(1)),
            Admission::Allow,
            "a second later the budget is back"
        );

        let mut limiter = RateLimiter::new(start);
        assert_eq!(limiter.admit(&pointer, start), Admission::Allow);
        assert_eq!(
            limiter.admit(&pointer, start),
            Admission::Allow,
            "burst slack"
        );
        assert_eq!(
            limiter.admit(&pointer, start),
            Admission::Drop,
            "pointer frames are thinned, never reported"
        );
        assert_eq!(
            limiter.admit(&pointer, start + Duration::from_millis(9)),
            Admission::Allow
        );
        assert_eq!(
            limiter.admit(&pointer, start + Duration::from_millis(9)),
            Admission::Drop,
            "about one frame per 8.3ms is the ceiling"
        );
    }

    #[test]
    fn an_input_frame_is_forwarded_verbatim_to_the_rest_of_the_room() {
        let hub = Arc::new(Mutex::new(Hub::default()));
        let a_probe = Arc::new(TestLink::default());
        let b_probe = Arc::new(TestLink::default());
        let a: Arc<dyn PeerLink> = a_probe.clone();
        let b: Arc<dyn PeerLink> = b_probe.clone();

        let (a_conn, b_conn) = {
            let mut hub = lock(&hub);
            let a_join = hub.join("room", &device("a"), a.clone());
            let b_join = hub.join("room", &device("b"), b.clone());
            (a_join.conn, b_join.conn)
        };
        assert_ne!(a_conn, b_conn);

        let wire = serde_json::to_string(
            &Envelope::of(kind::INPUT, &input_frame(None)).expect("encode input"),
        )
        .expect("serialize");
        let mut limiter = RateLimiter::new(Instant::now());

        assert!(handle_text(&wire, &hub, "room", a_conn, &a, &mut limiter));

        let delivered = b_probe.frames();
        assert_eq!(delivered.len(), 1, "the other peer receives it");
        assert_eq!(
            delivered[0].as_ref(),
            wire.as_str(),
            "the frame is relayed byte for byte, not rebuilt"
        );
        assert!(
            a_probe.frames().is_empty(),
            "the sender never sees its own frame"
        );
    }

    #[test]
    fn a_frame_the_relay_cannot_interpret_ends_the_connection() {
        let hub = Arc::new(Mutex::new(Hub::default()));
        let probe = Arc::new(TestLink::default());
        let link: Arc<dyn PeerLink> = probe.clone();
        let mut limiter = RateLimiter::new(Instant::now());

        assert!(
            !handle_text("this is not json", &hub, "room", 1, &link, &mut limiter),
            "an unparseable frame must close the connection instead of being \
             ignored forever"
        );

        let frames = probe.frames();
        assert_eq!(frames.len(), 1);
        assert!(
            frames[0].contains("\"code\":\"protocol\""),
            "{}",
            frames[0].as_ref()
        );

        // Same rule for a well-formed envelope carrying a payload of the wrong
        // shape: the peer is speaking a dialect this build does not understand.
        let probe = Arc::new(TestLink::default());
        let link: Arc<dyn PeerLink> = probe.clone();
        let wrong_shape =
            serde_json::to_string(&Envelope::of(kind::INPUT, &Heartbeat { t: 7 }).expect("encode"))
                .expect("serialize");
        assert!(!handle_text(
            &wrong_shape,
            &hub,
            "room",
            1,
            &link,
            &mut limiter
        ));
        let frames = probe.frames();
        assert!(
            frames[0].contains("\"code\":\"protocol\""),
            "{}",
            frames[0].as_ref()
        );
    }
}
