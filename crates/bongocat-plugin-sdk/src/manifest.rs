//! What a plugin says about itself, read from the `plugin.json` it ships.
//!
//! A plugin's identity and its copy used to be written twice: once in a Rust module
//! that spelled out every translation, and once in `plugin.json` for the card. The two
//! drifted, and the symptom was a card whose own text *changed* when the plugin was
//! started — a sentence that appeared and disappeared with a lifecycle rather than
//! describing the plugin. There is one document now, and this is how a plugin reads it.
//!
//! ```no_run
//! use bongocat_plugin_sdk::{SelfDescription, describe};
//! use std::sync::LazyLock;
//!
//! // A real plugin writes `describe(include_str!("../plugin.json"))`. The literal below
//! // stands in for it so this example is a test of the API rather than of the layout
//! // this crate happens to be checked out in.
//! static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
//!     describe(
//!         r#"{"id":"example-plugin","version":"1.0.0","name":"Example",
//!             "copy":{"focus":{"default":"Focus"}}}"#,
//!     )
//!     .expect("its own manifest")
//! });
//!
//! fn label(host_locale: &str) -> String {
//!     SELF.text("focus", host_locale)
//! }
//! ```
//!
//! The `include_str!("../plugin.json")` a real plugin writes is the whole of the
//! convention, and it is why a plugin's manifest is a file *beside* its `main.rs` rather
//! than anywhere else in its directory.
//!
//! # Why the manifest is embedded rather than read at runtime
//!
//! A plugin is a separate process whose working directory the host chooses, and the
//! manifest is a file inside the archive beside the binary. Reading it at runtime would
//! mean the plugin guessing where the host unpacked it — exactly the coupling ADR-0079
//! removed. So the plugin embeds the file at compile time, which makes the manifest part
//! of the binary: it cannot go missing, it cannot be edited under a running process, and
//! a plugin whose binary and manifest disagree is a plugin whose author finds out at
//! compile time rather than a user finding out on a card.
//!
//! # What the host still does
//!
//! It resolves and it draws. A [`LocalizedText`] arrives from the plugin carrying every
//! language the plugin has copy for, and the host picks the one the user reads — because
//! only the host knows the user's language. What the host never does is hold a
//! translation of a plugin's own words: a string the plugin did not send cannot be drawn,
//! and one it did send needs no second copy anywhere.
//!
//! # The two halves, and why they are one type
//!
//! [`SelfDescription::descriptor`] is the card: id, version, name, description, icon.
//! [`SelfDescription::text`] is the panel and the settings form. They are one type
//! because they are one file, and
//! splitting them would put the drift back — a plugin that updated its panel's words and
//! not its card's, which is precisely the bug this module exists to end.
//!
//! The type is spelled out rather than called `Self` because `Self` is a keyword, and
//! [`SelfDescription`] is what a plugin's own source sees.

use crate::{Error, Result};
use bongocat_plugin_protocol::{
    LocalizedText, PLUGIN_MANIFEST_FILE_NAME, PluginAnchor, PluginIcon, PluginId, PluginVersion,
};
use std::collections::BTreeMap;

/// The manifest file a plugin embeds to describe itself.
pub const MANIFEST_FILE_NAME: &str = PLUGIN_MANIFEST_FILE_NAME;

/// The longest a copy key may be, in bytes.
///
/// The bound a document can rely on rather than anything about the words: a key becomes
/// a JSON object member in the manifest the plugin ships, so this is the same class of
/// limit [`bongocat_plugin_protocol::MAXIMUM_CONFIG_KEY_BYTES`] puts on a settings key.
pub const MAXIMUM_COPY_KEY_BYTES: usize = 64;

/// The most strings one plugin may carry copy for.
///
/// A design bound, like the settings one: past this a plugin has a documentation file
/// wearing a manifest's clothes, and the manifest is read on every launch.
pub const MAXIMUM_COPY_ENTRIES: usize = 256;

/// What a plugin says about itself, and the words it says it with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelfDescription {
    id: String,
    version: String,
    name: LocalizedText,
    description: LocalizedText,
    author: String,
    icon: PluginIcon,
    copy: BTreeMap<String, LocalizedText>,
}

