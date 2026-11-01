//! The client half: one connection to a relay, reconnecting on failure.
//!
//! Consumers should treat this as an event stream. It owns its own thread, so
//! nothing here blocks the UI or the input path.
//!
//! # Threads and channels
//!
//! [`spawn`] starts exactly one thread and returns the two ends of the
//! connection: a bounded event queue the owner drains, and an unbounded command
//! channel the input path pushes into. Neither end can stall the other.
//!
//! * Events: the thread only ever offers with `try_send`, so a reader that stops
//!   consuming loses samples instead of applying backpressure to the socket.
//!   The one exception is a state event (`Joined`, `PeerJoined`, `PeerLeft`,
//!   `Reconnecting`, `Failed`): losing one of those would leave the owner with a
//!   stale picture of the room, so it is retried for at most 20 × 1 ms before it
//!   is dropped too. The thread never waits on the owner for longer than that.
//! * Commands: the thread drains the channel on every turn of its own loop and
//!   keeps at most `COMMAND_QUEUE_CAPACITY` of them in a local queue. When that
//!   queue is full it drops a pointer sample first, because a late cursor
//!   position is worthless while a late key press still records that the key
//!   moved. Anything still queued while the link is down is drained and dropped
//!   on every retry tick, so a reconnect cannot build a backlog either.
//!
//! # Timing
//!
//! The socket reads with a `SOCKET_POLL_INTERVAL` timeout. That timeout is also
//! the worst-case latency between a command reaching the channel and reaching
//! the wire: `read` gives up and the loop services commands in between. The same
//! loop sends a heartbeat every `HEARTBEAT_INTERVAL` and declares the link dead
//! when nothing at all has arrived for `LIVENESS_TIMEOUT` — which only works
//! because the read is bounded.
//!
//! # Reconnection
//!
//! A dead link is retried `MAX_RECONNECT_ATTEMPTS` times with a doubling delay
//! capped at `RECONNECT_MAX_DELAY`; a rejected token or protocol version is
//! never retried, because the same request would be rejected again.

use std::{
    collections::VecDeque,
    io::ErrorKind as IoErrorKind,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, de::DeserializeOwned};
use tungstenite::{Error as WsError, Message, WebSocket, stream::MaybeTlsStream};
use url::Url;

use crate::protocol::{
    DeviceInfo, Envelope, ErrorCode, Heartbeat, Hello, InputFrame, Joined, PeerInfo, PeerLeft,
    ProtocolError, kind,
};

/// What the connection reports upward.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Handshake accepted; the room currently holds these peers.
    Joined {
        peer_id: String,
        peers: Vec<PeerInfo>,
    },
    /// An input sample from another device.
    Input(InputFrame),
    PeerJoined(PeerInfo),
    PeerLeft(String),
    /// A heartbeat was answered: the round-trip to the relay in milliseconds.
    Latency(u64),
    /// The link dropped and is being retried. `attempt` counts from 1.
    Reconnecting {
        attempt: u32,
        reason: String,
    },
    /// Gave up after the configured number of attempts.
    Failed(String),
}

/// What the connection accepts from its owner.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    /// Send one input sample. Dropped silently if the send queue is full:
    /// a late keystroke is worthless, and blocking here would stall input.
    Publish(InputFrame),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Already resolved: `follow_local` resolves to the local relay's address
    /// before it reaches this type.
    pub url: String,
    pub token: String,
    pub device: DeviceInfo,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("invalid relay url {url}: {source}")]
    Url {
        url: String,
        source: url::ParseError,
    },
    /// The url parsed but does not address a websocket.
    #[error("unsupported relay url scheme `{scheme}`: expected ws (this build has no TLS)")]
    Scheme { scheme: String },
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("socket error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not start the client thread: {0}")]
    Thread(String),
}

/// How many events may wait for the owner before samples start being dropped.
const EVENT_CAPACITY: usize = 256;

/// How many commands may wait locally before the drop policy kicks in.
const COMMAND_QUEUE_CAPACITY: usize = 64;

/// How long a state event may be retried against a full event queue.
const CRITICAL_RETRY_ATTEMPTS: usize = 20;
const CRITICAL_RETRY_DELAY: Duration = Duration::from_millis(1);

/// Upper bound on the frames written per loop turn, so a backlog cannot starve
/// the read side of the loop.
const MAX_PUBLISH_PER_TURN: usize = 32;

/// Read timeout, and therefore the command-to-wire latency ceiling.
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(30);

/// A session that lived this long counts as healthy, so the next drop starts the
/// backoff ladder over instead of continuing to count towards `Failed`.
const STABLE_SESSION: Duration = LIVENESS_TIMEOUT;

const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

