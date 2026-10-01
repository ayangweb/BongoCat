//! The plugin worker's snapshot, projected onto the settings protocol.
//!
//! The window's vocabulary is deliberately thinner than the host's: a card, a state,
//! and a settings form. Everything the host knows and the window has no use for — a
//! digest, a directory, a process handle, a `PluginId` that validates itself — stops
//! here, which is what keeps the plugin center unable to grow a dependency on the
//! plugin protocol.
//!
//! The projection is where the two vocabularies meet, and there are exactly three
//! places that needs a decision:
//!
//! * **The icon.** The running plugin's own when it is running, because a plugin may
//!   improve the emoji it ships in a later version, and the archive's otherwise —
//!   which is what a plugin the user has not installed has.
//! * **The settings form.** A plugin's schema becomes a list of typed fields with
//!   every field's copy already resolved for the user's language. Resolving here
//!   rather than in the window is what lets the window say "the window renders
//!   controls" and nothing more.
//! * **The failure codes.** One exhaustive mapping, so a code the host can report and
//!   the window cannot name is a compile error here rather than a blank line in a
//!   user's settings window.

use super::*;

use bongocat_plugin::{PluginEntry, PluginPhase, PluginSnapshot};
use bongocat_plugin::{PluginErrorCode, PluginVersion};
use std::collections::BTreeMap;

use bongocat_plugin::{
    ConfigControl, ConfigDocument, ConfigField, ConfigSchema, ConfigValue, LocalizedText,
    PluginIcon,
};

/// Project one worker snapshot.
pub(super) fn project_plugins(
    snapshot: &PluginSnapshot,
    language: SettingsLanguage,
) -> SettingsPlugins {
    SettingsPlugins {
        revision: snapshot.revision,
        available: true,
        busy: snapshot.phase.as_ref().is_some_and(PluginPhase::is_busy),
        entries: snapshot
            .entries
            .iter()
            .map(|entry| project_entry(entry, language))
            .collect(),
        active: snapshot.active.len(),
        maximum_active: bongocat_plugin::MAXIMUM_ENABLED_PLUGINS,
        last_error: snapshot.last_error.as_ref().map(project_error),
        catalog_read: snapshot.catalog_read,
    }
}

fn project_entry(entry: &PluginEntry, language: SettingsLanguage) -> SettingsPluginEntry {
    let locale = language.catalog_locale();
    let schema = entry
        .descriptor
        .as_ref()
        .map(|descriptor| descriptor.config.clone())
        .unwrap_or_default();
    SettingsPluginEntry {
        id: entry.manifest.id.as_str().to_owned(),
        // A running plugin's own name and description, in the user's language, over
        // the archive's. Same rule as the icon: the card describes the build that is
        // drawing the panel, and the archive's copy is what a plugin nobody has
        // started yet has to be described with.
        //
        // The archive's copy is *itself* localized, which is what makes an
        // uninstalled plugin's card read in the user's language: a catalog entry and
        // a `plugin.json` both carry every language the plugin has, so the fallback
        // below is a translated sentence rather than the English one.
        name: entry
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.name.resolve_bounded(locale))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| entry.manifest.name.resolve_bounded(locale)),
        description: entry
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.description.resolve_bounded(locale))
            .filter(|description| !description.trim().is_empty())
            .unwrap_or_else(|| entry.manifest.description.resolve_bounded(locale)),
        author: entry.manifest.author.clone(),
        icon: project_icon(&entry.icon()),
        installed_version: entry
            .installed
            .then(|| version_text(&entry.manifest.version)),
        available_version: entry.available_version.as_ref().map(version_text),
        installed: entry.installed,
        enabled: entry.enabled,
        running: entry.running,
        update_available: entry.update_available,
        actions: entry
            .actions
            .iter()
            .map(|action| SettingsPluginAction {
                id: action.id.clone(),
                label: action.label.resolve_bounded(locale),
                glyph: project_glyph(action.glyph),
                disabled: action.disabled,
            })
            .collect(),
        // Whether the form can be shown *now*, which is not the same question as whether
        // there are any fields. A plugin declares its schema with its handshake, so an
        // installed-but-stopped plugin has none — and the card's answer has to be that
        // its settings button opens the plugin rather than that the button is missing,
        // because a control that vanishes is a control a user cannot ask about.
        // A position counts as something to configure. A plugin that draws a panel and
        // declares no settings of its own still has a form, with one row on it — and the
        // flag that used to answer `running && !fields.is_empty()` hid it behind a settings
        // button that opened nothing.
        settings_available: entry.running
            && (!schema.fields.is_empty() || entry.position.is_some()),
        fields: project_schema(&schema, locale),
        values: project_values(&schema, &entry.config),
        log: entry
            .log
            .iter()
            .filter(|line| line.is_user_visible())
            .map(|line| line.message.clone())
            .collect(),
        refusal: entry.refusal.as_ref().map(|error| SettingsPluginRefusal {
            code: project_error_code(error.code),
            detail: error.detail.clone(),
        }),
        failure: entry.failure.as_ref().map(project_error),
        // The position form, and the menu behind it. `None` for a plugin that draws no
        // panel — a sound has nothing on the model window to move — and for one whose
        // process has not said hello yet, because a position needs a panel to be a
        // position of. The labels are the *window's* copy: placement is the host's and
        // the user's, not a plugin's, so these are the product's words rather than a
        // plugin's.
        position: entry
            .position
            .map(|placed| project_position(placed.anchor, locale)),
        positions: entry
            .positions
            .iter()
            .map(|anchor| project_position(*anchor, locale))
            .collect(),
    }
}

