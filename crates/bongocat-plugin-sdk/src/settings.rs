//! Declaring the settings a plugin wants, and reading and writing its own values.
//!
//! The division of labour is the whole of the config design and it is worth stating
//! before the types: **the plugin declares the shape, the host draws the controls,
//! and the plugin keeps the file.** A plugin builds a [`Settings`] out of typed
//! fields, hands it to [`Descriptor::settings`], and from then on the settings
//! window renders it with the same controls the rest of BongoCat's settings use.
//! When the user changes one, the host sends the whole document back and this
//! crate writes it — atomically, privately — into the plugin's own data directory.
//!
//! What the host does *not* do is interpret anything. It checks that a value fits
//! the field the plugin declared and sends it back; it never decides what a value
//! means, never writes it, and has no plugin section in `config.json`. So a plugin
//! that renames a field, changes a range, or wants a value the host has no control
//! for is only ever a change inside the plugin.
//!
//! # Reading a value
//!
//! Three ways, and each answers a different question:
//!
//! * [`Values::get`] — the raw typed value, for a plugin that wants to branch on
//!   the kind.
//! * [`Values::integer`] and friends — the value as a concrete type, falling back
//!   to the field's default when the key is absent. **This is what a plugin should
//!   normally use**: a field the user has never touched reads as its default
//!   rather than as a missing value, so a plugin never has to write the same
//!   `unwrap_or` twice.
//! * [`Values::decode`] — the document as the plugin's own struct, through
//!   `serde`. For a plugin whose settings are one coherent thing rather than a set
//!   of independent switches.

use crate::Error;
use bongocat_plugin_protocol::{
    ChoiceOption, ConfigControl, ConfigDocument, ConfigField, ConfigKind, ConfigSchema,
    ConfigValue, LocalizedText, MAXIMUM_CONFIG_FIELDS,
};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The settings one plugin exposes.
///
/// Built by adding fields, then handed to [`crate::Descriptor::settings`]. The
/// order fields were added in is the order the panel shows them, because a settings
/// form is read top to bottom and the author knows which of their fields is the
/// headline.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    fields: Vec<ConfigField>,
}

impl Settings {
    /// Settings with nothing in them.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a field.
    ///
    /// Takes the field rather than the pieces so an author writes one call per
    /// setting with a name that reads as a sentence, and so a mistyped key is a
    /// compile error rather than a schema the host refuses at runtime.
    pub fn add(&mut self, field: Field) -> &mut Self {
        self.fields.push(field.into());
        self
    }

    /// Declare a field and keep the builder.
    pub fn with(mut self, field: Field) -> Self {
        self.add(field);
        self
    }

    /// Declare a field from a value that turns into one.
    ///
    /// The ergonomic form: a plugin's settings are usually built from a few typed
    /// values, and `Toggle::new("auto_start", "Start automatically")` in a `with`
    /// chain reads better than the same call through `Field`.
    pub fn with_value(&mut self, field: impl Into<Field>) -> &mut Self {
        self.add(field.into())
    }

    /// How many fields this plugin exposes.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The protocol's schema, checked.
    ///
    /// Checked here rather than left to the host so a plugin's own test run catches
    /// an out-of-range default — and so the author sees which field, rather than
    /// seeing the host refuse to start the plugin.
    pub fn to_schema(&self) -> Result<ConfigSchema, Error> {
        if self.fields.len() > MAXIMUM_CONFIG_FIELDS {
            return Err(Error::Config(format!(
                "{} settings is past the bound of {MAXIMUM_CONFIG_FIELDS}; a settings form that long \
                 is two plugins",
                self.fields.len()
            )));
        }
        let schema = ConfigSchema {
            schema_version: bongocat_plugin_protocol::CONFIG_SCHEMA_VERSION,
            fields: self.fields.clone(),
        };
        schema.validate().map_err(|error| {
            Error::Config(format!("this plugin's settings are not valid: {error}"))
        })?;
        Ok(schema)
    }
}

