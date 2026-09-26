//! What one frame of a model is worth.
//!
//! The runtime reads behaviour from a snapshot and the renderer reads geometry
//! from it, and neither reaches back into the model. That is what lets the
//! runtime decide a frame without waiting for the renderer, and the renderer
//! draw a frame without owning state the runtime can change underneath it.

use super::*;

/// A model-declared behavior that is safe to expose to settings and shortcut
/// configuration. It contains an identifier only, never a package path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ModelBehaviorSnapshot {
    Motion { group: String, index: usize },
    Expression { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSnapshot {
    pub id: ModelId,
    pub entry: String,
    pub behaviors: Vec<ModelBehaviorSnapshot>,
}
