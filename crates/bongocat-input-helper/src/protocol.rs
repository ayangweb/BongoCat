//! The private helper pipe contract. Both ends use this codec; no native types cross it.
//!
//! Version 1 keeps the existing 8-byte header and fixed-size little-endian packets.

pub const HEADER: &[u8; 8] = b"BCINPUT1";
pub const PACKET_SIZE: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMessage {
    Reset { enabled: bool },
    Key { code: u32, state: ButtonState },
    Button { code: u32, state: ButtonState },
    Motion { dx: f64, dy: f64 },
    Heartbeat { enabled: bool },
}

impl InputMessage {
    pub fn encode(self) -> [u8; PACKET_SIZE] {
        let (kind, code, x, y): (u32, u32, f64, f64) = match self {
            Self::Reset { enabled } => (0, u32::from(enabled), 0.0, 0.0),
            Self::Key { code, state } => (
                if state == ButtonState::Pressed { 1 } else { 2 },
                code,
                0.0,
                0.0,
            ),
            Self::Button { code, state } => (
                if state == ButtonState::Pressed { 3 } else { 4 },
                code,
                0.0,
                0.0,
            ),
            Self::Motion { dx, dy } => (5, 0, dx, dy),
            Self::Heartbeat { enabled } => (6, u32::from(enabled), 0.0, 0.0),
        };
        let mut bytes = [0; PACKET_SIZE];
        bytes[..4].copy_from_slice(&kind.to_le_bytes());
        bytes[4..8].copy_from_slice(&code.to_le_bytes());
        bytes[8..16].copy_from_slice(&x.to_le_bytes());
        bytes[16..].copy_from_slice(&y.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8; PACKET_SIZE]) -> Option<Self> {
        let kind = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let code = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        Some(match kind {
            0 => Self::Reset { enabled: code != 0 },
            1 => Self::Key {
                code,
                state: ButtonState::Pressed,
            },
            2 => Self::Key {
                code,
                state: ButtonState::Released,
            },
            3 => Self::Button {
                code,
                state: ButtonState::Pressed,
            },
            4 => Self::Button {
                code,
                state: ButtonState::Released,
            },
            5 => Self::Motion {
                dx: f64::from_le_bytes(bytes[8..16].try_into().unwrap()),
                dy: f64::from_le_bytes(bytes[16..].try_into().unwrap()),
            },
            6 => Self::Heartbeat { enabled: code != 0 },
            _ => return None,
        })
    }
}
