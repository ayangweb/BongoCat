//! What the plugin center shows, and what it can ask for.
//!
//! The window never sees a plugin manifest, a file path, a process handle or a
//! protocol message. It sees a card — a name, an icon, a version, a state — and a
//! **settings form the plugin declared**, which is the one part of a plugin the
//! window genuinely has to understand.
//!
//! That form is a closed set of typed fields rather than anything a plugin may
//! express, and the reason is the same as everywhere else in this protocol: a settings
//! window that renders whatever it is handed cannot be written once. Five controls, a
//! default each and a bound where the kind has one is a page that can be reasoned
//! about; an open vocabulary is a plugin runtime wearing a form's clothes.
//!
//! Two of the fields are load-bearing and easy to miss:
//!
//! * [`SettingsPlugins::available`] is false when the worker would not start. Without
//!   it the page would render an empty list, which reads as "there are no plugins"
//!   rather than "this build cannot run them".
//! * [`SettingsPluginEntry::refusal`] travels with the card rather than replacing it,
//!   so a plugin the catalog offers for a platform this host is not still appears —
//!   greyed, with a reason — instead of silently not existing.

use std::collections::BTreeMap;

/// One plugin's icon, as the card shows it.
///
/// A pair rather than a single name because emoji is what a plugin can write without
/// shipping a file and an image is what it will want next, and adding the second must
/// not be a change to the document every plugin already carries. Exactly one is
/// normally set; when both are, the image wins, because a picture is the more
/// specific answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsPluginIcon {
    /// A short emoji, drawn with the settings window's own font stack.
    pub emoji: Option<String>,
    /// A PNG under the plugin's own directory. Never absolute, never `..` — the host
    /// checked it before this reached the window.
    pub image: Option<String>,
}

impl SettingsPluginIcon {
    /// Whether this icon names nothing at all.
    pub fn is_empty(&self) -> bool {
        self.emoji.is_none() && self.image.is_none()
    }
}

/// Which control one setting is edited with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsFieldKind {
    Toggle,
    Integer,
    Decimal,
    Text,
    Choice,
}

impl SettingsFieldKind {
    pub const ALL: [Self; 5] = [
        Self::Toggle,
        Self::Integer,
        Self::Decimal,
        Self::Text,
        Self::Choice,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Text => "text",
            Self::Choice => "choice",
        }
    }
}

/// One value a user set.
///
/// A closed set of four scalars, because a control produces a scalar. The window
/// never guesses a kind from a value's shape — the field says which kind it is — so a
/// whole number and a real number are told apart by the field rather than by whether
/// the value happens to look integral.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsFieldValue {
    Bool(bool),
    Integer(i64),
    Decimal(f64),
    Text(String),
}

impl SettingsFieldValue {
    pub const fn kind(&self) -> SettingsFieldKind {
        match self {
            Self::Bool(_) => SettingsFieldKind::Toggle,
            Self::Integer(_) => SettingsFieldKind::Integer,
            Self::Decimal(_) => SettingsFieldKind::Decimal,
            Self::Text(_) => SettingsFieldKind::Text,
        }
    }

    /// Whether this value is of the kind `kind` is edited with.
    ///
    /// A choice reads back as text, which is exactly why the kind is read from the
    /// *field* rather than from the value's shape: a text value and a choice's value
    /// are the same bytes, and only the field knows which it is.
    pub fn fits(&self, kind: SettingsFieldKind) -> bool {
        match kind {
            SettingsFieldKind::Toggle => matches!(self, Self::Bool(_)),
            SettingsFieldKind::Integer => matches!(self, Self::Integer(_)),
            SettingsFieldKind::Decimal => matches!(self, Self::Decimal(_)),
            SettingsFieldKind::Text | SettingsFieldKind::Choice => {
                matches!(self, Self::Text(_))
            }
        }
    }
}

/// One option a choice setting offers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsFieldOption {
    /// What the plugin reads back. Stable and never localized.
    pub value: String,
    /// What the user sees, already resolved for their language.
    pub label: String,
}

/// One setting a plugin declared.
///
/// Every field of every control is present on every field rather than being optional
/// per kind, because a settings row that has to ask "does this one have a minimum?"
/// is a settings row that can render a spinner for a toggle. The `kind` says which of
/// them is read. Not `Eq`, because a real-number setting's bounds are not.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsPluginField {
    /// The plugin's own name for this setting. The only thing the window echoes back.
    pub key: String,
    /// The setting's name, in the user's language.
    pub label: String,
    /// One line explaining it, in the user's language.
    pub description: Option<String>,
    pub kind: SettingsFieldKind,
    /// The value this field starts at, or that it reads as when never set.
    pub default: SettingsFieldValue,
    /// Inclusive bounds, for the numeric controls.
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    /// A suffix shown after a number, in the user's language.
    pub unit: Option<String>,
    /// A hint shown while a text field is empty. Never a value.
    pub placeholder: Option<String>,
    /// Whether a text field is a box rather than a line.
    pub multiline: bool,
    /// The options, for a choice.
    pub options: Vec<SettingsFieldOption>,
}

