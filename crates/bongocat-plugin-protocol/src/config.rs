//! The settings a plugin wants the user to be able to change.
//!
//! The division of labour here is the whole of the config design, and it is worth
//! stating before the types: **the plugin declares the shape, the host draws the
//! controls, and the plugin keeps the file.** A plugin sends a schema — a closed
//! set of typed fields, each with a default, a range where its kind has one, and a
//! label in the plugin's own copy. The host renders that with the same controls
//! the rest of the settings window uses and sends a typed [`ConfigValue`] back. It
//! never decides what a value means, never writes it anywhere, and has no
//! `plugin` section in `config.json`.
//!
//! That is why the value type is a closed enum rather than `serde_json::Value`.
//! The host does not parse it, but it does *check* it against the field the
//! plugin declared, and a check needs a type it can compare against. An `any`
//! here would put the weakly-typed boundary this whole design removes right back
//! in the middle of it.
//!
//! A field the user edited is sent back as a single-field document, so a plugin
//! that writes its file on every change does not have to be handed a document it
//! already has, and the host does not have to hold a shadow copy that could drift
//! from what the plugin actually persisted.

use super::descriptor::LocalizedText;
use super::error::{PluginError, PluginErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The protocol version this module's wire shape belongs to.
///
/// Bumped by a change to any type in this file. A plugin and a host that disagree
/// about a field's meaning fail to start rather than writing each other's values,
/// which is why it is checked at the handshake and not per message.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// The most fields one plugin may declare.
///
/// A settings form is something a person reads, so this is a design bound rather
/// than a memory one: past about fifty fields it is a documentation page wearing a
/// form's clothes. A plugin with more than this has more than one panel's worth of
/// settings and should be two plugins.
pub const MAXIMUM_CONFIG_FIELDS: usize = 64;

/// The longest a field key may be, in bytes.
///
/// A key becomes a JSON object member in the plugin's own file, so the bound is
/// the one a document can rely on rather than anything about settings.
pub const MAXIMUM_CONFIG_KEY_BYTES: usize = 64;

/// The longest a text value or placeholder may be, in bytes.
pub const MAXIMUM_CONFIG_TEXT_BYTES: usize = 4096;

/// The longest a file path may be, in bytes.
///
/// Its own bound rather than the text one, and larger: a path is longer than a label, and
/// a Windows path under `MAXIMUM_CONFIG_TEXT_BYTES` is a path somebody could genuinely
/// have. Still finite, because it is a string in a JSON document and the host is about to
/// resolve it against the filesystem.
pub const MAXIMUM_CONFIG_PATH_BYTES: usize = 4096;

/// The most extensions one file field may ask the dialog to offer.
///
/// A filter list, not a list a person reads: past a handful the dialog stops narrowing
/// anything, which is the same reason [`MAXIMUM_CHOICE_OPTIONS`] exists for a menu.
pub const MAXIMUM_FILE_EXTENSIONS: usize = 16;

/// The longest one extension in a dialog filter may be.
pub const MAXIMUM_FILE_EXTENSION_BYTES: usize = 16;

/// The most choices one field may offer.
///
/// A `Select` is a menu; a list long enough to need a search field is a different
/// control, and adding one is a change to this file rather than a special case in
/// the settings window.
pub const MAXIMUM_CHOICE_OPTIONS: usize = 64;

/// The longest one choice's value or label may be.
pub const MAXIMUM_CHOICE_TEXT_BYTES: usize = 128;

/// One value a user set.
///
/// A closed set of four scalars, because a settings control produces a scalar.
/// A plugin that needs structure composes it itself — several fields, or a string
/// it parses — which is the right place for it: the plugin is the only side that
/// knows what the value is for.
///
/// # How a value is written
///
/// As the bare JSON scalar it is, and read back by what that scalar is:
/// `true`, `25`, `0.5`, `"meow"`. That is what makes a plugin's own `config.json`
/// a file a person can read and edit, and it round-trips exactly because a JSON
/// number with no fractional part and one with a fractional part are distinguishable.
///
/// The obvious alternative — a tag beside the value, `{"type":"integer","value":25}`
/// — is refused here on purpose. It is more explicit, and it would mean that reading
/// a field is ambiguous for any consumer that does not already hold the schema,
/// which is every consumer that is not the settings window. The kind is not
/// redundant information in practice: a plugin declares one kind per field, and the
/// schema is what the host checks a value against.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    Bool(bool),
    Integer(i64),
    Decimal(f64),
    Text(String),
}