/// One declared setting, before it is part of a [`Settings`].
///
/// Named after the control it is, rather than a generic `Field::new` with a kind
/// argument, so a plugin reads as a list of what the user sees.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    control: ConfigControl,
}

impl Field {
    /// A field with an explicit control, for the cases the named constructors do
    /// not cover.
    pub fn new(key: &str, label: &str, control: ConfigControl) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            control,
        }
    }

    /// This field, with a line explaining it under the control.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }

    /// This field, with a label resolved for several languages.
    ///
    /// The default is what a language with no entry shows, so a plugin that has copy
    /// for one language and not another degrades to something readable rather than
    /// to a key.
    pub fn localized(mut self, label: LocalizedText) -> Self {
        self.label = label;
        self
    }

    /// The key the plugin reads this value back under.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The kind of control this field is edited with.
    pub fn kind(&self) -> ConfigKind {
        self.control.kind()
    }

    /// This field's default, which is also what a value that has never been set
    /// reads as.
    pub fn default_value(&self) -> ConfigValue {
        self.control.declared_default()
    }
}

impl From<Field> for ConfigField {
    fn from(field: Field) -> Self {
        ConfigField {
            key: field.key,
            label: field.label,
            description: field.description,
            control: field.control,
        }
    }
}

/// An on/off setting.
#[derive(Clone, Debug, PartialEq)]
pub struct Toggle {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    default: bool,
}

impl Toggle {
    /// A switch that starts off.
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default: false,
        }
    }

    /// This switch, starting on.
    pub fn on_by_default(key: &str, label: &str) -> Self {
        Self {
            default: true,
            ..Self::new(key, label)
        }
    }

    /// This switch, with a line explaining it.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<Toggle> for Field {
    fn from(toggle: Toggle) -> Self {
        Field {
            key: toggle.key,
            label: toggle.label,
            description: toggle.description,
            control: ConfigControl::Toggle {
                default: toggle.default,
            },
        }
    }
}

/// A whole-number setting, with a range and a step.
#[derive(Clone, Debug, PartialEq)]
pub struct Integer {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    default: i64,
    minimum: i64,
    maximum: i64,
    step: i64,
    unit: Option<LocalizedText>,
}

impl Integer {
    /// A whole number with no bound and a default of zero.
    ///
    /// Almost always the wrong thing: a boundless spinner is a number whose
    /// meaning only the plugin knows, so prefer [`Self::ranged`] unless the value
    /// genuinely has no upper limit.
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default: 0,
            minimum: i64::MIN,
            maximum: i64::MAX,
            step: 1,
            unit: None,
        }
    }

    /// A whole number between `minimum` and `maximum`, starting at `default`.
    ///
    /// The order is default first because it is the one the author is thinking
    /// about: "twenty-five minutes, and it can be anything from one to a hundred
    /// and twenty".
    pub fn ranged(key: &str, label: &str, default: i64, minimum: i64, maximum: i64) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default,
            minimum,
            maximum,
            step: 1,
            unit: None,
        }
    }

    /// This setting's step.
    pub fn stepping(mut self, step: i64) -> Self {
        self.step = step;
        self
    }

    /// This setting's default.
    pub fn defaulting(mut self, default: i64) -> Self {
        self.default = default;
        self
    }

    /// A suffix shown after the number, in the plugin's own copy.
    pub fn with_unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// This setting, with a line explaining it.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<Integer> for Field {
    fn from(integer: Integer) -> Self {
        Field {
            key: integer.key,
            label: integer.label,
            description: integer.description,
            control: ConfigControl::Integer {
                default: integer.default,
                minimum: integer.minimum,
                maximum: integer.maximum,
                step: integer.step,
                unit: integer.unit,
            },
        }
    }
}

