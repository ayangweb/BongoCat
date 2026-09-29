//! What the plugin centre shows, and what it can ask for.
//!
//! The window never sees a plugin manifest, a file path, a digest or a network
//! answer. It sees a row, a state, and — when something is wrong — a code it can
//! turn into a sentence in the user's language. Everything below that line belongs
//! to the product and to the plugin host.
//!
//! Two of the fields are load-bearing and easy to miss:
//!
//! * [`SettingsPlugins::available`] is false when the worker would not start. Without
//!   it the page would render an empty list, which reads as "there are no plugins"
//!   rather than "this build cannot run them".
//! * [`SettingsPluginEntry::refusal`] travels with the row rather than replacing it,
//!   so a plugin the catalog offers for a platform this host is not still appears —
//!   greyed, with a reason — instead of silently not existing.

/// One plugin, as the centre lists it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsPluginEntry {
    /// The plugin's own identifier, and the only name the window needs to address it.
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    /// The version on disk, or `None` when the catalog only offers this plugin.
    pub installed_version: Option<String>,
    /// The version the catalog offers for this host, when it offers one.
    pub available_version: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    /// Whether the installed version is older than the one on offer.
    pub update_available: bool,
    /// Why this plugin cannot be installed here, when it cannot.
    pub refusal: Option<SettingsPluginRefusal>,
}

/// The whole plugin centre, as one read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
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
}

impl SettingsPlugins {
    /// Whether the host has nothing to show and nothing to say.
    ///
    /// An empty list with no error and no available host is the one state that means
    /// "the catalog has not been read yet", and it is rendered as the loading state
    /// rather than as an empty catalog.
    pub fn is_pending(&self) -> bool {
        self.available && !self.busy && self.entries.is_empty() && self.last_error.is_none()
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
    /// What the failure was about, when there was a subject. Never a path and never a
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
/// Grouped rather than restated one-for-one: two lists of the same thirty-five
/// failures would drift, and the one that drifted would be the one the window could
/// not show a sentence for. The grouping is the contract — a code added to the host
/// without a home here is a message that never appears.
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
    /// A manifest, scene, binding or asset inside the plugin is not valid.
    InvalidManifest,
    /// The panel could not be laid out or rasterized.
    RenderFailed,
    /// Anything the host reported that this list has not grown into.
    ///
    /// Not an omission to be tidied away: a host code added without a sentence here
    /// must still produce a message rather than an unreachable page.
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_read_is_distinguishable_from_an_empty_catalog() {
        let mut plugins = SettingsPlugins {
            available: true,
            ..SettingsPlugins::default()
        };
        assert!(plugins.is_pending(), "a read with nothing in it is pending");

        plugins.entries.push(SettingsPluginEntry {
            id: "pomodoro".to_string(),
            ..SettingsPluginEntry::default()
        });
        assert!(
            !plugins.is_pending(),
            "a catalog with a plugin in it is not pending"
        );
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
        let plugins = SettingsPlugins {
            available: true,
            last_error: Some(SettingsPluginError::new(
                SettingsPluginErrorCode::CatalogUnavailable,
            )),
            ..SettingsPlugins::default()
        };
        assert!(!plugins.is_pending(), "a failure is shown, not waited on");
    }
}