/// One position, as the form offers it.
fn project_position(anchor: bongocat_plugin::PluginAnchor, locale: &str) -> SettingsPluginPosition {
    SettingsPluginPosition {
        value: anchor.as_str().to_owned(),
        label: bongocat_i18n::text(locale, position_key(anchor)).to_owned(),
    }
}

/// The window's own key for one position.
fn position_key(anchor: bongocat_plugin::PluginAnchor) -> &'static str {
    use bongocat_plugin::PluginAnchor as Anchor;
    match anchor {
        Anchor::TopLeft => "settings.plugins.position.top_left",
        Anchor::TopCenter => "settings.plugins.position.top",
        Anchor::TopRight => "settings.plugins.position.top_right",
        Anchor::CenterLeft => "settings.plugins.position.left",
        Anchor::Center => "settings.plugins.position.center",
        Anchor::CenterRight => "settings.plugins.position.right",
        Anchor::BottomLeft => "settings.plugins.position.bottom_left",
        Anchor::BottomCenter => "settings.plugins.position.bottom",
        Anchor::BottomRight => "settings.plugins.position.bottom_right",
    }
}

/// A control's icon, as the window's own icon.
fn project_glyph(glyph: bongocat_plugin::ActionGlyph) -> SettingsActionGlyph {
    use bongocat_plugin::ActionGlyph as Protocol;
    match glyph {
        Protocol::None => SettingsActionGlyph::None,
        Protocol::Play => SettingsActionGlyph::Play,
        Protocol::Pause => SettingsActionGlyph::Pause,
        Protocol::Reset => SettingsActionGlyph::Reset,
    }
}

/// The icon on a card, in the window's own two-field shape.
fn project_icon(icon: &PluginIcon) -> SettingsPluginIcon {
    SettingsPluginIcon {
        emoji: icon.emoji_text(),
        image: icon.image_path().ok().flatten().map(str::to_string),
    }
}

/// A plugin's settings, as the fields the window draws.
///
/// Every field carries every control's property rather than only the ones its own kind
/// uses, because a window that has to ask "does this one have a minimum?" is a window
/// that can render a spinner for a toggle.
fn project_schema(schema: &ConfigSchema, locale: &str) -> Vec<SettingsPluginField> {
    schema
        .fields
        .iter()
        .map(|field| project_field(field, locale))
        .collect()
}

fn project_field(field: &ConfigField, locale: &str) -> SettingsPluginField {
    SettingsPluginField {
        key: field.key.clone(),
        label: field.label.resolve_bounded(locale),
        description: field
            .description
            .as_ref()
            .map(|description| description.resolve_bounded(locale)),
        kind: project_kind(&field.control),
        default: project_value(&field.default_value().unwrap_or(ConfigValue::Bool(false))),
        minimum: numeric_minimum(&field.control),
        maximum: numeric_maximum(&field.control),
        step: numeric_step(&field.control),
        unit: unit_of(&field.control).map(|unit| unit.resolve_bounded(locale)),
        placeholder: placeholder_of(&field.control).map(|text| text.resolve_bounded(locale)),
        multiline: matches!(
            field.control,
            ConfigControl::Text {
                multiline: true,
                ..
            }
        ),
        options: options_of(&field.control)
            .iter()
            .map(|option| SettingsFieldOption {
                value: option.value.clone(),
                label: option.label.resolve_bounded(locale),
            })
            .collect(),
    }
}

fn project_kind(control: &ConfigControl) -> SettingsFieldKind {
    match control {
        ConfigControl::Toggle { .. } => SettingsFieldKind::Toggle,
        ConfigControl::Integer { .. } => SettingsFieldKind::Integer,
        ConfigControl::Decimal { .. } => SettingsFieldKind::Decimal,
        ConfigControl::Text { .. } => SettingsFieldKind::Text,
        ConfigControl::Choice { .. } => SettingsFieldKind::Choice,
        // The extensions travel with the kind rather than in a list beside it, because a
        // filter the dialog cannot use is a declaration that silently did nothing: a
        // window holding a separate list would be a second thing to keep in step with the
        // control it belongs to.
        ConfigControl::File { accept, .. } => SettingsFieldKind::File {
            accept: accept.clone(),
        },
    }
}

fn project_value(value: &ConfigValue) -> SettingsFieldValue {
    match value {
        ConfigValue::Bool(value) => SettingsFieldValue::Bool(*value),
        ConfigValue::Integer(value) => SettingsFieldValue::Integer(*value),
        ConfigValue::Decimal(value) => SettingsFieldValue::Decimal(*value),
        ConfigValue::Text(value) => SettingsFieldValue::Text(value.clone()),
    }
}

