//! What one frame of a model contains.
//!
//! This is the boundary the renderer reads. Nothing above it holds a Cubism
//! pointer, which is what lets a snapshot be compared, asserted on, and sent across
//! a channel without a GPU in the picture. The dynamic flags say which parts of a
//! drawable changed, so a renderer can skip re-uploading a part that did not.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    Normal,
    Additive,
    Multiplicative,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawableSnapshot {
    pub id: DrawableId,
    pub dynamic_flags: DrawableDynamicFlags,
    pub render_order: i32,
    pub visible: bool,
    pub texture_id: TextureId,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub double_sided: bool,
    pub inverted_mask: bool,
    pub multiply_color: [f32; 4],
    pub screen_color: [f32; 4],
    pub masks: Vec<DrawableId>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

/// Per-frame changes reported by Cubism for one drawable.
///
/// Renderers use these flags to avoid rewriting GPU buffers whose source data
/// did not change during the current Core update.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DrawableDynamicFlags {
    pub visibility_changed: bool,
    pub opacity_changed: bool,
    pub draw_order_changed: bool,
    pub render_order_changed: bool,
    pub vertex_positions_changed: bool,
    pub blend_color_changed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSnapshot {
    pub canvas: CanvasInfo,
    pub bounds: ModelBounds,
    pub active_keys: Vec<KeyOverlay>,
    pub model_opacity: f32,
    pub mirror_horizontal: bool,
    pub drawables: Vec<DrawableSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyAssetId(pub(crate) usize);

impl KeyAssetId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyOverlay {
    pub asset_id: KeyAssetId,
    pub side: KeySide,
}
