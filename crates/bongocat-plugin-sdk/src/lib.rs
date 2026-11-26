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
/// The only keyboard identities here are a small set of physical special keys
/// exposed by the input activity contract. Letters, symbols, text
/// and pointer position still never cross this boundary.
///
/// Every counter is part of the contract: the message is one shape, so a host
/// that omits one is refused rather than read as zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputActivity {
    pub keyboard_down: u64,
    pub keyboard_up: u64,
    pub mouse_down: u64,
    pub mouse_up: u64,
    /// Space presses, a subset of [`Self::keyboard_down`].
    pub space_down: u64,
    /// Space releases, a subset of [`Self::keyboard_up`].
    pub space_up: u64,
    /// Enter presses, a subset of [`Self::keyboard_down`].
    pub enter_down: u64,
    /// Enter releases, a subset of [`Self::keyboard_up`].
    pub enter_up: u64,
    /// Backspace presses, a subset of [`Self::keyboard_down`].
    pub backspace_down: u64,
    /// Backspace releases, a subset of [`Self::keyboard_up`].
    pub backspace_up: u64,
    /// Tab presses, a subset of [`Self::keyboard_down`].
    pub tab_down: u64,
    /// Tab releases, a subset of [`Self::keyboard_up`].
    pub tab_up: u64,
    /// Left Shift presses, a subset of [`Self::keyboard_down`].
    pub shift_left_down: u64,
    /// Left Shift releases, a subset of [`Self::keyboard_up`].
    pub shift_left_up: u64,
    /// Right Shift presses, a subset of [`Self::keyboard_down`].
    pub shift_right_down: u64,
    /// Right Shift releases, a subset of [`Self::keyboard_up`].
    pub shift_right_up: u64,
    /// Left Control presses, a subset of [`Self::keyboard_down`].
    pub ctrl_left_down: u64,
    /// Left Control releases, a subset of [`Self::keyboard_up`].
    pub ctrl_left_up: u64,
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
    /// May publish a latest-value output with reserved sequence zero.
    PushOutputV1,
    Configuration,
    PersistentState,
    /// Keyboard/mouse edge counters with a small physical special-key subset;
    /// message contract version 1.
    ///
    /// The message it gates carries four device totals plus monotonic counters
    /// for the bounded physical special-key categories exposed by this contract.
    /// It carries no letters, symbols, text, pointer position or mouse-button
    /// identity.
    InputActivityV1,
    /// Ordered physical edges, without letters or text; no counter coalescing.
    InputEventsV1,
    /// The plugin may publish bounded [`UiNode::IconText`] display nodes.
    IconTextV1,
    /// The plugin localizes the text it publishes; the host delivers the
    /// resolved catalog tag with [`HostMessage::Locale`].
    ///
    /// The capability is the gate rather than the protocol version, exactly as
    /// [`Capability::InputActivityV1`] gates its message: a plugin declares what
    /// it handles, so the host never sends it a frame it would fail to parse.
    LocalizedViewsV1,
}

/// Privacy-bounded sound groups; ordinary keyboard keys share one identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputEventControl {
    Keyboard,
    Space,
    Enter,
    Backspace,
    Tab,
    ShiftLeft,
    ShiftRight,
    CtrlLeft,
    MouseLeft,
    MouseRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginInputEvent {
    pub control: InputEventControl,
    pub pressed: bool,
    pub captured_ms: u64,
}

/// The name and description one locale shows for a plugin.
///
/// Both belong to the plugin rather than to the catalog: they are the words the
/// plugin's own author wrote, so a catalog that repackaged them could not tell
/// the two apart from the ones it invented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCopy {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// [`Self::name`] and [`Self::description`] are the English copy and are
    /// never repeated here, so one language has exactly one source.
    pub localizations: BTreeMap<String, PluginCopy>,
}

