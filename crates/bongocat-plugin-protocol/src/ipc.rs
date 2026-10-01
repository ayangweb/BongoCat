//! The wire between a plugin and the host.
//!
//! One JSON document per line, in both directions, over the plugin's stdin and
//! stdout. There is no framing header, no length prefix and no handshake beyond
//! the first message, and that is the point: a plugin author can start one of
//! these by hand with `echo` and see what happens, and the transport is something
//! `std::io` already does on every platform.
//!
//! # Why not a socket, a pipe with a length prefix, or a shared file
//!
//! A socket needs a port, which means a port collision, a firewall question on
//! two platforms, and a way for a plugin to be reached by anything else on the
//! machine. A length prefix is a framing bug waiting to happen. A file needs a
//! watcher and a lock. The child's own pipes are created by `std::process`, are
//! private to that process pair, and end when either side exits — which is the
//! lifetime a plugin session actually wants.
//!
//! # The two rules that keep this honest
//!
//! 1. **stderr is the plugin's log, stdout is the protocol.** The host reads
//!    stderr and forwards it to its own log. A plugin that prints a diagnostic to
//!    stdout breaks the protocol, which is a visible failure rather than a silent
//!    one — and the SDK routes its own logging to stderr so an author does not have
//!    to remember.
//! 2. **Neither side reads the other's mind.** Every message is one of these
//!    variants, and a line that is not one is refused rather than skipped. A
//!    refused line is counted; a skipped one would let a version mismatch look
//!    like a plugin that is simply quiet.

use super::action::PluginAction;
use super::config::{ConfigDocument, ConfigSchema};
use super::descriptor::PluginDescriptor;
use super::error::{PluginError, PluginErrorCode};
use super::host_state::{HostState, InputEvent, ModelOutcome, ModelRequest};
use super::identity::{PluginId, PluginVersion};
use super::panel::PanelUpdate;
use serde::{Deserialize, Serialize};

/// The protocol version this host speaks.
///
/// Checked on the first message in each direction. A mismatch is refused before
/// anything is loaded rather than half-understood, because the alternative is a
/// host writing a document a plugin will read as a different shape — which is
/// the one failure mode a version number exists to prevent.
pub const PROTOCOL_VERSION: u32 = 1;

/// The most bytes one protocol line may be.
///
/// A panel update is the largest message and it is a scene document; a megabyte
/// is far past any panel and small enough that a plugin cannot make the host
/// allocate without limit by writing one enormous line.
pub const MAXIMUM_MESSAGE_BYTES: usize = 1024 * 1024;

/// The first thing the host sends, and the first thing a plugin sends.
///
/// The host's `hello` carries the protocol version, the plugin's own identity as
/// the store holds it, the two directories it owns, and the locale. The plugin's
/// `ready` answers with its descriptor. Neither side proceeds until it has the
/// other's, which is what makes "the card says one version and the process runs
/// another" a refusal rather than a mystery.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol_version: u32,
    pub app_version: String,
    /// The id the store installed this plugin under.
    pub id: PluginId,
    /// The version the store installed.
    pub version: PluginVersion,
    /// The plugin's own directory, absolute. The plugin may read its assets here.
    pub plugin_dir: String,
    /// The plugin's own data directory, absolute. The plugin owns everything in
    /// it; the host creates it and never writes inside it.
    pub data_dir: String,
    /// The user's locale, so a plugin can pick its own copy before its first frame.
    #[serde(default)]
    pub locale: String,
}

impl Hello {
    pub fn check_version(&self) -> Result<(), PluginError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(PluginError::with_detail(
                PluginErrorCode::ProtocolVersionMismatch,
                format!(
                    "this host speaks protocol {PROTOCOL_VERSION}, the plugin speaks {}",
                    self.protocol_version
                ),
            ));
        }
        Ok(())
    }
}