/// Granularity of the interruptible wait between reconnects.
const RETRY_SLICE: Duration = Duration::from_millis(25);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Starts the connection thread.
///
/// The url is validated here rather than on the thread so a malformed setting
/// surfaces as an error the caller can report, not as a stream of failed
/// reconnects. Everything else — connecting, handshaking, reconnecting — happens
/// on the thread.
pub fn spawn(
    config: ClientConfig,
) -> Result<(Receiver<ClientEvent>, Sender<ClientCommand>), ClientError> {
    let url = validate_server_url(&config.url)?;
    let (event_tx, event_rx) = mpsc::sync_channel(EVENT_CAPACITY);
    let (command_tx, command_rx) = mpsc::channel();
    thread::Builder::new()
        .name("bongocat-net-client".to_owned())
        .spawn(move || {
            run(
                config,
                url,
                Events::new(event_tx),
                CommandInbox::new(command_rx),
            );
        })
        .map_err(|error| ClientError::Thread(error.to_string()))?;
    Ok((event_rx, command_tx))
}

/// The longest wait before retry number `attempt` (1-based).
///
/// Doubles from one second and stops doubling at `RECONNECT_MAX_DELAY`, so the
/// ladder is 1s, 2s, 4s, 8s, 16s, 30s, 30s… Retrying faster only adds load to a
/// relay that is already failing; retrying slower makes a five-attempt budget
/// unusable.
fn backoff_for_attempt(attempt: u32) -> Duration {
    if attempt == 0 {
        return Duration::ZERO;
    }
    // The shift is clamped before the multiply so a large attempt cannot overflow.
    let doublings = attempt.saturating_sub(1).min(5);
    let delay = RECONNECT_BASE_DELAY
        .checked_mul(1_u32 << doublings)
        .unwrap_or(RECONNECT_MAX_DELAY);
    delay.min(RECONNECT_MAX_DELAY)
}

/// Accepts `ws` urls only.
///
/// A plain http(s) url is a configuration mistake, and connecting to it would
/// fail inside the websocket handshake with a much less useful message.
fn validate_server_url(raw: &str) -> Result<Url, ClientError> {
    let url = Url::parse(raw).map_err(|source| ClientError::Url {
        url: raw.to_owned(),
        source,
    })?;
    match url.scheme() {
        "ws" => Ok(url),
        // The parser accepts wss, but this build runs tungstenite without a
        // TLS feature, so the dial would fail inside the handshake with a far
        // less useful message than this one. Refuse it where the user typed it.
        scheme => Err(ClientError::Scheme {
            scheme: scheme.to_owned(),
        }),
    }
}

/// Why a session ended.
enum Outcome {
    /// The owner asked the connection to stop.
    Shutdown,
    /// The link failed and may be retried.
    Lost { reason: String },
    /// The relay refused this build or this token; retrying cannot help.
    Fatal { reason: String },
}

/// Runs connections until one ends the thread.
fn run(config: ClientConfig, url: Url, mut events: Events, mut inbox: CommandInbox) {
    let mut retries: u32 = 0;
    loop {
        let started = Instant::now();
        let (outcome, joined) = match Session::open(&url) {
            Ok(mut session) => match session.handshake(&config, &mut inbox, &mut events) {
                Ok(()) => (session.serve(&mut inbox, &mut events), true),
                Err(outcome) => (outcome, false),
            },
            Err(error) => (
                Outcome::Lost {
                    reason: error.to_string(),
                },
                false,
            ),
        };

        let reason = match outcome {
            Outcome::Shutdown => return,
            Outcome::Fatal { reason } => {
                events.critical(ClientEvent::Failed(reason));
                return;
            }
            Outcome::Lost { reason } => reason,
        };

        // A session that worked for a while earns a fresh ladder; a connection
        // that joins and immediately dies keeps counting, so a relay that keeps
        // closing us cannot keep the client in a permanent 1s retry loop.
        if joined && started.elapsed() >= STABLE_SESSION {
            retries = 0;
        }
        retries += 1;
        if retries > MAX_RECONNECT_ATTEMPTS {
            events.critical(ClientEvent::Failed(format!(
                "{reason} (gave up after {MAX_RECONNECT_ATTEMPTS} attempts)"
            )));
            return;
        }
        events.critical(ClientEvent::Reconnecting {
            attempt: retries,
            reason,
        });
        if !inbox.wait(backoff_for_attempt(retries)) {
            return;
        }
    }
}

/// One live socket, from handshake to failure.
struct Session {
    socket: Socket,
    /// When any frame last arrived, used for the liveness deadline.
    last_received: Instant,
    /// When this end last sent a heartbeat.
    last_ping: Instant,
}

impl Session {
    /// Connects and performs the websocket handshake, without the protocol
    /// handshake that needs the owner's token.
    fn open(url: &Url) -> Result<Self, ClientError> {
        let socket = open_socket(url)?;
        let now = Instant::now();
        Ok(Self {
            socket,
            last_received: now,
            last_ping: now,
        })
    }