/// One control a plugin wants drawn, as the card draws it.
///
/// The label is already resolved for the user's language, for the reason every other
/// string on this card is: the plugin ships its own copy and the window is the only
/// side that knows which language the user reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsPluginAction {
    /// The id a press sends back to the plugin. Opaque to the window.
    pub id: String,
    pub label: String,
    /// Which of the window's own icons stands for this control.
    pub glyph: SettingsActionGlyph,
    /// Whether the control is present but not pressable.
    pub disabled: bool,
}

/// Which icon a control wears, from the closed set the protocol defines.
///
/// A protocol enum rather than a string or a third name for it: the window's icon set
/// is the product's, and mapping a plugin's meaning onto it is a decision that belongs
/// in one place. Adding an icon here is a change to what every plugin can be offered,
/// so it happens in the protocol next to the set it maps from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsActionGlyph {
    /// No icon. The label is the whole control.
    #[default]
    None,
    Play,
    Pause,
    Reset,
}

impl SettingsActionGlyph {
    pub const ALL: [Self; 4] = [Self::None, Self::Play, Self::Pause, Self::Reset];

    /// Whether this asks for an icon at all.
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// One plugin, as the center lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPluginEntry {
    /// The plugin's own identifier, and the only name the window needs to address it.
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    /// The icon on this card. A plugin that declares none gets a letter, so a grid of
    /// cards is still a grid rather than a grid of blanks.
    pub icon: SettingsPluginIcon,
    /// The version on disk, or `None` when the catalog only offers this plugin.
    pub installed_version: Option<String>,
    /// The version the catalog offers for this host, when it offers one.
    pub available_version: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    /// Whether the plugin's process is alive right now.
    ///
    /// Distinct from `enabled` on purpose: a plugin the user switched on and whose
    /// process has not answered is a state the user has to be able to see, because it
    /// is what a crash looks like from outside.
    pub running: bool,
    /// Whether the installed version is older than the one on offer.
    pub update_available: bool,
    /// The controls this plugin wants the host to draw, in the order it wants them.
    ///
    /// Empty for a plugin that offered none, for one that is not running, and for one
    /// that is not installed — a control is something a running process asked for, so
    /// a card with these on it has a live button the user can press.
    pub actions: Vec<SettingsPluginAction>,
    /// Whether this plugin's settings form is available right now.
    ///
    /// The one fact the card needs that `fields` cannot answer. A plugin's schema
    /// arrives with its handshake, so a stopped plugin has no `fields` — and a card
    /// that hid its settings button in that case left a user who had just installed a
    /// plugin with a delete button and nothing else, and no way to find out why. With
    /// this flag the button is always there and this says whether pressing it will
    /// show a form or turn the plugin on first.
    pub settings_available: bool,
    /// The settings this plugin declared, in the order it declared them.
    pub fields: Vec<SettingsPluginField>,
    /// The current value of every field, defaults filled in.
    ///
    /// Complete rather than partial, so a field the user has never touched reads as
    /// its default and the form has no hole in it.
    pub values: BTreeMap<String, SettingsFieldValue>,
    /// What this plugin has written this run, oldest first. Only the lines a user
    /// would want to read.
    pub log: Vec<String>,
    /// Why this plugin cannot be installed here, when it cannot.
    pub refusal: Option<SettingsPluginRefusal>,
    /// Why the plugin is not running, when it is not.
    pub failure: Option<SettingsPluginError>,
}

impl SettingsPluginEntry {
    /// The value this field currently holds, or its default.
    ///
    /// The fallback is a second reader's guess rather than a hole in the form: the
    /// window and the plugin must not disagree about what a setting is set to.
    pub fn value_of(&self, key: &str) -> Option<&SettingsFieldValue> {
        self.values.get(key).or_else(|| {
            self.fields
                .iter()
                .find(|field| field.key == key)
                .map(|f| &f.default)
        })
    }

    /// The one field a window should show first, when there is one.
    ///
    /// A settings form with forty fields is a documentation page, and the plugin
    /// author knows which of theirs is the headline. This exists so the plugin center
    /// can put that one on the card and the rest behind a control.
    pub fn primary_field(&self) -> Option<&SettingsPluginField> {
        self.fields.first()
    }

    /// The letter a card shows when a plugin declared no icon.
    pub fn initial(&self) -> String {
        self.name
            .chars()
            .find(|character| character.is_alphanumeric())
            .map(|character| character.to_uppercase().to_string())
            .or_else(|| self.id.chars().next().map(|c| c.to_uppercase().to_string()))
            .unwrap_or_else(|| "?".to_string())
    }
}

