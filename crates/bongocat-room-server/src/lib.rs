//! Ephemeral, bounded, anonymous room. The network owns no user input or models.
#![forbid(unsafe_code)]

use axum::{
    Router,
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use bongocat_online_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::BTreeMap,
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc, watch},
    time::{interval, timeout},
};

const QUEUE_CAPACITY: usize = 64;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

struct Peer {
    messages: mpsc::Sender<ServerMessage>,
    stop: watch::Sender<bool>,
    tokens: f64,
    refilled: Instant,
}
struct Room {
    peers: BTreeMap<MemberId, Peer>,
    sequence: u64,
    next_id: u64,
}
impl Room {
    fn new() -> Self {
        Self {
            peers: BTreeMap::new(),
            sequence: 0,
            next_id: (rand::random::<u32>() as u64) << 32,
        }
    }
    fn next_sequence(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    fn broadcast(&mut self, message: ServerMessage, exclude: Option<MemberId>) {
        let failed: Vec<_> = self
            .peers
            .iter()
            .filter_map(|(id, peer)| {
                if Some(*id) != exclude && peer.messages.try_send(message.clone()).is_err() {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        for id in failed {
            self.remove(id);
        }
    }
    fn remove(&mut self, id: MemberId) {
        if let Some(peer) = self.peers.remove(&id) {
            let _ = peer.stop.send(true);
            let sequence = self.next_sequence();
            self.broadcast(
                ServerMessage::Left {
                    sequence,
                    member: id,
                },
                None,
            );
        }
    }
    fn join(
        &mut self,
        messages: mpsc::Sender<ServerMessage>,
        stop: watch::Sender<bool>,
    ) -> Result<MemberId, RoomError> {
        if self.peers.len() >= ROOM_CAPACITY {
            return Err(RoomError::RoomFull);
        }
        self.next_id += 1;
        let id = MemberId(self.next_id);
        let sequence = self.next_sequence();
        let mut members: Vec<_> = self.peers.keys().copied().collect();
        members.push(id);
        messages
            .try_send(ServerMessage::Welcome {
                protocol_version: PROTOCOL_VERSION,
                sequence,
                you: id,
                members,
            })
            .map_err(|_| RoomError::InvalidMessage)?;
        self.peers.insert(
            id,
            Peer {
                messages,
                stop,
                tokens: 2.0,
                refilled: Instant::now(),
            },
        );
        self.broadcast(
            ServerMessage::Joined {
                sequence,
                member: id,
            },
            Some(id),
        );
        Ok(id)
    }
    fn action(&mut self, id: MemberId, action: PawAction, now: Instant) -> Result<(), RoomError> {
        let peer = self.peers.get_mut(&id).ok_or(RoomError::InvalidMessage)?;
        peer.tokens = (peer.tokens
            + now.saturating_duration_since(peer.refilled).as_secs_f64() * 5.0)
            .min(2.0);
        peer.refilled = now;
        if peer.tokens < 1.0 {
            return Err(RoomError::RateLimited);
        }
        peer.tokens -= 1.0;
        let sequence = self.next_sequence();
        self.broadcast(
            ServerMessage::Action {
                sequence,
                member: id,
                action,
            },
            None,
        );
        Ok(())
    }
}

#[derive(Clone)]
struct ServerState {
    room: Arc<Mutex<Room>>,
    admission: Arc<Mutex<BTreeMap<IpAddr, (Instant, u32)>>>,
    connections: Arc<Semaphore>,
}
impl ServerState {
    fn room(&self) -> std::sync::MutexGuard<'_, Room> {
        self.room
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn admit(&self, ip: IpAddr, now: Instant) -> bool {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        admission.retain(|_, (start, _)| {
            now.saturating_duration_since(*start) < Duration::from_secs(10)
        });
        if admission.len() >= 1024 && !admission.contains_key(&ip) {
            return false;
        }
        let entry = admission.entry(ip).or_insert((now, 0));
        if entry.1 >= 20 {
            return false;
        }
        entry.1 += 1;
        true
    }
}

pub struct RoomServer {
    listener: TcpListener,
    state: ServerState,
}
impl RoomServer {
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(address).await?,
            state: ServerState {
                room: Arc::new(Mutex::new(Room::new())),
                admission: Arc::new(Mutex::new(BTreeMap::new())),
                connections: Arc::new(Semaphore::new(32)),
            },
        })
    }
    pub fn address(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
    pub async fn run(self, shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {
        let state = self.state;
        let shutdown_state = state.clone();
        let app = Router::new()
            .route("/health", get(|| async { "ok\n" }))
            .route("/ws", get(upgrade))
            .with_state(state);
        axum::serve(
            self.listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.await;
            let mut room = shutdown_state.room();
            for (_, peer) in std::mem::take(&mut room.peers) {
                let _ = peer.stop.send(true);
            }
        })
        .await
    }
}

async fn upgrade(
    State(state): State<ServerState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    if !state.admit(address.ip(), Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let Ok(permit) = state.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            connection(state, socket).await;
        })
}

async fn error(socket: &mut WebSocket, code: RoomError) {
    if let Ok(text) = serde_json::to_string(&ServerMessage::Error { code }) {
        let _ = timeout(IO_TIMEOUT, socket.send(Message::Text(text.into()))).await;
    }
    let _ = timeout(IO_TIMEOUT, socket.close()).await;
}

async fn connection(state: ServerState, mut socket: WebSocket) {
    let first = timeout(IO_TIMEOUT, socket.recv()).await;
    match first {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Join { protocol_version })
                if protocol_version == PROTOCOL_VERSION => {}
            Ok(ClientMessage::Join { .. }) => {
                error(&mut socket, RoomError::ProtocolMismatch).await;
                return;
            }
            _ => {
                error(&mut socket, RoomError::InvalidMessage).await;
                return;
            }
        },
        _ => {
            error(&mut socket, RoomError::InvalidMessage).await;
            return;
        }
    }
    let (messages, mut receiver) = mpsc::channel(QUEUE_CAPACITY);
    let (stop, mut stopping) = watch::channel(false);
    let joined = state.room().join(messages, stop);
    let id = match joined {
        Ok(id) => id,
        Err(code) => {
            error(&mut socket, code).await;
            return;
        }
    };
    let (mut writer, mut reader) = socket.split();
    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    let mut last_pong = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = stopping.changed() => break,
            _ = heartbeat.tick() => {
                if last_pong.elapsed() >= HEARTBEAT_TIMEOUT { break; }
                if !matches!(timeout(IO_TIMEOUT, writer.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break; }
            },
            message = receiver.recv() => {
                let Some(message) = message else { break; };
                let Ok(text) = serde_json::to_string(&message) else { break; };
                if !matches!(timeout(IO_TIMEOUT, writer.send(Message::Text(text.into()))).await, Ok(Ok(()))) { break; }
            },
            incoming = reader.next() => {
                let result = match incoming {
                    Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Action { action }) => state.room().action(id, action, Instant::now()),
                        _ => Err(RoomError::InvalidMessage),
                    },
                    Some(Ok(Message::Pong(_))) => { last_pong = tokio::time::Instant::now(); Ok(()) },
                    Some(Ok(Message::Ping(payload))) => {
                        if !matches!(timeout(IO_TIMEOUT, writer.send(Message::Pong(payload))).await, Ok(Ok(()))) { break; }
                        Ok(())
                    },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => Err(RoomError::InvalidMessage),
                };
                if let Err(code) = result {
                    if let Ok(text) = serde_json::to_string(&ServerMessage::Error { code }) {
                        let _ = timeout(IO_TIMEOUT, writer.send(Message::Text(text.into()))).await;
                    }
                    break;
                }
            }
        }
    }
    state.room().remove(id);
    let _ = timeout(IO_TIMEOUT, writer.close()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peer(
        room: &mut Room,
        capacity: usize,
    ) -> (
        MemberId,
        mpsc::Receiver<ServerMessage>,
        watch::Receiver<bool>,
    ) {
        let (tx, rx) = mpsc::channel(capacity);
        let (stop, stopped) = watch::channel(false);
        (room.join(tx, stop).unwrap(), rx, stopped)
    }
    #[test]
    fn capacity_initial_snapshot_and_cleanup() {
        let mut room = Room::new();
        let mut receivers = Vec::new();
        for n in 1..=8 {
            let (id, mut rx, _) = peer(&mut room, 64);
            assert!(
                matches!(rx.try_recv().unwrap(), ServerMessage::Welcome { you, members, .. } if you == id && members.len() == n)
            );
            receivers.push(rx);
        }
        let (tx, _) = mpsc::channel(64);
        let (stop, _) = watch::channel(false);
        assert_eq!(room.join(tx, stop), Err(RoomError::RoomFull));
        let id = *room.peers.keys().next().unwrap();
        room.remove(id);
        assert_eq!(room.peers.len(), 7);
    }
    #[test]
    fn slow_member_is_disconnected_without_blocking_others() {
        let mut room = Room::new();
        let (slow, _rx, stopped) = peer(&mut room, 1);
        let (_, mut healthy, _) = peer(&mut room, 64);
        assert!(!room.peers.contains_key(&slow));
        assert!(*stopped.borrow());
        assert!(matches!(
            healthy.try_recv().unwrap(),
            ServerMessage::Welcome { .. }
        ));
        assert!(
            matches!(healthy.try_recv().unwrap(), ServerMessage::Left { member, .. } if member == slow)
        );
    }
    #[test]
    fn action_rate_is_bounded_and_recovers() {
        let mut room = Room::new();
        let (id, _rx, _) = peer(&mut room, 64);
        let now = Instant::now();
        assert!(room.action(id, PawAction::Left, now).is_ok());
        assert!(room.action(id, PawAction::Right, now).is_ok());
        assert_eq!(
            room.action(id, PawAction::Both, now),
            Err(RoomError::RateLimited)
        );
        assert!(
            room.action(id, PawAction::Both, now + ACTION_INTERVAL)
                .is_ok()
        );
    }
    #[test]
    fn connection_admission_is_bounded_and_expires() {
        let state = ServerState {
            room: Arc::new(Mutex::new(Room::new())),
            admission: Arc::new(Mutex::new(BTreeMap::new())),
            connections: Arc::new(Semaphore::new(32)),
        };
        let ip = "127.0.0.1".parse().unwrap();
        let now = Instant::now();
        for _ in 0..20 {
            assert!(state.admit(ip, now));
        }
        assert!(!state.admit(ip, now));
        assert!(state.admit(ip, now + Duration::from_secs(10)));
    }
}