    /// Sends `hello` and waits for `joined`.
    fn handshake(
        &mut self,
        config: &ClientConfig,
        inbox: &mut CommandInbox,
        events: &mut Events,
    ) -> Result<(), Outcome> {
        let hello = Hello {
            token: config.token.clone(),
            device: config.device.clone(),
        };
        self.send(kind::HELLO, &hello)
            .map_err(SendProblem::into_outcome)?;
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        loop {
            inbox.drain();
            if inbox.is_shutdown() {
                return Err(Outcome::Shutdown);
            }
            match self.next_incoming()? {
                Some(Incoming::Joined(joined)) => {
                    events.critical(ClientEvent::Joined {
                        peer_id: joined.peer_id,
                        peers: joined.peers,
                    });
                    let now = Instant::now();
                    self.last_received = now;
                    self.last_ping = now;
                    return Ok(());
                }
                Some(Incoming::ServerError(error)) => apply(reaction_to_refusal(error))?,
                // Nothing else may arrive before `joined`; peers cannot exist
                // yet and a heartbeat before the handshake is meaningless.
                Some(_) | None => {}
            }
            if Instant::now() >= deadline {
                return Err(Outcome::Lost {
                    reason: "the relay did not answer the handshake within 5 s".to_owned(),
                });
            }
        }
    }

    /// Pumps commands into the socket and frames back to the owner until either
    /// side ends the session.
    fn serve(&mut self, inbox: &mut CommandInbox, events: &mut Events) -> Outcome {
        loop {
            inbox.drain();
            if inbox.is_shutdown() {
                return Outcome::Shutdown;
            }

            for _ in 0..MAX_PUBLISH_PER_TURN {
                match inbox.pop() {
                    Some(ClientCommand::Publish(frame)) => match self.send(kind::INPUT, &frame) {
                        Ok(()) => {}
                        // The socket is backpressured. Keep the rest queued and
                        // go back to reading: the relay is more likely to
                        // recover if we drain what it is sending us.
                        Err(SendProblem::WouldBlock) => break,
                        Err(problem) => return problem.into_outcome(),
                    },
                    Some(ClientCommand::Shutdown) => return Outcome::Shutdown,
                    None => break,
                }
            }

            if self.last_ping.elapsed() >= HEARTBEAT_INTERVAL {
                self.last_ping = Instant::now();
                match self.send(kind::PING, &Heartbeat { t: now_millis() }) {
                    Ok(()) | Err(SendProblem::WouldBlock) => {}
                    Err(problem) => return problem.into_outcome(),
                }
            }

            match self.next_incoming() {
                Ok(Some(incoming)) => {
                    if let Err(outcome) = self.handle(incoming, events) {
                        return outcome;
                    }
                }
                Ok(None) => {}
                Err(outcome) => return outcome,
            }

            if self.last_received.elapsed() >= LIVENESS_TIMEOUT {
                return Outcome::Lost {
                    reason: "the relay went silent for 30 s".to_owned(),
                };
            }
        }
    }

    /// Turns one decoded message into an event or a reply.
    fn handle(&mut self, incoming: Incoming, events: &mut Events) -> Result<(), Outcome> {
        match incoming {
            Incoming::Joined(joined) => events.critical(ClientEvent::Joined {
                peer_id: joined.peer_id,
                peers: joined.peers,
            }),
            Incoming::PeerJoined(peer) => events.critical(ClientEvent::PeerJoined(peer)),
            Incoming::PeerLeft(left) => events.critical(ClientEvent::PeerLeft(left.id)),
            Incoming::Input(frame) => events.emit(ClientEvent::Input(frame)),
            Incoming::Ping(beat) => {
                // A missed pong is not fatal: the liveness deadline decides,
                // and it is driven by anything the relay sends.
                let _ = self.send(kind::PONG, &beat);
            }
            Incoming::Pong(beat) => {
                // The relay echoes the ping's own timestamp, so the elapsed
                // time is the round-trip. The clock is monotonic on both ends
                // of this arithmetic, which is all a subtraction needs.
                let rtt = now_millis().saturating_sub(beat.t);
                events.emit(ClientEvent::Latency(rtt));
            }
            Incoming::ServerError(error) => return apply(reaction_to_refusal(error)),
        }
        Ok(())
    }