impl SelfDescription {
    /// Read a manifest, and check that it can be used.
    ///
    /// Takes the bytes rather than a path so the caller decides where they came from,
    /// which for a plugin is [`SelfDescription::load`] over an `include_str!`.
    pub fn from_manifest_bytes(bytes: &[u8]) -> Result<SelfDescription> {
        let manifest: WireManifest = serde_json::from_slice(bytes).map_err(|error| {
            Error::Config(format!(
                "this plugin's own {MANIFEST_FILE_NAME} could not be read: {error}"
            ))
        })?;
        let identity = Self {
            id: manifest.id,
            version: manifest.version,
            name: manifest.name,
            description: manifest.description,
            author: manifest.author,
            icon: manifest.icon,
            copy: manifest.copy,
        };
        identity.check()?;
        Ok(identity)
    }

    /// Read a manifest out of the document this plugin embeds.
    ///
    /// The call a plugin makes, once, at the top of `main` — see [`describe`] for the
    /// one-line form a plugin actually writes.
    pub fn load(manifest: &str) -> Result<SelfDescription> {
        Self::from_manifest_bytes(manifest.as_bytes())
    }

    /// A description with no id, no name and no copy.
    ///
    /// Only reachable through [`Default`], and only useful in a test that is about a
    /// lookup rather than about a manifest. A plugin with a real manifest is never bare.
    pub fn empty() -> SelfDescription {
        Self::default()
    }

    /// This plugin's id, as its manifest spells it.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// This plugin's version, as `major.minor.patch`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// This plugin's version, as the protocol's own type.
    ///
    /// [`None`] for a version that is not three numbers, which the descriptor then
    /// reports as a plugin whose manifest is wrong rather than as a plugin that will not
    /// start with no reason given.
    pub fn plugin_version(&self) -> Option<PluginVersion> {
        let mut parts = self.version.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        parts
            .next()
            .is_none()
            .then(|| PluginVersion::new(major, minor, patch))
    }

    /// This plugin's name, in every language it has copy for.
    pub fn name(&self) -> &LocalizedText {
        &self.name
    }

    /// One sentence about what this plugin does, in every language it has copy for.
    pub fn description(&self) -> &LocalizedText {
        &self.description
    }

    /// Who wrote this plugin.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// This plugin's icon.
    pub fn icon(&self) -> &PluginIcon {
        &self.icon
    }

    /// This plugin's own emoji, when its icon is one.
    ///
    /// The common case, named so a plugin does not have to reach into the icon for the
    /// field the card actually draws.
    pub fn emoji(&self) -> Option<&str> {
        self.icon.emoji.as_deref()
    }

    /// This string, in the language the user reads.
    ///
    /// `locale` is the host's, never the plugin's own guess: a plugin that picked its
    /// own would show one language on its panel and another on its settings form, and
    /// the user would have no way to tell which of the two is wrong.
    ///
    /// Falls back to the default rather than failing, because a language the plugin has
    /// no copy for should read as the author's own words and not as a blank panel. A key
    /// the plugin does not carry at all is a different thing and resolves to the key
    /// itself: a settings form labelled `focus_minutes` tells its author exactly which
    /// name to add, where an empty label tells them nothing.
    pub fn text(&self, key: &str, locale: &str) -> String {
        match self.copy.get(key) {
            Some(text) => text.resolve_bounded(locale),
            None => key.to_string(),
        }
    }

    /// This string as a [`LocalizedText`], for a declaration that carries one.
    ///
    /// The same words as [`SelfDescription::text`], unresolved — which is what a field label and a
    /// choice option want, because the host resolves them against the language the user
    /// reads and a plugin that resolved them early would freeze its own settings form in
    /// whatever language happened to be current when it started.
    pub fn label(&self, key: &str) -> LocalizedText {
        self.copy
            .get(key)
            .cloned()
            .unwrap_or_else(|| LocalizedText::from(key))
    }

