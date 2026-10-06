//! The multiplayer room worker: one thread that owns the socket.io connection
//! to the room service and publishes everything it learns into a
//! [`MultiplayerState`] the settings service reads.
//!
//! The worker owns no configuration and no business state. The settings service
//! hands it jobs with the server URL and nickname the user had when the command
//! was accepted, and the worker turns server answers into the published
//! projection. Chat lines also go straight to the runtime as bubble commands,
//! because the overlay must show them whether or not the settings window is
//! open.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use bongocat_runtime::RuntimeClient;
use bongocat_ui_protocol::{
    SettingsErrorCode, SettingsLobbyRoom, SettingsLobbyStatus, SettingsMultiplayer,
    SettingsMultiplayerError, SettingsMultiplayerStatus, SettingsRoomView,
};

use crate::app_log::{
    ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent, ApplicationLogHandle,
};

mod lobby;
mod payload;
mod peer;
mod socket;

/// How long the worker waits for a server acknowledgement before it reports
/// the operation as refused. Bounded here so a silent server can never pin the
/// worker past a shutdown.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the worker wakes to check its stop flag between jobs.
const JOB_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// The last published multiplayer state, and the version that tells the
/// snapshot clock when it moved.
///
/// The worker is the only writer; the settings service clones the published
/// state into a snapshot and compares versions while deciding whether the
/// revision moved.
#[derive(Clone, Default)]
pub(crate) struct MultiplayerState {
    signals: Arc<Mutex<std::collections::VecDeque<peer::IncomingSignal>>>,
    inner: Arc<Mutex<MultiplayerInner>>,
    pub(crate) scene: crate::RoomSceneHandle,
}

#[derive(Default)]
struct MultiplayerInner {
    version: u64,
    state: SettingsMultiplayer,
    /// The server identity supplied by the create/join acknowledgement.
    self_id: Option<String>,
    /// The nickname this connection joined with, for outgoing requests.
    self_name: Option<String>,
    last_error_seq: u64,
}

impl MultiplayerState {
    pub(crate) fn with_scene(scene: crate::RoomSceneHandle) -> Self {
        Self {
            scene,
            ..Self::default()
        }
    }
    pub(crate) fn snapshot(&self) -> SettingsMultiplayer {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut changed = false;
        if let Some(room) = &mut inner.state.room {
            for member in &mut room.members {
                let visible = member.is_self || self.scene.member_visible(&member.id);
                let progress = self
                    .scene
                    .1
                    .progress(&member.id)
                    .map(|progress| match progress {
                        bongocat_runtime::RoomModelProgress::Downloading { percent } => {
                            bongocat_ui_protocol::SettingsMemberModelProgress {
                                percent,
                                installing: false,
                            }
                        }
                        bongocat_runtime::RoomModelProgress::Installing => {
                            bongocat_ui_protocol::SettingsMemberModelProgress {
                                percent: Some(100),
                                installing: true,
                            }
                        }
                    });
                changed |= member.model_visible != visible || member.model_download != progress;
                member.model_visible = visible;
                member.model_download = progress;
            }
        }
        if changed {
            inner.version = inner.version.saturating_add(1);
        }
        inner.state.clone()
    }

    pub(crate) fn version(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .version
    }

    fn update(&self, apply: impl FnOnce(&mut MultiplayerInner)) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        apply(&mut inner);
        inner.version = inner.version.saturating_add(1);
    }

    pub(crate) fn set_status(&self, status: SettingsMultiplayerStatus) {
        self.update(|inner| inner.state.status = status);
    }

    /// The nickname the connection joined with, for the create/join payloads.
    pub(crate) fn joined_nickname(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .self_name
            .clone()
    }

    /// The room id this connection sits in, when it sits in one.
    pub(crate) fn room_id(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .room
            .as_ref()
            .map(|room| room.room_id.clone())
    }

    pub(crate) fn replace_room(&self, room: Option<SettingsRoomView>) {
        if room.is_none() {
            self.scene.clear();
        }
        self.update(|inner| inner.state.room = room);
    }

    pub(crate) fn record_error(&self, code: SettingsErrorCode) {
        self.update(|inner| {
            inner.last_error_seq = inner.last_error_seq.saturating_add(1);
            inner.state.last_error = Some(SettingsMultiplayerError {
                seq: inner.last_error_seq,
                code,
            });
        });
    }

    pub(crate) fn set_lobby(&self, status: SettingsLobbyStatus, rooms: Vec<SettingsLobbyRoom>) {
        self.update(|inner| {
            inner.state.lobby_status = status;
            inner.state.lobby = rooms;
        });
    }
}

