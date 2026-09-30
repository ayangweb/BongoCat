//! The multiplayer room projection: the connection, the room and its members,
//! the lobby list and the chat the overlay bubbles are drawn from.
//!
//! The room service is an external socket.io server; the worker that speaks to
//! it publishes this projection and the snapshot clock watches its version, the
//! same way the remote model library is published. The window never talks to
//! the server itself: it reads this section, and its commands are requests the
//! worker answers with the optimistic snapshot while the real outcome arrives
//! through later revisions.

use super::*;

/// How many chat lines the projection keeps, oldest first. The service drops
/// the front of the history past this bound, and the window renders exactly
/// what is here.
pub const CHAT_HISTORY_LIMIT: usize = 50;

/// The longest chat line the room service accepts, in characters.
pub const MAXIMUM_CHAT_CONTENT_CHARS: usize = 500;

/// The longest room name and password the room service accepts, in characters.
pub const MAXIMUM_ROOM_NAME_CHARS: usize = 24;
pub const MAXIMUM_ROOM_PASSWORD_CHARS: usize = 32;

/// Where the multiplayer connection stands. Being inside a room is carried by
/// [`SettingsMultiplayer::room`], not by this state, so "connected but in no
/// room" and "connecting" stay distinguishable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsMultiplayerStatus {
    /// No socket: the page has not connected yet, or the server dropped the
    /// connection. Re-creating or joining a room connects on demand.
    #[default]
    Disconnected,
    /// A connect attempt is running.
    Connecting,
    /// The socket is up, in or out of a room.
    Connected,
    /// The last connect attempt failed. The code is stable vocabulary the
    /// window translates; the detail goes to the application log.
    Failed(SettingsErrorCode),
}

/// The lobby list of `GET /bangocat/rooms`, one row per open room.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsLobbyStatus {
    /// The window has not asked for the list yet.
    #[default]
    Unloaded,
    /// A refresh is fetching the document.
    Loading,
    /// The list is on screen.
    Ready,
    /// The last refresh could not produce a list.
    Failed,
}

/// One member of the room as the window renders it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRoomMember {
    /// The server-assigned member identity (the socket id), which a host's
    /// kick request must carry verbatim.
    pub id: String,
    pub name: String,
    /// The model the member advertised, when it advertised one.
    pub model_name: Option<String>,
    pub is_host: bool,
    pub is_self: bool,
}

/// The room the connection currently sits in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRoomView {
    pub room_id: String,
    pub name: String,
    pub member_count: usize,
    pub max_members: u32,
    pub has_password: bool,
    pub members: Vec<SettingsRoomMember>,
}

impl SettingsRoomView {
    /// The member marked `is_self`, if the projection still carries it.
    pub fn self_member(&self) -> Option<&SettingsRoomMember> {
        self.members.iter().find(|member| member.is_self)
    }
}

/// One lobby row: what the room looks like from outside, without members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsLobbyRoom {
    pub room_id: String,
    pub name: String,
    pub member_count: u32,
    pub max_members: u32,
    pub has_password: bool,
    pub host_name: String,
}

/// One chat line, oldest first in [`SettingsMultiplayer::chat`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsChatMessage {
    pub sender: String,
    pub content: String,
    /// Milliseconds since the Unix epoch, as the server stamped it.
    pub sent_at: u64,
    pub is_self: bool,
}

/// The last failure a multiplayer operation produced, with the sequence that
/// lets the window show each error exactly once.
///
/// The code is stable vocabulary; the server's own message text never travels
/// into the projection, so what the window shows stays localized and the
/// detail goes to the application log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsMultiplayerError {
    pub seq: u64,
    pub code: SettingsErrorCode,
}

/// The whole multiplayer projection the settings window renders.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsMultiplayer {
    pub status: SettingsMultiplayerStatus,
    pub room: Option<SettingsRoomView>,
    pub lobby_status: SettingsLobbyStatus,
    pub lobby: Vec<SettingsLobbyRoom>,
    pub chat: Vec<SettingsChatMessage>,
    pub last_error: Option<SettingsMultiplayerError>,
}

impl SettingsMultiplayer {
    /// Whether a room operation makes sense right now. The window uses it to
    /// disable the create/join side while a connection is being established.
    pub const fn is_connecting(&self) -> bool {
        matches!(self.status, SettingsMultiplayerStatus::Connecting)
    }

    /// Whether the chat input and the room body have something to act on.
    pub fn is_in_room(&self) -> bool {
        self.room.is_some()
    }

    /// Append one chat line, keeping the history bounded. The worker calls
    /// this on every broadcast, including the lines the user sent.
    pub fn push_chat(&mut self, message: SettingsChatMessage) {
        self.chat.push(message);
        let overflow = self.chat.len().saturating_sub(CHAT_HISTORY_LIMIT);
        self.chat.drain(..overflow);
    }
}
