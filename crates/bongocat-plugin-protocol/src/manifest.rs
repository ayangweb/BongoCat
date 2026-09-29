//! What a plugin declares about itself, and what it contributes to the model window.
//!
//! `plugin.json` is the whole of a plugin's identity. It is read once, at load,
//! and everything in it is checked before the plugin is allowed to contribute
//! anything: an id that is not a safe directory name, a panel wider than the
//! model window, a scene that names a behavior that was never declared. A plugin
//! that fails a check contributes nothing at all rather than contributing part of
//! itself, because half a panel is harder to report than a refused plugin.

use super::behavior::{BehaviorId, BehaviorSpec, MAXIMUM_BEHAVIORS_PER_PLUGIN};
use super::error::{PluginError, PluginErrorCode};
use super::scene::SceneNode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The longest a plugin id may be, in bytes.
///
/// The id becomes a directory name under the plugin store, so the bound is
/// about what a filesystem accepts rather than about anything a plugin needs.
pub const MAXIMUM_PLUGIN_ID_BYTES: usize = 64;

/// The longest a plugin's display name may be, in characters.
pub const MAXIMUM_PLUGIN_NAME_CHARS: usize = 64;

/// The longest a plugin's description may be, in characters.
pub const MAXIMUM_PLUGIN_DESCRIPTION_CHARS: usize = 200;

/// The longest a behavior id may be, in bytes.
pub const MAXIMUM_BEHAVIOR_ID_BYTES: usize = 32;

/// The longest a button id may be, in bytes.
pub const MAXIMUM_BUTTON_ID_BYTES: usize = 32;

/// The most bindings paths one plugin may expose.
pub const MAXIMUM_BINDINGS_PER_PLUGIN: usize = 256;

/// The file name a plugin's manifest has inside its own directory.
pub const PLUGIN_MANIFEST_FILE_NAME: &str = "plugin.json";

/// A plugin's stable identity.
///
/// Lowercase ASCII letters, digits and single hyphens, starting and ending with a
/// letter or a digit. The rule is the one Obsidian settled on for the same
/// reason: the id becomes a directory name on two filesystems and a lookup key in
/// a published catalog, and a spelling that can differ by case between them is a
/// plugin that is installed twice.
///
/// Deserialization validates rather than trusting the document, because an id
/// read from a manifest becomes a path. A `#[serde(transparent)]` newtype would
/// accept `"../secrets"` and hand it straight to the store.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(id: impl Into<String>) -> Result<Self, PluginError> {
        let id = id.into();
        if id.is_empty() || id.len() > MAXIMUM_PLUGIN_ID_BYTES {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginId));
        }
        if !is_safe_plugin_id(&id) {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginId));
        }
        Ok(Self(id))
    }

    /// Read an id that is already known good.
    ///
    /// For the paths where the id came out of [`Self::new`] or off a validated
    /// manifest, so re-checking it would only be a second way to be wrong.
    pub fn from_validated(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for PluginId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

fn is_safe_plugin_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    let edge = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if !edge(bytes[0]) || !edge(bytes[bytes.len() - 1]) {
        return false;
    }
    let mut previous_was_hyphen = false;
    for &byte in bytes {
        if byte == b'-' {
            if previous_was_hyphen {
                return false;
            }
            previous_was_hyphen = true;
        } else if edge(byte) {
            previous_was_hyphen = false;
        } else {
            return false;
        }
    }
    true
}

/// A semantic version, ordered well enough to compare two plugin releases.
///
/// Written rather than borrowed from a version crate: the only comparison the
/// plugin centre needs is "is the catalog's version newer than what is
/// installed", and a three-number tuple does that without a dependency and
/// without a pre-release rule nobody would apply to a plugin catalogue anyway.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct PluginVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl From<String> for PluginVersion {
    fn from(value: String) -> Self {
        Self::parse(&value).unwrap_or_default()
    }
}

impl From<PluginVersion> for String {
    fn from(value: PluginVersion) -> Self {
        value.to_string()
    }
}

