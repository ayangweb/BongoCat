//! What a plugin is, and what the host lets it put on the model window.
//!
//! This crate is the whole contract between a plugin and BongoCat. It depends on
//! `serde` and on the render vocabulary — and on nothing else. It has no idea how
//! a plugin is found, downloaded, laid out on screen or driven; those decisions
//! belong to the crates above it, and a plugin author needs none of them to write
//! a valid `plugin.json`.
//!
//! # The shape of the system
//!
//! A plugin is **data, not code**. It declares a *manifest* and a *scene*: a tree
//! of panels, text, bars, rings and buttons, plus a set of *behaviors* the host
//! runs on its own clock and maps into values the scene binds to. Nothing a
//! plugin ships is executed. That is the decision the rest of the design rests
//! on, and it is recorded in ADR-0078 together with what it costs and what a
//! later `api_version` is expected to add.
//!
//! Being data has consequences that are deliberate rather than incidental:
//!
//! * A plugin cannot read a file, open a socket or run a process, because it has
//!   no way to ask for one. There is no permission system to get wrong, and no
//!   native code to keep the `unsafe` boundary of ADR-0005 out of.
//! * Everything a plugin can do is bounded by a constant declared right here, so
//!   the host can enforce it before the plugin is loaded rather than after it
//!   has drawn something.
//! * The model window is rendered by the host, so a panel is consistent with the
//!   rest of the product and follows the user's theme and scale without the
//!   plugin knowing they exist.
//!
//! # Versions
//!
//! Four version numbers, all distinct, and all of them strict:
//!
//! * `schema_version` — the shape of `plugin.json`. Like `config.json` it is a
//!   single accepted version with no migration, so a manifest the host does not
//!   recognise is refused rather than half-read.
//! * `api_version` — the feature level a plugin needs. A plugin declaring a higher
//!   one than the host implements is not loaded, rather than loaded with the
//!   features it asked for quietly missing.
//! * `version` — the plugin's own release, ordered for the plugin center's update
//!   comparison.
//! * the catalog's own `schema_version` — the shape of `plugins.json`.

#![forbid(unsafe_code)]

mod behavior;
mod catalog;
mod color;
mod error;
mod manifest;
pub mod scene;

pub use behavior::{
    BehaviorAction, BehaviorId, BehaviorSpec, CountdownSpec, CounterSpec, LocalTimeSpec,
    MAXIMUM_BEHAVIORS_PER_PLUGIN, MAXIMUM_TIMER_SECONDS, StopwatchSpec,
};
pub use catalog::{
    MAXIMUM_CATALOG_ENTRIES, PLUGIN_CATALOG_FILE_NAME, PLUGIN_CATALOG_REPOSITORY_NAME,
    PLUGIN_CATALOG_REPOSITORY_OWNER, PLUGIN_CATALOG_SCHEMA_VERSION, PLUGIN_SCHEMA_VERSION,
    PluginCatalog, PluginCatalogEntry, PluginDownload, SUPPORTED_PLUGIN_API_VERSION,
};
pub use color::{Color, ColorError};
pub use error::{PluginError, PluginErrorCode};
pub use manifest::{
    InstalledPlugin, MAXIMUM_BEHAVIOR_ID_BYTES, MAXIMUM_BINDINGS_PER_PLUGIN,
    MAXIMUM_BUTTON_ID_BYTES, MAXIMUM_PLUGIN_DESCRIPTION_CHARS, MAXIMUM_PLUGIN_ID_BYTES,
    MAXIMUM_PLUGIN_NAME_CHARS, NamedBehavior, OverlayContribution, PLUGIN_MANIFEST_FILE_NAME,
    PluginAnchor, PluginCapabilities, PluginId, PluginManifest, PluginVersion,
};
pub use scene::value::{BindingTable, BindingValue, ResolvedValue, SceneValue};
pub use scene::{
    Align, ButtonNode, ButtonVariant, DividerNode, ImageNode, MAXIMUM_SCENE_DEPTH,
    MAXIMUM_SCENE_NODES, ProgressBarNode, ProgressRingNode, SceneNode, SpacerNode, StackAxis,
    StackNode, TextNode, TextWeight,
};

/// The longest binding path a scene may name, in bytes.
///
/// A binding is `<behavior id>.<field>`, and the source is either a behavior the
/// manifest declared or the reserved word `host`. The bound exists so a scene
/// cannot name a path long enough to be worth interning at every redraw, and so a
/// malformed scene is refused rather than resolving to a lookup that misses every
/// time.
pub const MAXIMUM_BINDING_PATH_BYTES: usize = 96;

