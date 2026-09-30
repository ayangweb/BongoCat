//! What a plugin is, and the whole contract between a plugin and BongoCat.
//!
//! This crate is the contract. It depends on `serde` and on the render
//! vocabulary — and on nothing else. It has no idea how a plugin is found,
//! downloaded, started, laid out on screen or driven; those decisions belong to
//! the crates above it, and a plugin author needs none of them.
//!
//! # The shape of the system
//!
//! A plugin is **a program**. It is its own executable, in its own directory,
//! with its own dependencies and its own data, and the host starts it and speaks
//! a versioned line protocol over its pipes. Nothing a plugin ships is loaded into
//! the host's address space, so there is no `unsafe` boundary to keep and a plugin
//! that faults takes only itself down.
//!
//! What the host keeps is the *appearance*: a plugin sends a scene, and the same
//! layout, font, theme and raster the product already has turn it into the pixels
//! the model window draws. So a plugin computes and the host draws, which is why a
//! panel looks like part of the product without the plugin knowing what a theme
//! is.
//!
//! `docs/adr/0079-plugins-are-independent-processes.md` is the decision this
//! implements, and it is the place to look for why a plugin is a process and not a
//! library, a document, or a WebAssembly component.
//!
//! # What a plugin owns, and what it does not
//!
//! Owned by the plugin: its logic, its state, its persistence, its copy, its
//! dependencies, its dependencies' versions. Not owned by the plugin: the panel's
//! look, the font, the placement arithmetic, the model window, the config file it
//! is given a schema for. The line is drawn once, in the ADR, because getting it
//! wrong in either direction is a mistake the type system cannot catch.
//!
//! # Versions
//!
//! Four version numbers, all distinct, and all of them strict:
//!
//! * `schema_version` — the shape of `plugin.json`, and of a configuration
//!   schema. Like `config.json` it is a single accepted version with no
//!   migration, so a document the host does not recognise is refused rather than
//!   half-read.
//! * `api_version` — the feature level a plugin needs. A plugin declaring a higher
//!   one than the host implements is not loaded, rather than loaded with the
//!   features it asked for quietly missing.
//! * `PROTOCOL_VERSION` — the shape of the messages in both directions. Checked on
//!   the first message each way, so a mismatch is a refusal rather than a host
//!   writing a document a plugin will read as something else.
//! * `version` — the plugin's own release, ordered for the plugin center's update
//!   comparison.

#![forbid(unsafe_code)]

mod catalog;
mod color;
mod config;
mod control;
mod descriptor;
mod error;
mod host_state;
mod identity;
mod ipc;
mod panel;
pub mod scene;

pub use catalog::{
    MAXIMUM_CATALOG_ENTRIES, PLUGIN_CATALOG_FILE_NAME, PLUGIN_CATALOG_REPOSITORY_NAME,
    PLUGIN_CATALOG_REPOSITORY_OWNER, PLUGIN_CATALOG_SCHEMA_VERSION, PLUGIN_SCHEMA_VERSION,
    PluginCatalog, PluginCatalogEntry, PluginDownload, SUPPORTED_PLUGIN_API_VERSION,
};
pub use color::{Color, ColorError};
pub use config::{
    CONFIG_SCHEMA_VERSION, ChoiceOption, ConfigControl, ConfigDocument, ConfigField, ConfigKind,
    ConfigSchema, ConfigValue, MAXIMUM_CHOICE_OPTIONS, MAXIMUM_CONFIG_FIELDS,
    MAXIMUM_CONFIG_KEY_BYTES, MAXIMUM_CONFIG_TEXT_BYTES,
};
pub use control::{control_label, keypad_label};
pub use descriptor::{
    LocalizedText, MAXIMUM_BUTTON_ID_BYTES, MAXIMUM_BUTTONS_PER_PANEL,
    MAXIMUM_PLUGIN_DESCRIPTION_CHARS, MAXIMUM_PLUGIN_NAME_CHARS, PLUGIN_MANIFEST_FILE_NAME,
    PluginDescriptor, PluginIcon, PluginManifest, Subscription,
};
pub use error::{PluginError, PluginErrorCode};
pub use host_state::{
    HostState, InputEvent, MAXIMUM_BUBBLE_CHARS, MAXIMUM_BUBBLE_MILLIS, MAXIMUM_MOUSE_STEP,
    MINIMUM_BUBBLE_MILLIS, ModelOutcome, ModelRequest, ModelRequestKind,
};
pub use identity::{InstalledPlugin, MAXIMUM_PLUGIN_ID_BYTES, PluginId, PluginVersion};
pub use ipc::{
    Hello, HostMessage, LogLevel, MAXIMUM_MESSAGE_BYTES, PROTOCOL_VERSION, PluginMessage,
    PluginRuntimeStatus, check_line_length, parse_host_message, parse_plugin_message,
    write_message,
};
pub use ipc::{ModelAnswer, WallClock};
pub use panel::{PanelPlacement, PanelUpdate, PluginAnchor};
pub use scene::{
    Align, ButtonNode, ButtonVariant, DividerNode, ImageNode, MAXIMUM_SCENE_DEPTH,
    MAXIMUM_SCENE_NODES, ProgressBarNode, ProgressRingNode, SceneNode, SpacerNode, StackAxis,
    StackNode, TextNode, TextWeight,
};