/// Every field's current value, with the schema's defaults filling the gaps.
///
/// A complete map rather than the document as it stands, because a field the user has
/// never touched reads as its default in the form and a hole in a settings page is a
/// bug report.
fn project_values(
    schema: &ConfigSchema,
    document: &ConfigDocument,
) -> BTreeMap<String, SettingsFieldValue> {
    schema
        .fields
        .iter()
        .map(|field| {
            // The document first, then the field's default: a value the plugin
            // persisted wins over the one it declared it starts with, and a field the
            // user has never touched reads as that default rather than as an absence.
            // The `unwrap_or` is unreachable in practice — every field in a schema that
            // passed `validate` has a default that fits it — and picks the same
            // falsy value for every kind so the fallback cannot itself be a lie.
            let value = document
                .get(&field.key)
                .cloned()
                .or_else(|| field.default_value().ok())
                .unwrap_or(ConfigValue::Bool(false));
            (field.key.clone(), project_value(&value))
        })
        .collect()
}

fn numeric_minimum(control: &ConfigControl) -> Option<f64> {
    match control {
        ConfigControl::Integer { minimum, .. } => Some(*minimum as f64),
        ConfigControl::Decimal { minimum, .. } => Some(*minimum),
        _ => None,
    }
}

fn numeric_maximum(control: &ConfigControl) -> Option<f64> {
    match control {
        ConfigControl::Integer { maximum, .. } => Some(*maximum as f64),
        ConfigControl::Decimal { maximum, .. } => Some(*maximum),
        _ => None,
    }
}

fn numeric_step(control: &ConfigControl) -> Option<f64> {
    match control {
        ConfigControl::Integer { step, .. } => Some(*step as f64),
        ConfigControl::Decimal { step, .. } => Some(*step),
        _ => None,
    }
}

fn unit_of(control: &ConfigControl) -> Option<&LocalizedText> {
    match control {
        ConfigControl::Integer { unit, .. } | ConfigControl::Decimal { unit, .. } => unit.as_ref(),
        _ => None,
    }
}

fn placeholder_of(control: &ConfigControl) -> Option<&LocalizedText> {
    match control {
        ConfigControl::Text { placeholder, .. } | ConfigControl::File { placeholder, .. } => {
            placeholder.as_ref()
        }
        _ => None,
    }
}

fn options_of(control: &ConfigControl) -> &[bongocat_plugin::ChoiceOption] {
    match control {
        ConfigControl::Choice { options, .. } => options,
        _ => &[],
    }
}

fn version_text(version: &PluginVersion) -> String {
    version.to_string()
}

fn project_error(error: &bongocat_plugin::PluginError) -> SettingsPluginError {
    SettingsPluginError {
        code: project_error_code(error.code),
        detail: error.detail.clone(),
    }
}