impl Default for PluginVersion {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

impl PluginVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for PluginVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What a plugin asks the host for.
///
/// Every capability in this list is a *declaration*, and the host's answer is
/// what the plugin actually gets. There is no way to request a capability that
/// is not in this enum, which is what keeps the list honest: adding a capability
/// is a change to the product's trust surface, and it has to be made in this file
/// and in the ADR rather than discovered later.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PluginCapabilities {
    /// The only capability in the first version: contribute a scene to the model
    /// window. It is the default because a plugin that cannot draw is not a
    /// plugin yet.
    #[default]
    #[serde(rename = "model_window.overlay")]
    ModelWindowOverlay,
    /// Read the host's local wall clock. The `local_time` behavior needs it, and
    /// it is separate from the overlay so the split is visible in the manifest.
    #[serde(rename = "host.clock")]
    HostClock,
    /// A plugin may be pressed in the model window.
    #[serde(rename = "host.pointer")]
    HostPointer,
}

impl PluginCapabilities {
    pub const ALL: [Self; 3] = [Self::ModelWindowOverlay, Self::HostClock, Self::HostPointer];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelWindowOverlay => "model_window.overlay",
            Self::HostClock => "host.clock",
            Self::HostPointer => "host.pointer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|entry| entry.as_str() == value)
    }

    /// Whether the host grants a capability.
    ///
    /// The model window overlay is granted to every plugin because it is what
    /// makes a plugin load at all; the other two are granted because the product
    /// has no configuration for refusing them yet, and saying so in one place is
    /// what a future consent setting would replace.
    pub const fn is_granted(self) -> bool {
        matches!(
            self,
            Self::ModelWindowOverlay | Self::HostClock | Self::HostPointer
        )
    }
}

/// Where a contributed panel is pinned in the model window.
///
/// The same nine positions the overlay's layer placement uses, spelled the same
/// way, so a plugin author's choice reads identically in `plugin.json` and in the
/// renderer that places it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginAnchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl PluginAnchor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::CenterLeft => "center_left",
            Self::Center => "center",
            Self::CenterRight => "center_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }

    /// The overlay's own anchor, which is where the placement arithmetic lives.
    pub const fn to_overlay_anchor(self) -> bongocat_render::OverlayAnchor {
        match self {
            Self::TopLeft => bongocat_render::OverlayAnchor::TopLeft,
            Self::TopCenter => bongocat_render::OverlayAnchor::TopCenter,
            Self::TopRight => bongocat_render::OverlayAnchor::TopRight,
            Self::CenterLeft => bongocat_render::OverlayAnchor::CenterLeft,
            Self::Center => bongocat_render::OverlayAnchor::Center,
            Self::CenterRight => bongocat_render::OverlayAnchor::CenterRight,
            Self::BottomLeft => bongocat_render::OverlayAnchor::BottomLeft,
            Self::BottomCenter => bongocat_render::OverlayAnchor::BottomCenter,
            Self::BottomRight => bongocat_render::OverlayAnchor::BottomRight,
        }
    }
}

/// The panel a plugin contributes to the model window.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayContribution {
    #[serde(default)]
    pub anchor: PluginAnchor,
    /// Gap from the window edge, as a fraction of the window's width and height.
    #[serde(default, skip_serializing_if = "is_zero_pair")]
    pub margin: [f32; 2],
    /// The panel's width as a fraction of the window's width.
    #[serde(default = "default_width_fraction")]
    pub width_fraction: f32,
    #[serde(default = "default_one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    /// The panel's logical size in pixels, `[width, height]`.
    ///
    /// Logical rather than device, so a panel is the same size relative to the
    /// window at every display scale; the host multiplies by its own raster
    /// scale.
    pub size: [u32; 2],
    /// Behaviors the host runs for this plugin, in declaration order.
    #[serde(default)]
    pub behaviors: Vec<NamedBehavior>,
    /// The panel itself.
    pub scene: SceneNode,
}

const fn default_width_fraction() -> f32 {
    0.72
}

const fn default_one() -> f32 {
    1.0
}