    /// Reads at most one protocol message.
    ///
    /// `Ok(None)` covers both "nothing arrived within the poll window" and
    /// "a frame arrived that carries no message" — the caller has no reason to
    /// tell those apart.
    fn next_incoming(&mut self) -> Result<Option<Incoming>, Outcome> {
        match self.socket.read() {
            Ok(message) => {
                self.last_received = Instant::now();
                match message {
                    Message::Text(text) => decode(text.as_str()).map_err(outcome_for_decode),
                    Message::Close(_) => Err(Outcome::Lost {
                        reason: "the relay closed the connection".to_owned(),
                    }),
                    // Binary frames, and the websocket-level ping/pong pair, do
                    // not carry protocol messages. Reading a ping queues the
                    // matching pong, so it still has to be flushed out.
                    _ => {
                        self.flush();
                        Ok(None)
                    }
                }
            }
            // The read timeout elapsed mid-frame; tungstenite keeps the partial
            // frame, so this is "no data yet", not a broken connection.
            Err(WsError::Io(error))
                if matches!(
                    error.kind(),
                    IoErrorKind::WouldBlock | IoErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => Err(Outcome::Lost {
                reason: "the websocket closed".to_owned(),
            }),
            Err(error) => Err(Outcome::Lost {
                reason: error.to_string(),
            }),
        }
    }

    /// Writes one envelope, distinguishing "not now" from "not ever".
    fn send<T: Serialize>(&mut self, kind: &str, payload: &T) -> Result<(), SendProblem> {
        let envelope =
            Envelope::of(kind, payload).map_err(|error| SendProblem::Dead(error.to_string()))?;
        let text = serde_json::to_string(&envelope)
            .map_err(|error| SendProblem::Dead(error.to_string()))?;
        match self.socket.send(Message::Text(text.into())) {
            Ok(()) => Ok(()),
            Err(error) if is_timeout(&error) => Err(SendProblem::WouldBlock),
            Err(error) => Err(SendProblem::Dead(error.to_string())),
        }
    }

    /// Pushes out anything tungstenite queued (a pong, a close reply).
    ///
    /// A failed flush is not handled here: the next read or write reports the
    /// same dead socket, and only the loop can decide what to do about it.
    fn flush(&mut self) {
        let _ = self.socket.flush();
    }
}

/// The two ways a write can fail.
enum SendProblem {
    /// The socket buffer is full or the write deadline elapsed.
    WouldBlock,
    /// The socket is gone.
    Dead(String),
}

impl SendProblem {
    /// A write that cannot complete during a handshake is a dead link: there is
    /// nothing else the connection could be doing with it.
    fn into_outcome(self) -> Outcome {
        Outcome::Lost {
            reason: match self {
                Self::WouldBlock => "the socket write timed out".to_owned(),
                Self::Dead(reason) => reason,
            },
        }
    }
}

/// One message the owner cares about, or one this side has to answer.
enum Incoming {
    Joined(Joined),
    PeerJoined(PeerInfo),
    PeerLeft(PeerLeft),
    Input(InputFrame),
    Ping(Heartbeat),
    Pong(Heartbeat),
    ServerError(ProtocolError),
}

/// Why a frame could not be read as a protocol message.
enum DecodeError {
    /// The relay speaks a different protocol version.
    Version(ProtocolError),
    /// The frame is malformed, or a payload does not match its kind.
    Malformed(String),
}

/// Decodes one text frame.
///
/// Unknown kinds are ignored rather than rejected: a relay built for a later
/// minor version may send a message this build has never heard of, and dropping
/// the connection over it would make every such addition a breaking change.
fn decode(text: &str) -> Result<Option<Incoming>, DecodeError> {
    let envelope: Envelope =
        serde_json::from_str(text).map_err(|error| DecodeError::Malformed(error.to_string()))?;
    envelope.check_version().map_err(DecodeError::Version)?;
    let incoming = match envelope.kind.as_str() {
        kind::JOINED => Incoming::Joined(payload(&envelope)?),
        kind::PEER_JOINED => Incoming::PeerJoined(payload(&envelope)?),
        kind::PEER_LEFT => Incoming::PeerLeft(payload(&envelope)?),
        kind::INPUT => Incoming::Input(payload(&envelope)?),
        kind::PING => Incoming::Ping(payload(&envelope)?),
        kind::PONG => Incoming::Pong(payload(&envelope)?),
        kind::ERROR => Incoming::ServerError(payload(&envelope)?),
        // An unknown kind is ignored for the reason above.
        _ => return Ok(None),
    };
    Ok(Some(incoming))
}

fn payload<T: DeserializeOwned>(envelope: &Envelope) -> Result<T, DecodeError> {
    envelope
        .decode::<T>()
        .map_err(|error| DecodeError::Malformed(error.to_string()))
}

fn outcome_for_decode(error: DecodeError) -> Outcome {
    match error {
        DecodeError::Version(error) => Outcome::Fatal {
            reason: describe(&error),
        },
        DecodeError::Malformed(reason) => Outcome::Lost {
            reason: format!("the relay sent a frame this build cannot read: {reason}"),
        },
    }
}

/// The error code as it appears on the wire, so the message the owner sees
/// matches the one in the relay's log.
fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::BadVersion => "bad_version",
        ErrorCode::BadToken => "bad_token",
        ErrorCode::RateLimited => "rate_limited",
        ErrorCode::Protocol => "protocol",
        ErrorCode::Conflict => "conflict",
        ErrorCode::Internal => "internal",
    }
}

