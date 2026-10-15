//! A cancellable network worker with typed snapshots and a one-bit input seam.
#![forbid(unsafe_code)]

use bongocat_online_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};
use tokio::{
    sync::watch,
    time::{interval, sleep, timeout},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

pub use bongocat_online_protocol::{OnlineError, OnlinePhase, OnlineSnapshot};
#[derive(Clone, Copy, Debug)]
pub struct RemoteAction {
    pub generation: u64,
    pub member: MemberId,
    pub action: PawAction,
}
#[derive(Clone, Default)]
struct Control {
    endpoint: Option<String>,
    stopping: bool,
}
#[derive(Default)]
struct Published {
    snapshot: OnlineSnapshot,
    actions: VecDeque<RemoteAction>,
}
#[derive(Clone)]
pub struct OnlineClient {
    control: watch::Sender<Control>,
    published: Arc<Mutex<Published>>,
    activity: ActivitySignal,
}
impl OnlineClient {
    fn published(&self) -> std::sync::MutexGuard<'_, Published> {
        self.published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn snapshot(&self) -> OnlineSnapshot {
        self.published().snapshot.clone()
    }
    pub fn join(&self, endpoint: String) -> Result<(), OnlineError> {
        if !valid_server_address(&endpoint) {
            return Err(OnlineError::InvalidAddress);
        }
        if self.control.is_closed() {
            return Err(OnlineError::ServiceUnavailable);
        }
        self.activity.enable(false);
        self.control.send_replace(Control {
            endpoint: Some(endpoint),
            stopping: false,
        });
        Ok(())
    }
    pub fn leave(&self) {
        self.activity.enable(false);
        self.control.send_replace(Control::default());
        let mut p = self.published();
        p.actions.clear();
        p.snapshot.members.clear();
        p.snapshot.you = None;
        p.snapshot.render_failures.clear();
        p.snapshot.phase = OnlinePhase::Offline;
        p.snapshot.error = None;
        p.snapshot.revision += 1;
    }
    pub fn drain_actions(&self) -> Vec<RemoteAction> {
        self.published().actions.drain(..).collect()
    }
    pub fn report_render_failure(&self, member: MemberId) {
        let mut p = self.published();
        if p.snapshot.members.contains(&member) && !p.snapshot.render_failures.contains(&member) {
            p.snapshot.render_failures.push(member);
            p.snapshot.error = Some(OnlineError::RenderFailed);
            p.snapshot.revision += 1;
        }
    }
    pub fn retry_rendering(&self) {
        let mut p = self.published();
        p.snapshot.render_failures.clear();
        if p.snapshot.error == Some(OnlineError::RenderFailed) {
            p.snapshot.error = None;
        }
        p.snapshot.revision += 1;
    }
    fn reset(&self, generation: u64, phase: OnlinePhase, error: Option<OnlineError>) {
        self.activity.enable(false);
        let mut p = self.published();
        let revision = p.snapshot.revision + 1;
        p.actions.clear();
        p.snapshot = OnlineSnapshot {
            revision,
            generation,
            phase,
            error,
            ..OnlineSnapshot::default()
        };
    }
    fn accept(
        &self,
        generation: u64,
        message: ServerMessage,
        sequence: &mut Option<u64>,
    ) -> Result<(), OnlineError> {
        let mut p = self.published();
        if p.snapshot.generation != generation {
            return Err(OnlineError::InvalidProtocol);
        }
        match message {
            ServerMessage::Error { code } => return Err(OnlineError::Room(code)),
            ServerMessage::Welcome {
                protocol_version,
                sequence: incoming,
                you,
                members,
            } => {
                let unique: BTreeSet<_> = members.iter().copied().collect();
                if sequence.is_some()
                    || protocol_version != PROTOCOL_VERSION
                    || members.is_empty()
                    || members.len() > ROOM_CAPACITY
                    || unique.len() != members.len()
                    || !unique.contains(&you)
                {
                    return Err(OnlineError::InvalidProtocol);
                }
                *sequence = Some(incoming);
                p.snapshot.you = Some(you);
                p.snapshot.members = members;
                p.snapshot.phase = OnlinePhase::Connected;
            }
            ServerMessage::Joined {
                sequence: incoming,
                member,
            } => {
                check_sequence(sequence, incoming)?;
                if p.snapshot.members.contains(&member) || p.snapshot.members.len() >= ROOM_CAPACITY
                {
                    return Err(OnlineError::InvalidProtocol);
                }
                p.snapshot.members.push(member);
            }
            ServerMessage::Left {
                sequence: incoming,
                member,
            } => {
                check_sequence(sequence, incoming)?;
                if Some(member) == p.snapshot.you || !p.snapshot.members.contains(&member) {
                    return Err(OnlineError::InvalidProtocol);
                }
                p.snapshot.members.retain(|id| *id != member);
                p.snapshot.render_failures.retain(|id| *id != member);
                if p.snapshot.render_failures.is_empty()
                    && p.snapshot.error == Some(OnlineError::RenderFailed)
                {
                    p.snapshot.error = None;
                }
            }
            ServerMessage::Action {
                sequence: incoming,
                member,
                action,
            } => {
                check_sequence(sequence, incoming)?;
                if !p.snapshot.members.contains(&member) || p.actions.len() >= 64 {
                    return Err(OnlineError::InvalidProtocol);
                }
                if Some(member) != p.snapshot.you {
                    p.actions.push_back(RemoteAction {
                        generation,
                        member,
                        action,
                    });
                }
                return Ok(());
            }
        }
        p.snapshot.revision += 1;
        let connected = p.snapshot.phase == OnlinePhase::Connected;
        drop(p);
        if connected {
            self.activity.enable(true);
        }
        Ok(())
    }
}
fn check_sequence(last: &mut Option<u64>, incoming: u64) -> Result<(), OnlineError> {
    if last.and_then(|last| last.checked_add(1)) != Some(incoming) {
        return Err(OnlineError::InvalidProtocol);
    }
    *last = Some(incoming);
    Ok(())
}
pub struct OnlineOwner {
    client: OnlineClient,
    worker: Option<thread::JoinHandle<()>>,
    done: mpsc::Receiver<()>,
}
impl OnlineOwner {
    pub fn start(activity: ActivitySignal) -> Result<Self, OnlineError> {
        let (control, receiver) = watch::channel(Control::default());
        let client = OnlineClient {
            control,
            published: Arc::new(Mutex::new(Published::default())),
            activity,
        };
        let worker_client = client.clone();
        let (finished, done) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("bongocat-online".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(worker_loop(worker_client.clone(), receiver)),
                    Err(_) => worker_client.reset(
                        0,
                        OnlinePhase::Failed,
                        Some(OnlineError::ServiceUnavailable),
                    ),
                }
                let _ = finished.send(());
            })
            .map_err(|_| OnlineError::ServiceUnavailable)?;
        Ok(Self {
            client,
            worker: Some(worker),
            done,
        })
    }
    pub fn client(&self) -> OnlineClient {
        self.client.clone()
    }
    pub fn shutdown(&mut self) -> Result<(), OnlineError> {
        self.client.activity.enable(false);
        self.client.control.send_replace(Control {
            stopping: true,
            endpoint: None,
        });
        if self.worker.is_some() {
            self.done
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| OnlineError::ServiceUnavailable)?;
            if let Some(worker) = self.worker.take() {
                worker.join().map_err(|_| OnlineError::ServiceUnavailable)?;
            }
        }
        Ok(())
    }
}
impl Drop for OnlineOwner {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

async fn worker_loop(client: OnlineClient, mut control: watch::Receiver<Control>) {
    let mut generation = 0;
    loop {
        let desired = control.borrow_and_update().clone();
        if desired.stopping {
            break;
        }
        let Some(endpoint) = desired.endpoint else {
            client.reset(generation, OnlinePhase::Offline, None);
            if control.changed().await.is_err() {
                break;
            }
            continue;
        };
        let mut attempts = 0usize;
        loop {
            generation += 1;
            client.reset(
                generation,
                if attempts == 0 {
                    OnlinePhase::Connecting
                } else {
                    OnlinePhase::Reconnecting
                },
                None,
            );
            let result = tokio::select! { biased;
                _ = control.changed() => break,
                result = session(&client, generation, &endpoint) => result,
            };
            client.activity.enable(false);
            let error = result.err().unwrap_or(OnlineError::ConnectionFailed);
            if matches!(error, OnlineError::Room(_) | OnlineError::InvalidProtocol) {
                client.reset(generation, OnlinePhase::Failed, Some(error));
                let _ = control.changed().await;
                break;
            }
            if client.snapshot().phase == OnlinePhase::Connected {
                attempts = 0;
            }
            client.reset(generation, OnlinePhase::Reconnecting, Some(error));
            let delay = [1, 2, 4, 8, 15][attempts.min(4)];
            attempts += 1;
            tokio::select! { biased; _ = control.changed() => break, _ = sleep(Duration::from_secs(delay)) => {} }
        }
    }
    client.reset(generation, OnlinePhase::Offline, None);
}

async fn session(
    client: &OnlineClient,
    generation: u64,
    endpoint: &str,
) -> Result<(), OnlineError> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let (mut socket, _) = timeout(
        Duration::from_secs(5),
        connect_async_with_config(endpoint, Some(config), false),
    )
    .await
    .map_err(|_| OnlineError::ConnectionFailed)?
    .map_err(|_| OnlineError::ConnectionFailed)?;
    let join = serde_json::to_string(&ClientMessage::Join {
        protocol_version: PROTOCOL_VERSION,
    })
    .map_err(|_| OnlineError::InvalidProtocol)?;
    send_message(&mut socket, Message::Text(join.into())).await?;
    let mut sequence = None;
    let mut actions = interval(ACTION_INTERVAL);
    actions.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    let mut last_received = tokio::time::Instant::now();
    let handshake_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(handshake_deadline), if sequence.is_none() => return Err(OnlineError::ConnectionFailed),
            _ = heartbeat.tick() => {
                if sequence.is_none() && tokio::time::Instant::now() >= handshake_deadline { return Err(OnlineError::ConnectionFailed); }
                if last_received.elapsed() >= HEARTBEAT_TIMEOUT { return Err(OnlineError::ConnectionFailed); }
                send_message(&mut socket, Message::Ping(Vec::new().into())).await?;
            },
            _ = actions.tick() => {
                if client.activity.take() && sequence.is_some() {
                    let action = match rand::random_range(0..3) { 0 => PawAction::Left, 1 => PawAction::Right, _ => PawAction::Both };
                    let text = serde_json::to_string(&ClientMessage::Action { action }).map_err(|_| OnlineError::InvalidProtocol)?;
                    send_message(&mut socket, Message::Text(text.into())).await?;
                }
            },
            incoming = socket.next() => {
                last_received = tokio::time::Instant::now();
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let message = serde_json::from_str(&text).map_err(|_| OnlineError::InvalidProtocol)?;
                        client.accept(generation, message, &mut sequence)?;
                    },
                    Some(Ok(Message::Ping(payload))) => { send_message(&mut socket, Message::Pong(payload)).await?; },
                    Some(Ok(Message::Pong(_))) => {},
                    _ => return Err(OnlineError::ConnectionFailed),
                }
            }
        }
    }
}