/// One unit of work the settings service hands the worker. The server URL and
/// nickname travel in the job because configuration may change between the
/// command the window accepted and the job the worker runs.
pub(crate) enum MultiplayerJob {
    Connect {
        server_url: String,
        nickname: String,
    },
    Disconnect,
    CreateRoom {
        server_url: String,
        nickname: String,
        room_name: String,
        password: String,
    },
    JoinRoom {
        server_url: String,
        nickname: String,
        room_id: String,
        password: String,
    },
    LeaveRoom,
    SendChat {
        content: String,
    },
    KickMember {
        member_id: String,
    },
    RefreshLobby {
        server_url: String,
        nickname: String,
    },
}

/// Starts the worker thread. The service owns the join handle and the stop
/// flag; stopping is cooperative and bounded by the acknowledgement timeouts.
pub(crate) fn spawn_multiplayer_worker(
    jobs: async_channel::Receiver<MultiplayerJob>,
    state: MultiplayerState,
    runtime: RuntimeClient,
    log: ApplicationLogHandle,
    stop: Arc<AtomicBool>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("bongocat-multiplayer".to_owned())
        .spawn(move || run_worker(jobs, state, runtime, log, stop))
}

fn run_worker(
    jobs: async_channel::Receiver<MultiplayerJob>,
    state: MultiplayerState,
    runtime: RuntimeClient,
    log: ApplicationLogHandle,
    stop: Arc<AtomicBool>,
) {
    let mut session: Option<socket::RoomSession> = None;
    let mut published_model = None;
    let peers = peer::PeerService::start(runtime.clone(), state.clone(), log.clone()).ok();
    if peers.is_none() {
        log_multiplayer_degraded(
            &log,
            "peer_worker_start_failed",
            SettingsErrorCode::MultiplayerConnectFailed,
        );
    }
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let model_snapshot = runtime.snapshot();
        if let Some(session) = session.as_ref()
            && let Some(room_id) = state.room_id()
            && let Some(model) = model_snapshot.active_model
            && let Some(origin) = model_snapshot.active_model_origin
        {
            let source = match origin {
                bongocat_model::ModelOrigin::Preset => "preset",
                bongocat_model::ModelOrigin::Installed => "installed",
            };
            let share = state
                .scene
                .1
                .local()
                .filter(|share| share.id == model.id.as_str());
            let identity = (room_id, model.id.as_str().to_owned(), source, share);
            if published_model.as_ref() != Some(&identity)
                && socket::publish_model_with_share(
                    session,
                    &identity.1,
                    identity.2,
                    identity.3.as_ref(),
                )
            {
                published_model = Some(identity);
            }
        } else {
            published_model = None;
        }
        match jobs.try_recv() {
            Ok(job) => {
                handle_job(job, &mut session, &state, &runtime, &log);
                if let Some(peers) = &peers {
                    peers.set_session(session.clone());
                }
            }
            Err(async_channel::TryRecvError::Empty) => {
                thread::sleep(JOB_POLL_INTERVAL);
            }
            Err(async_channel::TryRecvError::Closed) => break,
        }
    }
    if let Some(peers) = peers
        && !peers.shutdown()
    {
        log_multiplayer_degraded(
            &log,
            "peer_worker_shutdown_failed",
            SettingsErrorCode::MultiplayerConnectFailed,
        );
    }
    if let Some(session) = session.take() {
        session.shutdown();
    }
    state.scene.clear();
}