/// What a running plugin says.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginMessage {
    /// The answer to the host's `hello`. The first message, and the only one that
    /// may not be repeated.
    ///
    /// Boxed, and for a size reason rather than a stylistic one: a descriptor carries the
    /// plugin's whole settings schema inline, which makes it several times larger than
    /// any other message, and `PluginMessage` is what the reader thread moves once per
    /// line for the life of a session. A plugin sends this once; everything after it is a
    /// panel or an answer, and those stay small. The wire format is unchanged — `Box` is
    /// transparent to `serde`.
    Ready { descriptor: Box<PluginDescriptor> },
    /// A panel to draw on the model window.
    Panel(Box<PanelUpdate>),
    /// Take the panel down without stopping the plugin.
    HidePanel,
    /// The controls this plugin wants the host to offer on its behalf, in the order it
    /// wants them drawn.
    ///
    /// A message rather than part of the handshake, and the reason is the whole
    /// design: an action carries a label, and a label that goes stale lies. A timer
    /// whose card still reads "Start" while its round is counting says the opposite of
    /// what pressing the button will do, so a plugin re-sends the list whenever the
    /// meaning of any of them changed.
    ///
    /// The list is *replaced*, never merged, so a plugin cannot leave behind a control
    /// it has stopped wanting — and a press of an id that is not in the current list is
    /// ignored rather than delivered, which is what lets a card be rebuilt from a
    /// snapshot without the window having to guess whether a button is still live.
    Actions { actions: Vec<PluginAction> },
    /// The plugin's configuration changed and the host should re-read it.
    ///
    /// The document travels with the message rather than being pulled, so the
    /// settings window never shows a value the plugin has not already written to
    /// its own file.
    ConfigChanged { config: ConfigDocument },
    /// Something worth logging, at a level the host maps onto its own.
    Log { level: LogLevel, message: String },
    /// Ask the model window's owner to do something, by name.
    ///
    /// This is the one thing a plugin asks *for* rather than reports, so it travels
    /// outward. `id` is the plugin's own counter, echoed back in the answer. Nothing
    /// about it is interpreted by the host beyond being a key, which is what lets a
    /// plugin keep a table of what it asked for without the host holding any state
    /// on its behalf.
    Request { id: u64, request: Box<ModelRequest> },
    /// The host's answer to one model request the plugin made.
    Answer(ModelAnswer),
    /// The plugin failed in a way the user should see.
    ///
    /// Carries the plugin's own code so the settings window can name it, and the
    /// detail is for the host's log rather than for the user — the same rule every
    /// other error in this system follows.
    Failed { error: PluginError },
    /// The plugin is exiting on purpose.
    Shutdown,
}

/// How loud a plugin's own log line is.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// The answer to one model request.
///
/// Carries the id the request was made with, so a plugin can keep a table of what
/// it asked for without the host holding any state on its behalf.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAnswer {
    /// The id the request carried.
    pub id: u64,
    pub outcome: ModelOutcome,
}

/// The user's local wall clock, as a reading of hours, minutes and seconds.
///
/// Three fields and nothing else, on purpose: a plugin formats this and cannot ask
/// for a date, a timezone or a locale, because the protocol has no way to name one.
/// That is what keeps a clock panel the same picture on every machine.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WallClock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl WallClock {
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour,
            minute,
            second,
        }
    }

    /// This reading as `HH:MM:SS`, written once here so every clock a plugin shows is
    /// spelled the same way.
    pub fn to_hms(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// This reading as `HH:MM`.
    pub fn to_hm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// What the host says to a running plugin.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    /// The first message. See [`Hello`].
    Hello(Hello),
    /// Time passed and the host's facts may have changed.
    ///
    /// One message at the host's own cadence, carrying both: a plugin that wants
    /// seconds gets them, and a plugin that wants the model name gets a fresh
    /// answer without a second message type. `elapsed_ms` is monotonic since the
    /// session started, so a plugin's own clock needs no wall time and cannot
    /// disagree with the host's.
    Tick {
        elapsed_ms: u64,
        state: HostState,
        /// The user's local wall clock, read by the host's main thread.
        ///
        /// Republished on every tick rather than fetched by the plugin, and that is
        /// not an oversight: reading the local UTC offset is only sound when one
        /// thread at a time asks, and the plugin worker's thread is not that one.
        clock: WallClock,
    },
    /// Something happened, for a plugin that asked for the feed.
    Input { events: Vec<InputEvent> },
    /// A press of one of this plugin's controls, as the id it declared.
    ///
    /// By id and not by position, because the plugin built the scene and knows
    /// what the rectangle meant. A press outside every button produces no message
    /// at all — the host drops it rather than reporting a miss the plugin would
    /// have to interpret.
    ///
    /// The same message carries a press of a [`PluginAction`], which is the point
    /// of an action using the same id vocabulary as a panel button: a control the
    /// host drew on its own card and a control the plugin drew on its panel are
    /// one thing to a plugin, so offering the first costs no second handler.
    Press { id: String },
    /// The user changed a setting. Carries the whole document, because a plugin
    /// writes its file atomically and a patch would have to be merged by a side
    /// that does not own the file.
    ConfigChanged { config: ConfigDocument },
    /// The host's answer to a model request this plugin made.
    ///
    /// Only ever delivered to a plugin that asked, and matched by the plugin's own
    /// id — a refusal is a normal answer rather than an error, because a model with
    /// no motion called "thinking" is a fact only the plugin can act on.
    ModelAnswer { id: u64, outcome: ModelOutcome },
    /// The host is stopping the plugin, and will close the pipes after this line.
    ///
    /// Sent before the pipes close so a plugin can flush its own state on the way
    /// out; a plugin that ignores it and exits anyway is not an error.
    Shutdown,
}