impl<'de> Deserialize<'de> for PluginMetadata {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            id: String,
            name: String,
            description: String,
            version: String,
            app_versions: String,
            protocol_version: u32,
            entry: String,
            icon: PluginIcon,
            capabilities: Vec<Capability>,
            /// A plugin that serves only English declares no other language.
            #[serde(default)]
            localizations: BTreeMap<String, PluginCopy>,
        }
        let Raw {
            id,
            name,
            description,
            version,
            app_versions,
            protocol_version,
            entry,
            icon,
            capabilities,
            mut localizations,
        } = Raw::deserialize(deserializer)?;
        // The manifest's own name and description are the English entry, so a
        // map that restates them would give one language two sources, which is
        // the ambiguity this shape removes.
        if localizations.remove(DEFAULT_LOCALE).is_some() {
            return Err(serde::de::Error::custom(
                "localizations must not restate the manifest's English copy",
            ));
        }
        Ok(Self {
            id,
            name,
            description,
            version,
            app_versions,
            protocol_version,
            entry,
            icon,
            capabilities,
            localizations,
        })
    }
}

impl Serialize for PluginMetadata {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut manifest = serializer.serialize_struct("PluginMetadata", 10)?;
        manifest.serialize_field("id", &self.id)?;
        manifest.serialize_field("name", &self.name)?;
        manifest.serialize_field("description", &self.description)?;
        manifest.serialize_field("version", &self.version)?;
        manifest.serialize_field("app_versions", &self.app_versions)?;
        manifest.serialize_field("protocol_version", &self.protocol_version)?;
        manifest.serialize_field("entry", &self.entry)?;
        manifest.serialize_field("icon", &self.icon)?;
        manifest.serialize_field("capabilities", &self.capabilities)?;
        manifest.serialize_field("localizations", &self.localizations)?;
        manifest.end()
    }
}

impl PluginMetadata {
    /// The name and description to show for `locale`.
    ///
    /// Missing or incomplete translations fall back as a pair, never field by
    /// field, so a card cannot mix languages.
    pub fn localized_copy(&self, locale: &str) -> (&str, &str) {
        let english: &str = &self.name;
        let english_description: &str = &self.description;
        localized_copy(&self.localizations, locale, (english, english_description))
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
    /// A read-only status with an inline icon; never a file path or URL.
    IconText {
        icon: StatusIcon,
        text: String,
    },
    Button {
        id: String,
        label: String,
        /// The glyph the host draws for this action.
        ///
        /// It is the plugin's own statement of what the button does: the host
        /// never guesses an icon from an id or a label, and it never carries a
        /// per-plugin rule.
        icon: ActionIcon,
        /// Whether the button is carrying out the action it names.
        ///
        /// The plugin owns this: it is the plugin that knows a preview is still
        /// playing, so the host draws the control carrying the action instead
        /// of a placeholder line beside it.
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
    /// options are the plugin's own vocabulary — the sounds it carries,
    /// which of them the user may pick — and the host has no way to invent them.
    /// `value` is the option currently selected and always one of `options`, so a
    /// plugin cannot publish a selection the list does not contain.
    Select {
        key: String,
        label: String,
        value: String,
        options: Vec<SelectOption>,
        /// Whether hovering an option asks the plugin to preview it.
        preview: bool,
    },
}

/// Small, bounded status artwork. Pixels are 32x32 straight (not premultiplied)
/// RGBA8. A short language badge is used when a source has no native icon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum StatusIcon {
    Rgba(Vec<u8>),
    Badge(String),
}

impl StatusIcon {
    /// A single neutral-color glyph on transparency is a template. Preserve
    /// multi-tone and colored artwork instead of washing out their details.
    pub fn is_template(&self) -> bool {
        let Self::Rgba(pixels) = self else {
            return false;
        };
        if !self.validate() {
            return false;
        }
        let mut base = None;
        let neutral = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 16)
            .all(|pixel| {
                let first = *base.get_or_insert(pixel[0]);
                pixel[0].abs_diff(pixel[1]) <= 8
                    && pixel[1].abs_diff(pixel[2]) <= 8
                    && first.abs_diff(pixel[0]) <= 16
            });
        neutral && base.is_some()
    }

