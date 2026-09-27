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
    pub show_taskbar_icon: bool,
    pub show_status_icon: bool,
}
