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
mod socket;

/// How long the worker waits for a server acknowledgement before it reports
/// the operation as refused. Bounded here so a silent server can never pin the
/// worker past a shutdown.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the worker wakes to check its stop flag between jobs.
const JOB_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The last published multiplayer state, and the version that tells the
/// snapshot clock when it moved.
///
/// The worker is the only writer; the settings service clones the published
/// state into a snapshot and compares versions while deciding whether the
/// revision moved.
#[derive(Clone, Default)]
pub(crate) struct MultiplayerState {
    inner: Arc<Mutex<MultiplayerInner>>,
}

#[derive(Default)]
struct MultiplayerInner {
    version: u64,
    state: SettingsMultiplayer,
    /// The server identity of this connection, learned from the create ack or
    /// the first member-joined echo. Room members are matched against it.
    self_id: Option<String>,
    /// The nickname this connection joined with, the fallback for `is_self`
    /// while the server identity is still unknown.
    self_name: Option<String>,
    /// Set while waiting for the member-joined echo that names this
    /// connection's identity.
    awaiting_self_name: Option<String>,
    last_error_seq: u64,
}

impl MultiplayerState {
    pub(crate) fn snapshot(&self) -> SettingsMultiplayer {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone()
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
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        match jobs.try_recv() {
            Ok(job) => {
                handle_job(job, &mut session, &state, &runtime, &log);
            }
            Err(async_channel::TryRecvError::Empty) => {
                thread::sleep(JOB_POLL_INTERVAL);
            }
            Err(async_channel::TryRecvError::Closed) => break,
        }
    }
    if let Some(session) = session.take() {
        session.shutdown();
    }
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