/// The whole plugin center, as one read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPlugins {
    /// Whether the worker's snapshot has moved since the last read.
    pub revision: u64,
    /// Whether a plugin host is running at all.
    pub available: bool,
    /// Whether the host is installing, removing or refreshing something.
    pub busy: bool,
    /// The plugins, in the order the host lists them.
    pub entries: Vec<SettingsPluginEntry>,
    /// How many panels are on the model window, and how many may be.
    pub active: usize,
    pub maximum_active: usize,
    /// The last failure, kept until something replaces it.
    pub last_error: Option<SettingsPluginError>,
    /// Whether a catalog has been read yet, successfully or not.
    ///
    /// The one field that keeps an empty catalog from rendering as a page that is
    /// still loading. Without it the page cannot answer "is it still reading?" for a
    /// host that has already looked and found nothing — and a page that says
    /// "reading" forever is worse than one that says "empty", because it tells the
    /// user the product is working when it has already finished.
    pub catalog_read: bool,
}

impl SettingsPlugins {
    /// Whether the catalog has not been read yet, so there is genuinely nothing to
    /// show.
    ///
    /// Deliberately not "the list is empty": an empty list is an answer, and only an
    /// unread catalog is a question. A read that *failed* is also an answer, and it
    /// arrives with `last_error` set rather than here.
    pub fn is_pending(&self) -> bool {
        self.available && !self.catalog_read && !self.busy
    }

    /// The entry for one plugin.
    pub fn entry(&self, id: &str) -> Option<&SettingsPluginEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Whether any installed plugin has settings, which is what decides whether the
    /// page needs a form at all.
    pub fn any_has_settings(&self) -> bool {
        self.entries.iter().any(|entry| !entry.fields.is_empty())
    }
}

/// Why one plugin cannot be installed on this host.
///
/// A code rather than a sentence, because the window is the only side that knows the
/// user's language and the product is the only side that knows what went wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsPluginRefusal {
    pub code: SettingsPluginErrorCode,
    /// The specific thing, when the code alone is not specific enough to be useful —
    /// an unsupported platform triple, a minimum app version, a missing asset.
    pub detail: Option<String>,
}

/// A plugin operation that failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsPluginError {
    pub code: SettingsPluginErrorCode,
    /// What the failure was about, when it had a subject. Never a path and never a
    /// key sequence: the window has no use for either, and both belong in the log.
    pub detail: Option<String>,
}

impl SettingsPluginError {
    pub fn new(code: SettingsPluginErrorCode) -> Self {
        Self { code, detail: None }
    }
}