    /// Whether this plugin carries copy for `key`.
    pub fn has(&self, key: &str) -> bool {
        self.copy.contains_key(key)
    }

    /// How many strings this plugin carries copy for.
    pub fn len(&self) -> usize {
        self.copy.len()
    }

    pub fn is_empty(&self) -> bool {
        self.copy.is_empty()
    }

    /// Every copy key, in name order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.copy.keys().map(String::as_str)
    }

    /// This plugin's own words, resolved against the host's language.
    ///
    /// The one-line form every plugin's `say` helper is: `self.say(host, "focus")`.
    pub fn say(&self, host: &crate::Host, key: &str) -> String {
        self.text(key, host.locale())
    }

    /// Whether this manifest can be used, and why not when it cannot.
    ///
    /// Checked once at load rather than per lookup, so a manifest with an unusable part
    /// is reported once with the offending name in the message — at launch, where a plugin
    /// author is looking — rather than as a settings form with a broken row in it.
    pub fn check(&self) -> Result<()> {
        if self.id.is_empty() || PluginId::new(self.id.clone()).is_err() {
            return Err(Error::Config(format!(
                "this plugin's id {:?} is not one the host accepts",
                self.id
            )));
        }
        if self.plugin_version().is_none() {
            return Err(Error::Config(format!(
                "this plugin's version {:?} is not major.minor.patch",
                self.version
            )));
        }
        if self.name.default.trim().is_empty() {
            return Err(Error::Config(
                "this plugin's manifest names no name to show on a card".to_owned(),
            ));
        }
        if self.copy.len() > MAXIMUM_COPY_ENTRIES {
            return Err(Error::Config(format!(
                "this plugin carries copy for {} strings, and the bound is {MAXIMUM_COPY_ENTRIES}",
                self.copy.len()
            )));
        }
        for (key, text) in &self.copy {
            if key.is_empty() || key.len() > MAXIMUM_COPY_KEY_BYTES {
                return Err(Error::Config(format!(
                    "the copy key {key:?} is not a usable name"
                )));
            }
            if text.default.trim().is_empty() {
                return Err(Error::Config(format!(
                    "the copy for {key:?} has no default to fall back to"
                )));
            }
        }
        self.icon.image_path().map_err(|error| {
            Error::Config(format!(
                "this plugin's icon is not a path the host will read: {error}"
            ))
        })?;
        Ok(())
    }

    /// The descriptor this manifest describes.
    ///
    /// The whole of what [`crate::Plugin::descriptor`] has to be for a plugin that keeps
    /// its metadata in its manifest, which is every plugin: the id, the version, the name,
    /// the sentence and the icon are already written down, and repeating them in Rust is
    /// the duplication this type exists to remove.
    pub fn descriptor(&self) -> crate::Descriptor {
        let mut descriptor = crate::Descriptor::new(&self.id, &self.name.default)
            .named(self.name.clone())
            .described(self.description.clone())
            .icon_value(self.icon.clone());
        if !self.author.is_empty() {
            descriptor = descriptor.author(&self.author);
        }
        if let Some(version) = self.plugin_version() {
            descriptor = descriptor.at(version);
        }
        descriptor
    }
}

/// The manifest as it appears on the wire.
///
/// Its own type rather than the protocol's [`bongocat_plugin_protocol::PluginManifest`]
/// on purpose, and the difference is what this type is for. The protocol's manifest is
/// the *archive's* document: the host reads it to check the id against the running
/// process, and it is strict — an unknown field is a manifest from a newer host. This one
/// is the *authoring* document, read by the plugin's own code, and it must be forgiving
/// about fields the host never sees (the copy table is one) while still refusing the
/// fields whose absence would produce a plugin that cannot start.
#[derive(serde::Deserialize)]
struct WireManifest {
    id: String,
    version: String,
    #[serde(default)]
    name: LocalizedText,
    #[serde(default)]
    description: LocalizedText,
    #[serde(default)]
    author: String,
    #[serde(default)]
    icon: PluginIcon,
    #[serde(default)]
    copy: BTreeMap<String, LocalizedText>,
}