impl HostMessage {
    /// Check the parts of a message that are about this host rather than about the
    /// plugin.
    pub fn check(&self) -> Result<(), PluginError> {
        if let Self::Hello(hello) = self {
            hello.check_version()?;
        }
        Ok(())
    }
}

/// One line's ceiling, applied before a line is parsed.
///
/// Checked on the bytes rather than after the parse, so a plugin cannot make the
/// host build a document of any size by writing one enormous line.
pub fn check_line_length(line: &[u8]) -> Result<(), PluginError> {
    if line.len() > MAXIMUM_MESSAGE_BYTES {
        return Err(PluginError::with_detail(
            PluginErrorCode::ProtocolInvalid,
            format!("a protocol line is longer than {MAXIMUM_MESSAGE_BYTES} bytes"),
        ));
    }
    Ok(())
}

/// Read one message from a line.
///
/// A line that is not a message this host knows is refused rather than skipped:
/// a version mismatch has to look like a version mismatch, not like a plugin that
/// has gone quiet.
pub fn parse_host_message(line: &[u8]) -> Result<HostMessage, PluginError> {
    check_line_length(line)?;
    let message: HostMessage = serde_json::from_slice(line)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::ProtocolInvalid, error))?;
    message.check()?;
    Ok(message)
}

/// Read one message from a line, as a plugin reads it.
pub fn parse_plugin_message(line: &[u8]) -> Result<PluginMessage, PluginError> {
    check_line_length(line)?;
    let message: PluginMessage = serde_json::from_slice(line)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::ProtocolInvalid, error))?;
    Ok(message)
}

/// Turn one message into the line that carries it.
///
/// Returns `None` for a message that cannot be written — which in practice means
/// a value that is not representable in JSON, and the caller's answer is to log
/// and carry on rather than to stop a plugin over one message it could not have
/// been read from anyway.
pub fn write_message<T: Serialize>(message: &T) -> Option<Vec<u8>> {
    serde_json::to_vec(message).ok()
}

