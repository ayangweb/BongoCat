//! The socket.io half of the multiplayer worker: connect, the server-event
//! handlers, and the request helpers the jobs call.
//!
//! The client runs on its own transport thread, so every handler here is a
//! small closure that publishes into [`MultiplayerState`]. Requests use
//! acknowledgements: the worker blocks on a bounded channel until the server
//! answers or [`ACK_TIMEOUT`] expires, which keeps a silent server from pinning
//! the worker past a shutdown.

use std::sync::mpsc;

use bongocat_runtime::{RuntimeClient, RuntimeCommand};
use bongocat_ui_protocol::{SettingsChatMessage, SettingsErrorCode, SettingsMultiplayerStatus};
use rust_socketio::{Event, Payload, RawClient, TransportType, client::Client};

use super::{
    ACK_TIMEOUT, MultiplayerState, log_multiplayer_degraded, log_multiplayer_recovered, payload,
    server_error_code,
};
use crate::app_log::ApplicationLogHandle;

/// The namespace the room service lives under.
const NAMESPACE: &str = "/bangocat";

/// One live connection to the room service. Cloneable; the clone shares the
/// same socket.
#[derive(Clone)]
pub(crate) struct RoomSession {
    server_url: String,
    client: Client,
}

impl RoomSession {
    pub(super) fn signal(&self, member_id: &str, signal: &super::peer::Signal) {
        let _ = self.client.emit(
            "peer:signal",
            serde_json::json!({ "to": member_id, "data": signal }),
        );
    }
    /// Whether this session already serves the URL a job was built for.
    pub(crate) fn serves(&self, server_url: &str) -> bool {
        self.server_url == server_url
    }

    /// Politely leave the room (if any) and close the socket. Every failure is
    /// swallowed: the connection is going away either way, and the next join
    /// starts a fresh session.
    pub(crate) fn shutdown(&self) {
        let _ = self
            .client
            .emit("room:leave", Payload::Text(vec![serde_json::json!({})]));
        let _ = self.client.disconnect();
    }
}

/// Open a connection to the room service and register the broadcast handlers.
///
/// The caller sets the connected status once this returns; the close and error
/// handlers are what return the projection to disconnected afterwards. The
/// nickname is remembered for outgoing requests; acknowledgements identify self.
pub(crate) fn connect(
    server_url: &str,
    nickname: &str,
    state: &MultiplayerState,
    runtime: &RuntimeClient,
    log: &ApplicationLogHandle,
) -> Result<RoomSession, SettingsErrorCode> {
    // The server requires a non-empty display name. Keep an explicit fallback
    // for installations where the nickname setting has not been configured.
    let nickname = if nickname.trim().is_empty() {
        "匿名用户"
    } else {
        nickname.trim()
    };
    let (connected_sender, connected_receiver) = mpsc::channel();
    let connected_log = log.clone();
    let builder = rust_socketio::ClientBuilder::new(server_url)
        .namespace(NAMESPACE)
        .transport_type(TransportType::Websocket)
        .reconnect(false)
        .on(
            Event::Connect,
            move |_payload: Payload, _client: RawClient| {
                log_multiplayer_recovered(&connected_log);
                let _ = connected_sender.send(());
            },
        );
    let closed_state = state.clone();
    let builder = builder.on(
        Event::Close,
        move |_payload: Payload, _client: RawClient| {
            handle_disconnected(&closed_state);
        },
    );
    let errored_log = log.clone();
    let errored_state = state.clone();
    let builder = builder.on(Event::Error, move |payload: Payload, _client: RawClient| {
        // A socket-level failure: transport, TLS or a malformed URL. The text
        // is a client-side error, not a server refusal, so it goes to the log
        // under one stable reason and the projection only loses the socket.
        log_multiplayer_degraded(&errored_log, "socket_error", socket_error_code(&payload));
        handle_disconnected(&errored_state);
    });
    let chat_state = state.clone();
    let chat_runtime = runtime.clone();
    let builder = builder.on("room:chat", move |payload: Payload, _client: RawClient| {
        handle_chat(payload, &chat_state, &chat_runtime);
    });
    let joined_state = state.clone();
    let builder = builder.on(
        "room:member-joined",
        move |payload: Payload, _client: RawClient| {
            handle_member_joined(payload, &joined_state);
        },
    );
    let left_state = state.clone();
    let builder = builder.on(
        "room:member-left",
        move |payload: Payload, _client: RawClient| {
            handle_member_left(payload, &left_state);
        },
    );
    let kicked_state = state.clone();
    let builder = builder.on(
        "room:kicked",
        move |_payload: Payload, _client: RawClient| {
            handle_kicked(&kicked_state);
        },
    );
    let model_state = state.clone();
    let signal_state = state.clone();
    let builder = builder.on(
        "peer:signal",
        move |payload: Payload, _client: RawClient| {
            let Some(value) = payload_value(payload) else {
                return;
            };
            if value.to_string().len() > 32768 {
                return;
            }
            let Some(signal) = payload::parse_body::<super::peer::IncomingSignal>(&value) else {
                return;
            };
            let Some(room) = signal_state.snapshot().room else {
                return;
            };
            if signal.room_id != room.room_id
                || !room
                    .members
                    .iter()
                    .any(|member| member.id == signal.from && !member.is_self)
            {
                return;
            }
            let mut signals = signal_state
                .signals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if signals.len() < 128 {
                signals.push_back(signal);
            }
        },
    );
    let builder = builder.on(
        "room:model-updated",
        move |payload: Payload, _client: RawClient| {
            handle_model_updated(payload, &model_state);
        },
    );
    match builder.connect() {
        Ok(client) => {
            // connect() starts polling but returns before the namespace's
            // Connect packet arrives. Emitting immediately can fail before open.
            if connected_receiver.recv_timeout(ACK_TIMEOUT).is_err() {
                let _ = client.disconnect();
                return Err(SettingsErrorCode::MultiplayerConnectFailed);
            }
            state.update(|inner| {
                inner.self_id = None;
                inner.self_name = Some(nickname.to_owned());
            });
            Ok(RoomSession {
                server_url: server_url.to_owned(),
                client,
            })
        }
        Err(_) => Err(SettingsErrorCode::MultiplayerConnectFailed),
    }
}