impl Serialize for ConfigValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Integer(value) => serializer.serialize_i64(*value),
            Self::Decimal(value) => serializer.serialize_f64(*value),
            Self::Text(value) => serializer.serialize_str(value),
        }
    }
}

impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `serde_json::Value` first rather than an untagged enum, because an untagged
        // one tries its arms in order and `Integer` would swallow a float on a
        // permissive number format. Reading the number's own text decides instead.
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Bool(value) => Ok(Self::Bool(value)),
            serde_json::Value::String(value) => Ok(Self::Text(value)),
            serde_json::Value::Number(number) => {
                if number.is_f64() && number.as_f64().is_some_and(|value| value.fract() != 0.0) {
                    return Ok(Self::Decimal(number.as_f64().unwrap_or_default()));
                }
                match number.as_i64() {
                    Some(value) => Ok(Self::Integer(value)),
                    None => Ok(Self::Decimal(number.as_f64().unwrap_or_default())),
                }
            }
            other => Err(serde::de::Error::custom(format!(
                "a setting must be a number, a string or true/false, not {other}"
            ))),
        }
    }
}

impl ConfigValue {
    /// The kind of control that produced this value.
    pub const fn kind(&self) -> ConfigKind {
        match self {
            Self::Bool(_) => ConfigKind::Toggle,
            Self::Integer(_) => ConfigKind::Integer,
            Self::Decimal(_) => ConfigKind::Decimal,
            Self::Text(_) => ConfigKind::Text,
        }
    }

    /// Whether this value fits `field`, and by how much it has to move.
    ///
    /// Returns the value to send rather than a bare `bool`, because a plugin that
    /// set a minimum and then received a value below it needs the clamped one —
    /// otherwise the control and the plugin's own file would disagree about what
    /// the user sees and what the user gets.
    pub fn fitted_to(&self, field: &ConfigField) -> Result<ConfigValue, PluginError> {
        let wrong_kind = || {
            PluginError::with_detail(
                PluginErrorCode::InvalidConfigValue,
                format!(
                    "{} is a {} field, not {}",
                    field.key,
                    field.control.kind().as_str(),
                    self.kind().as_str()
                ),
            )
        };
        match (&field.control, self) {
            (ConfigControl::Toggle { .. }, Self::Bool(value)) => Ok(Self::Bool(*value)),
            (
                ConfigControl::Integer {
                    minimum, maximum, ..
                },
                Self::Integer(value),
            ) => Ok(Self::Integer((*value).clamp(*minimum, *maximum))),
            (
                ConfigControl::Decimal {
                    minimum, maximum, ..
                },
                Self::Decimal(value),
            ) => {
                if !value.is_finite() {
                    return Err(PluginError::with_detail(
                        PluginErrorCode::InvalidConfigValue,
                        format!("{} is not a finite number", field.key),
                    ));
                }
                Ok(Self::Decimal(value.clamp(*minimum, *maximum)))
            }
            (
                ConfigControl::Text {
                    maximum_length,
                    multiline,
                    ..
                },
                Self::Text(value),
            ) => {
                if value.len() > MAXIMUM_CONFIG_TEXT_BYTES {
                    return Err(wrong_kind());
                }
                let mut fitted = value.clone();
                if let Some(limit) = maximum_length
                    && fitted.chars().count() > *limit
                {
                    fitted = fitted.chars().take(*limit).collect();
                }
                if !multiline {
                    fitted = fitted.replace(['\n', '\r'], " ");
                }
                Ok(Self::Text(fitted))
            }
            // A path is bounded like any other text, and a newline in one is a space
            // rather than a refused value: a path cannot contain a line break, and a value
            // carrying one was never a path anybody typed. Nothing here checks that the
            // file exists — a path is what the plugin will use, possibly after the user
            // moves the file, and refusing a setting because the file is not there *yet*
            // would make the control impossible to set up.
            (ConfigControl::File { .. }, Self::Text(value)) => {
                if value.len() > MAXIMUM_CONFIG_PATH_BYTES {
                    return Err(wrong_kind());
                }
                Ok(Self::Text(value.replace(['\n', '\r'], " ")))
            }
            (ConfigControl::Choice { options, .. }, Self::Text(value)) => {
                if options.iter().any(|option| option.value == *value) {
                    Ok(Self::Text(value.clone()))
                } else {
                    Err(PluginError::with_detail(
                        PluginErrorCode::InvalidConfigValue,
                        format!("{} has no option named {value:?}", field.key),
                    ))
                }
            }
            _ => Err(wrong_kind()),
        }
    }
}

