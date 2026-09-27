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
    /// Whether the Windows settings window keeps its taskbar button. Windows
    /// only: macOS never reads this field as a Dock icon.
    pub show_taskbar_icon: bool,
    /// Whether the macOS process shows a Dock icon. This is the process
    /// activation policy, not a window style, so it is macOS only and it is the
    /// one field of the group whose default is `false`: the product launches as
    /// a menu bar accessory app, exactly as it did before the field existed.
    pub show_dock_icon: bool,
    pub show_status_icon: bool,
}