fn is_zero_pair(value: &[f32; 2]) -> bool {
    value[0] == 0.0 && value[1] == 0.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

/// A behavior with the name its scene binds to.
///
/// Written flat, as `{"id": "timer", "kind": "countdown", "duration_seconds": 1500}`:
/// the name and the behavior read as one declaration, which is what a scene's
/// binding path refers to.
///
/// The reader is written out rather than derived because `#[serde(flatten)]`
/// cannot be combined with `deny_unknown_fields`. Derived, it would silently
/// accept `{"id": "t", "kind": "countdown", "step": 3}` — a field belonging to a
/// different behavior — and refuse only the fields the chosen spec happens to
/// name. Handled here, the spec's own `deny_unknown_fields` sees every field but
/// the id, so a mistyped field is still a load failure.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamedBehavior {
    pub id: BehaviorId,
    #[serde(serialize_with = "serialize_spec")]
    pub spec: BehaviorSpec,
}

fn serialize_spec<S: serde::Serializer>(
    spec: &BehaviorSpec,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    // The kind has to be written alongside the fields so the document reads the
    // same way it did on the way in. `BehaviorSpec` is internally tagged, so it
    // flattens to a map; merging `kind` into it is all the flatten would have
    // done.
    let mut as_map = serde_json::to_value(spec)
        .map_err(serde::ser::Error::custom)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    as_map.insert("kind".to_string(), serde_json::Value::from(spec.kind()));
    let mut map = serializer.serialize_map(Some(as_map.len()))?;
    for (key, value) in &as_map {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

impl<'de> Deserialize<'de> for NamedBehavior {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut object = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let Some(id) = object.remove("id") else {
            return Err(serde::de::Error::missing_field("id"));
        };
        let id: BehaviorId = serde_json::from_value(id).map_err(serde::de::Error::custom)?;
        let spec: BehaviorSpec = serde_json::from_value(serde_json::Value::Object(object))
            .map_err(serde::de::Error::custom)?;
        Ok(Self { id, spec })
    }
}

/// The one file that is a plugin.
///
/// Read from `<plugin directory>/plugin.json`. Every field but `id`, `name` and
/// `overlay` has a default, so a manifest that adds a field stays readable by an
/// older host and a manifest from a newer host is refused by `api_version` rather
/// than by a missing-field error nobody can act on.
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
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// The oldest BongoCat that can run this plugin.
    #[serde(default)]
    pub min_app_version: Option<PluginVersion>,
    #[serde(default)]
    pub capabilities: Vec<PluginCapabilities>,
    /// A PNG under the plugin's own directory, shown in the plugin centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub overlay: OverlayContribution,
}

impl PluginManifest {
    /// Read and check a manifest.
    ///
    /// The checks are in the order a user would want to hear about them: what the
    /// file says first, then whether this host can run it, then whether the
    /// things it names are the things it declared.
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
        if self.api_version > super::catalog::SUPPORTED_PLUGIN_API_VERSION {
            return Err(PluginError::new(PluginErrorCode::UnsupportedApiVersion));
        }
        if self.api_version == 0 {
            return Err(PluginError::new(PluginErrorCode::UnsupportedApiVersion));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > MAXIMUM_PLUGIN_NAME_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginName));
        }
        if self.description.chars().count() > MAXIMUM_PLUGIN_DESCRIPTION_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginDescription));
        }
        if self
            .icon
            .as_deref()
            .is_some_and(|icon| super::validate_relative_asset_path(icon).is_err())
        {
            return Err(PluginError::new(PluginErrorCode::InvalidAssetPath));
        }
        if !self
            .capabilities
            .iter()
            .all(|capability| capability.is_granted())
        {
            return Err(PluginError::new(PluginErrorCode::CapabilityNotGranted));
        }
        self.overlay.validate()
    }

    /// The directory a plugin's own files live under, resolved against its root.
    ///
    /// Every path a plugin names goes through this, which is the single place
    /// that refuses `..`, an absolute path, a drive letter and a UNC prefix.
    pub fn asset_path(&self, root: &Path, relative: &str) -> Result<PathBuf, PluginError> {
        super::validate_relative_asset_path(relative)?;
        Ok(root.join(relative))
    }
}