/// The kind of control a field is edited with.
///
/// A name rather than the control itself, so a caller can ask what a field is
/// without matching on the whole enum — and so adding a control is a variant here
/// and a place that renders it, rather than a shape every caller has to know.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigKind {
    Toggle,
    Integer,
    Decimal,
    Text,
    Choice,
    File,
}

impl ConfigKind {
    pub const ALL: [Self; 6] = [
        Self::Toggle,
        Self::Integer,
        Self::Decimal,
        Self::Text,
        Self::Choice,
        Self::File,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Text => "text",
            Self::Choice => "choice",
            Self::File => "file",
        }
    }
}

/// One choice a `Choice` field offers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    /// What the plugin reads back. A stable key, never localized.
    pub value: String,
    /// What the user sees, in the plugin's own copy.
    pub label: LocalizedText,
}

/// The control a field is edited with, and the bounds it enforces.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigControl {
    /// An on/off switch.
    Toggle {
        #[serde(default)]
        default: bool,
    },
    /// A whole number, with the range and step the spinner uses.
    Integer {
        #[serde(default = "default_zero")]
        default: i64,
        #[serde(default = "default_integer_minimum")]
        minimum: i64,
        #[serde(default = "default_integer_maximum")]
        maximum: i64,
        #[serde(default = "default_step")]
        step: i64,
        /// A suffix shown after the number, in the plugin's own copy — `"min"`,
        /// `"%"`. Never interpreted by the host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<LocalizedText>,
    },
    /// A real number, with the same three bounds.
    Decimal {
        #[serde(default)]
        default: f64,
        #[serde(default)]
        minimum: f64,
        #[serde(default = "default_decimal_maximum")]
        maximum: f64,
        #[serde(default = "default_decimal_step")]
        step: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<LocalizedText>,
    },
    /// A line, or a box.
    Text {
        #[serde(default)]
        default: String,
        /// A hint shown while the field is empty. Never a value: a placeholder that
        /// looked like data would be saved as data.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<LocalizedText>,
        /// Characters allowed. Absent means only the protocol's own bound applies.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum_length: Option<usize>,
        #[serde(default)]
        multiline: bool,
    },
    /// A menu of named options.
    Choice {
        #[serde(default)]
        default: String,
        options: Vec<ChoiceOption>,
    },
    /// A file on this machine, which the user can type the path of or pick from a dialog.
    ///
    /// A text field that opens a dialog, which is the whole of what it adds over
    /// [`Self::Text`]. The two facts are separated deliberately: **the value is a path the
    /// host never interprets, and the extensions are a filter for the dialog rather than a
    /// rule.** A plugin declaring `mp3` is asking to be offered mp3s, not asserting that
    /// the product can decode one — what can be decoded is the host's judgement when the
    /// file is actually used, and a file the dialog let through and the host cannot play
    /// is a file the user chose and was told about, rather than a file the plugin was
    /// stopped from naming.
    ///
    /// The host does check one thing about a path it is asked to act on: whether it is
    /// inside a directory it is willing to open. A plugin cannot make the product fetch a
    /// URL, and a path is the only thing it can name.
    File {
        #[serde(default)]
        default: String,
        /// Extensions the dialog offers, without a leading dot. Empty means the host's own
        /// list for the kind of file, which is better than an unfiltered dialog showing
        /// every file on the machine.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        accept: Vec<String>,
        /// A hint shown while the field is empty, in the plugin's own copy. Never a value:
        /// a placeholder that looked like data would be saved as data.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<LocalizedText>,
    },
}

