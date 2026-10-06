//! Private WebRTC adapter. No protocol-library types cross the application boundary.
use super::{MultiplayerState, socket::RoomSession};
use bongocat_input::{
    CursorPosition, CursorSample, CursorViewport, InputControl, InputEdge, InputEvent,
    InputResetReason, InputSource, InputSubscription, MouseButton, PhysicalKey,
};
use bongocat_runtime::RuntimeClient;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use webrtc::{
    data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit, RTCDataChannelState},
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
        RTCIceCandidateInit, RTCIceServer, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
        RTCSessionDescription,
    },
};

const EDGES: &str = "bongocat-input-v1";
const CURSOR: &str = "bongocat-cursor-v1";

/// Separate from REST/acknowledgement jobs so chat and lobby timeouts cannot block releases.
pub(super) struct PeerService {
    session: Arc<Mutex<Option<RoomSession>>>,
    stop: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
    asset_worker: std::thread::JoinHandle<()>,
}

impl PeerService {
    pub fn start(
        local: RuntimeClient,
        state: MultiplayerState,
        log: crate::app_log::ApplicationLogHandle,
    ) -> std::io::Result<Self> {
        let session = Arc::new(Mutex::new(None::<RoomSession>));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_session = Arc::clone(&session);
        let worker_stop = Arc::clone(&stop);
        let assets = state.scene.1.clone();
        let asset_stop = Arc::clone(&stop);
        let asset_worker = std::thread::Builder::new()
            .name("bongocat-room-models".to_owned())
            .spawn(move || assets.run(&asset_stop))?;
        let worker = std::thread::Builder::new()
            .name("bongocat-room-peers".to_owned())
            .spawn(move || {
                let mut network = PeerNetwork::new(local);
                let mut reported_failure = false;
                let mut overflows = 0;
                while !worker_stop.load(Ordering::Acquire) {
                    let session = worker_session
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    if network.tick(session.as_ref(), &state).is_err() {
                        if !reported_failure {
                            super::log_multiplayer_degraded(
                                &log,
                                "peer_connection_failed",
                                bongocat_ui_protocol::SettingsErrorCode::MultiplayerConnectFailed,
                            );
                        }
                        reported_failure = true;
                    } else {
                        reported_failure = false;
                    }
                    if network.overflow_count > overflows {
                        overflows = network.overflow_count;
                        super::log_multiplayer_degraded(
                            &log,
                            "peer_input_overflow_reset",
                            bongocat_ui_protocol::SettingsErrorCode::MultiplayerConnectFailed,
                        );
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
                network.shutdown();
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = asset_worker.join();
                return Err(error);
            }
        };
        Ok(Self {
            session,
            stop,
            worker,
            asset_worker,
        })
    }
    pub fn set_session(&self, session: Option<RoomSession>) {
        *self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = session;
    }
    pub fn shutdown(self) -> bool {
        self.stop.store(true, Ordering::Release);
        let peer_ok = self.worker.join().is_ok();
        let asset_ok = self.asset_worker.join().is_ok();
        peer_ok && asset_ok
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Signal {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IncomingSignal {
    pub room_id: String,
    pub from: String,
    pub data: Signal,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Control {
    Key(u16),
    Mouse(u8),
}

impl Control {
    fn capture(control: InputControl) -> Option<Self> {
        match control {
            InputControl::Key(key) => Some(Self::Key(key.hid_usage())),
            InputControl::Mouse(button) => Some(Self::Mouse(match button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                MouseButton::Middle => 2,
                MouseButton::Back => 3,
                MouseButton::Forward => 4,
                MouseButton::Other(_) => return None,
            })),
            _ => None,
        }
    }
    fn input(self) -> Option<InputControl> {
        match self {
            Self::Key(key) if key <= 0xff || key == bongocat_input::GLOBE_KEY_USAGE => {
                Some(InputControl::Key(PhysicalKey::from_hid_usage(key)))
            }
            Self::Mouse(button) => Some(InputControl::Mouse(match button {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                2 => MouseButton::Middle,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                _ => return None,
            })),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Frame {
    Edge { control: Control, down: bool },
    Reconcile { pressed: Vec<Control> },
    Reset,
    Cursor { x: f32, y: f32 },
}

#[derive(Default)]
struct Reception {
    pressed: BTreeSet<Control>,
    applied: BTreeSet<Control>,
    window_ready: bool,
    last_received: Option<Instant>,
}

struct Handler {
    outgoing: Arc<Mutex<VecDeque<(String, Signal)>>>,
    member_id: String,
    room_id: String,
    scene: crate::RoomSceneHandle,
    channels: Mutex<BTreeMap<String, Arc<dyn DataChannel>>>,
    reception: Mutex<Reception>,
    closed: AtomicBool,
}

impl Handler {
    fn signal(&self, signal: Signal) {
        let mut outgoing = self
            .outgoing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if outgoing.len() < 128 {
            outgoing.push_back((self.member_id.clone(), signal));
        } else {
            self.closed.store(true, Ordering::Release);
        }
    }
    async fn attach(self: &Arc<Self>, channel: Arc<dyn DataChannel>) {
        let Ok(label) = channel.label().await else {
            return;
        };
        if label != EDGES && label != CURSOR && label != crate::room_assets::MODEL_CHANNEL {
            let _ = channel.close().await;
            return;
        }
        if label != CURSOR
            && (channel.ordered().await != Ok(true)
                || channel.max_retransmits().await.ok().flatten().is_some()
                || channel
                    .max_packet_life_time()
                    .await
                    .ok()
                    .flatten()
                    .is_some())
        {
            let _ = channel.close().await;
            return;
        }
        let duplicate = {
            let mut channels = self
                .channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if channels.contains_key(&label) {
                true
            } else {
                channels.insert(label.clone(), channel.clone());
                false
            }
        };
        if duplicate {
            let _ = channel.close().await;
            return;
        }
        let handler = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = channel.poll().await {
                match event {
                    DataChannelEvent::OnMessage(message) => {
                        if message.data.len() > 8192 {
                            if label == crate::room_assets::MODEL_CHANNEL {
                                continue;
                            }
                            handler.reset();
                            continue;
                        }
                        if label == crate::room_assets::MODEL_CHANNEL {
                            if let Ok(frame) = serde_json::from_slice::<
                                crate::room_assets::ModelFrame,
                            >(&message.data)
                            {
                                let _ = handler.scene.1.receive(
                                    &handler.room_id,
                                    &handler.member_id,
                                    frame,
                                );
                            }
                            continue;
                        }
                        if let Ok(frame) = serde_json::from_slice::<Frame>(&message.data)
                            && matches!(frame, Frame::Cursor { .. }) == (label == CURSOR)
                        {
                            handler.receive(frame);
                        }
                    }
                    DataChannelEvent::OnClose | DataChannelEvent::OnError => {
                        if label == crate::room_assets::MODEL_CHANNEL {
                            break;
                        }
                        handler.reset();
                        handler.closed.store(true, Ordering::Release);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
    fn reset(&self) {
        let mut reception = self
            .reception
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reception.pressed.clear();
        reception.applied.clear();
        reception.window_ready = false;
        reception.last_received = None;
        if let Some(client) = self.scene.member_client(&self.member_id) {
            let _ = publish_frame(&client, &Frame::Reset);
        }
    }
    fn receive(&self, frame: Frame) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let mut reception = self
            .reception
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &frame {
            Frame::Edge { control, down } => {
                if control.input().is_none() {
                    return;
                }
                if *down {
                    reception.pressed.insert(*control);
                } else {
                    reception.pressed.remove(control);
                }
            }
            Frame::Reconcile { pressed } => {
                if pressed.len() > 256 || pressed.iter().any(|control| control.input().is_none()) {
                    return;
                }
                reception.pressed = pressed.iter().copied().collect();
            }
            Frame::Reset => reception.pressed.clear(),
            Frame::Cursor { x, y }
                if !x.is_finite() || !y.is_finite() || x.abs() > 1.0 || y.abs() > 1.0 =>
            {
                return;
            }
            _ => {}
        }
        if !matches!(frame, Frame::Cursor { .. }) {
            reception.last_received = Some(Instant::now());
        }
        if let Some(client) = self.scene.member_client(&self.member_id) {
            if matches!(frame, Frame::Cursor { .. }) {
                let _ = publish_frame(&client, &frame);
            } else {
                if matches!(frame, Frame::Reset) {
                    reception.window_ready = false;
                }
                synchronize_pressed(&client, &mut reception);
            }
        }
    }
    fn prepare_window(&self) {
        let mut reception = self
            .reception
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if reception
            .last_received
            .is_some_and(|time| time.elapsed() > Duration::from_secs(2))
        {
            drop(reception);
            self.reset();
            return;
        }
        if let Some(client) = self.scene.member_client(&self.member_id) {
            synchronize_pressed(&client, &mut reception);
        }
    }
}

fn synchronize_pressed(client: &RuntimeClient, reception: &mut Reception) {
    if !reception.window_ready {
        if !publish_frame(client, &Frame::Reset) {
            return;
        }
        reception.applied.clear();
        reception.window_ready = true;
    }
    let changes: Vec<_> = reception
        .applied
        .difference(&reception.pressed)
        .map(|control| (*control, false))
        .chain(
            reception
                .pressed
                .difference(&reception.applied)
                .map(|control| (*control, true)),
        )
        .collect();
    for (control, down) in changes {
        if !publish_frame(client, &Frame::Edge { control, down }) {
            reception.window_ready = false;
            return;
        }
        if down {
            reception.applied.insert(control);
        } else {
            reception.applied.remove(&control);
        }
    }
}

fn publish_frame(client: &RuntimeClient, frame: &Frame) -> bool {
    let at = client.input_timestamp();
    let event = match frame {
        Frame::Edge { control, down } => control.input().map(|control| InputEvent::Edge {
            control,
            edge: if *down {
                InputEdge::Down
            } else {
                InputEdge::Up
            },
            source: InputSource::Capture,
            at,
        }),
        Frame::Reconcile { pressed } => Some(InputEvent::Reconcile {
            pressed: pressed
                .iter()
                .filter_map(|control| control.input())
                .collect(),
            at,
        }),
        Frame::Reset => Some(InputEvent::Reset {
            reason: InputResetReason::ServiceRestart,
            at,
        }),
        Frame::Cursor { x, y } => {
            if let Ok(sample) = CursorSample::new(
                CursorPosition {
                    x: (1.0 - f64::from(*x)) / 2.0,
                    y: (1.0 - f64::from(*y)) / 2.0,
                },
                CursorViewport {
                    origin: CursorPosition { x: 0.0, y: 0.0 },
                    width: 1.0,
                    height: 1.0,
                },
                at,
            ) {
                let _ = client.cursor_producer().publish(sample);
            }
            None
        }
    };
    event.is_none_or(|event| client.input_producer().publish(event).is_ok())
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for EventHandler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(candidate) = event.candidate.to_json() {
            self.0.signal(Signal::Candidate {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_mline_index: candidate.sdp_mline_index,
            });
        }
    }
    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        self.0.attach(channel).await;
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(
            state,
            RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed
        ) {
            self.0.reset();
            self.0.closed.store(true, Ordering::Release);
        }
    }
}

struct EventHandler(Arc<Handler>);

struct Peer {
    connection: Arc<dyn PeerConnection>,
    handler: Arc<Handler>,
    pending: VecDeque<Frame>,
    candidates: Vec<RTCIceCandidateInit>,
    remote_description: bool,
    initialized: bool,
    started: Instant,
    last_heartbeat: Instant,
    advertised_model: Option<crate::room_assets::Advertisement>,
}

pub(super) struct PeerNetwork {
    outgoing: Arc<Mutex<VecDeque<(String, Signal)>>>,
    executor: tokio::runtime::Runtime,
    peers: BTreeMap<String, Peer>,
    room_id: Option<String>,
    subscription: InputSubscription,
    local: RuntimeClient,
    pressed: BTreeSet<Control>,
    cursor: Option<(f32, f32)>,
    pub overflow_count: u64,
}

impl PeerNetwork {
    #[cfg(test)]
    pub(super) fn connected_members(&self) -> usize {
        self.peers.values().filter(|peer| peer.initialized).count()
    }
    pub fn new(local: RuntimeClient) -> Self {
        Self {
            outgoing: Arc::default(),
            executor: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("WebRTC executor"),
            peers: BTreeMap::new(),
            room_id: None,
            subscription: local.input_producer().subscribe(),
            local,
            pressed: BTreeSet::new(),
            cursor: None,
            overflow_count: 0,
        }
    }
    pub fn tick(
        &mut self,
        session: Option<&RoomSession>,
        state: &MultiplayerState,
    ) -> Result<(), String> {
        let executor = &self.executor;
        let result = executor.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let room = state.snapshot().room;
                if self.room_id.as_deref() != room.as_ref().map(|room| room.room_id.as_str())
                    || session.is_none()
                {
                    for (_, peer) in std::mem::take(&mut self.peers) {
                        peer.handler.reset();
                        let _ = peer.connection.close().await;
                    }
                    self.room_id = room.as_ref().map(|room| room.room_id.clone());
                    self.cursor = None;
                    self.outgoing
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clear();
                }
                let mut frames = Vec::new();
                for event in self.subscription.drain() {
                    match event {
                        InputEvent::Edge { control, edge, .. } => {
                            if let Some(control) = Control::capture(control) {
                                let down = edge == InputEdge::Down;
                                if down {
                                    self.pressed.insert(control);
                                } else {
                                    self.pressed.remove(&control);
                                }
                                frames.push(Frame::Edge { control, down });
                            }
                        }
                        InputEvent::Reconcile { pressed, .. } => {
                            let confirmed: BTreeSet<_> =
                                pressed.into_iter().filter_map(Control::capture).collect();
                            self.pressed.retain(|control| confirmed.contains(control));
                            frames.push(Frame::Reconcile {
                                pressed: self.pressed.iter().copied().collect(),
                            });
                        }
                        InputEvent::Reset { .. } => {
                            self.pressed.clear();
                            frames.push(Frame::Reset);
                        }
                        _ => {}
                    }
                }
                self.overflow_count = self.overflow_count.max(self.subscription.overflow_count());
                let (Some(room), Some(_)) = (room, session) else {
                    return Ok(());
                };
                let Some(my_id) = room.self_member().map(|member| member.id.as_str()) else {
                    return Ok(());
                };
                let obsolete: Vec<_> = self
                    .peers
                    .iter()
                    .filter(|(id, peer)| {
                        !room.members.iter().any(|member| member.id == **id)
                            || peer.handler.closed.load(Ordering::Acquire)
                            || (!peer.initialized
                                && peer.started.elapsed() > Duration::from_secs(15))
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in obsolete {
                    if let Some(peer) = self.peers.remove(&id) {
                        peer.handler.reset();
                        let _ = peer.connection.close().await;
                    }
                }
                for member in room
                    .members
                    .iter()
                    .filter(|member| !member.is_self)
                    .take(32)
                {
                    if self.peers.contains_key(&member.id) {
                        continue;
                    }
                    let handler = Arc::new(Handler {
                        outgoing: Arc::clone(&self.outgoing),
                        member_id: member.id.clone(),
                        room_id: room.room_id.clone(),
                        scene: state.scene.clone(),
                        channels: Mutex::new(BTreeMap::new()),
                        reception: Mutex::new(Reception::default()),
                        closed: AtomicBool::new(false),
                    });
                    let configuration = RTCConfigurationBuilder::default()
                        .with_ice_servers(vec![RTCIceServer {
                            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                            ..Default::default()
                        }])
                        .build();
                    let connection = PeerConnectionBuilder::new()
                        .with_configuration(configuration)
                        .with_handler(Arc::new(EventHandler(handler.clone())))
                        .with_udp_addrs(vec!["0.0.0.0:0"])
                        .with_data_channel_send_buffer_limit(65536)
                        .build()
                        .await
                        .map_err(|error| error.to_string())?;
                    let connection: Arc<dyn PeerConnection> = Arc::new(connection);
                    if my_id < member.id.as_str() {
                        let edges = connection
                            .create_data_channel(EDGES, None)
                            .await
                            .map_err(|error| error.to_string())?;
                        handler.attach(edges).await;
                        let cursor = connection
                            .create_data_channel(
                                CURSOR,
                                Some(RTCDataChannelInit {
                                    ordered: false,
                                    max_retransmits: Some(0),
                                    ..Default::default()
                                }),
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                        handler.attach(cursor).await;
                        let models = connection
                            .create_data_channel(crate::room_assets::MODEL_CHANNEL, None)
                            .await
                            .map_err(|error| error.to_string())?;
                        handler.attach(models).await;
                        let offer = connection
                            .create_offer(None)
                            .await
                            .map_err(|error| error.to_string())?;
                        connection
                            .set_local_description(offer.clone())
                            .await
                            .map_err(|error| error.to_string())?;
                        handler.signal(Signal::Offer { sdp: offer.sdp });
                    }
                    self.peers.insert(
                        member.id.clone(),
                        Peer {
                            connection,
                            handler,
                            pending: VecDeque::new(),
                            candidates: Vec::new(),
                            remote_description: false,
                            initialized: false,
                            started: Instant::now(),
                            last_heartbeat: Instant::now(),
                            advertised_model: None,
                        },
                    );
                }
                let signals: Vec<_> = state
                    .signals
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .drain(..)
                    .collect();
                for signal in signals {
                    if signal.room_id != room.room_id {
                        continue;
                    }
                    let Some(peer) = self.peers.get_mut(&signal.from) else {
                        continue;
                    };
                    match signal.data {
                        Signal::Offer { sdp } if my_id > signal.from.as_str() => {
                            peer.connection
                                .set_remote_description(
                                    RTCSessionDescription::offer(sdp)
                                        .map_err(|error| error.to_string())?,
                                )
                                .await
                                .map_err(|error| error.to_string())?;
                            peer.remote_description = true;
                            let answer = peer
                                .connection
                                .create_answer(None)
                                .await
                                .map_err(|error| error.to_string())?;
                            peer.connection
                                .set_local_description(answer.clone())
                                .await
                                .map_err(|error| error.to_string())?;
                            peer.handler.signal(Signal::Answer { sdp: answer.sdp });
                        }
                        Signal::Answer { sdp } if my_id < signal.from.as_str() => {
                            peer.connection
                                .set_remote_description(
                                    RTCSessionDescription::answer(sdp)
                                        .map_err(|error| error.to_string())?,
                                )
                                .await
                                .map_err(|error| error.to_string())?;
                            peer.remote_description = true;
                        }
                        Signal::Candidate {
                            candidate,
                            sdp_mid,
                            sdp_mline_index,
                        } if peer.candidates.len() < 64 => {
                            peer.candidates.push(RTCIceCandidateInit {
                                candidate,
                                sdp_mid,
                                sdp_mline_index,
                                ..Default::default()
                            });
                        }
                        _ => {}
                    }
                    if peer.remote_description {
                        for candidate in peer.candidates.drain(..) {
                            peer.connection
                                .add_ice_candidate(candidate)
                                .await
                                .map_err(|error| error.to_string())?;
                        }
                    }
                }
                let cursor = self.local.cursor_producer().latest().map(|sample| {
                    let position = sample.normalized();
                    (position.x, position.y)
                });
                let cursor_changed = cursor != self.cursor;
                for peer in self.peers.values_mut() {
                    peer.handler.prepare_window();
                    let channels = peer
                        .handler
                        .channels
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let Some(edges) = channels.get(EDGES) else {
                        continue;
                    };
                    if edges.ready_state().await.ok() != Some(RTCDataChannelState::Open) {
                        continue;
                    }
                    if !peer.initialized {
                        peer.pending.push_back(Frame::Reset);
                        peer.pending.push_back(Frame::Reconcile {
                            pressed: self.pressed.iter().copied().collect(),
                        });
                        peer.initialized = true;
                    }
                    for frame in &frames {
                        if peer.pending.len() >= 256 {
                            peer.pending.clear();
                            peer.pending.push_back(Frame::Reset);
                            peer.pending.push_back(Frame::Reconcile {
                                pressed: self.pressed.iter().copied().collect(),
                            });
                            self.overflow_count = self.overflow_count.saturating_add(1);
                            break;
                        }
                        peer.pending.push_back(frame.clone());
                    }
                    let heartbeat = peer.last_heartbeat.elapsed() >= Duration::from_millis(500);
                    if heartbeat && peer.pending.len() < 256 {
                        peer.pending.push_back(Frame::Reconcile {
                            pressed: self.pressed.iter().copied().collect(),
                        });
                        peer.last_heartbeat = Instant::now();
                    }
                    while let Some(frame) = peer.pending.front() {
                        let text =
                            serde_json::to_string(frame).map_err(|error| error.to_string())?;
                        if edges.try_send_text(&text).await.is_err() {
                            break;
                        }
                        peer.pending.pop_front();
                    }
                    if (cursor_changed
                        || heartbeat
                        || peer.started.elapsed() < Duration::from_secs(1))
                        && let (Some(channel), Some((x, y))) = (channels.get(CURSOR), cursor)
                        && channel.ready_state().await.ok() == Some(RTCDataChannelState::Open)
                    {
                        let _ = channel
                            .try_send_text(
                                &serde_json::to_string(&Frame::Cursor { x, y })
                                    .expect("finite cursor"),
                            )
                            .await;
                    }
                    // Input edges get the first opportunity to use the shared SCTP buffer.
                    if peer.pending.is_empty()
                        && let Some(channel) = channels.get(crate::room_assets::MODEL_CHANNEL)
                        && channel.ready_state().await.ok() == Some(RTCDataChannelState::Open)
                    {
                        if let Some(model) = state.scene.1.local()
                            && peer.advertised_model.as_ref() != Some(&model)
                        {
                            let frame = crate::room_assets::ModelFrame::Offer {
                                model: model.clone(),
                            };
                            let text =
                                serde_json::to_string(&frame).map_err(|error| error.to_string())?;
                            if channel.try_send_text(&text).await.is_ok() {
                                peer.advertised_model = Some(model);
                            }
                        }
                        let mut frames =
                            state.scene.1.outgoing(&peer.handler.member_id).into_iter();
                        while let Some(frame) = frames.next() {
                            let text =
                                serde_json::to_string(&frame).map_err(|error| error.to_string())?;
                            if channel.try_send_text(&text).await.is_err() {
                                state.scene.1.retry(
                                    &peer.handler.member_id,
                                    std::iter::once(frame).chain(frames).collect(),
                                );
                                break;
                            }
                        }
                    }
                }
                self.cursor = cursor;
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok(())
            })
            .await
            .unwrap_or_else(|_| Err("WebRTC control timeout".to_owned()))
        });
        let outgoing: Vec<_> = self
            .outgoing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        if let Some(session) = session {
            for (member_id, signal) in outgoing {
                session.signal(&member_id, &signal);
            }
        }
        result
    }
    pub fn shutdown(&mut self) {
        for peer in self.peers.values() {
            peer.handler.reset();
        }
        self.executor.block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                for (_, peer) in std::mem::take(&mut self.peers) {
                    let _ = peer.connection.close().await;
                }
            })
            .await;
        });
    }
}