/// The longest a panel's logical side may be, in pixels.
///
/// A panel is rasterized into a texture the drawable samples, so this is a
/// memory bound as much as a design one. Four times the model window's own
/// default width is already a panel that covers the model it sits beside.
pub const MAXIMUM_PANEL_SIDE: u32 = 2048;

/// The smallest a panel's logical side may be, in pixels.
///
/// Below this there is no room for text at the sizes a model window is read at,
/// and a panel that small is a mistake rather than a design.
pub const MINIMUM_PANEL_SIDE: u32 = 32;

/// Check a path a plugin named, before it is joined to anything.
///
/// One check for every path a plugin can name — its icon, its images, its
/// executable — because the ways out of a plugin's own directory are worth
/// enumerating once: `..`, an absolute path, a Windows drive letter, a UNC prefix,
/// an empty component, and a NUL byte that would truncate the name on the way to
/// the filesystem.
///
/// The rule is deliberately narrow rather than clever: a relative path with
/// forward slashes, no `.` or `..` component, and no prefix of any kind. A
/// plugin that wants a file in a subdirectory can name one; a plugin that wants
/// to reach outside its own directory cannot spell it.
pub fn validate_relative_asset_path(value: &str) -> Result<(), PluginError> {
    if value.is_empty()
        || value.len() > 255
        || value.contains('\0')
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(PluginError::new(PluginErrorCode::InvalidAssetPath));
    }
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(PluginError::new(PluginErrorCode::InvalidAssetPath));
        }
    }
    Ok(())
}

/// Check a URL a catalog named, before anything is fetched from it.
///
/// HTTPS only, and only from the hosts this product already fetches releases
/// from. A catalog is signed, so its contents are authentic — but a signed
/// catalog that names an arbitrary host is a way for whoever holds the signing
/// key to make every client connect anywhere, and the signing key should not be
/// worth that.
pub fn validate_release_url(value: &str) -> Result<(), PluginError> {
    let Some((scheme, rest)) = value.split_once("://") else {
        return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
    };
    if scheme != "https" {
        return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once('@')
        .map_or_else(
            || rest.split('/').next().unwrap_or_default(),
            |(_, host)| host,
        );
    let host = host.split(':').next().unwrap_or_default();
    let permitted = host == GITHUB_HOST
        || GITHUB_PROXY_HOSTS
            .iter()
            .any(|proxy| host == *proxy || host.ends_with(&format!(".{proxy}")));
    if !permitted {
        return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
    }
    Ok(())
}

/// The only host a release asset is fetched from directly.
pub const GITHUB_HOST: &str = "github.com";

/// The proxy hosts, as bare names.
///
/// The prefixes live in `bongocat-update` next to the policy that uses them. A
/// catalog URL is checked here, at parse time, before anything is fetched, and it
/// has to know the same host set that the fetch will later be willing to use —
/// otherwise a catalog could pass validation and then be handed to a transport
/// that had to make the same decision again, later, on a different code path.
pub const GITHUB_PROXY_HOSTS: &[&str] = &[
    "gh-proxy.org",
    "cdn.gh-proxy.org",
    "v6.gh-proxy.org",
    "axisnow.gh-proxy.org",
    "v4.gh-proxy.org",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_relative_path_is_accepted() {
        for path in ["icon.png", "assets/icon.png", "a/b/c.png", "pomodoro"] {
            assert!(validate_relative_asset_path(path).is_ok(), "{path}");
        }
    }

    #[test]
    fn every_way_out_of_the_plugin_directory_is_refused() {
        for path in [
            "",
            "/etc/passwd",
            "\\windows\\system32",
            "C:/windows",
            "..",
            "../secret.png",
            "assets/../../secret.png",
            "./icon.png",
            "assets//icon.png",
            "C:\\windows",
            "icon.png\0",
        ] {
            assert!(
                validate_relative_asset_path(path).is_err(),
                "{path:?} must be refused"
            );
        }
    }

    #[test]
    fn a_release_url_must_be_https_on_a_host_this_product_fetches_from() {
        for url in [
            "https://github.com/ayangweb/BongoCat/releases/download/p/v.zip",
            "https://cdn.gh-proxy.org/https://github.com/a/b",
            "https://v4.gh-proxy.org/https://github.com/a/b",
        ] {
            assert!(validate_release_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://github.com/a/b",
            "https://example.invalid/a.zip",
            "https://github.com.evil.invalid/a.zip",
            "ftp://github.com/a",
            "github.com/a",
            "https://user:pass@evil.invalid/a.zip",
        ] {
            assert!(validate_release_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn an_anchor_spells_the_same_way_in_the_protocol_and_the_renderer() {
        for anchor in PluginAnchor::ALL {
            assert!(bongocat_render::OverlayAnchor::parse(anchor.as_str()).is_some());
        }
    }
}