    pub fn validate(&self) -> bool {
        match self {
            Self::Rgba(pixels) => pixels.len() == 32 * 32 * 4,
            Self::Badge(text) => {
                !text.trim().is_empty()
                    && text.chars().count() <= 3
                    && !text.chars().any(char::is_control)
            }
        }
    }
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
    /// The category the plugin draws this option under.
    ///
    /// A plugin groups its own vocabulary, and `None` means the list is not
    /// grouped. The label is localized by the plugin like every other option
    /// label.
    pub group: Option<String>,
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
    /// Auditioning or controlling a sound, such as previewing a sound.
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
    /// A button carrying `icon` and no in-flight state.
    pub fn button(id: impl Into<String>, label: impl Into<String>, icon: ActionIcon) -> Self {
        Self::Button {
            id: id.into(),
            label: label.into(),
            icon,
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
        let mut ids = std::collections::BTreeSet::new();
        self.nodes.len() <= 16
            && self.nodes.iter().all(|node| match node {
                UiNode::Text { .. } | UiNode::IconText { .. } => true,
                UiNode::Button { id, .. } => ids.insert(id),
                UiNode::Toggle { key, .. }
                | UiNode::Integer { key, .. }
                | UiNode::Select { key, .. } => ids.insert(key),
            })
            && self.nodes.iter().all(|node| match node {
                UiNode::Text { text } => text.len() <= 1024,
                UiNode::IconText { icon, text } => {
                    icon.validate()
                        && !text.trim().is_empty()
                        && text.len() <= 512
                        && !text.chars().any(char::is_control)
                }
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
                    ..
                } => {
                    let mut values = std::collections::BTreeSet::new();
                    valid_key(key)
                        && label.len() <= 128
                        && !options.is_empty()
                        && options.len() <= SELECT_OPTION_LIMIT
                        && options.iter().all(|option| {
                            values.insert(&option.value)
                                && valid_key(&option.value)
                                && option.value.len() <= 128
                                && option.label.len() <= 128
                                && option.group.as_ref().is_none_or(|group| {
                                    !group.trim().is_empty() && group.len() <= 128
                                })
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
    /// Device activity as monotonic totals and special-key counters.
    ///
    /// The host counts unique captured edges per device and direction, plus a
    /// bounded subset of physical special-key categories. The plugin compares them with the ones it saw last. Letters,
    /// symbols, text, pointer position and mouse-button identity do not cross
    /// this message, and repeats and reconciliation do not move any counter.
    ///
    /// Counters rather than events because the boundary is a coalesced,
    /// best-effort signal: the plugin plays a sound for "the keyboard was
    /// released since I last looked", which is what anonymous device activity
    /// represents, and the reliable edge queue never waits behind it (ADR-0091).
    InputEvents {
        events: Vec<PluginInputEvent>,
        overflow: u64,
    },
    InputActivity {
        elapsed_ms: u64,
        activity: InputActivity,
    },
    Action {
        id: String,
    },
    /// Preview one option from a plugin-published select without changing its
    /// configuration. The plugin owns the meaning of the key and value.
    Preview {
        key: String,
        value: String,
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
    /// Current user-facing failures, not view nodes or persisted session data.
    /// Stable ids let the host notify once until recovery, even across locale changes.
    pub errors: Vec<PluginError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginError {
    pub id: String,
    /// Localized by the plugin; never raw OS errors, paths or protocol diagnostics.
    pub message: String,
}

impl PluginOutput {
    pub fn validate(&self) -> bool {
        self.model.validate()
            && self.settings.validate()
            && self.state.len() <= 64 * 1024
            && self.config.len() <= 64
            && self.errors.len() <= 8
            && self.errors.iter().all(|error| {
                valid_key(&error.id)
                    && !error.message.trim().is_empty()
                    && error.message.len() <= 1024
                    && !error.message.chars().any(char::is_control)
            })
            && self
                .errors
                .iter()
                .map(|error| &error.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.errors.len()
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
mod tests;