fn default_zero() -> i64 {
    0
}
fn default_integer_minimum() -> i64 {
    i64::MIN
}
fn default_integer_maximum() -> i64 {
    i64::MAX
}
fn default_step() -> i64 {
    1
}
fn default_decimal_maximum() -> f64 {
    1.0
}
fn default_decimal_step() -> f64 {
    0.1
}

impl ConfigControl {
    pub const fn kind(&self) -> ConfigKind {
        match self {
            Self::Toggle { .. } => ConfigKind::Toggle,
            Self::Integer { .. } => ConfigKind::Integer,
            Self::Decimal { .. } => ConfigKind::Decimal,
            Self::Text { .. } => ConfigKind::Text,
            Self::Choice { .. } => ConfigKind::Choice,
            Self::File { .. } => ConfigKind::File,
        }
    }

    /// The value this control starts at, as declared.
    ///
    /// Deliberately unchecked. A default outside the control's own bounds is a
    /// mistake in the plugin's declaration, and the only side that can fix it is
    /// the plugin's author — so it surfaces as a load failure here rather than as a
    /// control that silently starts somewhere its author did not write.
    pub fn declared_default(&self) -> ConfigValue {
        match self {
            Self::Toggle { default } => ConfigValue::Bool(*default),
            Self::Integer { default, .. } => ConfigValue::Integer(*default),
            Self::Decimal { default, .. } => ConfigValue::Decimal(*default),
            Self::Text { default, .. } => ConfigValue::Text(default.clone()),
            Self::Choice { default, .. } => ConfigValue::Text(default.clone()),
            Self::File { default, .. } => ConfigValue::Text(default.clone()),
        }
    }

    /// Whether this control's bounds are internally consistent.
    pub fn bounds_are_sane(&self) -> bool {
        match self {
            Self::Toggle { .. } => true,
            // Every extension is checked rather than bounded as a list, because a filter
            // the dialog cannot use is a declaration that silently did nothing: a list of
            // extensions containing a path separator or a newline is a plugin that asked
            // for something that is not an extension.
            Self::File { accept, .. } => {
                accept.len() <= MAXIMUM_FILE_EXTENSIONS
                    && accept.iter().all(|extension| {
                        !extension.is_empty()
                            && extension.len() <= MAXIMUM_FILE_EXTENSION_BYTES
                            && extension
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    })
            }
            Self::Integer {
                default,
                minimum,
                maximum,
                step,
                ..
            } => minimum <= maximum && *step > 0 && *default >= *minimum && *default <= *maximum,
            Self::Decimal {
                default,
                minimum,
                maximum,
                step,
                ..
            } => {
                minimum.is_finite()
                    && maximum.is_finite()
                    && default.is_finite()
                    && step.is_finite()
                    && minimum <= maximum
                    && *step > 0.0
                    && *default >= *minimum
                    && *default <= *maximum
            }
            Self::Text {
                default,
                maximum_length,
                ..
            } => {
                default.len() <= MAXIMUM_CONFIG_TEXT_BYTES
                    && maximum_length
                        .is_none_or(|limit| limit > 0 && limit <= MAXIMUM_CONFIG_TEXT_BYTES)
                    && maximum_length.is_none_or(|limit| default.chars().count() <= limit)
            }
            Self::Choice { default, options } => {
                !options.is_empty() && options.iter().any(|option| option.value == *default)
            }
        }
    }
}

/// One setting a plugin exposes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    /// The plugin's own name for this setting.
    ///
    /// A stable key, never localized, and the only thing the host echoes back. It
    /// is what the plugin reads out of its file, so renaming one is a breaking
    /// change for that plugin and its business — which is why the host treats it as
    /// opaque and never derives meaning from it.
    pub key: String,
    /// The setting's name, in the plugin's own copy.
    pub label: LocalizedText,
    /// One line explaining it, in the plugin's own copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LocalizedText>,
    /// The control, and the bounds.
    ///
    /// Nested rather than flattened: a field and its control are two things, and
    /// flattening them would put `default`, `minimum` and `label` in one namespace
    /// where a plugin could not add a control option without colliding with a
    /// property of the field itself.
    pub control: ConfigControl,
}

