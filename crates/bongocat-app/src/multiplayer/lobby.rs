//! The lobby list: the service's one REST document, fetched with the same
//! bounded-timeout discipline the remote model catalog uses.
//!
//! The lobby needs no socket, so a user can look at open rooms before joining
//! anything; a failure is one stable code on the projection.

use std::io::Read;

use bongocat_ui_protocol::{SettingsErrorCode, SettingsLobbyRoom};

use super::payload;

/// The lobby list lives beside the socket namespace on the same service.
const LOBBY_PATH: &str = "/bangocat/rooms";

/// One response is a handful of room rows; anything past this is refused
/// rather than read.
const MAXIMUM_LOBBY_DOCUMENT_BYTES: u64 = 256 * 1024;

const CONNECT_TIMEOUT_SECS: u64 = 10;
const READ_TIMEOUT_SECS: u64 = 15;

/// Fetch the open-room list. The URL is the same base the socket connects to.
pub(crate) fn fetch_rooms(server_url: &str) -> Result<Vec<SettingsLobbyRoom>, SettingsErrorCode> {
    let url = format!("{}{LOBBY_PATH}", server_url.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout_read(std::time::Duration::from_secs(READ_TIMEOUT_SECS))
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|_| SettingsErrorCode::MultiplayerConnectFailed)?;
    let mut document = String::new();
    response
        .into_reader()
        .take(MAXIMUM_LOBBY_DOCUMENT_BYTES)
        .read_to_string(&mut document)
        .map_err(|_| SettingsErrorCode::MultiplayerResponseInvalid)?;
    let value: serde_json::Value = serde_json::from_str(&document)
        .map_err(|_| SettingsErrorCode::MultiplayerResponseInvalid)?;
    let rooms = value
        .get("rooms")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(rooms
        .iter()
        .filter_map(|room| {
            let row = payload::parse_body::<payload::ServerLobbyRow>(room)?;
            let name = if row.name.is_empty() {
                row.room_id.clone()
            } else {
                row.name
            };
            Some(SettingsLobbyRoom {
                room_id: row.room_id,
                name,
                member_count: row.member_count,
                max_members: row.max_members,
                has_password: row.has_password,
                host_name: row.host_name,
            })
        })
        .collect())
}