impl OverlayContribution {
    fn validate(&self) -> Result<(), PluginError> {
        if self.behaviors.len() > MAXIMUM_BEHAVIORS_PER_PLUGIN {
            return Err(PluginError::new(PluginErrorCode::TooManyBehaviors));
        }
        if self.size[0] == 0
            || self.size[1] == 0
            || self.size[0] > super::MAXIMUM_PANEL_SIDE
            || self.size[1] > super::MAXIMUM_PANEL_SIDE
        {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelSize));
        }
        if !self.width_fraction.is_finite()
            || !(0.05..=bongocat_render::OverlayLayerPlacement::MAXIMUM_WIDTH_FRACTION)
                .contains(&self.width_fraction)
        {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }
        if !self.margin.iter().all(|margin| {
            margin.is_finite()
                && (0.0..=bongocat_render::OverlayLayerPlacement::MAXIMUM_MARGIN_FRACTION)
                    .contains(margin)
        }) {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }

        let mut ids = BTreeSet::new();
        for behavior in &self.behaviors {
            if behavior.id.as_str().is_empty()
                || behavior.id.as_str().len() > MAXIMUM_BEHAVIOR_ID_BYTES
                || !behavior
                    .id
                    .as_str()
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(PluginError::new(PluginErrorCode::InvalidBehaviorId));
            }
            if !ids.insert(behavior.id.clone()) {
                return Err(PluginError::new(PluginErrorCode::DuplicateBehaviorId));
            }
        }
        for behavior in &self.behaviors {
            super::validate_behavior_spec(&behavior.spec)?;
        }

        let mut scene = super::scene::inspect::Inspector::new();
        super::scene::inspect::walk(&self.scene, 1, &mut scene)?;
        if scene.bindings.len() > MAXIMUM_BINDINGS_PER_PLUGIN {
            return Err(PluginError::new(PluginErrorCode::SceneTooLarge));
        }
        for binding in &scene.bindings {
            let Some((source, _)) = binding.split_once('.') else {
                return Err(PluginError::new(PluginErrorCode::InvalidBinding));
            };
            if !ids.iter().any(|id| id.as_str() == source) {
                return Err(PluginError::new(PluginErrorCode::UnknownBinding));
            }
        }
        for action in &scene.actions {
            let Some(target) = &action.target else {
                continue;
            };
            if !ids.contains(target) {
                return Err(PluginError::new(PluginErrorCode::UnknownBinding));
            }
        }
        let button_ids: BTreeSet<&str> = scene.buttons.iter().map(String::as_str).collect();
        if button_ids.len() != scene.buttons.len() {
            return Err(PluginError::new(PluginErrorCode::DuplicateButtonId));
        }
        Ok(())
    }
}

/// A plugin as the plugin store holds it on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPlugin {
    pub id: PluginId,
    pub version: PluginVersion,
    /// The version's own directory under the store.
    pub directory: PathBuf,
    /// Whether the user has this plugin switched on.
    pub enabled: bool,
}

impl InstalledPlugin {
    pub fn manifest_path(&self) -> PathBuf {
        self.directory.join(PLUGIN_MANIFEST_FILE_NAME)
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
        "overlay": {
            "size": [220, 120],
            "scene": {"type": "text", "value": "Focus"}
        }
    }"#;

    #[test]
    fn a_minimal_manifest_reads_with_every_default_applied() {
        let manifest = PluginManifest::parse(MINIMAL.as_bytes()).unwrap();
        assert_eq!(manifest.id.as_str(), "pomodoro");
        assert_eq!(manifest.version, PluginVersion::new(1, 0, 0));
        assert!(manifest.author.is_empty());
        assert_eq!(manifest.overlay.anchor, PluginAnchor::TopLeft);
        assert_eq!(manifest.overlay.width_fraction, 0.72);
        assert!(manifest.overlay.behaviors.is_empty());
    }

    #[test]
    fn an_id_that_could_escape_a_directory_is_refused() {
        for id in ["..", "a/b", "A-b", "-ab", "ab-", "a--b", "ab_c", ""] {
            assert!(PluginId::new(id).is_err(), "{id:?} must not be a plugin id");
        }
        assert!(PluginId::new("ab").is_err(), "two characters is too short");
        assert!(PluginId::new("a1-b2").is_ok());
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
    fn a_panel_larger_than_the_bound_is_refused() {
        let huge = MINIMAL.replace("\"size\": [220, 120]", "\"size\": [99999, 120]");
        assert_eq!(
            PluginManifest::parse(huge.as_bytes()).unwrap_err().code(),
            PluginErrorCode::InvalidPanelSize
        );
        let zero = MINIMAL.replace("\"size\": [220, 120]", "\"size\": [0, 120]");
        assert_eq!(
            PluginManifest::parse(zero.as_bytes()).unwrap_err().code(),
            PluginErrorCode::InvalidPanelSize
        );
    }

    #[test]
    fn a_binding_to_an_undeclared_behavior_is_refused() {
        let manifest = MINIMAL.replace(
            r#"{"type": "text", "value": "Focus"}"#,
            r#"{"type": "text", "value": {"binding": "timer.progress", "fallback": ""}}"#,
        );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::UnknownBinding
        );
    }