async fn send_message(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    message: Message,
) -> Result<(), OnlineError> {
    timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| OnlineError::ConnectionFailed)?
        .map_err(|_| OnlineError::ConnectionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_gaps_and_old_generations_are_rejected() {
        let (control, _) = watch::channel(Control::default());
        let client = OnlineClient {
            control,
            published: Arc::new(Mutex::new(Published::default())),
            activity: ActivitySignal::default(),
        };
        client.reset(2, OnlinePhase::Connecting, None);
        let mut sequence = None;
        let welcome = ServerMessage::Welcome {
            protocol_version: 1,
            sequence: 3,
            you: MemberId(1),
            members: vec![MemberId(1)],
        };
        assert!(client.accept(1, welcome.clone(), &mut sequence).is_err());
        assert!(client.accept(2, welcome, &mut sequence).is_ok());
        assert!(
            client
                .accept(
                    2,
                    ServerMessage::Joined {
                        sequence: 5,
                        member: MemberId(2)
                    },
                    &mut sequence
                )
                .is_err()
        );
    }
    #[test]
    fn worker_shutdown_is_bounded_while_connecting() {
        let mut owner = OnlineOwner::start(ActivitySignal::default()).unwrap();
        owner
            .client()
            .join("ws://192.0.2.1:8080/ws".into())
            .unwrap();
        assert!(owner.shutdown().is_ok());
    }
}
