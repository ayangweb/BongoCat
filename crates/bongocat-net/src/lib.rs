//! Cross-device input relay.
//!
//! The relay is a star hub: every device connects to exactly one relay, and the
//! relay fans input frames out to the rest of the room. A room is whatever
//! shares a token, so two devices that know the same token see each other and
//! nothing else — there is no separate room code to keep in sync.

pub mod client;
pub mod protocol;
pub mod relay;
pub mod status;
pub mod token;

pub use protocol::{
    Bounds, DeviceId, DeviceInfo, Envelope, ErrorCode, Hello, InputFrame, Joined, PROTOCOL_VERSION,
    PeerInfo, PeerLeft, ProtocolError,
};
pub use status::{
    NetworkStatus, RelayStatus, RoomState, RoomStatus, network_status, update_network_status,
};
pub use token::generate_token;