/// What a plugin is running, as the plugin center shows it.
///
/// The plugin's own descriptor, plus what the host observed about the process:
/// whether it is alive, and the last thing it said about itself. `running` is a
/// fact rather than an absence, because "this plugin is installed and its process
/// is not running" is a state the user has to be able to see — it is what a crash
/// looks like from outside.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRuntimeStatus {
    pub id: PluginId,
    pub descriptor: PluginDescriptor,
    pub running: bool,
    pub config: ConfigSchema,
    pub values: ConfigDocument,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigControl, ConfigField};
    use crate::descriptor::LocalizedText;

    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("pomodoro").expect("valid"),
            name: "Pomodoro".into(),
            version: PluginVersion::new(1, 0, 0),
            author: String::new(),
            description: LocalizedText::default(),
            icon: Default::default(),
            config: ConfigSchema {
                schema_version: crate::config::CONFIG_SCHEMA_VERSION,
                fields: vec![ConfigField {
                    key: "auto_start".to_string(),
                    label: LocalizedText::from("Auto start"),
                    description: None,
                    control: ConfigControl::Toggle { default: false },
                }],
            },
            draws_panel: true,
            subscriptions: vec![crate::descriptor::Subscription::Input],
        }
    }

    fn hello() -> Hello {
        Hello {
            protocol_version: PROTOCOL_VERSION,
            app_version: "2.0.1".to_string(),
            id: PluginId::new("pomodoro").expect("valid"),
            version: PluginVersion::new(1, 0, 0),
            plugin_dir: "/tmp/p".to_string(),
            data_dir: "/tmp/d".to_string(),
            locale: "en-US".to_string(),
        }
    }

    #[test]
    fn every_message_round_trips_through_one_line() {
        let messages = vec![
            HostMessage::Hello(hello()),
            HostMessage::Tick {
                elapsed_ms: 1234,
                state: HostState::new(Some("Cat".to_string()), true),
                clock: WallClock::new(9, 5, 3),
            },
            HostMessage::Input {
                events: vec![InputEvent::KeyDown {
                    control: "KeyA".to_string(),
                    repeat: false,
                }],
            },
            HostMessage::Press {
                id: "toggle".to_string(),
            },
            HostMessage::ConfigChanged {
                config: ConfigDocument::single(
                    "auto_start",
                    crate::config::ConfigValue::Bool(true),
                ),
            },
            HostMessage::Shutdown,
        ];
        for message in messages {
            let line = write_message(&message).expect("serializes");
            assert!(
                !line.contains(&b'\n'),
                "a message must be one line, or the framing has stopped being a line"
            );
            assert_eq!(parse_host_message(&line).expect("parses"), message);
        }

        let messages = vec![
            PluginMessage::Ready {
                descriptor: Box::new(descriptor()),
            },
            PluginMessage::HidePanel,
            PluginMessage::Actions {
                actions: vec![PluginAction {
                    id: "toggle".to_string(),
                    label: crate::descriptor::LocalizedText::from("Start"),
                    glyph: crate::action::ActionGlyph::Play,
                    disabled: false,
                }],
            },
            PluginMessage::Request {
                id: 7,
                request: Box::new(ModelRequest::PlayMotion {
                    name: "wave".to_string(),
                    restart: false,
                }),
            },
            PluginMessage::Answer(ModelAnswer {
                id: 7,
                outcome: ModelOutcome::Done,
            }),
            PluginMessage::Log {
                level: LogLevel::Warn,
                message: "slow".to_string(),
            },
            PluginMessage::Failed {
                error: PluginError::new(PluginErrorCode::RenderFailed),
            },
            PluginMessage::Shutdown,
        ];
        for message in messages {
            let line = write_message(&message).expect("serializes");
            assert_eq!(parse_plugin_message(&line).expect("parses"), message);
        }
    }

    #[test]
    fn a_protocol_version_this_host_does_not_speak_is_refused_before_anything_else() {
        let mut hello = hello();
        hello.protocol_version = PROTOCOL_VERSION + 1;
        let line = write_message(&HostMessage::Hello(hello)).expect("serializes");
        assert_eq!(
            parse_host_message(&line).unwrap_err().code(),
            PluginErrorCode::ProtocolVersionMismatch
        );
    }

    #[test]
    fn a_line_that_is_not_a_message_is_refused_rather_than_skipped() {
        assert_eq!(
            parse_host_message(b"not json").unwrap_err().code(),
            PluginErrorCode::ProtocolInvalid
        );
        assert_eq!(
            parse_host_message(br#"{"type":"something_new"}"#)
                .unwrap_err()
                .code(),
            PluginErrorCode::ProtocolInvalid,
            "an unknown variant has to look like a mismatch, not like silence"
        );
    }

    #[test]
    fn an_oversized_line_is_refused_on_its_bytes() {
        let line = vec![b'a'; MAXIMUM_MESSAGE_BYTES + 1];
        assert_eq!(
            parse_host_message(&line).unwrap_err().code(),
            PluginErrorCode::ProtocolInvalid
        );
    }

    #[test]
    fn a_descriptor_survives_the_wire_with_its_config_schema_intact() {
        let message = PluginMessage::Ready {
            descriptor: Box::new(descriptor()),
        };
        let line = write_message(&message).expect("serializes");
        let PluginMessage::Ready { descriptor: read } =
            parse_plugin_message(&line).expect("parses")
        else {
            panic!("expected a ready message");
        };
        assert_eq!(*read, descriptor());
        assert!(read.wants(crate::descriptor::Subscription::Input));
    }
}
