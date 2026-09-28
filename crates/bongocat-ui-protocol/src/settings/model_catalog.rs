//! The catalogue the picker shows, ready or not.
//!
//! One entry per model whether it loaded or not: a model that failed to load
//! still occupies a slot the user chose, and hiding it would make the failure
//! look like a model that was never installed.

use super::*;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettingsModelKey {
    pub id: String,
    pub origin: SettingsModelOrigin,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsModelCatalog {
    pub entries: Vec<SettingsModelEntry>,
    pub error: Option<SettingsModelCatalogError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelEntry {
    pub id: String,
    /// User-facing display name. The app layer falls back to the stable id
    /// when no editable title metadata exists (presets and legacy records).
    pub title: String,
    /// The mode resolved when this model was imported, or derived from the stable
    /// id for a build-shipped preset. It is absent only for an invalid hand-copied
    /// store entry; the page never infers it from the title.
    pub input_mode: Option<SettingsModelMode>,
    pub origin: SettingsModelOrigin,
    pub availability: SettingsModelAvailability,
    /// The package directory the model's files live in, when it is present.
    /// The settings page opens it, and derives nothing else from it: every
    /// other model fact already has its own field.
    pub directory: Option<PathBuf>,
    /// The cover image the package ships, if it ships one. A package without a
    /// cover is an ordinary package, so the page renders a placeholder instead
    /// of treating the entry as degraded.
    pub cover: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelOrigin {
    BuiltIn,
    Imported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsModelAvailability {
    Ready {
        behaviors: Vec<SettingsModelBehavior>,
    },
    Invalid {
        diagnostic: SettingsModelDiagnostic,
    },
}

/// A behavior declared by a validated model package. Settings uses this
/// strongly typed identity for preview and shortcut operations.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelBehavior {
    Motion { group: String, index: usize },
    Expression { name: String },
}