/// A real-number setting, with a range and a step.
#[derive(Clone, Debug, PartialEq)]
pub struct Decimal {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    default: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    unit: Option<LocalizedText>,
}

impl Decimal {
    /// A real number between `minimum` and `maximum`, starting at `default`.
    pub fn ranged(key: &str, label: &str, default: f64, minimum: f64, maximum: f64) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default,
            minimum,
            maximum,
            step: 0.1,
            unit: None,
        }
    }

    /// This setting's step.
    pub fn stepping(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// A suffix shown after the number.
    pub fn with_unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// This setting, with a line explaining it.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<Decimal> for Field {
    fn from(decimal: Decimal) -> Self {
        Field {
            key: decimal.key,
            label: decimal.label,
            description: decimal.description,
            control: ConfigControl::Decimal {
                default: decimal.default,
                minimum: decimal.minimum,
                maximum: decimal.maximum,
                step: decimal.step,
                unit: decimal.unit,
            },
        }
    }
}

/// A text setting.
#[derive(Clone, Debug, PartialEq)]
pub struct TextField {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    default: String,
    placeholder: Option<LocalizedText>,
    maximum_length: Option<usize>,
    multiline: bool,
}

impl TextField {
    /// A single line, empty.
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default: String::new(),
            placeholder: None,
            maximum_length: None,
            multiline: false,
        }
    }

    /// This setting's default.
    pub fn defaulting(mut self, default: &str) -> Self {
        self.default = default.to_string();
        self
    }

    /// A hint shown while the field is empty. Never a value: a placeholder that
    /// looked like data would be saved as data.
    pub fn placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// How many characters this field accepts.
    pub fn limited_to(mut self, characters: usize) -> Self {
        self.maximum_length = Some(characters);
        self
    }

    /// A box rather than a line.
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// This setting, with a line explaining it.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<TextField> for Field {
    fn from(field: TextField) -> Self {
        Field {
            key: field.key,
            label: field.label,
            description: field.description,
            control: ConfigControl::Text {
                default: field.default,
                placeholder: field.placeholder,
                maximum_length: field.maximum_length,
                multiline: field.multiline,
            },
        }
    }
}

/// One option a [`Choice`] offers.
///
/// A separate type from a tuple because a plugin's option list is data it will
/// usually also match against, and `("meow", "Meow")` at every use site is a pair
/// with two meanings and no names.
#[derive(Clone, Debug, PartialEq)]
pub struct Option_ {
    /// What the plugin reads back. Stable and never localized.
    pub value: String,
    /// What the user sees.
    pub label: LocalizedText,
}

impl Option_ {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.into(),
        }
    }
}

impl From<&str> for Option_ {
    fn from(value: &str) -> Self {
        Self::new(value, value)
    }
}

/// A menu of named options.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    key: String,
    label: LocalizedText,
    description: Option<LocalizedText>,
    default: String,
    options: Vec<Option_>,
}

impl Choice {
    /// A menu with these options, starting at the first.
    ///
    /// The first option is the default because a menu whose default is not in it is
    /// a schema the host refuses, and a plugin author writing the list top to
    /// bottom means the first one.
    pub fn new(key: &str, label: &str, options: Vec<Option_>) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            description: None,
            default: options
                .first()
                .map(|option| option.value.clone())
                .unwrap_or_default(),
            options,
        }
    }

    /// This menu, starting at this option.
    pub fn defaulting(mut self, value: &str) -> Self {
        self.default = value.to_string();
        self
    }

    /// This menu, with a line explaining it.
    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }

    /// This menu's options, in the order the panel shows them.
    pub fn options(&self) -> &[Option_] {
        &self.options
    }
}

impl From<Choice> for Field {
    fn from(choice: Choice) -> Self {
        Field {
            key: choice.key,
            label: choice.label,
            description: choice.description,
            control: ConfigControl::Choice {
                default: choice.default,
                options: choice
                    .options
                    .into_iter()
                    .map(|option| ChoiceOption {
                        value: option.value,
                        label: option.label,
                    })
                    .collect(),
            },
        }
    }
}

