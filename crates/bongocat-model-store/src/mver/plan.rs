//! What a conversion decided to do, before it writes anything.
//!
//! A plan is the answer to "is this folder a legacy source, and what would each
//! of its modes become". Holding it separately from the writer is what makes
//! detection speculative in the right way: a folder that plans nothing is not a
//! legacy source, and a mode with a missing layer still plans the bindings it
//! can draw.

use super::*;

/// How one output key image is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MverSlotImage {
    /// The legacy source already draws a composed image, so its bytes are
    /// installed as they are and are never re-encoded.
    Verbatim(String),
    /// The paw and the key cap are separate layers and are composed with the
    /// paw on top, matching how the legacy application draws them.
    Composite { hand: String, keyboard: String },
}

/// One key image a mode contributes, with its destination inside `resources`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MverSlot {
    pub(crate) reference: String,
    pub(crate) image: MverSlotImage,
}

/// Everything one legacy mode converts into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MverModePlan {
    pub(crate) mode: MverInputMode,
    /// Package-relative directory holding the mode's legacy resources.
    pub(crate) root: String,
    /// Package-relative directory holding the mode's Live2D package.
    pub(crate) model: String,
    pub(crate) background: Option<String>,
    pub(crate) cover: Option<String>,
    pub(crate) slots: Vec<MverSlot>,
}

/// The modes a legacy source carries, in [`MverInputMode::ALL`] order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MverPlan {
    pub(crate) modes: Vec<MverModePlan>,
}

impl MverPlan {
    pub(crate) fn modes(&self) -> impl Iterator<Item = MverInputMode> + '_ {
        self.modes.iter().map(|plan| plan.mode)
    }

    pub(crate) fn mode(&self, mode: MverInputMode) -> Option<&MverModePlan> {
        self.modes.iter().find(|plan| plan.mode == mode)
    }
}

pub(crate) fn indexed_image_reference(root: &str, directory: &str, index: usize) -> String {
    join_reference(&join_reference(root, directory), &format!("{index}.png"))
}

pub(crate) fn join_reference(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Whether `reference` is a direct child of the `directory` reference.
pub(crate) fn is_direct_child(reference: &str, directory: &str) -> bool {
    let Some(rest) = reference.strip_prefix(directory) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('/') else {
        return false;
    };
    !rest.is_empty() && !rest.contains('/')
}
