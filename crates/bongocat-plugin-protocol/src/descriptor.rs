//! What a plugin is called, and what it announces about itself when it runs.
//!
//! There are two descriptors and they answer different questions, which is why
//! they are two types rather than one with optional halves.
//!
//! * [`PluginManifest`] is the `plugin.json` inside the archive. It exists so the
//!   plugin center can list a plugin the user has **not** installed: a name, a
//!   description, an author, an icon and a version, plus the file to execute. It is
//!   metadata and nothing else — a panel, a timer or a socket listener cannot be
//!   written in it.
//! * [`PluginDescriptor`] is what a running plugin sends in its `ready` message.
//!   It carries the same identity plus the things only a running program knows:
//!   the configuration schema the settings window renders, and the feeds it wants.
//!
//! The host checks that they agree on id and version before it shows anything. A
//! card that says one version and runs another is worse than a plugin that will
//! not start, and the check costs one comparison.

use super::config::ConfigSchema;
use super::error::{PluginError, PluginErrorCode};
use super::identity::{MAXIMUM_PLUGIN_ID_BYTES, PluginId, PluginVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file name a plugin's descriptor has inside its own directory.
pub const PLUGIN_MANIFEST_FILE_NAME: &str = "plugin.json";

/// The longest a plugin's display name may be, in characters.
pub const MAXIMUM_PLUGIN_NAME_CHARS: usize = 64;

/// The longest a plugin's description may be, in characters.
pub const MAXIMUM_PLUGIN_DESCRIPTION_CHARS: usize = 200;

/// The longest a button id may be, in bytes.
pub const MAXIMUM_BUTTON_ID_BYTES: usize = 32;

/// The most buttons one panel may declare.
///
/// A bound on the press routing rather than on the scene: every button is an id
/// the host remembers so a click can be turned back into a message, and an
/// unbounded number of them is an unbounded table.
pub const MAXIMUM_BUTTONS_PER_PANEL: usize = 128;

/// The longest a resolved label may be, in characters.
///
/// A plugin supplies its own copy and the settings window draws it. The host
/// cannot know what a plugin will call a field, so it bounds the length instead
/// of the content: a label that would fill three rows is truncated, not refused.
pub const MAXIMUM_LABEL_CHARS: usize = 120;

/// A short piece of a plugin's own copy, resolved against the user's language.
///
/// A plugin ships its own strings, so the host cannot own them and cannot
/// localize them for it. What the host can do is hold a default and a table, ask
/// for one locale, and fall back — which is the whole of what a language switch
/// needs here.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedText {
    /// What to show when the table has nothing for the user's language.
    pub default: String,
    /// BCP-47-ish tags to text, e.g. `"zh-CN"`. Matched by the host, never by the
    /// plugin, so a plugin cannot decide which of its strings the user sees.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_locale: BTreeMap<String, String>,
}

impl LocalizedText {
    /// The text for `locale`, falling back to the default.
    ///
    /// Matched on the full tag first and then on the primary subtag, so a plugin
    /// that ships `"zh"` covers `zh-CN` and one that ships `"zh-CN"` still covers
    /// it exactly. A table with no entry for the language at all is the default,
    /// which is the only reason a plugin may leave the table empty.
    pub fn resolve(&self, locale: &str) -> &str {
        if let Some(exact) = self.by_locale.get(locale) {
            return exact;
        }
        let primary = locale.split(['-', '_']).next().unwrap_or(locale);
        if let Some(matched) = self
            .by_locale
            .iter()
            .find(|(tag, _)| tag.split(['-', '_']).next() == Some(primary))
            .map(|(_, text)| text)
        {
            return matched;
        }
        &self.default
    }

    /// The resolved text, cut to the length a label may occupy.
    ///
    /// Truncating rather than refusing: a label is prose the plugin owns, and a
    /// long one is a layout problem the host can solve without failing a load.
    pub fn resolve_bounded(&self, locale: &str) -> String {
        let resolved = self.resolve(locale);
        if resolved.chars().count() <= MAXIMUM_LABEL_CHARS {
            return resolved.to_string();
        }
        let mut out: String = resolved.chars().take(MAXIMUM_LABEL_CHARS).collect();
        out.push('…');
        out
    }
}

