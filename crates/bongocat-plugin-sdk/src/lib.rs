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
    pub name: String,
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
    /// Per-locale name and description, keyed by locale tag (`zh-CN`, `zh-TW`).
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
    /// `locale` is a catalog tag the host resolved, so the whole tag is what a
    /// plugin declares and what is matched first. A plugin that declared only
    /// the primary subtag — `vi` for a host sending `vi-VN` — is still served,
    /// because that is the RFC 4647 fallback every other catalog follows.
    /// Chinese is the one language this cannot decide: Simplified and
    /// Traditional are different written forms, so a manifest has to name
    /// `zh-CN` and `zh-TW` separately rather than relying on `zh`.
    pub fn localized_copy(&self, locale: &str) -> (&str, &str) {
        localized_copy(
            &self.localizations,
            locale,
            (self.name.as_str(), self.description.as_str()),
        )
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

/// The entry a plugin's own locale map serves for a resolved catalog tag.
///
/// The tag is matched whole first, then by its primary subtag. Chinese is the
/// one case the primary subtag cannot settle: Simplified and Traditional are
/// different written forms rather than regional variants, so a `zh` tag is
/// resolved by script subtag instead, and a manifest has to name `zh-CN` and
/// `zh-TW` separately rather than relying on `zh`.
fn localized_entry<'a>(
    localizations: &'a BTreeMap<String, PluginCopy>,
    locale: &str,
) -> Option<&'a PluginCopy> {
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    if let Some(copy) = localizations.get(&normalized) {
        return Some(copy);
    }
    let subtags = normalized.split('-').collect::<Vec<_>>();
    let primary = subtags.first().copied().unwrap_or("");
    if primary.is_empty() {
        return None;
    }
    if primary == "zh" {
        // The host already resolves a `zh` tag before it reaches a plugin, so
        // this only serves a caller that resolved the script itself. `hant` is
        // the one subtag that separates the written forms; without it the tag
        // reads as Simplified, exactly as the application's own resolution does.
        let wanted = if is_traditional_chinese(&normalized) {
            "zh-TW"
        } else {
            "zh-CN"
        };
        return localizations.get(wanted);
    }
    localizations
        .iter()
        .find(|(tag, _)| tag.split('-').next() == Some(primary) && tag.as_str() != "zh")
        .map(|(_, copy)| copy)
}

/// Whether a resolved catalog tag names the Traditional Chinese written form.
///
/// Chinese is the one language an RFC 4647 primary-subtag fallback cannot
/// settle — Simplified and Traditional are different written forms rather than
/// regional variants — so every place that picks Chinese copy has to ask the
/// script subtags rather than the region. The application resolves the tag
/// before it ever reaches a plugin, and a plugin asks this of the tag it was
/// handed, so both ends separate the scripts the same way.
pub fn is_traditional_chinese(locale: &str) -> bool {
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    let mut subtags = normalized.split('-');
    if subtags.next() != Some("zh") {
        return false;
    }
    subtags.any(|subtag| matches!(subtag, "hant" | "tw" | "hk" | "mo"))
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

    /// A whole-tag match wins, and Chinese resolves by script rather than by the
    /// primary subtag: these are different written forms, not regional variants.
    #[test]
    fn localization_matches_the_whole_tag_and_separates_the_chinese_scripts() {
        let metadata = localized_metadata();
        assert_eq!(metadata.localized_copy("zh-CN").0, "番茄钟");
        assert_eq!(metadata.localized_copy("zh-TW").0, "番茄鐘");
        // A script-qualified tag lands on the catalog that carries the script,
        // and a bare `zh` reads as Simplified — the same rule the application
        // applies before it ever hands a plugin a tag.
        assert_eq!(metadata.localized_copy("zh_Hans_CN").0, "番茄钟");
        assert_eq!(metadata.localized_copy("zh").0, "番茄钟");
        assert_eq!(metadata.localized_copy("zh-Hant").0, "番茄鐘");
    }

    /// A locale the plugin did not translate reads the manifest's own copy
    /// rather than an empty string or another locale's text.
    #[test]
    fn an_undeclared_locale_reads_the_manifest_fallback() {
        let metadata = localized_metadata();
        assert_eq!(metadata.localized_copy("fr-FR").0, "Pomodoro");

        // A plugin written before localization declares no map at all and still
        // resolves every locale.
        let legacy = PluginMetadata {
            localizations: BTreeMap::new(),
            ..localized_metadata()
        };
        assert_eq!(legacy.localized_copy("zh-CN").0, "Pomodoro");
    }

    /// A plugin that translated only one Chinese script serves only that script,
    /// rather than showing the other script's text to a reader of the first.
    #[test]
    fn a_single_chinese_script_does_not_serve_the_other() {
        let metadata = PluginMetadata {
            localizations: BTreeMap::from([(
                "zh-TW".to_owned(),
                PluginCopy {
                    name: "番茄鐘".into(),
                    description: "專注計時器".into(),
                },
            )]),
            ..localized_metadata()
        };
        assert_eq!(metadata.localized_copy("zh-TW").0, "番茄鐘");
        assert_eq!(metadata.localized_copy("zh-CN").0, "Pomodoro");
    }

    /// A manifest that declared only a primary subtag serves the host's tagged
    /// locale, which is the RFC 4647 fallback every other catalog follows.
    #[test]
    fn a_primary_subtag_serves_its_regional_locale() {
        let metadata = PluginMetadata {
            localizations: BTreeMap::from([(
                "vi".to_owned(),
                PluginCopy {
                    name: "Tiếng gõ".into(),
                    description: "Âm thanh khi gõ".into(),
                },
            )]),
            ..localized_metadata()
        };
        assert_eq!(metadata.localized_copy("vi-VN").0, "Tiếng gõ");
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
