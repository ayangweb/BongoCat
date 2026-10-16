//! Versioned process protocol. No Rust ABI or application internals cross it.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// The catalog tag a plugin falls back to when the host has not named one.
///
/// It is spelled with its region the way every shipped catalog is named for the
/// variety its copy was written in, and it is the same tag the application
/// resolves an unknown system language to.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Anonymous device edges, counted since the process started.
///
/// The host owns these counters and a plugin only ever compares them with the
/// values it saw last, which is why they are absolute totals rather than deltas:
/// a coalesced signal that was dropped while the host was busy cannot then be
/// miscounted, and a plugin that started late still sees a consistent picture.
///
/// Nothing about which key, button, modifier or pointer position produced an
/// edge is here — the four numbers are the whole message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputActivity {
    pub keyboard_down: u64,
    pub keyboard_up: u64,
    pub mouse_down: u64,
    pub mouse_up: u64,
}

impl InputActivity {
    /// Whether anything moved since `previous` was read.
    ///
    /// The host polls its own counters, so this is what keeps an idle desktop
    /// from costing a message per poll.
    pub fn advanced_since(self, previous: Self) -> bool {
        self != previous
    }
}

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
    /// Anonymous keyboard/mouse edge counters; message contract version 1.
    ///
    /// It supersedes [`Capability::KeyboardActivityV1`] for a plugin that has to
    /// tell a press from a release, or a key from a mouse button: the message it
    /// gates carries four monotonic counters rather than one, and still carries
    /// no key identity, text, modifiers or button identity.
    InputActivityV1,
    /// The plugin localizes the text it publishes; the host delivers the
    /// resolved catalog tag with [`HostMessage::Locale`].
    ///
    /// The capability is the gate rather than the protocol version, exactly as
    /// [`Capability::KeyboardActivityV1`] gates its message: a plugin built
    /// before this message existed never receives it, so an installed plugin
    /// that has not been updated keeps working instead of failing a frame it
    /// cannot parse.
    LocalizedViewsV1,
}

/// The name and description one locale shows for a plugin.
///
/// Both belong to the plugin rather than to the catalog: they are the words the
/// plugin's own author wrote, so a catalog that repackaged them could not tell
/// the two apart from the ones it invented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCopy {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
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
    /// Per-locale name and description; currently only `zh-CN` is served.
    ///
    /// The mandatory `name` and `description` above are the fallback a locale
    /// this map does not carry reads, so a plugin written before localization —
    /// and a plugin that has only ever had an English name — loads unchanged.
    #[serde(default)]
    pub localizations: BTreeMap<String, PluginCopy>,
}

impl PluginMetadata {
    /// The name and description to show for `locale`.
    ///
    /// Missing or incomplete translations fall back as a pair, never field by
    /// field, so a card cannot mix languages.
    pub fn localized_copy(&self, locale: &str) -> (&str, &str) {
        localized_copy(
            &self.localizations,
            locale,
            (self.name.as_str(), self.description.as_str()),
        )
    }

    /// The supported locale to send to this plugin's views. The metadata map
    /// declares its translated catalogs, keeping the card and controls aligned.
    pub fn view_locale(&self, locale: &str) -> &'static str {
        if localized_entry(&self.localizations, locale).is_some() {
            "zh-CN"
        } else {
            DEFAULT_LOCALE
        }
    }
}

/// Plugins follow the application's resolved language, without a separate
/// preference. Only Simplified Chinese and English are currently supported.
pub fn plugin_locale(locale: &str) -> &'static str {
    if locale.replace('_', "-").eq_ignore_ascii_case("zh-CN") {
        "zh-CN"
    } else {
        DEFAULT_LOCALE
    }
}