impl From<&str> for LocalizedText {
    fn from(value: &str) -> Self {
        Self {
            default: value.to_string(),
            by_locale: BTreeMap::new(),
        }
    }
}

impl From<String> for LocalizedText {
    fn from(value: String) -> Self {
        Self {
            default: value,
            by_locale: BTreeMap::new(),
        }
    }
}

/// A plugin's icon, as the card in the plugin center shows it.
///
/// Emoji today, an image tomorrow, and the field is a pair rather than a single
/// name so adding the second is not a change to the document every plugin
/// already ships. At most one is honoured, and the image wins when both are
/// present — a picture is the more specific answer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginIcon {
    /// A short emoji, rendered as text by the host's own font stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// A PNG under the plugin's own directory. Never absolute, never `..`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl PluginIcon {
    /// The emoji, when the icon is one.
    ///
    /// Bounded rather than validated: a plugin that names a long emoji sequence
    /// gets it truncated on screen, and there is no reason to refuse a load for a
    /// card decoration.
    pub fn emoji_text(&self) -> Option<String> {
        self.emoji.as_ref().map(|emoji| {
            let out: String = emoji.chars().take(8).collect();
            out.trim().to_string()
        })
    }

    /// The relative path of the icon image, checked against the plugin's directory.
    pub fn image_path(&self) -> Result<Option<&str>, PluginError> {
        match &self.image {
            Some(image) => {
                super::validate_relative_asset_path(image)?;
                Ok(Some(image.as_str()))
            }
            None => Ok(None),
        }
    }
}

/// The archive's own metadata.
///
/// Every field but `id`, `name` and `executable` has a default, so a manifest
/// that adds a field stays readable by an older host and a manifest from a newer
/// host is refused by `api_version` rather than by a missing-field error nobody
/// can act on.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// The shape of this file. One accepted version, no migration.
    pub schema_version: u32,
    /// The feature level this plugin needs from the host.
    pub api_version: u32,
    pub id: PluginId,
    pub name: String,
    pub version: PluginVersion,
    /// The oldest BongoCat that can run this plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<PluginVersion>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: PluginIcon,
    /// The file to execute, relative to this plugin's own directory.
    ///
    /// Named rather than assumed so an archive may carry more than one binary and
    /// so the path is subject to the same traversal check as every other path a
    /// plugin names.
    pub executable: String,
}

impl PluginManifest {
    /// Read and check a manifest.
    pub fn parse(bytes: &[u8]) -> Result<Self, PluginError> {
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::ManifestInvalid, error))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        if self.schema_version != super::catalog::PLUGIN_SCHEMA_VERSION {
            return Err(PluginError::new(PluginErrorCode::UnsupportedSchemaVersion));
        }
        if self.api_version == 0 || self.api_version > super::catalog::SUPPORTED_PLUGIN_API_VERSION
        {
            return Err(PluginError::new(PluginErrorCode::UnsupportedApiVersion));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > MAXIMUM_PLUGIN_NAME_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginName));
        }
        if self.description.chars().count() > MAXIMUM_PLUGIN_DESCRIPTION_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginDescription));
        }
        self.icon.image_path()?;
        // The executable goes through the same path check as an asset, because it
        // is one: a path from a document that becomes something the host runs.
        super::validate_relative_asset_path(&self.executable)?;
        Ok(())
    }

    /// Where this plugin's executable lives.
    pub fn executable_path(&self, root: &Path) -> Result<PathBuf, PluginError> {
        super::validate_relative_asset_path(&self.executable)?;
        Ok(root.join(&self.executable))
    }

    /// The icon, with only the parts this host can show.
    pub fn display_icon(&self) -> PluginIcon {
        PluginIcon {
            emoji: self.icon.emoji_text(),
            image: self.icon.image.clone(),
        }
    }
}