/// A request that never reached the server, or whose acknowledgement never
/// arrived. Both read the same to the user: the operation did not happen.
enum RequestFailure {
    Emit,
    Acknowledgement,
}

/// One server answer: either the payload the handler wanted or a stable code.
enum AckOutcome {
    Ok(serde_json::Value),
    Refused(SettingsErrorCode),
}

/// Send one request and wait for its acknowledgement, bounded by
/// [`ACK_TIMEOUT`]. The socket's transport thread runs the callback, so the
/// worker only ever blocks on the receive side.
fn request(
    client: &Client,
    event: &'static str,
    body: serde_json::Value,
) -> Result<AckOutcome, RequestFailure> {
    let (sender, receiver) = mpsc::channel();
    client
        .emit_with_ack(
            event,
            Payload::Text(vec![body]),
            ACK_TIMEOUT,
            move |payload: Payload, _client: RawClient| {
                let _ = sender.send(ack_value(payload));
            },
        )
        .map_err(|_| RequestFailure::Emit)?;
    receiver
        .recv_timeout(ACK_TIMEOUT)
        .map_err(|_| RequestFailure::Acknowledgement)
}

/// Decode one acknowledgement payload into the frame the service sends:
/// `{ ok, error?, ...business fields }`.
fn ack_value(payload: Payload) -> AckOutcome {
    let value = match payload_value(payload) {
        // rust_socketio 0.6 passes the complete acknowledgement arguments
        // array as one text value; broadcasts already contain the arguments.
        Some(serde_json::Value::Array(mut arguments)) if arguments.len() == 1 => {
            arguments.remove(0)
        }
        Some(value) if value.is_object() => value,
        None => return AckOutcome::Refused(SettingsErrorCode::MultiplayerResponseInvalid),
        _ => return AckOutcome::Refused(SettingsErrorCode::MultiplayerResponseInvalid),
    };
    match value.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => AckOutcome::Ok(value),
        _ => {
            let message = value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            AckOutcome::Refused(server_error_code(message))
        }
    }
}

