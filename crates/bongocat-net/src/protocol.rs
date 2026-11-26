//! Wire protocol for the cross-device input relay.
//!
//! One WebSocket text frame carries one [`Envelope`]. The envelope keeps the
//! version and the message kind at the top level so a peer can reject an
//! incompatible build before it tries to interpret the payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped whenever a message shape changes. A mismatch is a hard rejection:
/// silently half-understanding a peer is worse than refusing it.
///
/// v2 added the required `edge` on [`InputFrame`]. The bump is not about
/// compatibility with a released peer — there is none — but about the two
/// machines this ships to: updating one and forgetting the other must fail at
/// the handshake, not turn into frames the relay quietly drops.
pub const PROTOCOL_VERSION: u32 = 2;

/// Message kinds, kept as constants so both ends cannot drift on a typo.
pub mod kind {
    pub const HELLO: &str = "hello";
    pub const JOINED: &str = "joined";
    pub const INPUT: &str = "input";
    pub const PEER_JOINED: &str = "peer_joined";
    pub const PEER_LEFT: &str = "peer_left";
    pub const PING: &str = "ping";
    pub const PONG: &str = "pong";
    pub const POINTER: &str = "pointer";
    pub const ERROR: &str = "error";
}

/// A device identity. Stable across restarts; generated on first run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a peer announces about itself during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub publish: bool,
}

/// The sending display's logical geometry, used to map pointer coordinates
/// onto whichever display the receiver is drawing on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Client → relay, always the first frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub token: String,
    pub device: DeviceInfo,
}

/// A peer as seen by everyone else in the room.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub publish: bool,
}

/// One normalized pointer position, sender-side.
///
/// `x` and `y` live in the sender's own 0..1 space (the capture normalizes
/// against its viewport before publishing), so the receiver interprets them in
/// whatever viewport it draws with — a multi-monitor boundary walks like the
/// pointer walked, no geometry needs to travel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointerFrame {
    pub src: String,
    pub seq: u64,
    pub t: u64,
    pub x: f64,
    pub y: f64,
}

/// Relay → client, the answer to [`Hello`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joined {
    pub peer_id: String,
    pub peers: Vec<PeerInfo>,
}

/// One input sample. `seq` is per-sender and monotonic; a gap means loss, not
/// reordering, and is reported rather than repaired — a stale key press has no
/// value once it is late.
///
/// `edge` is what makes the frame interpretable on its own: without it a
/// receiver can tell *that* a key changed but not whether it went down or up,
/// and so can never reconstruct which keys are held. It is deliberately not
/// folded into `control`, because the same control appears on both edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputFrame {
    pub src: String,
    pub seq: u64,
    pub t: u64,
    pub control: bongocat_input::InputControl,
    pub edge: bongocat_input::InputEdge,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}

/// Relay → everyone, when a peer disconnects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerLeft {
    pub id: String,
}

/// Ping and pong share a shape: the sender's clock, echoed back verbatim.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Heartbeat {
    pub t: u64,
}

/// Why the relay refused a connection or dropped a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadVersion,
    BadToken,
    RateLimited,
    /// A frame arrived that the relay could not interpret at all.
    ///
    /// This is a disconnect rather than a dropped frame on purpose: a peer that
    /// keeps sending undecodable payloads after a successful handshake is
    /// speaking a different dialect, and letting it stay connected would look
    /// like "nothing happens" from the far end.
    Protocol,
    /// Another connection took over this device's identity.
    ///
    /// Two machines sharing a device id are otherwise indistinguishable from a
    /// flaky link: they evict each other until both exhaust their retries, and
    /// the person sees "reconnecting" forever. Naming the conflict is what lets
    /// the client say what is actually wrong instead of retrying into a loop.
    Conflict,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

/// One framed message on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("failed to (de)serialize payload: {0}")]
    Payload(#[from] serde_json::Error),
}

impl Envelope {
    pub fn new(kind: &str, payload: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            kind: kind.to_string(),
            payload,
        }
    }

    /// Wraps a typed payload into a versioned envelope.
    pub fn of<T: Serialize>(kind: &str, payload: &T) -> Result<Self, EncodeError> {
        Ok(Self::new(kind, serde_json::to_value(payload)?))
    }

    /// Reads the payload back as a concrete message type.
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T, EncodeError> {
        Ok(serde_json::from_value(self.payload.clone())?)
    }

    pub fn check_version(&self) -> Result<(), ProtocolError> {
        if self.v == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ProtocolError {
                code: ErrorCode::BadVersion,
                message: format!(
                    "relay speaks protocol v{PROTOCOL_VERSION}, peer sent v{}",
                    self.v
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_a_heartbeat() {
        let beat = Heartbeat { t: 42 };
        let envelope = Envelope::of(kind::PING, &beat).expect("encode");
        let wire = serde_json::to_string(&envelope).expect("serialize");
        assert!(wire.contains("\"type\":\"ping\""), "{wire}");
        let back: Envelope = serde_json::from_str(&wire).expect("deserialize");
        assert_eq!(back.v, PROTOCOL_VERSION);
        let beat: Heartbeat = back.decode().expect("decode");
        assert_eq!(beat.t, 42);
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut envelope = Envelope::new(kind::PING, Value::Null);
        envelope.v = PROTOCOL_VERSION + 1;
        let error = envelope.check_version().expect_err("must reject");
        assert_eq!(error.code, ErrorCode::BadVersion);
    }

    #[test]
    fn input_frame_carries_a_key_code() {
        // Built through JSON rather than a constructor so this test also pins
        // the wire shape of the control enum.
        let control: bongocat_input::InputControl =
            serde_json::from_value(serde_json::json!({ "Key": 30 })).expect("decode control");
        let frame = InputFrame {
            src: "pc-01".into(),
            seq: 7,
            t: 1_762_000_000_000,
            control,
            edge: bongocat_input::InputEdge::Down,
            bounds: None,
        };
        let wire = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(wire["control"]["Key"], 30);
        assert_eq!(wire["edge"], "Down", "the edge travels with the frame");
        assert!(wire.get("bounds").is_none(), "bounds omitted when absent");
    }
}