fn describe(error: &ProtocolError) -> String {
    format!("{}: {}", error_code_name(error.code), error.message)
}

/// What a refusal from the relay asks the connection to do.
#[derive(Debug)]
enum Reaction {
    /// The refusal concerns a frame the relay already dropped for us. The
    /// session itself is still healthy and must not be torn down.
    Continue,
    /// The link is unusable right now but probably will not be for long.
    Retry(String),
    /// The relay will answer the same request the same way.
    Stop(String),
}

/// Maps a refusal from the relay onto what the connection should do about it.
fn reaction_to_refusal(error: ProtocolError) -> Reaction {
    match error.code {
        // The same hello would be refused the same way: the token is wrong, the two
        // builds do not agree on shapes, or this device id is already taken by a live
        // connection. None of them improves on a retry.
        ErrorCode::BadVersion | ErrorCode::BadToken | ErrorCode::Protocol | ErrorCode::Conflict => {
            Reaction::Stop(describe(&error))
        }
        // The relay answers a burst with this and keeps the connection, having
        // dropped the frames it refused. Reconnecting would throw away the whole
        // room — every pressed key included — to punish one dropped frame.
        ErrorCode::RateLimited => Reaction::Continue,
        // Briefly broken, or on its way down: the next attempt may well work.
        ErrorCode::Internal => Reaction::Retry(describe(&error)),
    }
}

/// Turns a reaction into the loop's control flow.
fn apply(reaction: Reaction) -> Result<(), Outcome> {
    match reaction {
        Reaction::Continue => Ok(()),
        Reaction::Retry(reason) => Err(Outcome::Lost { reason }),
        Reaction::Stop(reason) => Err(Outcome::Fatal { reason }),
    }
}

/// Opens the websocket, with timeouts on the socket it ends up reading.
fn open_socket(url: &Url) -> Result<Socket, ClientError> {
    match url.scheme() {
        "ws" => {
            let stream = connect_stream(url)?;
            apply_stream_options(&stream)?;
            let (socket, _response) =
                tungstenite::client(url.as_str(), MaybeTlsStream::Plain(stream))
                    .map_err(|error| ClientError::Connect(error.to_string()))?;
            Ok(socket)
        }
        scheme => Err(ClientError::Scheme {
            scheme: scheme.to_owned(),
        }),
    }
}

/// Resolves and connects, trying every address the host has.
fn connect_stream(url: &Url) -> Result<TcpStream, ClientError> {
    let host = url
        .host_str()
        .ok_or_else(|| ClientError::Connect("the url names no host".to_owned()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ClientError::Connect("the url names no port".to_owned()))?;
    let addresses: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|error| ClientError::Connect(format!("cannot resolve {host}:{port}: {error}")))?
        .collect();
    let mut failure = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => failure = Some(format!("cannot connect to {address}: {error}")),
        }
    }
    Err(ClientError::Connect(failure.unwrap_or_else(|| {
        format!("{host}:{port} resolved to no address")
    })))
}

fn apply_stream_options(stream: &TcpStream) -> Result<(), ClientError> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(SOCKET_POLL_INTERVAL))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    Ok(())
}