impl ConfigField {
    /// The default this field starts at, checked against the control it sits in.
    pub fn default_value(&self) -> Result<ConfigValue, PluginError> {
        self.control.declared_default().fitted_to(self)
    }

    /// Bring one value into this field's range.
    pub fn fit(&self, value: &ConfigValue) -> Result<ConfigValue, PluginError> {
        value.fitted_to(self)
    }
}

/// Every setting one plugin exposes, in the order the panel shows them.
///
/// The order is the plugin's, not sorted: a settings form is read top to bottom
/// and the author knows which of their fields is the headline. Keys are sorted
/// only where order does not matter — the document a plugin writes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSchema {
    #[serde(default = "config_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub fields: Vec<ConfigField>,
}

fn config_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

impl Default for ConfigSchema {
    /// An empty schema *at the version this host speaks*.
    ///
    /// Written out rather than derived, because a derived `Default` leaves
    /// `schema_version` at zero — and a descriptor that declares no settings at
    /// all would then be a schema version one ahead of nothing.
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            fields: Vec::new(),
        }
    }
}

impl ConfigSchema {
    /// Check a schema, refusing what the settings window cannot draw.
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(PluginError::new(PluginErrorCode::UnsupportedSchemaVersion));
        }
        if self.fields.len() > MAXIMUM_CONFIG_FIELDS {
            return Err(PluginError::new(PluginErrorCode::TooManyEnabled));
        }
        let mut keys = BTreeSet::new();
        for field in &self.fields {
            if field.key.is_empty()
                || field.key.len() > MAXIMUM_CONFIG_KEY_BYTES
                || !field
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
            }
            if !keys.insert(field.key.clone()) {
                return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
            }
            if !field.control.bounds_are_sane() {
                return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
            }
            // A schema whose own default does not fit its own control is refused
            // here rather than producing a control that starts somewhere the
            // plugin does not expect.
            field.default_value()?;
            if let ConfigControl::Choice { options, .. } = &field.control {
                if options.len() > MAXIMUM_CHOICE_OPTIONS {
                    return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
                }
                let mut values = BTreeSet::new();
                for option in options {
                    if option.value.is_empty()
                        || option.value.len() > MAXIMUM_CHOICE_TEXT_BYTES
                        || option.label.default.len() > MAXIMUM_CHOICE_TEXT_BYTES
                    {
                        return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
                    }
                    if !values.insert(option.value.clone()) {
                        return Err(PluginError::new(PluginErrorCode::InvalidConfigSchema));
                    }
                }
            }
        }
        Ok(())
    }

    /// The field with this key.
    pub fn field(&self, key: &str) -> Option<&ConfigField> {
        self.fields.iter().find(|field| field.key == key)
    }

    /// Every field's default, which is what a plugin is handed before it has read
    /// its own file.
    pub fn defaults(&self) -> ConfigDocument {
        ConfigDocument(
            self.fields
                .iter()
                .filter_map(|field| {
                    field
                        .default_value()
                        .ok()
                        .map(|value| (field.key.clone(), value))
                })
                .collect(),
        )
    }
}

/// A whole set of values, keyed by field.
///
/// A `BTreeMap` because a plugin writes this as its own document and a file whose
/// member order changes on every write is a file that shows a diff the user did
/// not make.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConfigDocument(pub BTreeMap<String, ConfigValue>);