/// The leave-everything pair every disconnect path shares: the socket is gone,
/// so room membership is gone with it. Members are identified by their
/// connection, which is why no reconnection is attempted here.
fn handle_disconnected(state: &MultiplayerState) {
    state.scene.clear();
    state.update(|inner| {
        inner.state.status = SettingsMultiplayerStatus::Disconnected;
        inner.state.room = None;
        inner.self_id = None;
        inner.self_name = None;
    });
}

fn handle_chat(payload: Payload, state: &MultiplayerState, runtime: &RuntimeClient) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(chat) = payload::parse_body::<payload::ServerChat>(&value) else {
        return;
    };
    let room = state.snapshot().room;
    if room
        .as_ref()
        .is_none_or(|room| room.room_id != chat.room_id)
        || chat
            .content
            .chars()
            .take(bongocat_ui_protocol::MAXIMUM_CHAT_CONTENT_CHARS + 1)
            .count()
            > bongocat_ui_protocol::MAXIMUM_CHAT_CONTENT_CHARS
    {
        return;
    }
    let Some(member) = room
        .as_ref()
        .and_then(|room| room.members.iter().find(|member| member.id == chat.from_id))
    else {
        return;
    };
    if member.is_self {
        let _ = runtime.send(RuntimeCommand::ShowChatBubble {
            sender: chat.from_name.clone(),
            content: chat.content.clone(),
        });
    } else {
        state
            .scene
            .show_chat(&chat.from_id, chat.from_name.clone(), chat.content.clone());
    }
    state.update(|inner| {
        let is_self = inner.self_id.as_deref() == Some(chat.from_id.as_str());
        inner.state.push_chat(SettingsChatMessage {
            sender: chat.from_name.clone(),
            content: chat.content.clone(),
            sent_at: chat.sent_at,
            is_self,
        });
    });
}

fn handle_member_joined(payload: Payload, state: &MultiplayerState) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(joined) = payload::parse_body::<payload::ServerMemberJoined>(&value) else {
        return;
    };
    let Some(member) = joined.member else {
        return;
    };
    state.update(|inner| {
        if let Some(room) = &mut inner.state.room {
            let known = room.members.iter().any(|existing| existing.id == member.id);
            if !known {
                room.members
                    .push(payload::project_member(&member, inner.self_id.as_deref()));
                room.member_count = room.members.len();
            }
        }
    });
    adopt_model_share(state, &member.id, member.model.as_ref());
}

fn handle_member_left(payload: Payload, state: &MultiplayerState) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(left) = payload::parse_body::<payload::ServerMemberLeft>(&value) else {
        return;
    };
    state.scene.remove_member(&left.member_id);
    state.update(|inner| {
        let was_self = inner
            .self_id
            .as_deref()
            .is_some_and(|id| id == left.member_id);
        if was_self {
            inner.state.room = None;
            return;
        }
        if let Some(room) = &mut inner.state.room {
            room.members.retain(|member| member.id != left.member_id);
            room.member_count = room.members.len();
            if let Some(new_host) = &left.new_host_id {
                for member in &mut room.members {
                    member.is_host = *new_host == member.id;
                }
            }
        }
    });
}

fn handle_kicked(state: &MultiplayerState) {
    state.scene.clear();
    state.update(|inner| {
        inner.state.room = None;
        inner.self_id = None;
    });
    state.record_error(SettingsErrorCode::MultiplayerKicked);
}

fn handle_model_updated(payload: Payload, state: &MultiplayerState) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(update) = payload::parse_body::<payload::ServerModelUpdated>(&value) else {
        return;
    };
    state.update(|inner| {
        if let Some(room) = &mut inner.state.room
            && let Some(member) = room
                .members
                .iter_mut()
                .find(|member| member.id == update.member_id)
        {
            member.model_name = update.model.as_ref().and_then(|model| model.name.clone());
            member.model_key = update.model.as_ref().and_then(payload::ServerModel::key);
        }
    });
    adopt_model_share(state, &update.member_id, update.model.as_ref());
}