/// The binding paths the host itself provides.
///
/// The list is closed and lives here rather than in the engine, so adding a host
/// fact is a change to the product's surface and has to be made in one place where
/// a reviewer will see it. A behavior id may not contain a `.`, so no behavior can
/// collide with the `host` source.
pub const HOST_BINDING_PATHS: &[&str] = &[
    "host.overlay_visible",
    "host.model_name",
    "host.pressed_key_count",
];

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
/// One check for every path a plugin can name — its icon, its images — because
/// the ways out of a plugin's own directory are worth enumerating once: `..`, an
/// absolute path, a Windows drive letter, a UNC prefix, an empty component, and a
/// NUL byte that would truncate the name on the way to the filesystem.
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

/// Check a behavior's own values.
///
/// Only a clock format is refused here. Everything else a behavior can declare
/// out of range is *clamped* by [`clamp_behavior_spec`] rather than refused,
/// because a panel with a zero-second duration is still the panel its author
/// meant to write and the one-line mistake is cheaper to survive than to report.
/// A clock format is different: there is no sensible default for a format that
/// is not a subset of `HH`/`MM`/`SS`, and guessing one would show a time nobody
/// asked for.
pub fn validate_behavior_spec(spec: &BehaviorSpec) -> Result<(), PluginError> {
    if let BehaviorSpec::LocalTime(clock) = spec {
        validate_time_format(&clock.format)?;
    }
    Ok(())
}

/// Whether `format` is a run of `HH`, `MM` and `SS` separated by non-alphabetic
/// characters.
///
/// A closed subset on purpose. The alternative — handing the format to a general
/// date library — would let a plugin ask for a month name in a locale it never
/// named, and the host would have to pick one.
pub fn validate_time_format(format: &str) -> Result<(), PluginError> {
    if format.is_empty() || format.len() > 32 {
        return Err(PluginError::new(PluginErrorCode::InvalidTimeFormat));
    }
    let bytes = format.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(b"HH") || rest.starts_with(b"MM") || rest.starts_with(b"SS") {
            index += 2;
        } else if rest[0].is_ascii_alphanumeric() {
            return Err(PluginError::new(PluginErrorCode::InvalidTimeFormat));
        } else {
            index += 1;
        }
    }
    Ok(())
}

/// Bring a behavior's own values into range.
///
/// Called once, when a plugin is loaded, so every consumer of a behavior can
/// assume its values are in range and none of them has to clamp.
pub fn clamp_behavior_spec(spec: &mut BehaviorSpec) {
    use BehaviorSpec::{Countdown, Counter, LocalTime, Stopwatch};
    match spec {
        Countdown(countdown) => {
            countdown.duration_seconds = countdown.duration_seconds.clamp(1, MAXIMUM_TIMER_SECONDS);
        }
        Stopwatch(stopwatch) => {
            stopwatch.period_seconds = stopwatch
                .period_seconds
                .map(|period| period.clamp(1, MAXIMUM_TIMER_SECONDS));
        }
        LocalTime(_) => {}
        Counter(counter) => {
            if counter.step == 0 {
                counter.step = 1;
            }
            if counter.minimum > counter.maximum {
                std::mem::swap(&mut counter.minimum, &mut counter.maximum);
            }
            counter.initial = counter.initial.clamp(counter.minimum, counter.maximum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_relative_path_is_accepted() {
        for path in ["icon.png", "assets/icon.png", "a/b/c.png"] {
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
    fn a_counter_with_no_step_or_an_inverted_range_is_brought_into_range() {
        let mut spec = BehaviorSpec::Counter(CounterSpec {
            initial: 50,
            minimum: 10,
            maximum: 0,
            step: 0,
            loop_back: false,
        });
        clamp_behavior_spec(&mut spec);
        let BehaviorSpec::Counter(counter) = spec else {
            panic!("expected a counter");
        };
        assert_eq!(counter.step, 1);
        assert_eq!(counter.minimum, 0);
        assert_eq!(counter.maximum, 10);
        assert_eq!(counter.initial, 10);
    }

    #[test]
    fn a_zero_duration_countdown_is_brought_into_range() {
        let mut spec = BehaviorSpec::Countdown(CountdownSpec {
            duration_seconds: 0,
            auto_start: false,
            auto_repeat: false,
        });
        clamp_behavior_spec(&mut spec);
        let BehaviorSpec::Countdown(countdown) = spec else {
            panic!("expected a countdown");
        };
        assert_eq!(countdown.duration_seconds, 1);
    }

    #[test]
    fn an_absurd_period_is_brought_into_range() {
        let mut spec = BehaviorSpec::Stopwatch(StopwatchSpec {
            auto_start: false,
            period_seconds: Some(u32::MAX),
        });
        clamp_behavior_spec(&mut spec);
        let BehaviorSpec::Stopwatch(stopwatch) = spec else {
            panic!("expected a stopwatch");
        };
        assert_eq!(stopwatch.period_seconds, Some(MAXIMUM_TIMER_SECONDS));
    }
}