impl ConfigDocument {
    pub fn new(values: BTreeMap<String, ConfigValue>) -> Self {
        Self(values)
    }

    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.0.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ConfigValue)> {
        self.0.iter()
    }

    /// Check every value against the schema, dropping keys the schema does not
    /// name and clamping the rest.
    ///
    /// This is the only place the host touches a value, and all it does is make
    /// the value fit the shape the plugin itself declared. A key that is not in
    /// the schema is dropped rather than refused: it is a field a newer plugin
    /// wrote, or one the user removed from the schema, and a plugin reading its
    /// own file is the only side that can decide what to do about either.
    pub fn fitted_to(&self, schema: &ConfigSchema) -> ConfigDocument {
        let mut fitted = BTreeMap::new();
        for (key, value) in &self.0 {
            let Some(field) = schema.field(key) else {
                continue;
            };
            if let Ok(value) = field.fit(value) {
                fitted.insert(key.clone(), value);
            }
        }
        ConfigDocument(fitted)
    }

    /// Every field's value, with the schema's defaults filling the gaps.
    ///
    /// What the settings window renders: a plugin that has not written a field yet
    /// has a value for it, because the default *is* the value the plugin would
    /// have used.
    pub fn completed_with(&self, schema: &ConfigSchema) -> BTreeMap<String, ConfigValue> {
        let mut complete = schema.defaults().0;
        for (key, value) in &self.0 {
            let Some(field) = schema.field(key) else {
                continue;
            };
            if let Ok(value) = field.fit(value) {
                complete.insert(key.clone(), value);
            }
        }
        complete
    }

    /// A document carrying one field's new value.
    pub fn single(key: impl Into<String>, value: ConfigValue) -> Self {
        let mut values = BTreeMap::new();
        values.insert(key.into(), value);
        Self(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle() -> ConfigField {
        ConfigField {
            key: "auto_start".to_string(),
            label: "Auto start".into(),
            description: None,
            control: ConfigControl::Toggle { default: false },
        }
    }

    fn minutes() -> ConfigField {
        ConfigField {
            key: "minutes".to_string(),
            label: "Minutes".into(),
            description: None,
            control: ConfigControl::Integer {
                default: 25,
                minimum: 1,
                maximum: 120,
                step: 5,
                unit: Some("min".into()),
            },
        }
    }

    fn sound() -> ConfigField {
        ConfigField {
            key: "sound".to_string(),
            label: "Sound".into(),
            description: None,
            control: ConfigControl::Choice {
                default: "meow".to_string(),
                options: vec![
                    ChoiceOption {
                        value: "meow".to_string(),
                        label: "Meow".into(),
                    },
                    ChoiceOption {
                        value: "none".to_string(),
                        label: "Silent".into(),
                    },
                ],
            },
        }
    }

    /// A file field, the way a plugin that plays a sound of the user's own declares one.
    fn sound_file(accept: &[&str]) -> ConfigField {
        ConfigField {
            key: "sound_path".to_string(),
            label: "Audio file".into(),
            description: None,
            control: ConfigControl::File {
                default: String::new(),
                accept: accept
                    .iter()
                    .map(|extension| (*extension).to_owned())
                    .collect(),
                placeholder: None,
            },
        }
    }

    fn schema(fields: Vec<ConfigField>) -> ConfigSchema {
        ConfigSchema {
            schema_version: CONFIG_SCHEMA_VERSION,
            fields,
        }
    }

    #[test]
    fn a_schema_of_ordinary_fields_validates() {
        schema(vec![toggle(), minutes(), sound()])
            .validate()
            .unwrap();
    }

    #[test]
    fn two_fields_may_not_share_a_key() {
        let error = schema(vec![minutes(), minutes()]).validate().unwrap_err();
        assert_eq!(error.code(), PluginErrorCode::InvalidConfigSchema);
    }

    #[test]
    fn a_key_that_is_not_an_identifier_is_refused() {
        for key in ["", "has space", "has.dot", "a/b", &"x".repeat(65)] {
            let mut field = minutes();
            field.key = key.to_string();
            assert_eq!(
                schema(vec![field]).validate().unwrap_err().code(),
                PluginErrorCode::InvalidConfigSchema,
                "{key:?} must be refused"
            );
        }
    }

    #[test]
    fn bounds_that_are_backwards_or_a_zero_step_are_refused() {
        let inverted = ConfigField {
            key: "k".to_string(),
            label: "k".into(),
            description: None,
            control: ConfigControl::Integer {
                default: 5,
                minimum: 10,
                maximum: 1,
                step: 1,
                unit: None,
            },
        };
        assert!(!inverted.control.bounds_are_sane());

        let stationary = ConfigField {
            key: "k".to_string(),
            label: "k".into(),
            description: None,
            control: ConfigControl::Integer {
                default: 5,
                minimum: 0,
                maximum: 10,
                step: 0,
                unit: None,
            },
        };
        assert!(!stationary.control.bounds_are_sane());
    }

    #[test]
    fn a_default_outside_its_own_bounds_is_refused_rather_than_clamped_away() {
        // The plugin declared both, so one of them is a mistake in the plugin and
        // the plugin's author is who can fix it.
        let outside = ConfigField {
            key: "k".to_string(),
            label: "k".into(),
            description: None,
            control: ConfigControl::Integer {
                default: 500,
                minimum: 1,
                maximum: 120,
                step: 1,
                unit: None,
            },
        };
        assert_eq!(
            schema(vec![outside]).validate().unwrap_err().code(),
            PluginErrorCode::InvalidConfigSchema
        );
    }

    #[test]
    fn a_choice_whose_default_is_not_one_of_its_options_is_refused() {
        let dangling = ConfigField {
            key: "k".to_string(),
            label: "k".into(),
            description: None,
            control: ConfigControl::Choice {
                default: "nope".to_string(),
                options: vec![ChoiceOption {
                    value: "meow".to_string(),
                    label: "Meow".into(),
                }],
            },
        };
        assert_eq!(
            schema(vec![dangling]).validate().unwrap_err().code(),
            PluginErrorCode::InvalidConfigSchema
        );
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused() {
        assert_eq!(
            minutes().fit(&ConfigValue::Bool(true)).unwrap_err().code(),
            PluginErrorCode::InvalidConfigValue
        );
        assert_eq!(
            toggle().fit(&ConfigValue::Integer(1)).unwrap_err().code(),
            PluginErrorCode::InvalidConfigValue
        );
    }

    #[test]
    fn an_integer_below_the_minimum_is_clamped_rather_than_refused() {
        // The user typed 0 into a field whose minimum is 1. Refusing would leave
        // the control and the plugin's file disagreeing about what was set; the
        // clamped value is what the user gets.
        assert_eq!(
            minutes().fit(&ConfigValue::Integer(0)).unwrap(),
            ConfigValue::Integer(1)
        );
        assert_eq!(
            minutes().fit(&ConfigValue::Integer(9999)).unwrap(),
            ConfigValue::Integer(120)
        );
    }

    #[test]
    fn a_text_value_is_cut_to_its_own_limit_and_never_carries_a_newline_into_a_line() {
        let field = ConfigField {
            key: "k".to_string(),
            label: "k".into(),
            description: None,
            control: ConfigControl::Text {
                default: String::new(),
                placeholder: None,
                maximum_length: Some(4),
                multiline: false,
            },
        };
        assert_eq!(
            field
                .fit(&ConfigValue::Text("abcdefgh".to_string()))
                .unwrap(),
            ConfigValue::Text("abcd".to_string())
        );
        assert_eq!(
            field.fit(&ConfigValue::Text("a\nb".to_string())).unwrap(),
            ConfigValue::Text("a b".to_string())
        );
    }

    #[test]
    fn a_choice_value_that_is_not_an_option_is_refused() {
        assert_eq!(
            sound()
                .fit(&ConfigValue::Text("moo".to_string()))
                .unwrap_err()
                .code(),
            PluginErrorCode::InvalidConfigValue
        );
        assert_eq!(
            sound().fit(&ConfigValue::Text("meow".to_string())).unwrap(),
            ConfigValue::Text("meow".to_string())
        );
    }

    #[test]
    fn a_document_is_completed_from_the_schema_rather_than_filling_a_field_with_nothing() {
        let schema = schema(vec![toggle(), minutes()]);
        let document = ConfigDocument::single("minutes", ConfigValue::Integer(50));
        let complete = document.completed_with(&schema);
        assert_eq!(complete.get("minutes"), Some(&ConfigValue::Integer(50)));
        assert_eq!(
            complete.get("auto_start"),
            Some(&ConfigValue::Bool(false)),
            "a field with no value has the plugin's own default, not an absence"
        );
    }

    #[test]
    fn a_file_field_is_a_path_the_host_does_not_interpret() {
        // The whole of what the kind adds: a path, bounded like text, with nothing checked
        // about whether the file is there. A file the user has not chosen yet, or one they
        // will choose after setting this up, is a path like any other — refusing it because
        // the bytes are not on the machine right now would make the control impossible to
        // set up at all.
        let field = sound_file(&["mp3", "flac"]);
        let document = ConfigDocument(
            [(
                "sound_path".to_string(),
                ConfigValue::Text("/Users/you/sounds/click.mp3".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            document
                .completed_with(&schema(vec![field.clone()]))
                .get("sound_path"),
            Some(&ConfigValue::Text(
                "/Users/you/sounds/click.mp3".to_string()
            ))
        );
        assert_eq!(field.control.kind(), ConfigKind::File);
    }

    #[test]
    fn an_empty_path_is_a_value_rather_than_a_missing_setting() {
        // The empty case is real: "use the model's own sound" is a path that is blank, and
        // a form that dropped the field would leave the plugin reading a default it never
        // showed the user.
        let document = ConfigDocument(
            [("sound_path".to_string(), ConfigValue::Text(String::new()))]
                .into_iter()
                .collect(),
        );
        assert_eq!(
            document
                .completed_with(&schema(vec![sound_file(&["mp3"])]))
                .get("sound_path"),
            Some(&ConfigValue::Text(String::new()))
        );
    }

    #[test]
    fn a_path_longer_than_a_path_can_be_is_refused_rather_than_stored() {
        let field_default = ConfigValue::Text(String::new());
        let document = ConfigDocument(
            [(
                "sound_path".to_string(),
                ConfigValue::Text("a".repeat(MAXIMUM_CONFIG_PATH_BYTES + 1)),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            document
                .completed_with(&schema(vec![sound_file(&["mp3"])]))
                .get("sound_path"),
            Some(&field_default),
            "because the host is about to resolve it against the filesystem and a megabyte of \
             path is not one — and a refused value reads as the field's own default rather \
             than as a missing setting, which is the same answer as one the user never typed"
        );
    }

    #[test]
    fn a_newline_in_a_path_is_a_space_rather_than_a_line_break() {
        // A path cannot contain a line break, so a value carrying one was never a path
        // anybody typed — and a settings file is a line-oriented document, where a raw
        // newline in a value is a document that cannot be read back.
        let document = ConfigDocument(
            [(
                "sound_path".to_string(),
                ConfigValue::Text("/Users/you/click\n.mp3".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            document
                .completed_with(&schema(vec![sound_file(&["mp3"])]))
                .get("sound_path"),
            Some(&ConfigValue::Text("/Users/you/click .mp3".to_string()))
        );
    }

    #[test]
    fn a_filter_that_is_not_a_list_of_extensions_is_refused() {
        // A filter the dialog cannot use is a declaration that silently did nothing, and a
        // list containing a separator or a newline is a plugin that asked for something
        // that is not an extension at all. It is refused rather than dropped, so the author
        // finds out at load.
        for unusable in [
            vec![""],
            vec!["mp3", ""],
            vec!["../etc"],
            vec!["a/b"],
            vec!["a\nb"],
            vec![&"x".repeat(MAXIMUM_FILE_EXTENSION_BYTES + 1)],
            vec!["mp3"; MAXIMUM_FILE_EXTENSIONS + 1],
        ] {
            let schema = schema(vec![sound_file(&unusable)]);
            assert!(
                schema.validate().is_err(),
                "{unusable:?} is not a list of file extensions"
            );
        }
        assert!(
            schema(vec![sound_file(&["mp3", "flac", "mp3"])])
                .validate()
                .is_ok(),
            "while a list the dialog can use is accepted, and repetition is not a reason to \
             refuse a declaration"
        );
        assert!(
            schema(vec![sound_file(&[])]).validate().is_ok(),
            "and an empty list is the host's own rather than an unfiltered dialog"
        );
    }

    #[test]
    fn a_key_the_schema_does_not_name_is_dropped_rather_than_passed_on() {
        let schema = schema(vec![minutes()]);
        let mut values = BTreeMap::new();
        values.insert("minutes".to_string(), ConfigValue::Integer(30));
        values.insert("removed_setting".to_string(), ConfigValue::Bool(true));
        let document = ConfigDocument(values);
        assert_eq!(
            document
                .completed_with(&schema)
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["minutes".to_string()]
        );
    }
}