fn handle_job(
    job: MultiplayerJob,
    session_slot: &mut Option<socket::RoomSession>,
    state: &MultiplayerState,
    runtime: &RuntimeClient,
    log: &ApplicationLogHandle,
) {
    match job {
        MultiplayerJob::Connect {
            server_url,
            nickname,
        } => {
            let Some(_session) =
                ensure_connected(session_slot, &server_url, &nickname, state, runtime, log)
            else {
                return;
            };
            state.set_lobby(SettingsLobbyStatus::Loading, Vec::new());
            match lobby::fetch_rooms(&server_url) {
                Ok(rooms) => state.set_lobby(SettingsLobbyStatus::Ready, rooms),
                Err(code) => {
                    state.set_lobby(SettingsLobbyStatus::Failed, Vec::new());
                    state.record_error(code);
                }
            }
        }
        MultiplayerJob::Disconnect => {
            if let Some(session) = session_slot.take() {
                session.shutdown();
            }
            state.set_status(SettingsMultiplayerStatus::Disconnected);
            state.replace_room(None);
            state.set_lobby(SettingsLobbyStatus::Unloaded, Vec::new());
        }
        MultiplayerJob::CreateRoom {
            server_url,
            nickname,
            room_name,
            password,
        } => {
            let Some(session) =
                ensure_connected(session_slot, &server_url, &nickname, state, runtime, log)
            else {
                return;
            };
            if socket::create_room(&session, state, &room_name, &password) {
                state.set_lobby(SettingsLobbyStatus::Loading, Vec::new());
                match lobby::fetch_rooms(&server_url) {
                    Ok(rooms) => state.set_lobby(SettingsLobbyStatus::Ready, rooms),
                    Err(code) => {
                        state.set_lobby(SettingsLobbyStatus::Failed, Vec::new());
                        state.record_error(code);
                    }
                }
            }
        }
        MultiplayerJob::JoinRoom {
            server_url,
            nickname,
            room_id,
            password,
        } => {
            let Some(session) =
                ensure_connected(session_slot, &server_url, &nickname, state, runtime, log)
            else {
                return;
            };
            socket::join_room(&session, state, &room_id, &password);
        }
        MultiplayerJob::LeaveRoom => {
            if let Some(session) = session_slot.as_ref() {
                socket::leave_room(session, state);
            } else {
                state.replace_room(None);
            }
        }
        MultiplayerJob::SendChat { content } => {
            match session_slot.as_ref() {
                Some(session) => socket::send_chat(session, state, &content),
                None => state.record_error(SettingsErrorCode::MultiplayerNotInRoom),
            };
        }
        MultiplayerJob::KickMember { member_id } => match session_slot.as_ref() {
            Some(session) => socket::kick_member(session, state, &member_id),
            None => state.record_error(SettingsErrorCode::MultiplayerNotInRoom),
        },
        MultiplayerJob::RefreshLobby {
            server_url,
            nickname,
        } => {
            let Some(_session) =
                ensure_connected(session_slot, &server_url, &nickname, state, runtime, log)
            else {
                state.set_lobby(SettingsLobbyStatus::Failed, Vec::new());
                return;
            };
            state.set_lobby(SettingsLobbyStatus::Loading, Vec::new());
            match lobby::fetch_rooms(&server_url) {
                Ok(rooms) => state.set_lobby(SettingsLobbyStatus::Ready, rooms),
                Err(code) => {
                    state.set_lobby(SettingsLobbyStatus::Failed, Vec::new());
                    state.record_error(code);
                }
            }
        }
    }
}

/// Return the live session, connecting first when there is none or when the
/// server URL changed. A failed connect leaves the slot empty so the next job
/// tries again from scratch.
fn ensure_connected(
    session_slot: &mut Option<socket::RoomSession>,
    server_url: &str,
    nickname: &str,
    state: &MultiplayerState,
    runtime: &RuntimeClient,
    log: &ApplicationLogHandle,
) -> Option<socket::RoomSession> {
    if let Some(existing) = session_slot.as_ref() {
        if existing.serves(server_url) {
            return Some(existing.clone());
        }
        // The user pointed the page at a different service; drop the old
        // connection before opening the new one.
        existing.shutdown();
        *session_slot = None;
        state.replace_room(None);
    }
    state.set_status(SettingsMultiplayerStatus::Connecting);
    match socket::connect(server_url, nickname, state, runtime, log) {
        Ok(session) => {
            state.set_status(SettingsMultiplayerStatus::Connected);
            *session_slot = Some(session.clone());
            Some(session)
        }
        Err(code) => {
            log_multiplayer_degraded(log, "connect", code);
            state.set_status(SettingsMultiplayerStatus::Failed(code));
            None
        }
    }
}