/// Why a plugin operation was refused.
///
/// The protocol's own codes, grouped into the sentences a window can actually write.
/// Grouped rather than restated one-for-one: two lists of the same codes would drift,
/// and the one that drifted would be the one the window could not show a sentence for.
/// The grouping is the contract — a code added to the host without a home here is a
/// message that never appears.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPluginErrorCode {
    /// The host is not running, so nothing can be asked of it.
    HostUnavailable,
    /// The catalog does not offer this plugin for this host, or it is not installed.
    NotPublished,
    /// The plugin is already installed at the version the catalog offers.
    AlreadyInstalled,
    /// The catalog could not be read, from any source.
    CatalogUnavailable,
    /// The transfer failed or timed out.
    NetworkUnavailable,
    /// The archive did not match the digest the catalog announced.
    ChecksumMismatch,
    /// The archive's signature did not verify against the release key.
    SignatureInvalid,
    /// The archive, or the store, could not be written or read.
    StoreWriteFailed,
    /// Enough panels are already on the model window.
    TooManyEnabled,
    /// A plugin's own descriptor or settings were not valid.
    InvalidManifest,
    /// The panel could not be laid out or rasterized.
    RenderFailed,
    /// The plugin's process could not be started, or did not answer.
    PluginFailed,
    /// Anything the host reported that this list has not grown into.
    ///
    /// Not an omission to be tidied away: a host code added without a sentence here
    /// must still produce a message rather than an unreachable page.
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, kind: SettingsFieldKind) -> SettingsPluginField {
        SettingsPluginField {
            key: key.to_string(),
            label: key.to_string(),
            description: None,
            kind,
            default: match kind {
                SettingsFieldKind::Toggle => SettingsFieldValue::Bool(false),
                SettingsFieldKind::Integer => SettingsFieldValue::Integer(1),
                SettingsFieldKind::Decimal => SettingsFieldValue::Decimal(0.5),
                _ => SettingsFieldValue::Text(String::new()),
            },
            minimum: None,
            maximum: None,
            step: None,
            unit: None,
            placeholder: None,
            multiline: false,
            options: Vec::new(),
        }
    }

    #[test]
    fn only_an_unread_catalog_is_pending() {
        // The distinction the field exists for. "The list is empty" was the old test,
        // and it made a catalog that had been read and had nothing in it look like a
        // read still in progress — which is a page that says "loading" forever.
        let unread = SettingsPlugins {
            available: true,
            ..SettingsPlugins::default()
        };
        assert!(unread.is_pending(), "a catalog nobody has read is pending");

        let read_and_empty = SettingsPlugins {
            available: true,
            catalog_read: true,
            ..SettingsPlugins::default()
        };
        assert!(
            !read_and_empty.is_pending(),
            "a read catalog with nothing in it is an answer, not a question"
        );

        // Entries in the list are not what settles the page. Installed plugins are
        // listed from disk before any catalog is read, so a host with three of them
        // and no catalog is still waiting for the catalog.
        let read_with_entries = SettingsPlugins {
            available: true,
            catalog_read: true,
            entries: vec![SettingsPluginEntry {
                id: "pomodoro".to_string(),
                ..SettingsPluginEntry::default()
            }],
            ..SettingsPlugins::default()
        };
        assert!(!read_with_entries.is_pending());
    }

    #[test]
    fn a_degraded_host_is_never_pending() {
        let plugins = SettingsPlugins::default();
        assert!(
            !plugins.is_pending(),
            "no host is a failure to report, not a catalog to wait for"
        );
    }

    #[test]
    fn a_failed_read_is_never_pending() {
        // A read that failed *was* a read. Without `catalog_read` on this path the
        // host would leave it false and the page would show a loading state for a
        // catalog it had already given up on.
        let plugins = SettingsPlugins {
            available: true,
            catalog_read: true,
            last_error: Some(SettingsPluginError::new(
                SettingsPluginErrorCode::CatalogUnavailable,
            )),
            ..SettingsPlugins::default()
        };
        assert!(!plugins.is_pending(), "a failure is shown, not waited on");
    }

    #[test]
    fn a_refresh_in_flight_keeps_the_previous_answer_on_screen() {
        let plugins = SettingsPlugins {
            available: true,
            busy: true,
            catalog_read: true,
            entries: vec![SettingsPluginEntry {
                id: "pomodoro".to_string(),
                ..SettingsPluginEntry::default()
            }],
            ..SettingsPlugins::default()
        };
        assert!(
            !plugins.is_pending(),
            "a refresh is not a pending read; the list the user is reading stays"
        );
    }

    #[test]
    fn a_field_the_user_never_touched_reads_as_the_plugins_own_default() {
        // The window and the plugin must not disagree about what a setting is set to,
        // so a missing value falls back rather than showing a hole.
        let entry = SettingsPluginEntry {
            fields: vec![field("minutes", SettingsFieldKind::Integer)],
            values: BTreeMap::new(),
            ..SettingsPluginEntry::default()
        };
        assert_eq!(
            entry.value_of("minutes"),
            Some(&SettingsFieldValue::Integer(1)),
            "and it is the field's default, not an absence"
        );
        assert_eq!(entry.value_of("nope"), None);
    }

    #[test]
    fn a_plugins_first_field_is_the_one_a_card_shows() {
        let entry = SettingsPluginEntry {
            fields: vec![
                field("minutes", SettingsFieldKind::Integer),
                field("auto_start", SettingsFieldKind::Toggle),
            ],
            ..SettingsPluginEntry::default()
        };
        assert_eq!(
            entry.primary_field().map(|field| field.key.as_str()),
            Some("minutes"),
            "because a form with forty fields is a documentation page, and the author knows \\
             which of theirs is the headline"
        );
    }

    #[test]
    fn a_card_with_no_icon_gets_a_letter_rather_than_a_blank() {
        let entry = SettingsPluginEntry {
            name: "Pomodoro".to_string(),
            ..SettingsPluginEntry::default()
        };
        assert_eq!(entry.initial(), "P");
        assert!(entry.icon.is_empty());

        let no_name = SettingsPluginEntry {
            id: "key-stats".to_string(),
            ..SettingsPluginEntry::default()
        };
        assert_eq!(
            no_name.initial(),
            "K",
            "and an id is the second answer, because a plugin always has one"
        );
    }

    #[test]
    fn a_choice_field_is_matched_by_its_text_value() {
        let mut choice = field("sound", SettingsFieldKind::Choice);
        choice.options = vec![SettingsFieldOption {
            value: "meow".to_string(),
            label: "Meow".to_string(),
        }];
        assert!(
            SettingsFieldValue::Text("meow".to_string()).fits(choice.kind),
            "a choice reads back as text, which is why the kind is read from the field rather \\
             than from the value's shape"
        );
        assert!(
            !SettingsFieldValue::Integer(1).fits(choice.kind),
            "and a number is not a choice"
        );
    }
}