fn adopt_model_share(
    state: &MultiplayerState,
    member_id: &str,
    model: Option<&payload::ServerModel>,
) {
    let Some(room) = state.snapshot().room else {
        return;
    };
    state.scene.1.room(
        Some(&room.room_id),
        room.members
            .iter()
            .filter(|member| !member.is_self)
            .take(32)
            .map(|member| member.id.clone())
            .collect(),
    );
    if let Some(model) = model
        && let Some(key) = model.key()
        && key.origin == bongocat_ui_protocol::SettingsModelOrigin::Imported
        && let Some(share) = model.meta.as_ref().and_then(|meta| meta.share.clone())
        && share.id == key.id
    {
        state
            .scene
            .1
            .advertise_remote(&room.room_id, member_id, share);
    }
}

/// Decode a broadcast payload into its JSON body. Socket.io text payloads wrap
/// each argument as one JSON string.
fn payload_value(payload: Payload) -> Option<serde_json::Value> {
    match payload {
        Payload::Text(parts) => parts.into_iter().next(),
        _ => None,
    }
}

fn socket_error_code(_payload: &Payload) -> SettingsErrorCode {
    SettingsErrorCode::MultiplayerConnectFailed
}

/// Create a room and join it as its host. The create acknowledgement names the
/// host id, which is this connection's identity from then on.
pub(crate) fn create_room(
    session: &RoomSession,
    state: &MultiplayerState,
    room_name: &str,
    password: &str,
) -> bool {
    let mut body = serde_json::json!({});
    if let Some(nickname) = state.joined_nickname() {
        body["name"] = serde_json::Value::String(nickname);
    }
    if !room_name.is_empty() {
        body["roomName"] = serde_json::Value::String(room_name.to_owned());
    }
    if !password.is_empty() {
        body["password"] = serde_json::Value::String(password.to_owned());
    }
    match request(&session.client, "room:create", body) {
        Ok(AckOutcome::Ok(value)) => adopt_created_room(state, &value),
        Ok(AckOutcome::Refused(code)) => {
            state.record_error(code);
            false
        }
        Err(_) => {
            state.record_error(SettingsErrorCode::MultiplayerServerRefused);
            false
        }
    }
}

/// Publish membership before returning success to the worker that fetches
/// the lobby. A malformed success must not leave a phantom room in the UI.
fn adopt_created_room(state: &MultiplayerState, acknowledgement: &serde_json::Value) -> bool {
    match acknowledgement.get("room") {
        Some(room) => adopt_room(state, room, true, None),
        None => {
            state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
            false
        }
    }
}

