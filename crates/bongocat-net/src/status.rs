//! What this process's connection to a room is doing right now.
//!
//! A process serves at most one relay and joins at most one room, so the answer is
//! process-wide rather than owned by a handle. The threads that open the sockets are
//! not the thread that paints the settings window, and carrying a handle across that
//! boundary would mean threading it through the input pipeline, which by design knows
//! nothing about rooms.
//!
//! It is a snapshot rather than a stream: a writer replaces what is true and a reader
//! takes a copy, so a settings frame can draw it without subscribing to anything and
//! without a slow reader being able to stall a connection.

use crate::protocol::PeerInfo;
use std::sync::{Mutex, OnceLock};

/// What the room connection is doing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RoomState {
    /// Nothing has been asked to join a room.
    #[default]
    Off,
    /// The first handshake has not completed yet.
    Connecting,
    /// In the room, with this many other devices in it.
    Connected { peers: u16 },
    /// The link dropped and a retry is scheduled.
    Reconnecting,
    /// Gave up after the configured number of attempts.
    Failed,
}

/// The room this process has joined, if it has joined one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomStatus {
    pub state: RoomState,
    /// The device whose input this process is drawing.
    ///
    /// `None` until the first frame from it arrives: a room can hold several
    /// publishers and one pet draws one of them, so the binding is a fact about
    /// what has been seen rather than about what was configured.
    pub bound: Option<String>,
    /// The other devices the room currently holds.
    ///
    /// The settings window's device list offers these by name: a peer that has
    /// joined is a fact the room announced, and retyping its id into a text
    /// field would only transcribe what the page already knows.
    pub peers: Vec<PeerInfo>,
}

/// The relay this process serves, if it serves one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayStatus {
    /// What the relay is bound to, spelled as a URL a peer could dial.
    pub url: String,
}

/// The multiplayer side of this process, as it stands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkStatus {
    /// The relay this process serves, if it serves one.
    pub relay: Option<RelayStatus>,
    /// The room this process has joined, if it has joined one.
    pub room: Option<RoomStatus>,
}

static STATUS: OnceLock<Mutex<NetworkStatus>> = OnceLock::new();

fn status() -> &'static Mutex<NetworkStatus> {
    STATUS.get_or_init(|| Mutex::new(NetworkStatus::default()))
}

/// The status as it stands now.
///
/// A poisoned lock reports the default rather than panicking: a thread that died
/// holding this mutex has already lost its connection, and a settings frame that
/// cannot be drawn would turn that into a second failure.
pub fn network_status() -> NetworkStatus {
    status()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Replace the parts of the status one writer owns.
pub fn update_network_status(change: impl FnOnce(&mut NetworkStatus)) {
    if let Ok(mut guard) = status().lock() {
        change(&mut guard);
    }
}