pub(crate) fn log_multiplayer_degraded(
    log: &ApplicationLogHandle,
    operation: &'static str,
    code: SettingsErrorCode,
) {
    log.record(
        ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
            .with_context(ApplicationLogContext::Service("multiplayer"))
            .with_context(ApplicationLogContext::Operation(operation))
            .with_context(ApplicationLogContext::Reason(code.as_str())),
    );
}

pub(crate) fn log_multiplayer_recovered(log: &ApplicationLogHandle) {
    log.record(
        ApplicationLogEvent::new(ApplicationLogCode::ServiceRecovered)
            .with_context(ApplicationLogContext::Service("multiplayer")),
    );
}

/// Map a server refusal message to a stable settings error code.
///
/// The service answers `ok:false` with a human-readable string and no code, so
/// the worker matches the known phrases and falls back to the generic refusal;
/// the raw text never travels into the projection, which keeps what the window
/// shows localized and the detail in the log.
pub(crate) fn server_error_code(raw: &str) -> SettingsErrorCode {
    let raw = raw.trim();
    if raw.contains("不存在") {
        SettingsErrorCode::MultiplayerRoomNotFound
    } else if raw.contains("密码") {
        SettingsErrorCode::MultiplayerRoomPasswordWrong
    } else if raw.contains("已满") || raw.contains("满员") {
        SettingsErrorCode::MultiplayerRoomFull
    } else if raw.contains("已在") || raw.contains("已经在") {
        SettingsErrorCode::MultiplayerAlreadyInRoom
    } else if raw.contains("过快") || raw.contains("频繁") {
        SettingsErrorCode::MultiplayerChatRateLimited
    } else {
        SettingsErrorCode::MultiplayerServerRefused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a room service via BONGOCAT_TEST_SERVER_URL and real WebRTC model channels"]
    fn two_clients_transfer_custom_model_over_separate_channel() {
        use crate::room_assets::Advertisement;
        use bongocat_config::{ModelInputMode, StorageLayout};
        let url = std::env::var("BONGOCAT_TEST_SERVER_URL").unwrap();
        let temp = tempfile::tempdir().unwrap();
        let log = ApplicationLogHandle::install(temp.path()).unwrap();
        let host_runtime = bongocat_runtime::RuntimeOwner::start(false, 128);
        let guest_runtime = bongocat_runtime::RuntimeOwner::start(false, 128);
        let host = MultiplayerState::default();
        let guest = MultiplayerState::default();
        host.scene.1.configure(temp.path().join("host"));
        guest.scene.1.configure(temp.path().join("guest"));
        let host_session =
            socket::connect(&url, "model-host", &host, &host_runtime.client(), &log).unwrap();
        assert!(socket::create_room(
            &host_session,
            &host,
            "model-channel-test",
            ""
        ));
        let ad = Advertisement {
            id: "shared-model".into(),
            title: "P2P cat".into(),
            input_mode: ModelInputMode::Standard,
            library_url: None,
            cache_id: None,
        };
        host.scene.1.set_local(Some((
            ad.clone(),
            crate::tests::repository_preset_root().join("standard"),
        )));
        let host_peer =
            peer::PeerService::start(host_runtime.client(), host.clone(), log.clone()).unwrap();
        host_peer.set_session(Some(host_session.clone()));
        for _ in 0..200 {
            if host.scene.1.local().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(host.scene.1.local().is_some());
        assert!(socket::publish_model_with_share(
            &host_session,
            &ad.id,
            "installed",
            Some(&ad)
        ));
        let guest_session =
            socket::connect(&url, "model-guest", &guest, &guest_runtime.client(), &log).unwrap();
        socket::join_room(&guest_session, &guest, &host.room_id().unwrap(), "");
        assert!(guest.snapshot().room.is_some());
        let guest_peer =
            peer::PeerService::start(guest_runtime.client(), guest.clone(), log).unwrap();
        guest_peer.set_session(Some(guest_session.clone()));
        let mut acquired = None;
        for _ in 0..1500 {
            acquired = guest.scene.1.acquired().pop();
            if acquired.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut app = crate::Application::start_with_layout(StorageLayout::under(
            temp.path().join("library"),
            crate::BUILD_ENVIRONMENT,
        ))
        .unwrap();
        let imported = acquired
            .as_ref()
            .map(|item| app.import_room_model(item, &guest.scene.1));
        let started = std::time::Instant::now();
        assert!(guest_peer.shutdown());
        assert!(host_peer.shutdown());
        let shutdown = started.elapsed();
        socket::leave_room(&guest_session, &guest);
        guest_session.shutdown();
        socket::leave_room(&host_session, &host);
        host_session.shutdown();
        guest_runtime.shutdown(Duration::from_secs(2)).unwrap();
        host_runtime.shutdown(Duration::from_secs(2)).unwrap();
        let catalog = app.config().model.imported_models.clone();
        app.shutdown().unwrap();
        assert!(
            shutdown < Duration::from_secs(2),
            "model workers must join within budget"
        );
        let id = imported
            .expect("model bytes arrived over actual data channel")
            .unwrap();
        assert_ne!(id, ad.id);
        assert!(
            catalog
                .iter()
                .any(|record| record.id == id && record.title == ad.title)
        );
    }

    #[test]
    #[ignore = "requires a local room service via BONGOCAT_TEST_SERVER_URL"]
    fn two_clients_join_with_host_model_and_same_nickname() {
        let url = std::env::var("BONGOCAT_TEST_SERVER_URL").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let log = ApplicationLogHandle::install(directory.path()).unwrap();
        let runtime = bongocat_runtime::RuntimeOwner::start(false, 64);
        let host = MultiplayerState::default();
        let guest = MultiplayerState::default();
        let host_session = socket::connect(&url, "same", &host, &runtime.client(), &log).unwrap();
        assert!(socket::create_room(
            &host_session,
            &host,
            "join-regression",
            ""
        ));
        assert!(socket::publish_model(&host_session, "standard", "preset"));
        std::thread::sleep(Duration::from_millis(200));
        let room_id = host.room_id().unwrap();
        let guest_session = socket::connect(&url, "same", &guest, &runtime.client(), &log).unwrap();
        socket::join_room(&guest_session, &guest, &room_id, "");
        let snapshot = guest.snapshot();
        socket::leave_room(&guest_session, &guest);
        guest_session.shutdown();
        socket::leave_room(&host_session, &host);
        host_session.shutdown();
        runtime.shutdown(Duration::from_secs(2)).unwrap();
        assert!(snapshot.last_error.is_none(), "{:?}", snapshot.last_error);
        let room = snapshot.room.expect("guest entered room");
        assert_eq!(room.members.len(), 2);
        assert!(!room.self_member().unwrap().is_host);
    }

    #[test]
    #[ignore = "requires a room service via BONGOCAT_TEST_SERVER_URL and opens real WebRTC sockets"]
    fn two_clients_peer_workers_deliver_release_and_join_on_shutdown() {
        use bongocat_input::{InputControl, InputEdge, InputEvent, InputSource, PhysicalKey};
        let url = std::env::var("BONGOCAT_TEST_SERVER_URL").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let log = ApplicationLogHandle::install(directory.path()).unwrap();
        let host_runtime = bongocat_runtime::RuntimeOwner::start(false, 256);
        let guest_runtime = bongocat_runtime::RuntimeOwner::start(false, 256);
        let remote_runtime = bongocat_runtime::RuntimeOwner::start(false, 256);
        let host = MultiplayerState::default();
        let guest = MultiplayerState::default();
        let host_session =
            socket::connect(&url, "worker-host", &host, &host_runtime.client(), &log).unwrap();
        assert!(socket::create_room(
            &host_session,
            &host,
            "worker-regression",
            ""
        ));
        let guest_session =
            socket::connect(&url, "worker-guest", &guest, &guest_runtime.client(), &log).unwrap();
        socket::join_room(&guest_session, &guest, &host.room_id().unwrap(), "");
        let guest_id = guest
            .snapshot()
            .room
            .unwrap()
            .self_member()
            .unwrap()
            .id
            .clone();
        host.scene
            .register_test_client(guest_id, remote_runtime.client());
        let host_worker =
            peer::PeerService::start(host_runtime.client(), host.clone(), log.clone()).unwrap();
        let guest_worker =
            peer::PeerService::start(guest_runtime.client(), guest.clone(), log).unwrap();
        host_worker.set_session(Some(host_session.clone()));
        guest_worker.set_session(Some(guest_session.clone()));
        std::thread::sleep(Duration::from_millis(100));
        for (edge, expected) in [(InputEdge::Down, 1), (InputEdge::Up, 0)] {
            let client = guest_runtime.client();
            client
                .input_producer()
                .publish(InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::KEY_A),
                    edge,
                    source: InputSource::Capture,
                    at: client.input_timestamp(),
                })
                .unwrap();
            for _ in 0..1500 {
                if remote_runtime.client().snapshot().input.pressed_key_count == expected {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                remote_runtime.client().snapshot().input.pressed_key_count,
                expected
            );
        }
        let shutdown_started = std::time::Instant::now();
        assert!(guest_worker.shutdown());
        assert!(host_worker.shutdown());
        assert!(shutdown_started.elapsed() < Duration::from_secs(4));
        socket::leave_room(&guest_session, &guest);
        guest_session.shutdown();
        socket::leave_room(&host_session, &host);
        host_session.shutdown();
        for runtime in [host_runtime, guest_runtime, remote_runtime] {
            runtime.shutdown(Duration::from_secs(2)).unwrap();
        }
    }

    #[test]
    #[ignore = "requires a room service via BONGOCAT_TEST_SERVER_URL and opens real WebRTC sockets"]
    fn two_clients_webrtc_routes_edges_cursor_and_disconnect_reset() {
        use bongocat_input::{InputControl, InputEdge, InputEvent, InputSource, PhysicalKey};
        let url = std::env::var("BONGOCAT_TEST_SERVER_URL").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let log = ApplicationLogHandle::install(directory.path()).unwrap();
        let host_runtime = bongocat_runtime::RuntimeOwner::start(false, 256);
        let guest_runtime = bongocat_runtime::RuntimeOwner::start(false, 256);
        let host_remote = bongocat_runtime::RuntimeOwner::start(false, 256);
        let guest_remote = bongocat_runtime::RuntimeOwner::start(false, 256);
        let model = Arc::new(
            bongocat_model::PresetModelCatalog::open(
                crate::tests::repository_preset_root(),
                bongocat_model::ModelPackageLimits::default(),
            )
            .unwrap()
            .load(&bongocat_model::ModelId::parse("standard").unwrap())
            .unwrap(),
        );
        let bindings = Arc::new(crate::model_input::input_bindings_for_committed_model(
            &model,
        ));
        for client in [host_remote.client(), guest_remote.client()] {
            let sequence = client
                .send(
                    bongocat_runtime::RuntimeCommand::ActivateModelWithBindings {
                        model: Arc::clone(&model),
                        input_bindings: Arc::clone(&bindings),
                    },
                )
                .unwrap();
            client
                .wait_for_command(sequence, Duration::from_secs(2))
                .unwrap();
        }
        let host = MultiplayerState::default();
        let guest = MultiplayerState::default();
        let host_session =
            socket::connect(&url, "same", &host, &host_runtime.client(), &log).unwrap();
        assert!(socket::create_room(
            &host_session,
            &host,
            "webrtc-regression",
            ""
        ));
        let room_id = host.room_id().unwrap();
        let guest_session =
            socket::connect(&url, "same", &guest, &guest_runtime.client(), &log).unwrap();
        socket::join_room(&guest_session, &guest, &room_id, "");
        let guest_id = guest
            .snapshot()
            .room
            .unwrap()
            .self_member()
            .unwrap()
            .id
            .clone();
        let host_id = host
            .snapshot()
            .room
            .unwrap()
            .self_member()
            .unwrap()
            .id
            .clone();
        host.scene
            .register_test_client(guest_id, host_remote.client());
        guest
            .scene
            .register_test_client(host_id, guest_remote.client());
        let mut host_network = peer::PeerNetwork::new(host_runtime.client());
        let mut guest_network = peer::PeerNetwork::new(guest_runtime.client());
        let pump = |host_network: &mut peer::PeerNetwork, guest_network: &mut peer::PeerNetwork| {
            host_network.tick(Some(&host_session), &host).unwrap();
            guest_network.tick(Some(&guest_session), &guest).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        };
        for _ in 0..2000 {
            pump(&mut host_network, &mut guest_network);
            if host_network.connected_members() == 1 && guest_network.connected_members() == 1 {
                break;
            }
        }
        assert_eq!(
            host_network.connected_members(),
            1,
            "host WebRTC channel opened"
        );
        assert_eq!(
            guest_network.connected_members(),
            1,
            "guest WebRTC channel opened"
        );
        let guest_client = guest_runtime.client();
        guest_client
            .cursor_producer()
            .publish(
                bongocat_input::CursorSample::new(
                    bongocat_input::CursorPosition { x: 25.0, y: 75.0 },
                    bongocat_input::CursorViewport {
                        origin: bongocat_input::CursorPosition { x: 0.0, y: 0.0 },
                        width: 100.0,
                        height: 100.0,
                    },
                    guest_client.input_timestamp(),
                )
                .unwrap(),
            )
            .unwrap();
        for _ in 0..200 {
            pump(&mut host_network, &mut guest_network);
            if host_remote.client().snapshot().cursor.sample.is_some() {
                break;
            }
        }
        let position = host_remote
            .client()
            .snapshot()
            .cursor
            .sample
            .expect("remote cursor")
            .normalized();
        assert_eq!((position.x, position.y), (0.5, -0.5));
        let host_client = host_runtime.client();
        host_client
            .input_producer()
            .publish(InputEvent::Edge {
                control: InputControl::Mouse(bongocat_input::MouseButton::Left),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: host_client.input_timestamp(),
            })
            .unwrap();
        for _ in 0..200 {
            pump(&mut host_network, &mut guest_network);
            if guest_remote
                .client()
                .snapshot()
                .input
                .pressed_mouse_button_count
                == 1
            {
                break;
            }
        }
        assert_eq!(
            guest_remote
                .client()
                .snapshot()
                .input
                .pressed_mouse_button_count,
            1
        );
        for (edge, expected) in [
            (InputEdge::Down, 1),
            (InputEdge::Up, 0),
            (InputEdge::Down, 1),
        ] {
            let client = guest_runtime.client();
            client
                .input_producer()
                .publish(InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::KEY_A),
                    edge,
                    source: InputSource::Capture,
                    at: client.input_timestamp(),
                })
                .unwrap();
            for _ in 0..200 {
                pump(&mut host_network, &mut guest_network);
                if host_remote.client().snapshot().input.pressed_key_count == expected {
                    break;
                }
            }
            assert_eq!(
                host_remote.client().snapshot().input.pressed_key_count,
                expected
            );
            assert_eq!(
                host_remote.client().snapshot().model_input.left_hand_down,
                expected == 1
            );
            assert_eq!(host_runtime.client().snapshot().input.pressed_key_count, 0);
        }
        guest_network.shutdown();
        for _ in 0..200 {
            host_network.tick(Some(&host_session), &host).unwrap();
            if host_remote.client().snapshot().input.pressed_key_count == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            host_remote.client().snapshot().input.pressed_key_count,
            0,
            "disconnect clears held key"
        );
        host_network.shutdown();
        socket::leave_room(&guest_session, &guest);
        guest_session.shutdown();
        socket::leave_room(&host_session, &host);
        host_session.shutdown();
        for runtime in [host_runtime, guest_runtime, host_remote, guest_remote] {
            runtime.shutdown(Duration::from_secs(2)).unwrap();
        }
    }

    #[test]
    #[ignore = "requires a local bango-server via BONGOCAT_TEST_SERVER_URL"]
    fn create_room_enters_as_host_and_refreshes_lobby_against_server() {
        use bongocat_runtime::RuntimeOwner;
        let server_url = std::env::var("BONGOCAT_TEST_SERVER_URL").expect("test server URL");
        let directory = tempfile::tempdir().expect("temporary logs");
        let log = ApplicationLogHandle::install(directory.path()).expect("test log");
        let runtime = RuntimeOwner::start(false, 16);
        let state = MultiplayerState::default();
        let mut session = None;
        handle_job(
            MultiplayerJob::CreateRoom {
                server_url: server_url.clone(),
                nickname: "regression-host".to_owned(),
                room_name: "自动入房回归测试".to_owned(),
                password: String::new(),
            },
            &mut session,
            &state,
            &runtime.client(),
            &log,
        );
        // Clean up the actual socket before assertions, even if create failed.
        let snapshot = state.snapshot();
        let session = session.expect("connected session");
        socket::leave_room(&session, &state);
        session.shutdown();
        runtime
            .shutdown(Duration::from_secs(1))
            .expect("runtime shutdown");

        assert!(snapshot.last_error.is_none(), "{:?}", snapshot.last_error);
        let room = snapshot
            .room
            .expect("create automatically entered the room");
        assert!(room.self_member().expect("creator identity").is_host);
        assert_eq!(room.member_count, 1);
        assert_eq!(snapshot.lobby_status, SettingsLobbyStatus::Ready);
        let row = snapshot
            .lobby
            .iter()
            .find(|row| row.room_id == room.room_id)
            .expect("automatically fetched lobby contains the created room");
        assert_eq!(row.member_count, 1);
        assert_eq!(row.host_name, "regression-host");
        assert!(
            !lobby::fetch_rooms(&server_url)
                .expect("lobby after leave")
                .iter()
                .any(|row| row.room_id == room.room_id)
        );
    }

    #[test]
    fn server_refusals_map_to_stable_codes() {
        assert_eq!(
            server_error_code("房间不存在"),
            SettingsErrorCode::MultiplayerRoomNotFound
        );
        assert_eq!(
            server_error_code("房间密码错误"),
            SettingsErrorCode::MultiplayerRoomPasswordWrong
        );
        assert_eq!(
            server_error_code("房间人数已满"),
            SettingsErrorCode::MultiplayerRoomFull
        );
        assert_eq!(
            server_error_code("你已在其他房间中"),
            SettingsErrorCode::MultiplayerAlreadyInRoom
        );
        assert_eq!(
            server_error_code("发送过快，请稍后再试"),
            SettingsErrorCode::MultiplayerChatRateLimited
        );
        assert_eq!(
            server_error_code("something unexpected"),
            SettingsErrorCode::MultiplayerServerRefused
        );
    }

    #[test]
    fn error_recording_bumps_the_sequence_so_the_window_sees_each_once() {
        let state = MultiplayerState::default();
        assert!(state.snapshot().last_error.is_none());
        state.record_error(SettingsErrorCode::MultiplayerConnectFailed);
        state.record_error(SettingsErrorCode::MultiplayerRoomFull);
        let snapshot = state.snapshot();
        let error = snapshot.last_error.expect("last error");
        assert_eq!(error.seq, 2);
        assert_eq!(error.code, SettingsErrorCode::MultiplayerRoomFull);
    }

    #[test]
    fn lobby_publication_carries_its_status() {
        let state = MultiplayerState::default();
        state.set_lobby(
            SettingsLobbyStatus::Ready,
            vec![SettingsLobbyRoom {
                room_id: "K7XQ2M".to_owned(),
                name: "测试房".to_owned(),
                member_count: 3,
                max_members: 8,
                has_password: true,
                host_name: "host".to_owned(),
            }],
        );
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lobby_status, SettingsLobbyStatus::Ready);
        assert_eq!(snapshot.lobby.len(), 1);
        assert_eq!(snapshot.lobby[0].room_id, "K7XQ2M");
    }
}