/// Join a room using the member identity supplied in the acknowledgement.
pub(crate) fn join_room(
    session: &RoomSession,
    state: &MultiplayerState,
    room_id: &str,
    password: &str,
) {
    let mut body = serde_json::json!({ "roomId": room_id });
    // The deployed service can omit memberId. Its model echo gives an exact
    // request identity without relying on nicknames or member ordering.
    static JOIN_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = JOIN_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let join_token = format!("{}-{nonce}-{sequence}", std::process::id());
    body["model"] = serde_json::json!({ "meta": { "join_token": join_token } });
    if let Some(nickname) = state.joined_nickname() {
        body["name"] = serde_json::Value::String(nickname);
    }
    if !password.is_empty() {
        body["password"] = serde_json::Value::String(password.to_owned());
    }
    match request(&session.client, "room:join", body) {
        Ok(AckOutcome::Ok(value)) => {
            if !adopt_joined_room(state, &value, Some(&join_token)) {
                // The server already added us. Roll back the actual membership
                // when no exact identity can be established.
                let _ = request(&session.client, "room:leave", serde_json::json!({}));
            }
        }
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
}

fn adopt_joined_room(
    state: &MultiplayerState,
    acknowledgement: &serde_json::Value,
    join_token: Option<&str>,
) -> bool {
    let Some(room) = acknowledgement.get("room") else {
        state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
        return false;
    };
    let echoed_identity = payload::parse_body::<payload::ServerRoom>(room).and_then(|room| {
        let mut matches = room.members.iter().filter(|member| {
            join_token.is_some()
                && member
                    .model
                    .as_ref()
                    .and_then(|model| model.meta.as_ref())
                    .and_then(|meta| meta.join_token.as_deref())
                    == join_token
        });
        let identity = matches.next()?.id.clone();
        matches.next().is_none().then_some(identity)
    });
    adopt_room(
        state,
        room,
        false,
        acknowledgement
            .get("memberId")
            .and_then(serde_json::Value::as_str)
            .or(echoed_identity.as_deref()),
    )
}

pub(crate) fn leave_room(session: &RoomSession, state: &MultiplayerState) {
    match request(&session.client, "room:leave", serde_json::json!({})) {
        Ok(AckOutcome::Ok(_)) => {
            state.scene.clear();
            state.update(|inner| {
                inner.state.room = None;
            });
        }
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
}

#[cfg(test)]
pub(crate) fn publish_model(session: &RoomSession, model_id: &str, source: &str) -> bool {
    publish_model_with_share(session, model_id, source, None)
}

pub(crate) fn publish_model_with_share(
    session: &RoomSession,
    model_id: &str,
    source: &str,
    share: Option<&crate::room_assets::Advertisement>,
) -> bool {
    session
        .client
        .emit(
            "room:model-update",
            serde_json::json!({ "model": { "name": model_id, "meta": { "source": source, "share": share } } }),
        )
        .is_ok()
}

pub(crate) fn send_chat(session: &RoomSession, state: &MultiplayerState, content: &str) {
    if state.room_id().is_none() {
        state.record_error(SettingsErrorCode::MultiplayerNotInRoom);
        return;
    }
    match request(
        &session.client,
        "room:chat",
        serde_json::json!({ "content": content }),
    ) {
        Ok(AckOutcome::Ok(_)) => {}
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
}

pub(crate) fn kick_member(session: &RoomSession, state: &MultiplayerState, member_id: &str) {
    let Some(room) = state.snapshot().room else {
        state.record_error(SettingsErrorCode::MultiplayerNotInRoom);
        return;
    };
    if !room.can_kick_member(member_id) {
        state.record_error(SettingsErrorCode::MultiplayerServerRefused);
        return;
    }
    match request(
        &session.client,
        "room:kick",
        serde_json::json!({ "memberId": member_id }),
    ) {
        Ok(AckOutcome::Ok(_)) => {}
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
}

/// Turn an acknowledgement's room view into the projection and publish it. A
/// create identifies the host; join supplies the acknowledged member id.
fn adopt_room(
    state: &MultiplayerState,
    room_value: &serde_json::Value,
    is_create: bool,
    member_id: Option<&str>,
) -> bool {
    let Some(server_room) = payload::parse_body::<payload::ServerRoom>(room_value) else {
        state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
        return false;
    };
    if server_room.room_id.is_empty()
        || server_room.host_id.is_empty()
        || !server_room
            .members
            .iter()
            .any(|member| member.id == server_room.host_id)
    {
        state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
        return false;
    }
    let self_id = if is_create {
        Some(server_room.host_id.as_str())
    } else {
        member_id
    };
    let Some(self_id) = self_id
        .filter(|id| !id.is_empty() && server_room.members.iter().any(|member| member.id == *id))
    else {
        state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
        return false;
    };
    state.update(|inner| {
        let view = payload::project_room(&server_room, Some(self_id));
        inner.self_id = Some(self_id.to_owned());
        inner.state.room = Some(view);
        inner.state.status = SettingsMultiplayerStatus::Connected;
    });
    for member in &server_room.members {
        if member.id != self_id {
            adopt_model_share(state, &member.id, member.model.as_ref());
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deployed_join_echo_requires_unique_token_and_never_guesses_by_nickname() {
        let body = json!({ "ok": true, "room": { "roomId": "room", "hostId": "host", "members": [
            {"id": "host", "name": "same"},
            {"id": "guest", "name": "same", "model": {"meta": {"join_token": "request"}}}
        ] }});
        let state = MultiplayerState::default();
        assert!(adopt_joined_room(&state, &body, Some("request")));
        assert_eq!(
            state.snapshot().room.unwrap().self_member().unwrap().id,
            "guest"
        );
        for token in [None, Some("other")] {
            assert!(!adopt_joined_room(
                &MultiplayerState::default(),
                &body,
                token
            ));
        }
        let mut ambiguous = body;
        ambiguous["room"]["members"][0]["model"] = json!({"meta":{"join_token":"request"}});
        assert!(!adopt_joined_room(
            &MultiplayerState::default(),
            &ambiguous,
            Some("request")
        ));
    }

    #[test]
    fn chat_bubbles_do_not_cross_same_named_member_identities() {
        let state = MultiplayerState::default();
        assert!(adopt_joined_room(
            &state,
            &json!({
                "memberId": "self",
                "room": { "roomId": "room", "hostId": "self", "members": [
                    { "id": "self", "name": "same" },
                    { "id": "peer", "name": "same" }
                ] }
            }),
            None,
        ));
        let runtime = bongocat_runtime::RuntimeOwner::start(false, 16);
        let client = runtime.client();
        for (id, expected_commands, expected_history) in
            [("peer", 0, 1), ("self", 1, 2), ("unknown", 1, 2)]
        {
            handle_chat(
                Payload::from(
                    json!({ "roomId": "room", "fromId": id, "fromName": "same", "content": "hello" }),
                ),
                &state,
                &client,
            );
            assert_eq!(
                client.snapshot().command_transport.enqueued,
                expected_commands
            );
            assert_eq!(state.snapshot().chat.len(), expected_history);
        }
        runtime.shutdown(std::time::Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn join_ack_identifies_same_named_guest_without_a_join_echo() {
        let state = MultiplayerState::default();
        let body = json!({
            "memberId": "guest",
            "room": {
                "roomId": "room", "hostId": "host", "memberCount": 2,
                "members": [
                    {"id": "host", "name": "same"},
                    {"id": "guest", "name": "same"}
                ]
            }
        });
        assert!(adopt_joined_room(&state, &body, None));
        let room = state.snapshot().room.unwrap();
        assert_eq!(room.self_member().unwrap().id, "guest");
        assert!(!room.can_kick_member("host"));
        assert!(room.members[0].is_host);

        for member_id in [json!(null), json!(""), json!("missing")] {
            let state = MultiplayerState::default();
            let mut invalid = body.clone();
            invalid["memberId"] = member_id;
            assert!(!adopt_joined_room(&state, &invalid, None));
            assert!(state.snapshot().room.is_none());
        }
    }

    #[test]
    fn socketio_create_ack_publishes_the_creator_as_host() {
        let state = MultiplayerState::default();
        let body = json!({
            "ok": true,
            "room": {
                "roomId": "K7XQ2M", "name": "测试房", "hostId": "host-socket",
                "memberCount": 1, "maxMembers": 8, "hasPassword": true,
                "members": [{"id": "host-socket", "name": "host", "isHost": true}]
            }
        });
        // Use the same conversion as rust_socketio's handle_ack.
        let AckOutcome::Ok(value) = ack_value(Payload::from(json!([body]).to_string())) else {
            panic!("create acknowledgement must succeed");
        };
        assert!(adopt_created_room(&state, &value));
        let snapshot = state.snapshot();
        assert_eq!(snapshot.status, SettingsMultiplayerStatus::Connected);
        let room = snapshot.room.expect("creator entered the room");
        assert_eq!(room.room_id, "K7XQ2M");
        assert_eq!(room.member_count, 1);
        assert_eq!(room.max_members, 8);
        assert!(room.has_password);
        let host = room.self_member().expect("creator is self");
        assert_eq!(host.id, "host-socket");
        assert!(host.is_host);
    }

    #[test]
    fn rejected_or_invalid_create_ack_does_not_publish_membership() {
        let AckOutcome::Refused(code) = ack_value(Payload::from(
            json!([{"ok": false, "error": "已经在房间中，请先退出当前房间"}]).to_string(),
        )) else {
            panic!("refusal must remain a refusal");
        };
        assert_eq!(code, SettingsErrorCode::MultiplayerAlreadyInRoom);
        for value in [json!({"ok": true}), json!({"ok": true, "room": {}})] {
            let state = MultiplayerState::default();
            assert!(!adopt_created_room(&state, &value));
            let snapshot = state.snapshot();
            assert!(snapshot.room.is_none());
            assert_eq!(
                snapshot.last_error.unwrap().code,
                SettingsErrorCode::MultiplayerResponseInvalid
            );
        }
    }
}
