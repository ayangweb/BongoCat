//! Public-room wire contract. No input, model, platform, or GUI types.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

pub fn valid_server_address(address: &str) -> bool {
    if address.len() > 512
        || !address.starts_with("ws://")
        || address[5..]
            .split_once('/')
            .is_none_or(|(_, path)| path != "ws")
    {
        return false;
    }
    let Ok(url) = url::Url::parse(address) else {
        return false;
    };
    url.scheme() == "ws"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/ws"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.port_or_known_default().is_some()
}

pub const PROTOCOL_VERSION: u16 = 1;
pub const ROOM_CAPACITY: usize = 8;
pub const MAX_MESSAGE_BYTES: usize = 4096;
pub const ACTION_INTERVAL: Duration = Duration::from_millis(200);
pub const PAW_DURATION: Duration = Duration::from_millis(120);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MemberId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PawAction {
    Left,
    Right,
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Join { protocol_version: u16 },
    Action { action: PawAction },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomError {
    RoomFull,
    ProtocolMismatch,
    InvalidMessage,
    RateLimited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Welcome {
        protocol_version: u16,
        sequence: u64,
        you: MemberId,
        members: Vec<MemberId>,
    },
    Joined {
        sequence: u64,
        member: MemberId,
    },
    Left {
        sequence: u64,
        member: MemberId,
    },
    Action {
        sequence: u64,
        member: MemberId,
        action: PawAction,
    },
    Error {
        code: RoomError,
    },
}

/// One bit, armed only for an established room connection. No raw input can be
/// read back from this boundary. Reconnect and exit discard pending activity.
#[derive(Clone, Debug, Default)]
pub struct ActivitySignal(Arc<AtomicU64>);
impl ActivitySignal {
    pub fn enable(&self, enabled: bool) {
        let _ = self
            .0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                Some((state & !3).wrapping_add(4) | u64::from(enabled))
            });
    }
    pub fn mark(&self) {
        let state = self.0.load(Ordering::SeqCst);
        if state & 1 != 0 {
            // A connection change invalidates an overlapping old notification.
            let _ = self
                .0
                .compare_exchange(state, state | 2, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
    pub fn take(&self) -> bool {
        self.0.fetch_and(!2, Ordering::SeqCst) & 3 == 3
    }
}

/// Remote paws expire by receiver time, without inventing synthetic keys.
#[derive(Clone, Copy, Debug, Default)]
pub struct RemotePaws {
    active: Option<(PawAction, Duration)>,
}
impl RemotePaws {
    pub fn play(&mut self, action: PawAction, now: Duration) {
        self.active = Some((action, now.saturating_add(PAW_DURATION)));
    }
    pub fn hands(&self, now: Duration) -> (bool, bool) {
        match self
            .active
            .filter(|(_, end)| now < *end)
            .map(|(action, _)| action)
        {
            Some(PawAction::Left) => (true, false),
            Some(PawAction::Right) => (false, true),
            Some(PawAction::Both) => (true, true),
            None => (false, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn address_validation_keeps_plain_room_endpoints_separate_from_update_urls() {
        for address in [
            "ws://127.0.0.1:8080/ws",
            "ws://[::1]:8080/ws",
            "ws://example.com/ws",
        ] {
            assert!(valid_server_address(address));
        }
        for address in [
            "https://example.com/ws",
            "wss://example.com/ws",
            "ws://user:pass@example.com/ws",
            "ws://example.com/a/../ws",
            "ws://example.com/ws?key=a",
            "ws://example.com:99999/ws",
        ] {
            assert!(!valid_server_address(address));
        }
    }
    #[test]
    fn raw_input_and_sender_injection_are_rejected() {
        for message in [
            r#"{"type":"action","action":"left","key":65}"#,
            r#"{"type":"action","action":"left","member":7}"#,
            r#"{"type":"action","action":"left","at":123}"#,
            r#"{"type":"action","action":"left","x":42}"#,
        ] {
            assert!(serde_json::from_str::<ClientMessage>(message).is_err());
        }
        assert_eq!(
            serde_json::to_string(&ClientMessage::Action {
                action: PawAction::Left
            })
            .unwrap(),
            r#"{"type":"action","action":"left"}"#
        );
    }
    #[test]
    fn expiry_does_not_require_release_or_visible_frames() {
        let mut paws = RemotePaws::default();
        paws.play(PawAction::Both, Duration::ZERO);
        assert_eq!(
            paws.hands(PAW_DURATION - Duration::from_millis(1)),
            (true, true)
        );
        assert_eq!(paws.hands(PAW_DURATION), (false, false));
        paws.play(PawAction::Left, Duration::from_secs(1));
        assert_eq!(paws.hands(Duration::from_secs(1)), (true, false));
        assert_eq!(paws.hands(Duration::from_secs(9)), (false, false));
    }
    #[test]
    fn disconnected_activity_is_discarded_and_connected_activity_coalesces() {
        let signal = ActivitySignal::default();
        signal.mark();
        signal.enable(true);
        assert!(!signal.take());
        for _ in 0..1000 {
            signal.mark();
        }
        assert!(signal.take());
        assert!(!signal.take());
        signal.mark();
        signal.enable(false);
        signal.enable(true);
        assert!(!signal.take());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnlineError {
    InvalidAddress,
    ServiceUnavailable,
    ConnectionFailed,
    InvalidProtocol,
    Room(RoomError),
    RenderFailed,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OnlinePhase {
    #[default]
    Offline,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}
#[derive(Clone, Debug, Default)]
pub struct OnlineSnapshot {
    pub revision: u64,
    pub generation: u64,
    pub phase: OnlinePhase,
    pub you: Option<MemberId>,
    pub members: Vec<MemberId>,
    pub error: Option<OnlineError>,
    pub render_failures: Vec<MemberId>,
}
