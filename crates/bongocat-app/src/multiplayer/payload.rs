//! The server payload shapes, parsed once at the socket boundary and
//! converted into the protocol vocabulary.
//!
//! Only what the window renders survives the conversion: member ids the kick
//! request needs, names, model titles and the room shell. Everything else the
//! service sends — timestamps, hashes, custom metadata — is dropped here.

use serde::Deserialize;

use bongocat_ui_protocol::{SettingsRoomMember, SettingsRoomView};

/// One member as the service sends it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerMember {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) model: Option<ServerModel>,
    #[serde(default)]
    pub(crate) is_host: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ServerModel {
    #[serde(default)]
    pub(crate) name: Option<String>,
}

/// The room view the create, join and state acknowledgements carry.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerRoom {
    #[serde(default)]
    pub(crate) room_id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) host_id: String,
    #[serde(default)]
    pub(crate) member_count: u32,
    #[serde(default)]
    pub(crate) max_members: u32,
    #[serde(default)]
    pub(crate) has_password: bool,
    #[serde(default)]
    pub(crate) members: Vec<ServerMember>,
}

/// The chat broadcast, which the service sends to every member including the
/// sender.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerChat {
    #[serde(default)]
    pub(crate) from_id: String,
    #[serde(default)]
    pub(crate) from_name: String,
    #[serde(default)]
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) sent_at: u64,
}

/// One lobby row of `GET /bangocat/rooms`: the room as seen from outside,
/// without members and with the host's name resolved server-side.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerLobbyRow {
    #[serde(default)]
    pub(crate) room_id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) member_count: u32,
    #[serde(default)]
    pub(crate) max_members: u32,
    #[serde(default)]
    pub(crate) has_password: bool,
    #[serde(default)]
    pub(crate) host_name: String,
}

/// The member-joined broadcast; it is also how the connection learns its own
/// server identity after joining.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ServerMemberJoined {
    #[serde(default)]
    pub(crate) member: Option<ServerMember>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerMemberLeft {
    #[serde(default)]
    pub(crate) member_id: String,
    #[serde(default)]
    pub(crate) new_host_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerModelUpdated {
    #[serde(default)]
    pub(crate) member_id: String,
    #[serde(default)]
    pub(crate) model: Option<ServerModel>,
}

/// Parse one server broadcast body. A payload that does not shape-match is
/// dropped: the projection stays at its last good value rather than half-updating.
pub(crate) fn parse_body<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

/// Convert a server room into the projection, marking the member this
/// connection is. Identity is the server member id when it is known and the
/// nickname otherwise, because the create acknowledgement names the host (the
/// creator) while a join learns its id from the member-joined echo.
pub(crate) fn project_room(
    server: &ServerRoom,
    self_id: Option<&str>,
    self_name: Option<&str>,
) -> SettingsRoomView {
    let members = server
        .members
        .iter()
        .map(|member| project_member(member, self_id, self_name))
        .collect();
    SettingsRoomView {
        room_id: server.room_id.clone(),
        name: if server.name.is_empty() {
            server.room_id.clone()
        } else {
            server.name.clone()
        },
        member_count: server.member_count as usize,
        max_members: server.max_members,
        has_password: server.has_password,
        members,
    }
}

pub(crate) fn project_member(
    member: &ServerMember,
    self_id: Option<&str>,
    self_name: Option<&str>,
) -> SettingsRoomMember {
    let is_self = Some(member.id.as_str()) == self_id
        || (self_id.is_none() && self_name.is_some_and(|name| name == member.name));
    SettingsRoomMember {
        id: member.id.clone(),
        name: member.name.clone(),
        model_name: member.model.as_ref().and_then(|model| model.name.clone()),
        is_host: member.is_host,
        is_self,
    }
}