/// Where a plugin's own settings live, and what is in them.
///
/// The path is inside the plugin's data directory, which the host created and never
/// writes inside. Two files rather than one, deliberately: `config.json` holds what
/// the user set, and `state.json` is where a plugin puts anything else it needs to
/// remember between runs. Keeping them apart means "reset my settings" is one
/// unlink and a plugin never has to decide which half of its state the user meant.
#[derive(Clone, Debug)]
pub struct Store {
    directory: PathBuf,
    config_path: PathBuf,
    state_path: PathBuf,
}

impl Store {
    /// The store for a plugin's data directory, which must already exist.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        Self {
            config_path: directory.join("config.json"),
            state_path: directory.join("state.json"),
            directory,
        }
    }

    /// The directory this store owns.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The file the user's settings are in.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// The file a plugin's own state is in.
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Read the user's settings, or the defaults when there are none.
    ///
    /// A file that will not parse is a **failure**, not a fallback: a plugin that
    /// silently reverted to defaults would show a user their twenty-five minutes
    /// becoming one, with nothing to say why. The host reports it and the plugin
    /// stops, which is a state the user can act on.
    pub fn read_values(&self, schema: &ConfigSchema) -> Result<Values, Error> {
        match std::fs::read(&self.config_path) {
            Ok(bytes) => {
                let document: ConfigDocument = serde_json::from_slice(&bytes).map_err(|error| {
                    Error::Config(format!("{}: {error}", self.config_path.display()))
                })?;
                Ok(Values::new(schema, document.completed_with(schema)))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Values::new(schema, schema.defaults().0))
            }
            Err(error) => Err(Error::Io {
                path: self.config_path.display().to_string(),
                detail: error.to_string(),
            }),
        }
    }

    /// Write the user's settings, atomically and privately.
    pub fn write_values(&self, values: &Values) -> Result<(), Error> {
        self.write_document(&values.document)
    }

    /// Write a document the plugin built itself.
    pub fn write_document(&self, document: &ConfigDocument) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|error| Error::Config(error.to_string()))?;
        atomic_write(&self.config_path, &bytes)
    }

    /// Read a value of the plugin's own struct from the state file.
    ///
    /// For the settings that are *not* user-facing — a running total, a cursor, a
    /// socket's port — the plugin's own type is the right shape, and making it go
    /// through [`Values`] would mean declaring a field for every internal counter.
    pub fn read_state<T: DeserializeOwned>(&self) -> Result<Option<T>, Error> {
        match std::fs::read(&self.state_path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| Error::Config(format!("{}: {error}", self.state_path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Io {
                path: self.state_path.display().to_string(),
                detail: error.to_string(),
            }),
        }
    }

    /// Write the plugin's own state, atomically and privately.
    pub fn write_state<T: serde::Serialize>(&self, state: &T) -> Result<(), Error> {
        let bytes =
            serde_json::to_vec_pretty(state).map_err(|error| Error::Config(error.to_string()))?;
        atomic_write(&self.state_path, &bytes)
    }
}

/// Write a file through a temporary in the same directory, then replace.
///
/// The same shape `bongocat-storage` uses for `config.json`, written out here
/// because a plugin is a separate program with its own dependencies and must not
/// grow one to save a file. Same-directory is the load-bearing part: a temporary
/// anywhere else would be a rename across a filesystem boundary, which is not
/// atomic, and a plugin's own state is exactly the kind of thing that must not be
/// half-written by a crash.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path.parent().ok_or_else(|| Error::Io {
        path: path.display().to_string(),
        detail: "the path has no directory".to_string(),
    })?;
    std::fs::create_dir_all(parent).map_err(|error| Error::Io {
        path: parent.display().to_string(),
        detail: error.to_string(),
    })?;
    let temporary = parent.join(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin-state")
    ));
    std::fs::write(&temporary, bytes).map_err(|error| Error::Io {
        path: temporary.display().to_string(),
        detail: error.to_string(),
    })?;
    make_private(&temporary);
    std::fs::rename(&temporary, path).map_err(|error| Error::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })
}

