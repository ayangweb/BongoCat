//! Desktop integration preferences.
//!
//! These are system-owned surfaces rather than model or overlay state, so they
//! have their own namespace: a reader looking for "does the app start with the
//! window" should not have to read the overlay's placement rules to find out.

use super::*;

/// Desktop integration preferences. These are system-owned surfaces rather
/// than model or overlay state, so they have their own namespace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    /// Whether the Windows model window keeps its taskbar button, default
    /// `false`. Windows only: macOS never reads this field as a Dock icon, and
    /// the settings window keeps its own taskbar button either way.
    pub show_taskbar_icon: bool,
    /// Whether the macOS Dock icon stays visible while only the cat overlay is
    /// running, default `false`. Settings and update windows always show the Dock
    /// icon, including when minimized. This preference does not close windows.
    pub show_dock_icon: bool,
    pub show_status_icon: bool,
}
