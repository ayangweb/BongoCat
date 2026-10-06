//! The server payload shapes, parsed once at the socket boundary and
//! converted into the protocol vocabulary.
//!
//! Member identity, model identity/source, names and the room shell survive
//! conversion into project types. Asset hashes and other model metadata are
//! dropped; the room never imports remote assets.

use serde::Deserialize;

use bongocat_ui_protocol::{
    SettingsModelKey, SettingsModelOrigin, SettingsRoomMember, SettingsRoomView,
};

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
    #[serde(default)]
    pub(crate) meta: Option<ServerModelMeta>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ServerModelMeta {
    #[serde(default)]
    pub(crate) share: Option<crate::room_assets::Advertisement>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    pub(crate) join_token: Option<String>,
}

impl ServerModel {
    pub(crate) fn key(&self) -> Option<SettingsModelKey> {
        let origin = match self.meta.as_ref()?.source.as_deref()? {
            "preset" => SettingsModelOrigin::BuiltIn,
            "installed" => SettingsModelOrigin::Imported,
            _ => return None,
        };
        Some(SettingsModelKey {
            id: self.name.clone()?,
            origin,
        })
    }
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
    pub(crate) room_id: String,
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

/// The member-joined broadcast identifies another member.
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

/// Project a room using the server-acknowledged connection identity.
pub(crate) fn project_room(server: &ServerRoom, self_id: Option<&str>) -> SettingsRoomView {
    let members = server
        .members
        .iter()
        .map(|member| {
            let mut projected = project_member(member, self_id);
            projected.is_host = member.id == server.host_id;
            projected
        })
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

pub(crate) fn project_member(member: &ServerMember, self_id: Option<&str>) -> SettingsRoomMember {
    let is_self = Some(member.id.as_str()) == self_id;
    SettingsRoomMember {
        model_visible: true,
        model_download: None,
        id: member.id.clone(),
        name: member.name.clone(),
        model_name: member.model.as_ref().and_then(|model| model.name.clone()),
        model_key: member.model.as_ref().and_then(ServerModel::key),
        is_host: member.is_host,
        is_self,
    }
}