/// A feed a plugin can ask for.
///
/// The list is closed and it lives here rather than in the host, because adding
/// an entry is a change to what the product will hand a plugin — and that has to
/// be made in one file a reviewer will see, in the same place as the protocol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Subscription {
    /// Key edges, mouse buttons and mouse movement.
    ///
    /// Already validated by the platform layer and already owned by the runtime;
    /// a subscription decides whether a plugin is *told* about them, not whether
    /// they are believed.
    Input,
    /// The active model's name and whether the model window is visible.
    HostState,
    /// Motion and expression requests by name.
    ModelReaction,
}

impl Subscription {
    pub const ALL: [Self; 3] = [Self::Input, Self::HostState, Self::ModelReaction];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::HostState => "host_state",
            Self::ModelReaction => "model_reaction",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|entry| entry.as_str() == value)
    }
}

/// What a running plugin announced about itself.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDescriptor {
    pub id: PluginId,
    pub name: String,
    pub version: PluginVersion,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: PluginIcon,
    /// The settings the plugin wants the user to be able to change.
    #[serde(default)]
    pub config: ConfigSchema,
    /// The feeds it wants.
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
}

impl PluginDescriptor {
    /// Check the parts of a descriptor that are about the plugin rather than about
    /// this host.
    ///
    /// The subscription list is checked here rather than filtered: a plugin that
    /// asks for a feed the host does not implement is a plugin built against a
    /// different host, and half of it working is worse than a clear refusal.
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.name.trim().is_empty() || self.name.chars().count() > MAXIMUM_PLUGIN_NAME_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginName));
        }
        if self.description.chars().count() > MAXIMUM_PLUGIN_DESCRIPTION_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginDescription));
        }
        if self.id.as_str().len() > MAXIMUM_PLUGIN_ID_BYTES {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginId));
        }
        self.icon.image_path()?;
        self.config.validate()?;
        let mut seen = std::collections::BTreeSet::new();
        for subscription in &self.subscriptions {
            if !seen.insert(*subscription) {
                // Harmless in itself, and refused only so a descriptor is one
                // canonical spelling of itself.
                return Err(PluginError::with_detail(
                    PluginErrorCode::ProtocolInvalid,
                    "a subscription is listed twice",
                ));
            }
        }
        Ok(())
    }

    /// Whether this plugin asked for a feed.
    pub fn wants(&self, subscription: Subscription) -> bool {
        self.subscriptions.contains(&subscription)
    }

    /// Whether the descriptor agrees with the archive it came out of.
    ///
    /// The version is compared and the name is not: a plugin is free to improve
    /// the sentence on its card in a patch release, and refusing to start it over
    /// a caption would be a bug, not a check.
    pub fn agrees_with(&self, manifest: &PluginManifest) -> Result<(), PluginError> {
        if self.id != manifest.id {
            return Err(PluginError::with_detail(
                PluginErrorCode::PluginHandshakeFailed,
                "the running plugin announced a different id than its archive",
            ));
        }
        if self.version != manifest.version {
            return Err(PluginError::with_detail(
                PluginErrorCode::PluginHandshakeFailed,
                "the running plugin announced a different version than its archive",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
        "schema_version": 1,
        "api_version": 1,
        "id": "pomodoro",
        "name": "Pomodoro",
        "version": "1.0.0",
        "executable": "pomodoro"
    }"#;

    #[test]
    fn a_minimal_manifest_reads_with_every_default_applied() {
        let manifest = PluginManifest::parse(MINIMAL.as_bytes()).unwrap();
        assert_eq!(manifest.id.as_str(), "pomodoro");
        assert_eq!(manifest.version, PluginVersion::new(1, 0, 0));
        assert!(manifest.author.is_empty());
        assert!(manifest.icon.emoji.is_none());
    }

    #[test]
    fn a_schema_or_api_version_this_host_does_not_know_is_refused() {
        let bumped_schema = MINIMAL.replace("\"schema_version\": 1", "\"schema_version\": 2");
        assert_eq!(
            PluginManifest::parse(bumped_schema.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::UnsupportedSchemaVersion
        );
        let bumped_api = MINIMAL.replace("\"api_version\": 1", "\"api_version\": 99");
        assert_eq!(
            PluginManifest::parse(bumped_api.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::UnsupportedApiVersion
        );
    }

    #[test]
    fn an_executable_that_escapes_the_plugin_directory_is_refused() {
        // The executable is a path from a document that the host then runs, so it
        // goes through exactly the check an image asset does.
        for path in ["../evil", "/usr/bin/evil", "C:/evil", "a/../../evil", ""] {
            let document = MINIMAL.replace(
                "\"executable\": \"pomodoro\"",
                &format!("\"executable\": {path:?}"),
            );
            assert_eq!(
                PluginManifest::parse(document.as_bytes())
                    .unwrap_err()
                    .code(),
                PluginErrorCode::InvalidAssetPath,
                "{path:?} must be refused"
            );
        }
    }

    #[test]
    fn an_icon_that_escapes_the_plugin_directory_is_refused() {
        let manifest = MINIMAL.replace(
            r#""version": "1.0.0","#,
            r#""version": "1.0.0", "icon": {"image": "../../etc/passwd"},"#,
        );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::InvalidAssetPath
        );
    }

    #[test]
    fn an_emoji_icon_survives_a_manifest_and_a_long_one_is_cut_rather_than_refused() {
        let manifest = PluginManifest::parse(
            MINIMAL
                .replace(
                    r#""version": "1.0.0","#,
                    r#""version": "1.0.0", "icon": {"emoji": "🍅"},"#,
                )
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(manifest.display_icon().emoji_text().as_deref(), Some("🍅"));

        let long: PluginIcon = serde_json::from_str(
            r#"{"emoji":"aaaaaaaaaabbbbbbbbbbccccccccccddddddddddeeeeeeeeeeffffffffff"}"#,
        )
        .unwrap();
        assert_eq!(
            long.emoji_text().as_deref().map(str::len),
            Some(8),
            "eight characters, then the card draws what fits"
        );
    }

    #[test]
    fn a_label_resolves_by_full_tag_then_by_primary_language_then_by_default() {
        let text: LocalizedText = serde_json::from_str(
            r#"{"default":"Timer","by_locale":{"zh-CN":"计时器","zh":"中文"}}"#,
        )
        .unwrap();
        assert_eq!(text.resolve("zh-CN"), "计时器");
        assert_eq!(text.resolve("zh-TW"), "中文", "falls back to the language");
        assert_eq!(text.resolve("ja"), "Timer", "and to the default");
        assert_eq!(text.resolve("en-US"), "Timer");
    }

    #[test]
    fn an_over_long_label_is_cut_with_one_ellipsis() {
        let text = LocalizedText::from("x".repeat(MAXIMUM_LABEL_CHARS + 40));
        let bounded = text.resolve_bounded("en-US");
        assert_eq!(bounded.chars().count(), MAXIMUM_LABEL_CHARS + 1);
        assert!(
            bounded.ends_with('…'),
            "and it is one ellipsis, not three dots"
        );
    }

    #[test]
    fn a_descriptor_that_disagrees_with_its_archive_is_refused() {
        let manifest = PluginManifest::parse(MINIMAL.as_bytes()).unwrap();
        let other: PluginDescriptor =
            serde_json::from_str(r#"{"id":"pomodoro","name":"Pomodoro","version":"2.0.0"}"#)
                .unwrap();
        assert_eq!(
            other.agrees_with(&manifest).unwrap_err().code(),
            PluginErrorCode::PluginHandshakeFailed
        );

        let renamed: PluginDescriptor =
            serde_json::from_str(r#"{"id":"pomodoro","name":"Renamed","version":"1.0.0"}"#)
                .unwrap();
        assert!(
            renamed.agrees_with(&manifest).is_ok(),
            "a plugin may reword its own card without becoming unloadable"
        );
    }

    #[test]
    fn a_subscription_listed_twice_is_refused() {
        let descriptor: PluginDescriptor = serde_json::from_str(
            r#"{"id":"pomodoro","name":"P","version":"1.0.0","subscriptions":["input","input"]}"#,
        )
        .unwrap();
        assert_eq!(
            descriptor.validate().unwrap_err().code(),
            PluginErrorCode::ProtocolInvalid
        );
    }
}