/// Read the manifest a plugin ships and check it, or report why it could not be read.
///
/// A free function rather than a method because a plugin's own manifest is a compile-time
/// constant it embeds, and a plugin wants a *value* it can name in a `static` — which a
/// fallible constructor is not.
pub fn describe(manifest: &str) -> Result<SelfDescription> {
    SelfDescription::load(manifest)
}

/// The nine anchors, for a plugin that lets the user choose one.
///
/// Re-exported here so a plugin writing a position setting names it from the same place
/// it names everything else, and does not have to know that the enum lives in the
/// protocol crate.
pub const ANCHORS: [PluginAnchor; 9] = PluginAnchor::ALL;

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "schema_version": 1,
        "api_version": 1,
        "id": "pomodoro",
        "version": "1.0.0",
        "author": "BongoCat",
        "name": {"default": "Pomodoro", "by_locale": {"zh-CN": "番茄钟"}},
        "description": {"default": "A focus timer.", "by_locale": {"zh-CN": "专注计时器。"}},
        "icon": {"emoji": "🍅"},
        "executable": "pomodoro",
        "copy": {
            "focus": {"default": "Focus", "by_locale": {"zh-CN": "专注"}},
            "reset": {"default": "Reset", "by_locale": {"zh-CN": "重置"}}
        }
    }"#;

    fn loaded() -> SelfDescription {
        SelfDescription::load(MANIFEST).expect("a manifest this plugin ships")
    }

    #[test]
    fn a_manifest_is_the_identity_and_the_copy_in_one_document() {
        let me = loaded();
        assert_eq!(me.id(), "pomodoro");
        assert_eq!(me.version(), "1.0.0");
        assert_eq!(me.plugin_version(), Some(PluginVersion::new(1, 0, 0)));
        assert_eq!(me.author(), "BongoCat");
        assert_eq!(me.emoji(), Some("🍅"));
        assert_eq!(me.name().resolve("zh-CN"), "番茄钟");
        assert_eq!(me.description().resolve("zh-CN"), "专注计时器。");
        assert_eq!(me.text("focus", "zh-CN"), "专注");
    }

    #[test]
    fn a_card_and_a_panel_read_the_same_document_and_cannot_disagree() {
        // The bug this type exists for: the card's sentence and the panel's words were
        // written in two files, and a card changed its own text when the plugin started.
        // Here there is one document, so there is nothing to keep in step.
        let me = loaded();
        let descriptor = me.descriptor();
        assert_eq!(descriptor.id(), "pomodoro");
        assert_eq!(descriptor.name().resolve("zh-CN"), "番茄钟");
        assert_eq!(
            descriptor.description_text().resolve("zh-CN"),
            "专注计时器。",
            "the card reads the manifest's own sentence, in the reader's language"
        );
        assert_eq!(*descriptor.plugin_version(), PluginVersion::new(1, 0, 0));
    }

    #[test]
    fn a_manifest_that_is_not_the_shape_a_plugin_needs_is_refused_with_a_reason() {
        // Each of these would produce a plugin that cannot start, and each is refused
        // with the part that is wrong in the message rather than as a refusal with no
        // detail, because that is what a plugin author has to work from.
        let no_id = SelfDescription::load(r#"{"id":"","version":"1.0.0","name":"X"}"#)
            .expect_err("an id is required");
        assert!(no_id.to_string().contains("id"), "{no_id}");

        let spaced_id = SelfDescription::load(r#"{"id":"a b","version":"1.0.0","name":"X"}"#)
            .expect_err("a space is not an id");
        assert!(spaced_id.to_string().contains("id"), "{spaced_id}");

        let bad_version = SelfDescription::load(r#"{"id":"pomodoro","version":"one","name":"X"}"#)
            .expect_err("a version is three numbers");
        assert!(bad_version.to_string().contains("version"), "{bad_version}");

        let no_name = SelfDescription::load(r#"{"id":"pomodoro","version":"1.0.0","name":""}"#)
            .expect_err("a name");
        assert!(no_name.to_string().contains("name"), "{no_name}");

        let escaping_icon = SelfDescription::load(
            r#"{"id":"pomodoro","version":"1.0.0","name":"X","icon":{"image":"../../etc/passwd"}}"#,
        )
        .expect_err("an icon is a path the host reads");
        assert!(
            escaping_icon.to_string().contains("icon"),
            "{escaping_icon}"
        );
    }

    #[test]
    fn a_manifest_that_is_not_json_names_the_file_rather_than_saying_a_parse_error() {
        let error =
            SelfDescription::load("{ not json").expect_err("a broken manifest is a failure");
        assert!(error.to_string().contains(MANIFEST_FILE_NAME), "{error}");
    }

    #[test]
    fn a_manifest_with_no_copy_is_still_a_usable_manifest() {
        // The metadata a card needs is not the copy a panel needs, and a plugin that has
        // not moved its words yet still has a card that reads correctly. Refusing the
        // load would take that plugin's panel away over a missing table.
        let me = SelfDescription::load(
            r#"{"id":"pomodoro","version":"1.0.0","name":"Pomodoro","executable":"pomodoro"}"#,
        )
        .expect("a manifest without copy still reads");
        assert!(me.is_empty());
        assert_eq!(
            me.text("focus", "en-US"),
            "focus",
            "and a missing key names itself, which is a message its author can act on"
        );
    }

    #[test]
    fn a_label_carries_every_language_rather_than_one_already_resolved_string() {
        // A settings form and a choice option take a `LocalizedText` and the host
        // resolves it. A plugin that resolved early would freeze its own form in the
        // language that happened to be current when it started, which is exactly the bug
        // a language switch is supposed to fix.
        let me = loaded();
        let label = me.label("focus");
        assert_eq!(label.resolve("en-US"), "Focus");
        assert_eq!(label.resolve("zh-CN"), "专注");
    }

    #[test]
    fn a_copy_table_that_cannot_be_used_is_refused_at_load() {
        let empty_default = SelfDescription::load(
            r#"{"id":"plugin","version":"1.0.0","name":"P","copy":{"focus":{"default":"  "}}}"#,
        )
        .expect_err("an empty default has nothing to fall back to");
        assert!(
            empty_default.to_string().contains("focus"),
            "{empty_default}"
        );

        let long_key = SelfDescription::load(&format!(
            r#"{{"id":"plugin","version":"1.0.0","name":"P","copy":{{"{}":{{"default":"x"}}}}}}"#,
            "k".repeat(MAXIMUM_COPY_KEY_BYTES + 1)
        ))
        .expect_err("a key longer than a JSON member may be is not a usable name");
        assert!(long_key.to_string().contains("copy key"), "{long_key}");

        let entries: BTreeMap<String, LocalizedText> = (0..=MAXIMUM_COPY_ENTRIES)
            .map(|index| (format!("k{index}"), LocalizedText::from("x")))
            .collect();
        let oversized = WireManifest {
            id: "plugin".to_owned(),
            version: "1.0.0".to_owned(),
            name: LocalizedText::from("P"),
            description: LocalizedText::default(),
            author: String::new(),
            icon: PluginIcon::default(),
            copy: entries,
        };
        let me = SelfDescription {
            id: oversized.id,
            version: oversized.version,
            name: oversized.name,
            description: oversized.description,
            author: oversized.author,
            icon: oversized.icon,
            copy: oversized.copy,
        };
        assert!(me.check().is_err());
    }

    #[test]
    fn every_key_is_reachable_and_ordered_so_nothing_is_shadowed() {
        let me = loaded();
        assert_eq!(
            me.keys().collect::<Vec<_>>(),
            ["focus", "reset"],
            "in name order, so a manifest a person reads is a manifest whose order means \
             something"
        );
    }

    #[test]
    fn the_anchor_list_is_the_nine_positions_the_model_window_has() {
        assert_eq!(ANCHORS.len(), 9);
        assert_eq!(ANCHORS[0], PluginAnchor::TopLeft);
        assert_eq!(ANCHORS[4], PluginAnchor::Center);
        assert_eq!(ANCHORS[8], PluginAnchor::BottomRight);
    }
}