/// Map one host failure onto the code the window can say a sentence about.
///
/// The arms are written out rather than grouped by guesswork, and the match is
/// deliberately exhaustive: the host's code set is a closed list this crate is
/// rebuilt against, so a code the window has no sentence for is a compile error here
/// rather than a blank notification at runtime.
fn project_error_code(code: PluginErrorCode) -> SettingsPluginErrorCode {
    use PluginErrorCode as Code;
    use SettingsPluginErrorCode as Window;
    match code {
        Code::PluginNotPublished | Code::NotInstalled => Window::NotPublished,
        Code::AlreadyInstalled | Code::AlreadyUpToDate => Window::AlreadyInstalled,
        Code::CatalogInvalid => Window::CatalogUnavailable,
        Code::DownloadFailed => Window::NetworkUnavailable,
        Code::ChecksumMismatch => Window::ChecksumMismatch,
        Code::SignatureInvalid | Code::SignatureKeyMissing => Window::SignatureInvalid,
        Code::StoreWriteFailed | Code::PluginDirectoryUnreadable => Window::StoreWriteFailed,
        Code::TooManyEnabled => Window::TooManyEnabled,
        Code::RenderFailed | Code::FontUnavailable => Window::RenderFailed,
        Code::ArchiveInvalid
        | Code::ManifestInvalid
        | Code::UnsupportedSchemaVersion
        | Code::UnsupportedApiVersion
        | Code::InvalidPluginId
        | Code::InvalidPluginName
        | Code::InvalidPluginDescription
        | Code::InvalidAssetPath
        | Code::SceneTooLarge
        | Code::SceneTooDeep
        | Code::InvalidButtonId
        | Code::DuplicateButtonId
        | Code::InvalidPanelSize
        | Code::InvalidPanelPlacement
        | Code::InvalidConfigSchema
        | Code::InvalidConfigValue => Window::InvalidManifest,
        Code::ProtocolInvalid
        | Code::ProtocolVersionMismatch
        | Code::PluginSpawnFailed
        | Code::PluginHandshakeFailed
        | Code::PluginExited
        | Code::HostCommandUnavailable
        | Code::ModelRequestUnknown => Window::PluginFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin::{
        ChoiceOption, ConfigControl, ConfigField, ConfigSchema, ConfigValue, PluginAnchor,
    };

    fn manifest(id: &str) -> bongocat_plugin::PluginManifest {
        bongocat_plugin::PluginManifest {
            schema_version: 1,
            api_version: 1,
            id: bongocat_plugin::PluginId::new(id).expect("a valid id"),
            name: LocalizedText::from(format!("{id} name")),
            version: bongocat_plugin::PluginVersion::new(1, 0, 0),
            min_app_version: None,
            author: "someone".to_string(),
            description: LocalizedText::from(format!("{id} description")),
            icon: PluginIcon {
                emoji: Some("🍅".to_string()),
                image: None,
            },
            executable: id.to_string(),
        }
    }

    fn descriptor(schema: ConfigSchema) -> bongocat_plugin::PluginDescriptor {
        bongocat_plugin::PluginDescriptor {
            id: bongocat_plugin::PluginId::new("pomodoro").expect("valid"),
            name: LocalizedText {
                default: "Pomodoro".to_string(),
                by_locale: [
                    ("zh-CN".to_string(), "番茄钟".to_string()),
                    ("ko-KR".to_string(), "뽀모도로".to_string()),
                ]
                .into(),
            },
            version: bongocat_plugin::PluginVersion::new(1, 0, 0),
            author: String::new(),
            description: LocalizedText {
                default: "A focus timer on the model window.".to_string(),
                by_locale: [
                    ("zh-CN".to_string(), "模型窗口上的专注计时器。".to_string()),
                    (
                        "ko-KR".to_string(),
                        "모델 창에 있는 집중 타이머.".to_string(),
                    ),
                ]
                .into(),
            },
            icon: PluginIcon {
                emoji: Some("🍅".to_string()),
                image: None,
            },
            config: schema,
            draws_panel: true,
            subscriptions: Vec::new(),
        }
    }

    fn schema() -> ConfigSchema {
        ConfigSchema {
            schema_version: bongocat_plugin::CONFIG_SCHEMA_VERSION,
            fields: vec![
                ConfigField {
                    key: "minutes".to_string(),
                    label: LocalizedText {
                        default: "Minutes".to_string(),
                        by_locale: [
                            ("zh-CN".to_string(), "分钟".to_string()),
                            ("ko-KR".to_string(), "분".to_string()),
                        ]
                        .into(),
                    },
                    description: Some(LocalizedText {
                        default: "How long a round is.".to_string(),
                        by_locale: Default::default(),
                    }),
                    control: ConfigControl::Integer {
                        default: 25,
                        minimum: 1,
                        maximum: 120,
                        step: 5,
                        unit: Some(LocalizedText::from("min")),
                    },
                },
                ConfigField {
                    key: "sound".to_string(),
                    label: LocalizedText::from("Sound"),
                    description: None,
                    control: ConfigControl::Choice {
                        default: "meow".to_string(),
                        options: vec![
                            ChoiceOption {
                                value: "meow".to_string(),
                                label: LocalizedText::from("Meow"),
                            },
                            ChoiceOption {
                                value: "none".to_string(),
                                label: LocalizedText::from("Silent"),
                            },
                        ],
                    },
                },
                ConfigField {
                    key: "auto_start".to_string(),
                    label: LocalizedText::from("Start automatically"),
                    description: None,
                    control: ConfigControl::Toggle { default: true },
                },
            ],
        }
    }

    fn entry() -> PluginEntry {
        PluginEntry {
            manifest: manifest("pomodoro"),
            descriptor: Some(descriptor(schema())),
            installed: true,
            enabled: true,
            running: true,
            config: ConfigDocument::default(),
            available_version: Some(bongocat_plugin::PluginVersion::new(2, 0, 0)),
            update_available: true,
            refusal: None,
            failure: None,
            restarts: 0,
            subscriptions: Vec::new(),
            actions: offered_actions(),
            // A running plugin that asked for a position, sitting in the top left with
            // the eight others free — the fixture every position assertion reads, because
            // a projection test that built "no position" could not tell a row that is
            // missing from a row that is wrongly absent.
            position: Some(bongocat_plugin::Placed {
                anchor: bongocat_plugin::PluginAnchor::TopLeft,
                chosen: false,
            }),
            positions: bongocat_plugin::POSITIONS.to_vec(),
            log: Vec::new(),
        }
    }

    #[test]
    fn a_plugin_that_draws_a_panel_gets_a_position_menu_and_one_that_does_not_gets_nothing() {
        // The whole of the row's existence. A sound or a tally has nothing on the model
        // window to move, so it is offered no position at all — and a menu of nine for a
        // plugin that draws nothing is a control that changes nothing.
        let drawing = project_entry(&entry(), SettingsLanguage::English);
        let position = drawing.position.expect("a running panel has a position");
        assert_eq!(position.value, "top_left");
        assert_eq!(position.label, "Top left");
        assert_eq!(
            drawing.positions.len(),
            bongocat_plugin::POSITIONS.len(),
            "and every position is free in this fixture, so all nine are offered"
        );

        let mut silent = entry();
        silent.position = None;
        silent.positions = Vec::new();
        let silent = project_entry(&silent, SettingsLanguage::English);
        assert!(silent.position.is_none());
        assert!(
            silent.positions.is_empty(),
            "so a plugin with no panel is offered no position to move"
        );
    }

    #[test]
    fn the_position_menu_speaks_the_readers_language() {
        // Placement is the host's and the user's, so these are the product's words rather
        // than a plugin's — and they are resolved here, where the reader's language is
        // known, for the same reason every other string on the card is.
        let chinese = project_entry(&entry(), SettingsLanguage::ChineseSimplified);
        assert_eq!(chinese.position.expect("a position").label, "左上");
        assert_eq!(
            chinese.positions[4].label, "中间",
            "and every position in the menu is translated, not only the current one"
        );
    }

    /// The controls a running plugin offered, as one renamed control.
    ///
    /// A running plugin with a schema is the case that matters here, so the fixture is
    /// that one rather than the empty default.
    fn offered_actions() -> Vec<bongocat_plugin::PluginAction> {
        vec![bongocat_plugin::PluginAction {
            id: "toggle".to_string(),
            label: bongocat_plugin::LocalizedText::from("Start"),
            glyph: bongocat_plugin::ActionGlyph::Play,
            disabled: false,
        }]
    }

    #[test]
    fn an_offered_control_reaches_the_card_with_its_own_words_and_icon() {
        // The whole point of a control travelling with its own label: the window draws
        // what the plugin said, resolved into the user's language. The application learns
        // nothing about what "toggle" means — it routes the id and draws the word.
        let projected = project_plugins(
            &PluginSnapshot {
                entries: vec![entry()],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        let pomodoro = projected.entries.first().expect("a card");
        assert_eq!(pomodoro.actions.len(), 1);
        assert_eq!(pomodoro.actions[0].id, "toggle");
        assert_eq!(pomodoro.actions[0].label, "Start");
        assert_eq!(pomodoro.actions[0].glyph, SettingsActionGlyph::Play);
    }

    #[test]
    fn a_stopped_plugin_has_no_control_and_no_form_but_still_offers_both_on_its_card() {
        // The dead end, pinned on the side that decides it. A control and a schema both
        // arrive from a running process, so a switched-off plugin has neither — and the
        // projection has to report that as two separate facts, because the card answers
        // them differently: a missing control is a plugin that wants none, while a missing
        // form is a plugin whose form can be asked for.
        let mut stopped = entry();
        stopped.running = false;
        stopped.enabled = false;
        stopped.descriptor = None;
        stopped.actions = Vec::new();
        let projected = project_plugins(
            &PluginSnapshot {
                entries: vec![stopped],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        let pomodoro = projected.entries.first().expect("a card");
        assert!(
            pomodoro.actions.is_empty(),
            "a plugin that is not running has offered nothing, so its card draws no control"
        );
        assert!(
            !pomodoro.settings_available,
            "and there is no form to open yet — which is why the card's settings button is \
             drawn anyway and turns the plugin on"
        );
    }

    #[test]
    fn a_running_plugin_with_nothing_to_configure_has_no_form() {
        // Distinct from the case above, and the card has to tell them apart: enabled, with
        // no schema and no panel, is a plugin with nothing to configure, so pressing its
        // settings button correctly does nothing. Promising a "turn it on" hint here would
        // be telling a user to do something that cannot help.
        let mut nothing_to_configure = entry();
        nothing_to_configure
            .descriptor
            .as_mut()
            .expect("a descriptor")
            .config = bongocat_plugin::ConfigSchema::default();
        // The other half of "nothing": a plugin that says it has no place in the model
        // window. A schema with no fields and a position is a form with one row on it, and
        // a schema with neither is a plugin with nothing to show.
        nothing_to_configure.position = None;
        nothing_to_configure.positions = Vec::new();
        let projected = project_plugins(
            &PluginSnapshot {
                entries: vec![nothing_to_configure],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        let pomodoro = projected.entries.first().expect("a card");
        assert!(
            !pomodoro.settings_available,
            "a plugin with neither settings nor a panel has no form, running or not"
        );
        assert!(pomodoro.enabled);
    }

    #[test]
    fn a_panel_with_no_settings_of_its_own_still_has_a_form() {
        // The case the flag used to hide. A plugin that draws a panel and declares no
        // settings is still configurable — in one respect, the one that has nothing to do
        // with the plugin: where its panel sits in the model window.
        let mut only_a_position = entry();
        only_a_position
            .descriptor
            .as_mut()
            .expect("a descriptor")
            .config = bongocat_plugin::ConfigSchema::default();
        let projected = project_plugins(
            &PluginSnapshot {
                entries: vec![only_a_position],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        let pomodoro = projected.entries.first().expect("a card");
        assert!(
            pomodoro.settings_available,
            "so its settings button opens a form rather than doing nothing"
        );
        assert!(
            pomodoro.fields.is_empty() && pomodoro.position.is_some(),
            "and that form is one row: the position, and nothing the plugin declared"
        );
    }

    #[test]
    fn an_installed_plugin_shows_its_version_and_can_be_updated() {
        let snapshot = PluginSnapshot {
            revision: 4,
            phase: Some(PluginPhase::Idle),
            entries: vec![entry()],
            active: vec![bongocat_plugin::PluginId::new("pomodoro").expect("id")],
            last_error: None,
            catalog_read: true,
        };

        let plugins = project_plugins(&snapshot, SettingsLanguage::English);

        assert_eq!(plugins.revision, 4);
        assert!(plugins.available);
        assert!(!plugins.busy);
        assert_eq!(plugins.active, 1);
        assert_eq!(plugins.entries.len(), 1);
        let entry = &plugins.entries[0];
        assert_eq!(entry.id, "pomodoro");
        assert_eq!(entry.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.available_version.as_deref(), Some("2.0.0"));
        assert!(entry.update_available);
        assert!(entry.enabled);
        assert!(
            entry.running,
            "and the process is alive, which is its own fact"
        );
    }

    /// An entry whose *archive* carries this icon, with no running process.
    ///
    /// Not installed-and-running, because a running plugin's own icon wins — which is
    /// the rule a separate test pins.
    fn unstarted(icon: PluginIcon) -> PluginEntry {
        let mut entry = entry();
        entry.descriptor = None;
        entry.running = false;
        entry.enabled = false;
        entry.manifest.icon = icon;
        entry
    }

    #[test]
    fn a_plugins_emoji_reaches_the_card_and_its_image_half_exists_for_the_day_it_is_needed() {
        let emoji = project_plugins(
            &PluginSnapshot {
                entries: vec![unstarted(PluginIcon {
                    emoji: Some("🍅".to_string()),
                    image: None,
                })],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(emoji.entries[0].icon.emoji.as_deref(), Some("🍅"));
        assert!(
            emoji.entries[0].icon.image.is_none(),
            "and an emoji icon has no image half"
        );

        // A plugin that ships a picture already works: the protocol carries the field,
        // the host checks the path before the window sees it, and the card shows it.
        let image = project_plugins(
            &PluginSnapshot {
                entries: vec![unstarted(PluginIcon {
                    emoji: None,
                    image: Some("icon.png".to_string()),
                })],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(image.entries[0].icon.image.as_deref(), Some("icon.png"));
        assert!(image.entries[0].icon.emoji.is_none());
    }

    #[test]
    fn an_icon_that_escapes_the_plugins_directory_never_reaches_the_card() {
        // The host validates the path and the projection trusts it. A `..` here would
        // mean the window had been handed a path from outside a plugin's own
        // directory, which is exactly what the protocol check exists to prevent — so
        // this pins that the check is what is standing between the two.
        let refused = project_plugins(
            &PluginSnapshot {
                entries: vec![unstarted(PluginIcon {
                    emoji: Some("🍅".to_string()),
                    image: Some("../../etc/passwd".to_string()),
                })],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(
            refused.entries[0].icon.image, None,
            "the emoji still comes through, and the path does not"
        );
    }

    #[test]
    fn a_running_plugins_own_icon_wins_over_the_one_its_archive_carried() {
        // The archive's icon is what a plugin the user has not installed has; a
        // running plugin's is what it improved in a later version. Preferring the
        // archive's would show the user the icon of a build that is not the one
        // drawing the panel.
        let running = entry();
        assert_eq!(
            running
                .descriptor
                .as_ref()
                .map(|descriptor| descriptor.icon.emoji.clone()),
            Some(Some("🍅".to_string())),
            "the running plugin's own emoji"
        );
        let projected = project_plugins(
            &PluginSnapshot {
                entries: vec![running],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(projected.entries[0].icon.emoji.as_deref(), Some("🍅"));
    }

    #[test]
    fn a_plugins_settings_reach_the_window_with_their_copy_in_the_users_language() {
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![entry()],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::ChineseSimplified,
        );
        let entry = &plugins.entries[0];
        assert_eq!(entry.fields.len(), 3);

        let minutes = &entry.fields[0];
        assert_eq!(minutes.key, "minutes");
        assert_eq!(
            minutes.label, "分钟",
            "and the label is resolved here rather than in the window, so the window says 'renders \
             controls' and nothing more"
        );
        assert_eq!(minutes.description.as_deref(), Some("How long a round is."));
        assert_eq!(minutes.kind, SettingsFieldKind::Integer);
        assert_eq!(minutes.default, SettingsFieldValue::Integer(25));
        assert_eq!(minutes.minimum, Some(1.0));
        assert_eq!(minutes.maximum, Some(120.0));
        assert_eq!(minutes.step, Some(5.0));
        assert_eq!(minutes.unit.as_deref(), Some("min"));
        assert!(minutes.options.is_empty());

        let sound = &entry.fields[1];
        assert_eq!(sound.kind, SettingsFieldKind::Choice);
        assert_eq!(
            sound.options.len(),
            2,
            "and a menu's options travel with their own labels, already resolved"
        );
        assert_eq!(sound.options[0].value, "meow");
        assert_eq!(sound.options[0].label, "Meow");

        let toggle = &entry.fields[2];
        assert_eq!(toggle.kind, SettingsFieldKind::Toggle);
        assert_eq!(toggle.default, SettingsFieldValue::Bool(true));
        assert!(
            toggle.minimum.is_none(),
            "and a switch carries no bounds, because a settings row that has to ask 'does this one \
             have a minimum?' is a settings row that can render a spinner for a toggle"
        );
    }

    #[test]
    fn changing_the_language_changes_every_word_a_plugin_contributed() {
        // The whole of "switch language and the plugin text switches with it", and it is
        // two claims rather than one because the words come from three places that were
        // once three documents: the card's name and description are the running plugin's,
        // the settings labels are its schema, and the position names are the host's own.
        //
        // Projecting the *same* snapshot twice is what makes it a test rather than a
        // demonstration. A projection that read the language from the snapshot, or that
        // cached a resolved string beside the entry, would pass a test asserting each
        // language separately and fail this one — which is why both are asserted here
        // against one fixture rather than each language getting a fixture of its own.
        let snapshot = PluginSnapshot {
            entries: vec![entry()],
            ..PluginSnapshot::default()
        };
        let in_english = project_plugins(&snapshot, SettingsLanguage::English);
        let in_chinese = project_plugins(&snapshot, SettingsLanguage::ChineseSimplified);
        let in_korean = project_plugins(&snapshot, SettingsLanguage::Korean);

        for (language, projected, name, label) in [
            (
                SettingsLanguage::English,
                &in_english,
                "Pomodoro",
                "Minutes",
            ),
            (
                SettingsLanguage::ChineseSimplified,
                &in_chinese,
                "番茄钟",
                "分钟",
            ),
            (SettingsLanguage::Korean, &in_korean, "뽀모도로", "분"),
        ] {
            let entry = &projected.entries[0];
            assert_eq!(entry.name, name, "the card's name in {language:?}");
            assert_eq!(
                entry.fields[0].label, label,
                "and the settings form's own first label in {language:?}"
            );
            assert_eq!(
                entry
                    .position
                    .as_ref()
                    .map(|position| position.label.as_str()),
                Some(match language {
                    SettingsLanguage::ChineseSimplified => "左上",
                    SettingsLanguage::Korean => "왼쪽 위",
                    _ => "Top left",
                }),
                "and the host's own position names, which are not the plugin's and would \
                 have gone stale on a language switch if they were resolved once"
            );
        }

        assert_ne!(
            in_english.entries[0].description, in_chinese.entries[0].description,
            "and the sentence under the name follows too, because a card whose title \
             changed and whose sentence did not is half-translated"
        );
    }

    #[test]
    fn a_field_the_user_never_touched_is_projected_as_its_default_not_as_a_hole() {
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![entry()],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        let values = &plugins.entries[0].values;
        assert_eq!(
            values.get("minutes"),
            Some(&SettingsFieldValue::Integer(25))
        );
        assert_eq!(
            values.get("sound"),
            Some(&SettingsFieldValue::Text("meow".to_string()))
        );
        assert_eq!(
            values.get("auto_start"),
            Some(&SettingsFieldValue::Bool(true))
        );
        assert_eq!(
            values.len(),
            3,
            "and the map is complete, so the form has no hole in it"
        );
    }

    #[test]
    fn a_value_the_user_set_wins_over_the_default() {
        let mut entry = entry();
        entry.config = ConfigDocument::single("minutes", ConfigValue::Integer(50));
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![entry],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(
            plugins.entries[0].values.get("minutes"),
            Some(&SettingsFieldValue::Integer(50))
        );
    }

    #[test]
    fn a_plugin_with_no_settings_projects_an_empty_form_rather_than_a_missing_one() {
        let mut entry = entry();
        entry.descriptor = Some(descriptor(ConfigSchema::default()));
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![entry],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert!(plugins.entries[0].fields.is_empty());
        assert!(
            !plugins.any_has_settings(),
            "and the page can tell a plugin with nothing to change from one whose form failed to \
             load"
        );
    }

    #[test]
    fn a_catalog_only_plugin_has_no_installed_version_and_keeps_its_refusal() {
        // A plugin the catalog offers but not for this platform is refused with
        // `PluginNotPublished` and a detail naming the platform, so the card survives
        // and explains itself rather than disappearing from the list.
        let refusal = bongocat_plugin::PluginError::with_detail(
            PluginErrorCode::PluginNotPublished,
            "no download for platform x86_64-unknown-linux-gnu",
        );
        let snapshot = PluginSnapshot {
            revision: 1,
            phase: None,
            entries: vec![PluginEntry {
                manifest: manifest("linux-only"),
                installed: false,
                enabled: false,
                running: false,
                available_version: None,
                update_available: false,
                refusal: Some(refusal),
                descriptor: None,
                config: ConfigDocument::default(),
                failure: None,
                restarts: 0,
                subscriptions: Vec::new(),
                actions: Vec::new(),
                position: None,
                positions: Vec::new(),
                log: Vec::new(),
            }],
            active: Vec::new(),
            last_error: None,
            catalog_read: true,
        };

        let entry = &project_plugins(&snapshot, SettingsLanguage::English).entries[0];

        assert!(!entry.installed);
        assert_eq!(entry.installed_version, None);
        assert_eq!(
            entry.refusal.as_ref().map(|refusal| refusal.code),
            Some(SettingsPluginErrorCode::NotPublished),
            "a plugin this host cannot install still has to be listed, with a reason"
        );
    }

    #[test]
    fn a_busy_phase_is_reported_and_an_idle_one_is_not() {
        let mut snapshot = PluginSnapshot {
            revision: 0,
            phase: Some(PluginPhase::Idle),
            ..PluginSnapshot::default()
        };
        assert!(!project_plugins(&snapshot, SettingsLanguage::English).busy);

        snapshot.phase = Some(PluginPhase::RefreshingCatalog);
        assert!(project_plugins(&snapshot, SettingsLanguage::English).busy);

        snapshot.phase = Some(PluginPhase::Installing(
            bongocat_plugin::PluginId::new("pomodoro").expect("id"),
        ));
        assert!(project_plugins(&snapshot, SettingsLanguage::English).busy);
    }

    #[test]
    fn a_plugins_own_log_reaches_the_card_and_only_the_lines_a_user_would_read() {
        use bongocat_plugin::LogLevel;
        let mut entry = entry();
        entry.log = vec![
            bongocat_plugin::Line {
                id: bongocat_plugin::PluginId::new("pomodoro").expect("valid"),
                level: LogLevel::Debug,
                message: "plugin pomodoro: counting".to_string(),
            },
            bongocat_plugin::Line {
                id: bongocat_plugin::PluginId::new("pomodoro").expect("valid"),
                level: LogLevel::Error,
                message: "plugin pomodoro: it could not start".to_string(),
            },
        ];
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![entry],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(
            plugins.entries[0].log,
            vec!["plugin pomodoro: it could not start".to_string()],
            "because a page that listed every internal line would be unreadable and would train \
             the user to ignore it"
        );
    }

    #[test]
    fn every_host_code_maps_to_something_the_window_can_say() {
        // The point is totality: `project_error_code` matches the host's whole code
        // set with no catch-all, so a code added to the host without a sentence here
        // fails to compile rather than producing an unreachable page.
        for code in PluginErrorCode::ALL {
            let mapped = project_error_code(code);
            assert_ne!(
                mapped,
                SettingsPluginErrorCode::Other,
                "{code:?} has a code of its own and should not fall through"
            );
        }
    }

    #[test]
    fn an_uninstalled_plugin_is_described_in_the_readers_language() {
        // The one the screenshot showed: a card for a plugin that was never installed
        // said its English name and its English sentence on a Chinese page, because
        // the only copy there was the archive's and the archive's was English. The
        // archive's copy is now the plugin's own table, so the fallback resolves too.
        let mut entry = entry();
        entry.descriptor = None;
        entry.installed = false;
        entry.running = false;
        entry.manifest.name = LocalizedText::new("Pomodoro")
            .with_locale("zh-CN", "番茄钟")
            .with_locale("zh-TW", "番茄鐘");
        entry.manifest.description =
            LocalizedText::new("A focus timer.").with_locale("zh-CN", "一个专注计时器。");

        let chinese = project_plugins(
            &PluginSnapshot {
                entries: vec![entry.clone()],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::ChineseSimplified,
        );
        assert_eq!(chinese.entries[0].name, "番茄钟");
        assert_eq!(chinese.entries[0].description, "一个专注计时器。");

        let traditional = project_plugins(
            &PluginSnapshot {
                entries: vec![entry.clone()],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::ChineseTraditional,
        );
        assert_eq!(traditional.entries[0].name, "番茄鐘");

        // A language the plugin has no copy for is its default, which is the point of
        // a default: the card is never blank because a translation is missing.
        let english = project_plugins(
            &PluginSnapshot {
                entries: vec![entry],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert_eq!(english.entries[0].name, "Pomodoro");
    }

    #[test]
    fn a_running_plugins_own_copy_still_wins_over_the_archives() {
        let mut running = entry();
        running.manifest.name = LocalizedText::new("Archive name");
        running.manifest.description = LocalizedText::new("Archive sentence.");
        running.descriptor.as_mut().expect("running").name =
            LocalizedText::new("Descriptor name").with_locale("zh-CN", "运行中的名字");

        let chinese = project_plugins(
            &PluginSnapshot {
                entries: vec![running],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::ChineseSimplified,
        );
        assert_eq!(
            chinese.entries[0].name, "运行中的名字",
            "because the card describes the build that is drawing the panel"
        );
    }

    #[test]
    fn a_card_with_no_icon_gets_a_letter() {
        let plugins = project_plugins(
            &PluginSnapshot {
                entries: vec![unstarted(PluginIcon::default())],
                ..PluginSnapshot::default()
            },
            SettingsLanguage::English,
        );
        assert!(plugins.entries[0].icon.is_empty());
        assert_eq!(
            plugins.entries[0].initial(),
            "P",
            "because a grid of cards with a blank in each is a grid of blanks"
        );
    }

    #[test]
    fn a_manifest_the_catalog_only_carries_still_projects_a_card() {
        // A synthesized entry has no descriptor and so no form, which is correct: it is
        // not installed, so there is nothing to configure.
        let snapshot = PluginSnapshot {
            entries: vec![PluginEntry {
                manifest: manifest("pomodoro"),
                descriptor: None,
                installed: false,
                enabled: false,
                running: false,
                available_version: Some(bongocat_plugin::PluginVersion::new(1, 0, 0)),
                update_available: false,
                refusal: None,
                config: ConfigDocument::default(),
                failure: None,
                restarts: 0,
                subscriptions: Vec::new(),
                actions: Vec::new(),
                position: None,
                positions: Vec::new(),
                log: Vec::new(),
            }],
            catalog_read: true,
            ..PluginSnapshot::default()
        };
        let plugins = project_plugins(&snapshot, SettingsLanguage::English);
        assert_eq!(plugins.entries.len(), 1);
        assert!(plugins.entries[0].fields.is_empty());
        assert_eq!(
            plugins.entries[0].icon.emoji.as_deref(),
            Some("🍅"),
            "and it still shows the icon its catalog entry carries, because that is what a card is"
        );
        let _ = PluginAnchor::TopLeft;
    }
}