/// The name and description a plugin's own locale map serves for a resolved
/// catalog tag, falling back to the copy the manifest always carries.
///
/// It is a free function rather than only a method on [`PluginMetadata`] because
/// the center renders a projection of the plugin — a card carries the map and
/// the fallback without carrying a whole manifest — and both ends have to resolve
/// a tag the same way.
pub fn localized_copy<'a>(
    localizations: &'a BTreeMap<String, PluginCopy>,
    locale: &str,
    fallback: (&'a str, &'a str),
) -> (&'a str, &'a str) {
    localized_entry(localizations, locale).map_or(fallback, |copy| {
        (copy.name.as_str(), copy.description.as_str())
    })
}

/// Only a complete Simplified Chinese entry overrides the English fallback.
fn localized_entry<'a>(
    localizations: &'a BTreeMap<String, PluginCopy>,
    locale: &str,
) -> Option<&'a PluginCopy> {
    if plugin_locale(locale) != "zh-CN" {
        return None;
    }
    localizations
        .iter()
        .find(|(tag, copy)| {
            plugin_locale(tag) == "zh-CN"
                && !copy.name.trim().is_empty()
                && !copy.description.trim().is_empty()
        })
        .map(|(_, copy)| copy)
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
        /// The glyph the host draws for this action, when the plugin names one.
        ///
        /// A plugin that names none gets the host's neutral action glyph: the
        /// host never guesses an icon from an id or a label, and it never
        /// carries a per-plugin rule.
        #[serde(default)]
        icon: Option<ActionIcon>,
        /// Whether the button is carrying out the action it names.
        ///
        /// The plugin owns this: it is the plugin that knows a preview is still
        /// playing. It replaces the "playing…" line such a plugin used to
        /// publish beside the button.
        #[serde(default)]
        loading: bool,
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
    /// One choice out of a list the plugin itself publishes.
    ///
    /// The list travels with the node rather than living in the host because the
    /// options are the plugin's own vocabulary — which typing sounds it carries,
    /// which of them the user may pick — and the host has no way to invent them.
    /// `value` is the option currently selected and always one of `options`, so a
    /// plugin cannot publish a selection the list does not contain.
    Select {
        key: String,
        label: String,
        value: String,
        options: Vec<SelectOption>,
    },
}

/// One option of a [`UiNode::Select`].
///
/// The label is what the row draws and the value is what the plugin is handed
/// back, which is the same split the host applies to every plugin copy: readable
/// text never has to double as an identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// The glyph a plugin asks the host to draw on one of its buttons.
///
/// The set is closed on purpose. The host owns the artwork, a plugin names the
/// meaning, and a name outside this set is a protocol error rather than a
/// free-form string the host would have to resolve against an icon theme it
/// does not share with a separately built process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionIcon {
    Play,
    Pause,
    Restart,
    /// Auditioning or controlling a sound, such as a typing sound's preview.
    Volume,
}

impl ActionIcon {
    /// The snake-case name the host and the plugin both serialize.
    ///
    /// The host keys its own glyph table by this value rather than by matching
    /// the enum, so the two processes agree on a name and neither has to be
    /// rebuilt to read the other's output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Restart => "restart",
            Self::Volume => "volume",
        }
    }
}

impl UiNode {
    /// A button with no declared glyph and no in-flight state.
    ///
    /// A plugin that asks for nothing specific still says what it means: the
    /// value carries an absent icon rather than a host guess, so the host can
    /// keep drawing its neutral action glyph without claiming the button does
    /// something the plugin never said it does.
    pub fn button(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Button {
            id: id.into(),
            label: label.into(),
            icon: None,
            loading: false,
        }
    }
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
                UiNode::Button { id, label, .. } => valid_key(id) && label.len() <= 128,
                UiNode::Toggle { key, label, .. } => valid_key(key) && label.len() <= 128,
                UiNode::Integer {
                    key,
                    label,
                    value,
                    min,
                    max,
                } => valid_key(key) && label.len() <= 128 && min <= value && value <= max,
                UiNode::Select {
                    key,
                    label,
                    value,
                    options,
                } => {
                    valid_key(key)
                        && label.len() <= 128
                        && !options.is_empty()
                        && options.len() <= SELECT_OPTION_LIMIT
                        && options.iter().all(|option| {
                            valid_key(&option.value)
                                && option.value.len() <= 128
                                && option.label.len() <= 128
                        })
                        && options.iter().any(|option| &option.value == value)
                }
            })
    }
}

