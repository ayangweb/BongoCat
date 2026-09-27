//! Where an update may come from, and how often it checks.
//!
//! An update channel is configuration rather than a constant because a
//! development build and a shipped one answer differently, and the difference has
//! to be visible in the document a user can read.

use super::*;

/// Automatic update policy. The interval remains persisted when the switch is
/// off, just like the other preference pairs in the configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UpdateConfig {
    pub check_automatically: bool,
    /// Whole hours to wait after an automatic update check before checking again.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 8760))
    )]
    pub check_interval_hours: u16,
}
