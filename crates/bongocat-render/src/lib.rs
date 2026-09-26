//! The render vocabulary: what a frame is, what it draws from, and how one gets
//! from the runtime to the overlay.
//!
//! This crate is deliberately platform-neutral. It owns the shapes, the identities
//! and the validation; D3D11 and Metal own the drawing. That is what lets a
//! snapshot be asserted on in an ordinary test and a validation rule be proved
//! once rather than per backend.

#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub use bongocat_input::{GLOBE_KEY_USAGE, GamepadButton};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DrawableId(usize);

impl DrawableId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

mod channel;
mod commit;
mod geometry;
mod identity;
mod key;
mod resources;
mod snapshot;
#[cfg(test)]
mod tests;
mod transport;
mod validate;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use channel::{RenderConsumer, RenderProducer, latest_render_channel};
pub use commit::{
    ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome, ModelCommitToken, RenderFrame,
};
pub use geometry::{CanvasInfo, ModelBounds, Vertex};
pub use identity::TextureId;
pub use key::{
    FUNCTION_KEY_NAMES, FUNCTION_KEY_USAGES, KeyIdentity, KeyPress, KeyPressSet, KeySide,
    function_key_index, function_key_name,
};
pub use resources::{BackgroundAsset, KeyAsset, RenderResources, TextureAsset};
pub use snapshot::{
    BlendMode, DrawableDynamicFlags, DrawableSnapshot, KeyAssetId, KeyOverlay, RenderSnapshot,
};
pub use transport::{ModelCommitFeedbackError, RenderPublishError, RenderTransportDiagnostics};
pub use validate::{RenderSnapshotValidationError, validate_render_snapshot};