    #[test]
    fn a_button_targeting_an_undeclared_behavior_is_refused() {
        let manifest = MINIMAL.replace(
            r#"{"type": "text", "value": "Focus"}"#,
            r#"{"type": "button", "id": "go", "label": "Go", "action": "toggle", "target": "timer"}"#,
        );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::UnknownBinding
        );
    }

    #[test]
    fn two_behaviors_may_not_share_a_name() {
        let manifest = MINIMAL.replace(
            r#""scene": {"type": "text", "value": "Focus"}"#,
            r#""behaviors": [{"id": "t", "kind": "counter"}, {"id": "t", "kind": "counter"}],
               "scene": {"type": "text", "value": "Focus"}"#,
        );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::DuplicateBehaviorId
        );
    }

    #[test]
    fn two_buttons_may_not_share_a_name() {
        let manifest = MINIMAL
            .replace(
                r#"{"type": "text", "value": "Focus"}"#,
                r#"{"type": "stack", "children": [
                    {"type": "button", "id": "go", "label": "a", "action": "toggle", "target": "t"},
                    {"type": "button", "id": "go", "label": "b", "action": "toggle", "target": "t"}
                ]}"#,
            )
            .replace(
                r#""scene""#,
                r#""behaviors": [{"id": "t", "kind": "counter"}], "scene""#,
            );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::DuplicateButtonId
        );
    }

    #[test]
    fn an_icon_that_escapes_the_plugin_directory_is_refused() {
        let manifest = MINIMAL.replace(
            r#""version": "1.0.0","#,
            r#""version": "1.0.0", "icon": "../../etc/passwd","#,
        );
        assert_eq!(
            PluginManifest::parse(manifest.as_bytes())
                .unwrap_err()
                .code(),
            PluginErrorCode::InvalidAssetPath
        );
    }

    #[test]
    fn a_declared_capability_the_host_withholds_is_refused() {
        // Every current capability is granted, so this asserts the wiring rather
        // than a refusal: the check has to be reached, not skipped.
        let manifest = MINIMAL.replace(
            r#""version": "1.0.0","#,
            r#""version": "1.0.0", "capabilities": ["model_window.overlay", "host.clock"],"#,
        );
        assert!(PluginManifest::parse(manifest.as_bytes()).is_ok());
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(
            PluginVersion::parse("1.2.3"),
            Some(PluginVersion::new(1, 2, 3))
        );
        assert!(PluginVersion::parse("1.2").is_none());
        assert!(PluginVersion::parse("1.2.3.4").is_none());
        assert!(PluginVersion::parse("x.y.z").is_none());
        assert!(PluginVersion::new(1, 0, 0) < PluginVersion::new(1, 0, 1));
        assert!(PluginVersion::new(1, 9, 0) < PluginVersion::new(2, 0, 0));
    }

    /// The reference plugin the development catalog points at.
    ///
    /// Parsed here rather than only when a developer installs it, because it is the
    /// one manifest a plugin author reads: if it stops being valid, every example
    /// anyone copies is wrong too.
    const REFERENCE_POMODORO: &str = include_str!("../../../plugins/pomodoro/plugin.json");

    #[test]
    fn the_shipped_reference_plugin_is_a_valid_manifest() {
        let manifest = PluginManifest::parse(REFERENCE_POMODORO.as_bytes())
            .expect("the reference plugin must be a manifest this host accepts");
        assert_eq!(manifest.id.as_str(), "pomodoro");
        assert_eq!(manifest.overlay.behaviors.len(), 1);
        assert_eq!(manifest.overlay.behaviors[0].id.as_str(), "timer");
    }
}
