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
use rust_socketio::{Event, Payload, RawClient, client::Client};

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
/// nickname is remembered in the state so `is_self` can fall back to it until
/// the member-joined echo names this connection's server identity.
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
                inner.awaiting_self_name = None;
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
    state.update(|inner| {
        inner.state.status = SettingsMultiplayerStatus::Disconnected;
        inner.state.room = None;
        inner.self_id = None;
        inner.self_name = None;
        inner.awaiting_self_name = None;
    });
}

fn handle_chat(payload: Payload, state: &MultiplayerState, runtime: &RuntimeClient) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(chat) = payload::parse_body::<payload::ServerChat>(&value) else {
        return;
    };
    // The overlay bubble rides on the same broadcast the history does, so a
    // member's own line bubbles too and every viewer sees the same message.
    let _ = runtime.send(RuntimeCommand::ShowChatBubble {
        sender: chat.from_name.clone(),
        content: chat.content.clone(),
    });
    state.update(|inner| {
        let is_self = match &inner.self_id {
            Some(id) => chat.from_id == *id,
            None => inner
                .self_name
                .as_deref()
                .is_some_and(|name| name == chat.from_name),
        };
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
        // The service echoes every join to the whole room, including the
        // joiner. The first echo carrying our own nickname is therefore how a
        // plain join learns which member it is; a create already knew from the
        // host id.
        if let Some(awaiting) = &inner.awaiting_self_name
            && *awaiting == member.name
        {
            inner.self_id = Some(member.id.clone());
            inner.awaiting_self_name = None;
        }
        if let Some(room) = &mut inner.state.room {
            let known = room.members.iter().any(|existing| existing.id == member.id);
            if !known {
                room.members.push(payload::project_member(
                    &member,
                    inner.self_id.as_deref(),
                    inner.self_name.as_deref(),
                ));
                room.member_count = room.members.len();
            }
        }
    });
}

fn handle_member_left(payload: Payload, state: &MultiplayerState) {
    let Some(value) = payload_value(payload) else {
        return;
    };
    let Some(left) = payload::parse_body::<payload::ServerMemberLeft>(&value) else {
        return;
    };
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
        }
    });
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
        Some(room) => adopt_room(state, room, true),
        None => {
            state.record_error(SettingsErrorCode::MultiplayerResponseInvalid);
            false
        }
    }
}

/// Join an existing room. This connection's member id is unknown until the
/// member-joined echo arrives, so the join only records which name to watch.
pub(crate) fn join_room(
    session: &RoomSession,
    state: &MultiplayerState,
    room_id: &str,
    password: &str,
) {
    let mut body = serde_json::json!({ "roomId": room_id });
    if let Some(nickname) = state.joined_nickname() {
        body["name"] = serde_json::Value::String(nickname);
    }
    if !password.is_empty() {
        body["password"] = serde_json::Value::String(password.to_owned());
    }
    match request(&session.client, "room:join", body) {
        Ok(AckOutcome::Ok(value)) => {
            state.update(|inner| {
                inner.awaiting_self_name = inner.self_name.clone();
            });
            if let Some(room) = value.get("room") {
                adopt_room(state, room, false);
            }
        }
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
}

pub(crate) fn leave_room(session: &RoomSession, state: &MultiplayerState) {
    match request(&session.client, "room:leave", serde_json::json!({})) {
        Ok(AckOutcome::Ok(_)) => {
            state.update(|inner| {
                inner.state.room = None;
            });
        }
        Ok(AckOutcome::Refused(code)) => state.record_error(code),
        Err(_) => state.record_error(SettingsErrorCode::MultiplayerServerRefused),
    }
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
/// create names this connection the host; a join leaves the identity to the
/// member-joined echo and marks nobody as self until it arrives.
fn adopt_room(state: &MultiplayerState, room_value: &serde_json::Value, is_create: bool) -> bool {
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
    state.update(|inner| {
        let view = payload::project_room(
            &server_room,
            if is_create {
                Some(server_room.host_id.as_str())
            } else {
                inner.self_id.as_deref()
            },
            inner.self_name.as_deref(),
        );
        if is_create {
            inner.self_id = Some(server_room.host_id.clone());
        } else {
            inner.awaiting_self_name = inner.self_name.clone();
        }
        inner.state.room = Some(view);
        inner.state.status = SettingsMultiplayerStatus::Connected;
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