fn is_timeout(error: &WsError) -> bool {
    matches!(
        error,
        WsError::Io(io_error)
            if matches!(io_error.kind(), IoErrorKind::WouldBlock | IoErrorKind::TimedOut)
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The owner's end of the event queue.
struct Events {
    sender: SyncSender<ClientEvent>,
}

impl Events {
    fn new(sender: SyncSender<ClientEvent>) -> Self {
        Self { sender }
    }

    /// Offers a per-frame event and never waits for the reader.
    fn emit(&self, event: ClientEvent) {
        let _ = self.sender.try_send(event);
    }

    /// Offers an event the owner needs to keep its picture of the room correct.
    ///
    /// Bounded on purpose: a reader that has stopped consuming must not be able
    /// to wedge the socket thread, so after the retry budget runs out the event
    /// is given up on like any other.
    fn critical(&self, mut event: ClientEvent) {
        for _ in 0..CRITICAL_RETRY_ATTEMPTS {
            match self.sender.try_send(event) {
                Ok(()) => return,
                Err(TrySendError::Full(returned)) => {
                    event = returned;
                    thread::sleep(CRITICAL_RETRY_DELAY);
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
    }
}

/// Whether a command may be dropped when the queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClass {
    /// A pointer sample: superseded by the next one.
    Continuous,
    /// A button edge: carries information no later frame repeats.
    Discrete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// The queue had room.
    Queued,
    /// The oldest continuous command made room.
    ReplacedContinuous,
    /// Nothing continuous was queued, so the oldest command made room.
    ReplacedOldest,
    /// The incoming command itself was the least valuable, and was dropped.
    Rejected,
}

/// Decides what a full queue does with one more command.
///
/// The policy is a free function so it can be tested without a socket or a
/// thread, and so the rule lives in exactly one place:
///
/// 1. room left: queue it;
/// 2. otherwise evict the oldest continuous command;
/// 3. otherwise, if nothing continuous is queued, evict the oldest command for a
///    discrete arrival — a bounded queue cannot keep everything, and dropping
///    the newest would keep the stalest edges;
/// 4. otherwise (a continuous arrival into a queue of edges) drop the arrival.
fn admit<C>(
    queue: &mut VecDeque<C>,
    capacity: usize,
    class_of: impl Fn(&C) -> CommandClass,
    incoming: C,
) -> Admission {
    if capacity == 0 {
        return Admission::Rejected;
    }
    if queue.len() < capacity {
        queue.push_back(incoming);
        return Admission::Queued;
    }
    if let Some(index) = queue
        .iter()
        .position(|queued| class_of(queued) == CommandClass::Continuous)
    {
        queue.remove(index);
        queue.push_back(incoming);
        return Admission::ReplacedContinuous;
    }
    if class_of(&incoming) == CommandClass::Continuous {
        return Admission::Rejected;
    }
    queue.pop_front();
    queue.push_back(incoming);
    Admission::ReplacedOldest
}

/// What the owner pushed, waiting for the socket.
///
/// The channel in front of this queue is unbounded because its sender belongs to
/// the input path, which must never block. Draining it into a bounded queue on
/// every turn is what keeps the memory finite.
struct CommandInbox {
    receiver: Receiver<ClientCommand>,
    queue: VecDeque<ClientCommand>,
    shutdown: bool,
}

impl CommandInbox {
    fn new(receiver: Receiver<ClientCommand>) -> Self {
        Self {
            receiver,
            queue: VecDeque::with_capacity(COMMAND_QUEUE_CAPACITY),
            shutdown: false,
        }
    }

    /// Moves everything the owner pushed into the bounded local queue.
    fn drain(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok(ClientCommand::Shutdown) => {
                    self.shutdown = true;
                }
                Ok(command) => {
                    let _ = admit(
                        &mut self.queue,
                        COMMAND_QUEUE_CAPACITY,
                        command_class,
                        command,
                    );
                }
                Err(TryRecvError::Empty) => return,
                // The owner dropped its sender, which is the same instruction as
                // an explicit shutdown: nothing else will arrive.
                Err(TryRecvError::Disconnected) => {
                    self.shutdown = true;
                    return;
                }
            }
        }
    }

    fn pop(&mut self) -> Option<ClientCommand> {
        self.queue.pop_front()
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    /// Waits out a retry delay, still draining the inbox so a shutdown is
    /// noticed within one slice instead of after the whole backoff.
    ///
    /// Returns `false` when the owner asked to stop.
    fn wait(&mut self, delay: Duration) -> bool {
        let deadline = Instant::now() + delay;
        loop {
            self.drain();
            if self.shutdown {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            thread::sleep((deadline - now).min(RETRY_SLICE));
        }
    }
}

fn command_class(command: &ClientCommand) -> CommandClass {
    match command {
        ClientCommand::Publish(frame) if is_continuous(frame) => CommandClass::Continuous,
        ClientCommand::Publish(_) | ClientCommand::Shutdown => CommandClass::Discrete,
    }
}

/// Whether a frame is a pointer sample rather than a button edge.
///
/// The control cannot answer this: every value it can hold is a button, and the
/// pointer's position travels with the sender's display geometry in `bounds`.
/// A frame that carries geometry is a sampling of where the pointer is — worth
/// dropping first when the link falls behind, because a stale position says
/// nothing once it arrives. A frame without geometry records that something was
/// pressed, and no later frame repeats that.
///
/// Today the input path publishes button edges only and leaves `bounds` empty, so
/// every frame is a discrete event and nothing is dropped ahead of anything else.
/// A pointer stream, if one is added, has to carry geometry anyway, which makes
/// this rule start applying without a second change here.
fn is_continuous(frame: &InputFrame) -> bool {
    frame.bounds.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Bounds;
    use bongocat_input::{InputControl, InputEdge, MouseButton, PhysicalKey};

    /// A button edge: what the input path publishes today.
    fn frame(control: InputControl, seq: u64) -> InputFrame {
        InputFrame {
            src: "pc-01".to_owned(),
            seq,
            t: 1_762_000_000_000,
            control,
            edge: InputEdge::Down,
            bounds: None,
        }
    }

    /// A pointer sample: a frame that carries the sender's display geometry.
    fn pointer_frame(seq: u64) -> InputFrame {
        InputFrame {
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 1_920.0,
                height: 1_080.0,
            }),
            ..frame(InputControl::Mouse(MouseButton::Left), seq)
        }
    }

    #[test]
    fn websocket_urls_are_accepted_and_other_schemes_are_not() {
        assert_eq!(
            validate_server_url("ws://127.0.0.1:8787/session")
                .expect("plain websocket")
                .scheme(),
            "ws"
        );
        let error = validate_server_url("wss://relay.example.com/session")
            .expect_err("wss parses but this build has no TLS to dial it with");
        assert!(matches!(error, ClientError::Scheme { scheme } if scheme == "wss"));

        let error = validate_server_url("https://relay.example.com")
            .expect_err("http is a configuration mistake, not a websocket");
        assert!(matches!(error, ClientError::Scheme { scheme } if scheme == "https"));

        assert!(matches!(
            validate_server_url("not a url").expect_err("unparsable"),
            ClientError::Url { .. }
        ));
    }

    #[test]
    fn backoff_doubles_and_stops_at_the_cap() {
        let expected = [1, 2, 4, 8, 16, 30, 30, 30];
        for (index, seconds) in expected.iter().enumerate() {
            let attempt = index as u32 + 1;
            assert_eq!(
                backoff_for_attempt(attempt),
                Duration::from_secs(*seconds),
                "attempt {attempt}"
            );
        }
        // Past the ladder the delay stays capped rather than growing without
        // bound, which also means a huge attempt count cannot overflow the shift.
        for attempt in [1, 5, 6, 64, 1_000, u32::MAX] {
            let delay = backoff_for_attempt(attempt);
            assert!(delay <= RECONNECT_MAX_DELAY, "attempt {attempt}");
            assert!(delay >= RECONNECT_BASE_DELAY, "attempt {attempt}");
        }
        assert_eq!(backoff_for_attempt(0), Duration::ZERO);
    }

    /// One admission against the real command type, so the test exercises the
    /// classifier the connection actually uses.
    fn push(
        queue: &mut VecDeque<ClientCommand>,
        capacity: usize,
        incoming: ClientCommand,
    ) -> Admission {
        admit(queue, capacity, command_class, incoming)
    }

    /// A pointer sample: carries the sender's display geometry.
    fn pointer(seq: u64) -> ClientCommand {
        ClientCommand::Publish(pointer_frame(seq))
    }

    /// A button edge: no geometry, so nothing later repeats it.
    fn key(seq: u64) -> ClientCommand {
        ClientCommand::Publish(frame(InputControl::Key(PhysicalKey::KEY_A), seq))
    }

    /// A mouse click is an edge, not a sample, even though it names a mouse
    /// button: the input path publishes clicks with empty `bounds`.
    fn click(seq: u64) -> ClientCommand {
        ClientCommand::Publish(frame(InputControl::Mouse(MouseButton::Left), seq))
    }

    #[test]
    fn a_full_queue_drops_pointer_samples_before_button_edges() {
        let mut queue: VecDeque<ClientCommand> = VecDeque::new();
        assert_eq!(push(&mut queue, 3, pointer(0)), Admission::Queued);
        assert_eq!(push(&mut queue, 3, key(1)), Admission::Queued);
        assert_eq!(push(&mut queue, 3, pointer(2)), Admission::Queued);

        // The oldest pointer sample is the one that goes, even though the key
        // edge is older still.
        assert_eq!(
            push(&mut queue, 3, pointer(3)),
            Admission::ReplacedContinuous
        );
        assert_eq!(queue.len(), 3);
        let seqs: Vec<u64> = queue
            .iter()
            .map(|command| match command {
                ClientCommand::Publish(frame) => frame.seq,
                ClientCommand::Shutdown => u64::MAX,
            })
            .collect();
        assert_eq!(
            seqs,
            [1_u64, 2, 3],
            "the key edge survived, the first sample did not"
        );
    }

    #[test]
    fn a_pointer_sample_is_refused_when_only_edges_are_queued() {
        let mut queue: VecDeque<ClientCommand> = VecDeque::new();
        assert_eq!(push(&mut queue, 2, key(0)), Admission::Queued);
        assert_eq!(push(&mut queue, 2, key(1)), Admission::Queued);

        // Both slots hold edges, so the arriving sample is the one to lose.
        assert_eq!(push(&mut queue, 2, pointer(9)), Admission::Rejected);
        assert_eq!(queue.len(), 2);

        // A third edge does make room, by taking the oldest slot.
        assert_eq!(push(&mut queue, 2, key(2)), Admission::ReplacedOldest);
        assert_eq!(queue.len(), 2);
        assert!(matches!(
            queue.back(),
            Some(ClientCommand::Publish(frame)) if frame.seq == 2
        ));
    }

    #[test]
    fn shutdown_is_never_dropped_for_a_pointer_sample() {
        let mut queue: VecDeque<ClientCommand> = VecDeque::new();
        assert_eq!(
            push(&mut queue, 1, ClientCommand::Shutdown),
            Admission::Queued
        );
        assert_eq!(push(&mut queue, 1, pointer(0)), Admission::Rejected);
        assert!(matches!(queue.back(), Some(ClientCommand::Shutdown)));
    }

    #[test]
    fn only_frames_with_display_bounds_count_as_pointer_samples() {
        assert!(is_continuous(&pointer_frame(0)));
        // A click names a mouse button but is still an edge: nothing after it
        // repeats that the button went down, so it must not be the first thing
        // a full queue discards.
        assert!(!is_continuous(&frame(
            InputControl::Mouse(MouseButton::Left),
            0
        )));
        assert!(!is_continuous(&frame(
            InputControl::Key(PhysicalKey::KEY_A),
            0
        )));
    }

    /// The reaction policy is what decides whether a refused client backs off,
    /// stops, or keeps a healthy session, so it is pinned here rather than left
    /// to the shape of a `match`.
    #[test]
    fn a_refusal_only_reconnects_when_retrying_could_help() {
        let reaction = |code| {
            reaction_to_refusal(ProtocolError {
                code,
                message: "because".to_owned(),
            })
        };
        let stops = |code| matches!(reaction(code), Reaction::Stop(_));

        // Retrying any of these asks the relay the same question again.
        assert!(stops(ErrorCode::BadVersion));
        assert!(stops(ErrorCode::BadToken));
        assert!(stops(ErrorCode::Protocol));
        // Two machines sharing a device id evict each other; retrying would
        // turn that into a fight with the other machine instead of a message.
        assert!(stops(ErrorCode::Conflict));

        // A relay that is briefly broken may recover on its own.
        assert!(matches!(reaction(ErrorCode::Internal), Reaction::Retry(_)));

        // The relay drops the frames of a burst and keeps the connection, so
        // tearing the room down over it would lose far more than the burst.
        assert!(matches!(
            reaction(ErrorCode::RateLimited),
            Reaction::Continue
        ));
    }

    #[test]
    fn a_queue_of_edges_keeps_every_click() {
        let mut queue: VecDeque<ClientCommand> = VecDeque::new();
        assert_eq!(push(&mut queue, 2, click(0)), Admission::Queued);
        assert_eq!(push(&mut queue, 2, key(1)), Admission::Queued);

        // Both slots hold edges, so only the oldest one makes room: a sample
        // would have been refused here, a click is not.
        assert_eq!(push(&mut queue, 2, click(2)), Admission::ReplacedOldest);
        assert_eq!(queue.len(), 2);
        assert!(matches!(
            queue.back(),
            Some(ClientCommand::Publish(frame)) if frame.seq == 2
        ));
    }

    fn text_of(message: Message) -> String {
        match message {
            Message::Text(text) => text.as_str().to_owned(),
            other => panic!("expected a text frame, got {other:?}"),
        }
    }

    /// The only test that uses a socket: a stub relay on loopback proves the
    /// thread actually handshakes, reports `Joined` and puts a published frame
    /// on the wire. It is deliberately generous with its deadlines — a loaded CI
    /// machine is slow, not broken.
    #[test]
    fn a_loopback_relay_receives_the_handshake_and_one_published_frame() {
        use std::net::TcpListener;

        const DEADLINE: Duration = Duration::from_secs(10);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let (seen_tx, seen_rx) = mpsc::channel::<String>();

        let relay = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut socket = tungstenite::accept(stream).expect("websocket handshake");

            let hello = text_of(socket.read().expect("hello frame"));
            let hello: Envelope = serde_json::from_str(&hello).expect("hello envelope");
            assert_eq!(hello.kind, kind::HELLO);

            let joined = Envelope::of(
                kind::JOINED,
                &Joined {
                    peer_id: "peer-1".to_owned(),
                    peers: Vec::new(),
                },
            )
            .expect("joined envelope");
            socket
                .send(Message::Text(
                    serde_json::to_string(&joined)
                        .expect("joined wire form")
                        .into(),
                ))
                .expect("send joined");

            let input = text_of(socket.read().expect("input frame"));
            let input: Envelope = serde_json::from_str(&input).expect("input envelope");
            seen_tx.send(input.kind).expect("report the frame");
        });

        let config = ClientConfig {
            url: format!("ws://127.0.0.1:{port}/session"),
            token: "room-token".to_owned(),
            device: DeviceInfo {
                id: "pc-01".to_owned(),
                name: "workstation".to_owned(),
                publish: true,
            },
        };
        let (events, commands) = spawn(config).expect("spawn client");

        let joined = events.recv_timeout(DEADLINE).expect("joined event");
        assert!(
            matches!(&joined, ClientEvent::Joined { peer_id, .. } if peer_id == "peer-1"),
            "unexpected first event: {joined:?}"
        );

        commands
            .send(ClientCommand::Publish(frame(
                InputControl::Mouse(MouseButton::Left),
                7,
            )))
            .expect("publish");

        let received = seen_rx
            .recv_timeout(DEADLINE)
            .expect("the relay never saw the published frame");
        assert_eq!(received, kind::INPUT);

        let _ = commands.send(ClientCommand::Shutdown);
        relay.join().expect("relay thread");
    }
}
