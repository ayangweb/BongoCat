//! Versioned process protocol. No Rust ABI or application internals cross it.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ConfigValue {
    Boolean(bool),
    Integer(i64),
    Text(String),
}
pub type Configuration = BTreeMap<String, ConfigValue>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PluginIcon {
    Emoji(String),
    LocalImage(String),
    RemoteImage(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ModelUi,
    Configuration,
    PersistentState,
    /// Anonymous, coalesced keyboard-down activity; message contract version 1.
    KeyboardActivityV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub app_versions: String,
    pub protocol_version: u32,
    pub entry: String,
    pub icon: PluginIcon,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiNode {
    Text {
        text: String,
    },
    Button {
        id: String,
        label: String,
    },
    Toggle {
        key: String,
        label: String,
        value: bool,
    },
    Integer {
        key: String,
        label: String,
        value: i64,
        min: i64,
        max: i64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginView {
    pub anchor: Anchor,
    pub nodes: Vec<UiNode>,
}

impl PluginView {
    pub fn validate(&self) -> bool {
        self.nodes.len() <= 16
            && self.nodes.iter().all(|node| match node {
                UiNode::Text { text } => text.len() <= 1024,
                UiNode::Button { id, label } => valid_key(id) && label.len() <= 128,
                UiNode::Toggle { key, label, .. } => valid_key(key) && label.len() <= 128,
                UiNode::Integer {
                    key,
                    label,
                    value,
                    min,
                    max,
                } => valid_key(key) && label.len() <= 128 && min <= value && value <= max,
            })
    }
}
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Handshake {
        protocol_version: u32,
        id: String,
    },
    Start {
        config: Configuration,
        state: Vec<u8>,
    },
    Tick {
        elapsed_ms: u64,
    },
    KeyboardActivity {
        elapsed_ms: u64,
    },
    Action {
        id: String,
    },
    Configure {
        config: Configuration,
    },
    Reset,
    Pause,
    Resume,
    Stop,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginOutput {
    pub model: PluginView,
    pub settings: PluginView,
    pub config: Configuration,
    pub state: Vec<u8>,
}
impl PluginOutput {
    pub fn validate(&self) -> bool {
        self.model.validate()
            && self.settings.validate()
            && self.state.len() <= 64 * 1024
            && self.config.len() <= 64
            && self.config.iter().all(|(k, v)| {
                valid_key(k)
                    && match v {
                        ConfigValue::Text(s) => s.len() <= 4096,
                        _ => true,
                    }
            })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub sequence: u64,
    pub message: HostMessage,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub sequence: u64,
    pub result: Result<PluginOutput, String>,
}

pub trait Plugin {
    fn id(&self) -> &'static str;
    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String>;
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plugin frame too large",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
pub fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plugin frame too large",
        ));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Stdout is exclusively framed protocol, never plugin logging.
pub fn run(plugin: &mut impl Plugin) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut ready = false;
    loop {
        let request: Request = read_frame(&mut input)?;
        let stop = matches!(request.message, HostMessage::Stop);
        let result = match request.message {
            HostMessage::Handshake {
                protocol_version,
                ref id,
            } => {
                if ready || protocol_version != PROTOCOL_VERSION || id != plugin.id() {
                    Err("incompatible plugin handshake".into())
                } else {
                    ready = true;
                    Ok(PluginOutput::default())
                }
            }
            message if ready => plugin.handle(message),
            _ => Err("handshake required".into()),
        };
        write_frame(
            &mut output,
            &Response {
                sequence: request.sequence,
                result,
            },
        )?;
        if stop {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversized_and_truncated_frames() {
        assert!(
            read_frame::<Request>(&mut ((MAX_MESSAGE_BYTES as u32 + 1).to_le_bytes().as_slice()))
                .is_err()
        );
        assert!(read_frame::<Request>(&mut [4, 0, 0, 0, 1].as_slice()).is_err());
    }
    #[test]
    fn protocol_roundtrip() {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Request {
                sequence: 7,
                message: HostMessage::Pause,
            },
        )
        .unwrap();
        let request: Request = read_frame(&mut bytes.as_slice()).unwrap();
        assert_eq!(request.sequence, 7);
    }
    #[test]
    fn limits_views_and_config_ids() {
        assert!(!valid_key("../escape"));
        assert!(
            !PluginView {
                nodes: vec![UiNode::Text {
                    text: "x".repeat(1025)
                }],
                ..Default::default()
            }
            .validate()
        );
    }
}