/// Mark a file as the user's own, on the platforms where that is a thing.
fn make_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        // Windows has no permission bits on a file; the directory the host created
        // is already per-user, which is the property that matters there.
        let _ = path;
    }
}

/// The user's settings, with every declared field present.
///
/// Every key in the schema has a value, because a field the user has never touched
/// reads as its default. That is the property that lets a plugin ask
/// [`Values::integer`] and get a number rather than an `Option`.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    schema: ConfigSchema,
    values: BTreeMap<String, ConfigValue>,
    document: ConfigDocument,
}

impl Values {
    fn new(schema: &ConfigSchema, values: BTreeMap<String, ConfigValue>) -> Self {
        Self {
            schema: schema.clone(),
            document: ConfigDocument(values.clone()),
            values,
        }
    }

    /// The raw value under a key, for a plugin that branches on the kind.
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }

    /// The value as a switch, or the field's default when the key is absent.
    pub fn flag(&self, key: &str) -> bool {
        match self.values.get(key) {
            Some(ConfigValue::Bool(value)) => *value,
            _ => self.default_of(key),
        }
    }

    /// The value as a whole number, or the field's default.
    pub fn integer(&self, key: &str) -> i64 {
        match self.values.get(key) {
            Some(ConfigValue::Integer(value)) => *value,
            _ => self.default_integer(key),
        }
    }

    /// The value as a real number, or the field's default.
    pub fn decimal(&self, key: &str) -> f64 {
        match self.values.get(key) {
            Some(ConfigValue::Decimal(value)) => *value,
            _ => self.default_decimal(key),
        }
    }

    /// The value as text, or the field's default.
    pub fn text(&self, key: &str) -> String {
        match self.values.get(key) {
            Some(ConfigValue::Text(value)) => value.clone(),
            _ => self.default_text(key),
        }
    }

    /// Every key that has a value, sorted.
    pub fn keys(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }

    /// The document, for sending back to the host or writing to disk.
    pub fn document(&self) -> &ConfigDocument {
        &self.document
    }

    /// The document as the plugin's own struct.
    ///
    /// The convenient path for a plugin whose settings are one coherent thing: the
    /// struct is the plugin's own type, with its own defaults for anything the user
    /// has not set, and this is the one call that produces it.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_value(serde_json::to_value(&self.document).map_err(|error| {
            Error::Config(format!(
                "this plugin's settings cannot be represented: {error}"
            ))
        })?)
        .map_err(|error| Error::Config(format!("this plugin's settings are not readable: {error}")))
    }

    fn default_of(&self, key: &str) -> bool {
        self.default_value(key)
            .and_then(|value| match value {
                ConfigValue::Bool(value) => Some(value),
                _ => None,
            })
            .unwrap_or(false)
    }

    fn default_integer(&self, key: &str) -> i64 {
        self.default_value(key)
            .and_then(|value| match value {
                ConfigValue::Integer(value) => Some(value),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn default_decimal(&self, key: &str) -> f64 {
        self.default_value(key)
            .and_then(|value| match value {
                ConfigValue::Decimal(value) => Some(value),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    fn default_text(&self, key: &str) -> String {
        self.default_value(key)
            .and_then(|value| match value {
                ConfigValue::Text(value) => Some(value),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The schema these values were read against.
    pub fn schema(&self) -> &ConfigSchema {
        &self.schema
    }

    fn default_value(&self, key: &str) -> Option<ConfigValue> {
        self.schema
            .field(key)
            .and_then(|field| field.default_value().ok())
    }
}

/// Bring a document a host sent into the shape this plugin declared.
///
/// Used by the SDK's run loop: the host sends the whole document, this fits it to
/// the schema, and the plugin's own store writes it. A key the schema does not
/// name is dropped rather than refused, because it is a field a newer version of
/// this same plugin wrote and keeping it would mean preserving settings this build
/// has no control for.
pub(crate) fn fit(document: &ConfigDocument, schema: &ConfigSchema) -> Values {
    let completed = document.completed_with(schema);
    Values {
        schema: schema.clone(),
        document: ConfigDocument(completed.clone()),
        values: completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> ConfigSchema {
        Settings::new()
            .with(Toggle::new("auto_start", "Start automatically").into())
            .with(
                Integer::ranged("minutes", "Minutes", 25, 1, 120)
                    .stepping(5)
                    .with_unit("min")
                    .into(),
            )
            .with(Decimal::ranged("volume", "Volume", 0.5, 0.0, 1.0).into())
            .with(
                TextField::new("greeting", "Greeting")
                    .defaulting("hi")
                    .into(),
            )
            .with(
                Choice::new(
                    "sound",
                    "Sound",
                    vec![Option_::new("meow", "Meow"), Option_::new("none", "Silent")],
                )
                .into(),
            )
            .to_schema()
            .expect("an ordinary schema is valid")
    }

    #[test]
    fn a_schema_of_every_control_validates() {
        let schema = schema();
        assert_eq!(schema.fields.len(), 5);
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|f| f.control.kind())
                .collect::<Vec<_>>(),
            vec![
                ConfigKind::Toggle,
                ConfigKind::Integer,
                ConfigKind::Decimal,
                ConfigKind::Text,
                ConfigKind::Choice
            ]
        );
    }

    #[test]
    fn a_field_keeps_the_order_the_plugin_declared_it_in() {
        let schema = schema();
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>(),
            ["auto_start", "minutes", "volume", "greeting", "sound"]
        );
    }

    #[test]
    fn a_default_outside_its_own_range_is_refused_by_the_plugin_itself() {
        // The host would refuse this too, but a plugin's own test run should say
        // which field is wrong rather than that the host would not start it.
        let error = Settings::new()
            .with(Integer::ranged("minutes", "Minutes", 0, 1, 120).into())
            .to_schema()
            .expect_err("a default below its own minimum is a mistake");
        assert!(error.to_string().contains("not valid"), "{error}");
    }

    #[test]
    fn an_empty_choice_is_refused_because_it_has_no_default() {
        assert!(
            Settings::new()
                .with(Choice::new("sound", "Sound", Vec::new()).into())
                .to_schema()
                .is_err()
        );
    }

    #[test]
    fn a_value_the_user_never_touched_reads_as_the_fields_default() {
        let values = Values::new(&schema(), schema().defaults().0);
        assert!(!values.flag("auto_start"));
        assert_eq!(values.integer("minutes"), 25);
        assert_eq!(values.decimal("volume"), 0.5);
        assert_eq!(values.text("greeting"), "hi");
        assert_eq!(
            values.text("sound"),
            "meow",
            "a menu defaults to its first option"
        );
    }

    #[test]
    fn a_key_the_schema_does_not_name_reads_as_nothing_rather_than_as_a_panic() {
        let values = Values::new(&schema(), schema().defaults().0);
        assert_eq!(values.integer("nope"), 0);
        assert!(!values.flag("nope"));
        assert_eq!(values.get("nope"), None);
    }

    #[test]
    fn a_value_of_the_wrong_kind_reads_as_the_default_rather_than_as_itself() {
        let mut values = schema().defaults().0;
        values.insert("minutes".to_string(), ConfigValue::Bool(true));
        let values = Values::new(&schema(), values);
        assert_eq!(
            values.integer("minutes"),
            25,
            "a switch where a number was declared is a document the host would not send; reading the \
             default is the honest answer"
        );
    }

    #[test]
    fn a_document_from_the_host_is_fitted_before_it_is_read() {
        let mut incoming = BTreeMap::new();
        incoming.insert("minutes".to_string(), ConfigValue::Integer(500));
        incoming.insert("removed_setting".to_string(), ConfigValue::Bool(true));
        let values = fit(&ConfigDocument(incoming), &schema());
        assert_eq!(
            values.integer("minutes"),
            120,
            "above the maximum, so clamped to it rather than refused"
        );
        assert_eq!(
            values.keys(),
            schema()
                .defaults()
                .0
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "and a key this build has no field for is not carried forward"
        );
    }

    #[test]
    fn a_document_decodes_into_the_plugins_own_struct() {
        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct Mine {
            minutes: i64,
            sound: String,
            #[serde(default)]
            auto_start: bool,
        }
        let mut values = schema().defaults().0;
        values.insert("minutes".to_string(), ConfigValue::Integer(50));
        let values = Values::new(&schema(), values);
        assert_eq!(
            values.decode::<Mine>().expect("decodes"),
            Mine {
                minutes: 50,
                sound: "meow".to_string(),
                auto_start: false
            }
        );
    }

    #[test]
    fn a_store_writes_settings_that_survive_a_read() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::new(directory.path());
        let schema = schema();
        let values = Values::new(&schema, schema.defaults().0);
        store.write_values(&values).expect("writes");

        let read = store.read_values(&schema).expect("reads");
        assert_eq!(read.integer("minutes"), 25);
        assert!(
            store.config_path().starts_with(directory.path()),
            "and it wrote inside the plugin's own directory"
        );
    }

    #[test]
    fn a_store_with_nothing_written_yet_reads_the_defaults() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::new(directory.path());
        let schema = schema();
        let values = store
            .read_values(&schema)
            .expect("a missing file is the defaults");
        assert_eq!(values.integer("minutes"), 25);
    }

    #[test]
    fn settings_and_state_are_two_files_so_one_reset_does_not_touch_the_other() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::new(directory.path());
        store
            .write_state(&serde_json::json!({"sessions": 4}))
            .expect("writes state");
        let schema = schema();
        let values = Values::new(&schema, schema.defaults().0);
        store.write_values(&values).expect("writes settings");

        assert_eq!(
            store.read_state::<serde_json::Value>().expect("reads"),
            Some(serde_json::json!({"sessions": 4}))
        );
        std::fs::remove_file(store.config_path()).expect("the user resets their settings");
        assert_eq!(
            store
                .read_state::<serde_json::Value>()
                .expect("still reads"),
            Some(serde_json::json!({"sessions": 4})),
            "a reset of the settings must not reset the plugin's own memory"
        );
    }

    #[test]
    fn a_settings_file_that_will_not_parse_is_a_failure_rather_than_a_silent_reset() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::new(directory.path());
        std::fs::write(store.config_path(), b"{ not json").expect("writes nonsense");
        let error = store
            .read_values(&schema())
            .expect_err("a corrupt file is reported, not swallowed");
        assert!(
            matches!(error, Error::Config(_)),
            "because a plugin that silently reverted to defaults would show a user their twenty-five \
             minutes becoming one with nothing to say why: {error}"
        );
    }

    #[test]
    fn a_label_can_carry_its_own_languages() {
        let field = Field::new(
            "minutes",
            "Minutes",
            ConfigControl::Integer {
                default: 25,
                minimum: 1,
                maximum: 120,
                step: 5,
                unit: None,
            },
        )
        .localized(LocalizedText {
            default: "Minutes".to_string(),
            by_locale: [("zh-CN".to_string(), "分钟".to_string())].into(),
        });
        let ConfigField { label, .. } = field.into();
        assert_eq!(label.resolve("zh-CN"), "分钟");
        assert_eq!(label.resolve("en-US"), "Minutes");
    }
}