/// How many options one [`UiNode::Select`] may publish.
///
/// The bound is the same kind of limit as the node count: a settings panel is a
/// handful of controls, and a list longer than this is a catalog the plugin
/// should have split rather than one the host should have to paginate.
pub const SELECT_OPTION_LIMIT: usize = 64;

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
    /// Anonymous device activity, as four monotonic counters.
    ///
    /// The host counts unique captured edges per device and direction; the
    /// plugin compares them with the ones it saw last. Nothing about which key,
    /// button, modifier or pointer position produced an edge crosses this
    /// message, and repeats and reconciliation do not move any counter, so a
    /// plugin cannot reconstruct what was typed from it.
    ///
    /// Counters rather than events because the boundary is a coalesced,
    /// best-effort signal: the plugin plays a sound for "the keyboard was
    /// released since I last looked", which is what a typing sound is, and the
    /// reliable edge queue never waits behind it (ADR-0091).
    InputActivity {
        elapsed_ms: u64,
        activity: InputActivity,
    },
    Action {
        id: String,
    },
    Configure {
        config: Configuration,
    },
    /// The locale the host is showing, so the plugin can localize its own views.
    ///
    /// It is delivered after `Start` and whenever the user changes the display
    /// language; a plugin that never receives one publishes
    /// [`DEFAULT_LOCALE`]'s copy.
    Locale {
        locale: String,
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

    fn localized_metadata() -> PluginMetadata {
        PluginMetadata {
            id: "com.bongocat.pomodoro".into(),
            name: "Pomodoro".into(),
            description: "A focus timer".into(),
            version: "1.0.0".into(),
            app_versions: ">=2.3.1".into(),
            protocol_version: PROTOCOL_VERSION,
            entry: "bongocat-pomodoro".into(),
            icon: PluginIcon::Emoji("🍅".into()),
            capabilities: vec![Capability::LocalizedViewsV1],
            localizations: BTreeMap::from([
                (
                    "zh-CN".to_owned(),
                    PluginCopy {
                        name: "番茄钟".into(),
                        description: "专注计时器".into(),
                    },
                ),
                (
                    "zh-TW".to_owned(),
                    PluginCopy {
                        name: "番茄鐘".into(),
                        description: "專注計時器".into(),
                    },
                ),
            ]),
        }
    }

    #[test]
    fn plugins_serve_only_simplified_chinese_and_english() {
        let mut metadata = localized_metadata();
        metadata.localizations.insert(
            "fr-FR".into(),
            PluginCopy {
                name: "Minuteur".into(),
                description: "Un minuteur".into(),
            },
        );
        for locale in ["zh-CN", "zh_cn", "ZH-cn"] {
            assert_eq!(plugin_locale(locale), "zh-CN");
            assert_eq!(metadata.view_locale(locale), "zh-CN");
            assert_eq!(metadata.localized_copy(locale), ("番茄钟", "专注计时器"));
        }
        for locale in [
            "en-US",
            "en-GB",
            "zh-TW",
            "zh-Hant",
            "zh",
            "zh-Hans-CN",
            "ar-SA",
            "vi-VN",
            "pt-BR",
            "ko-KR",
            "fr-FR",
            "",
            "unknown",
        ] {
            assert_eq!(plugin_locale(locale), DEFAULT_LOCALE);
            assert_eq!(metadata.view_locale(locale), DEFAULT_LOCALE);
            assert_eq!(
                metadata.localized_copy(locale),
                ("Pomodoro", "A focus timer")
            );
        }
    }

    #[test]
    fn missing_or_partial_copy_falls_back_for_the_entire_plugin() {
        for copy in [
            None,
            Some(PluginCopy {
                name: "番茄钟".into(),
                description: " ".into(),
            }),
            Some(PluginCopy {
                name: "".into(),
                description: "专注计时器".into(),
            }),
            Some(serde_json::from_str::<PluginCopy>(r#"{"name":"番茄钟"}"#).unwrap()),
            Some(serde_json::from_str::<PluginCopy>(r#"{"description":"专注计时器"}"#).unwrap()),
        ] {
            let metadata = PluginMetadata {
                localizations: copy
                    .into_iter()
                    .map(|copy| ("zh-CN".into(), copy))
                    .collect(),
                ..localized_metadata()
            };
            assert_eq!(metadata.view_locale("zh-CN"), DEFAULT_LOCALE);
            assert_eq!(
                metadata.localized_copy("zh-CN"),
                ("Pomodoro", "A focus timer")
            );
        }
    }

    /// A button a plugin publishes without asking for a glyph still validates,
    /// and so does one that names a glyph and is carrying it out.
    #[test]
    fn a_button_may_declare_an_icon_and_an_in_flight_state() {
        let view = PluginView {
            nodes: vec![
                UiNode::button("toggle", "Start"),
                UiNode::Button {
                    id: "preview".into(),
                    label: "Preview".into(),
                    icon: Some(ActionIcon::Volume),
                    loading: true,
                },
            ],
            ..Default::default()
        };
        assert!(view.validate());
        // The absent glyph and the in-flight state are both optional, so a
        // frame written before either existed still parses.
        let legacy: PluginView = serde_json::from_str(
            r#"{"anchor":"top_right","nodes":[{"type":"button","id":"toggle","label":"Start"}]}"#,
        )
        .unwrap();
        assert!(legacy.validate());
        assert!(matches!(
            legacy.nodes.as_slice(),
            [UiNode::Button {
                icon: None,
                loading: false,
                ..
            }]
        ));
    }

    /// A select is valid only when the choice it publishes is one of its own
    /// options, so the host never draws a selection the list cannot show.
    #[test]
    fn a_select_carries_its_own_options_and_a_member_value() {
        let node = |value: &str, options: Vec<SelectOption>| UiNode::Select {
            key: "keyboard_pack".into(),
            label: "Keyboard sound".into(),
            value: value.into(),
            options,
        };
        let options = vec![
            SelectOption {
                value: "alps-skcm-blue".into(),
                label: "SKCM Blue · Alps".into(),
            },
            SelectOption {
                value: "kailh-box-navy".into(),
                label: "Box Navy · Kailh".into(),
            },
        ];
        let view = PluginView {
            nodes: vec![node("kailh-box-navy", options.clone())],
            ..Default::default()
        };
        assert!(view.validate());

        for invalid in [
            node("unknown-pack", options.clone()),
            node("kailh-box-navy", Vec::new()),
            node(
                "kailh-box-navy",
                vec![SelectOption {
                    value: "../escape".into(),
                    label: "Box Navy".into(),
                }],
            ),
        ] {
            assert!(
                !PluginView {
                    nodes: vec![invalid],
                    ..Default::default()
                }
                .validate()
            );
        }
    }

    /// The activity message carries counters and nothing else, and a plugin
    /// frames it the same way on both sides of the pipe.
    #[test]
    fn input_activity_round_trips_counters_only() {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Request {
                sequence: 1,
                message: HostMessage::InputActivity {
                    elapsed_ms: 1_000,
                    activity: InputActivity {
                        keyboard_down: 3,
                        keyboard_up: 2,
                        mouse_down: 1,
                        mouse_up: 1,
                    },
                },
            },
        )
        .unwrap();
        let request: Request = read_frame(&mut bytes.as_slice()).unwrap();
        let HostMessage::InputActivity {
            elapsed_ms,
            activity,
        } = request.message
        else {
            panic!("the frame is the activity message it was written as");
        };
        assert_eq!(elapsed_ms, 1_000);
        assert_eq!(
            activity,
            InputActivity {
                keyboard_down: 3,
                keyboard_up: 2,
                mouse_down: 1,
                mouse_up: 1,
            }
        );
        assert!(activity.advanced_since(InputActivity::default()));
        assert!(!activity.advanced_since(activity));
    }
}
