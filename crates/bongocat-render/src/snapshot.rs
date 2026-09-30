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
    /// The multiplayer chat bubble floating above the model, when one is
    /// showing. The runtime owns the message and its lifetime; the renderer
    /// only blits the rasterized texture.
    pub chat_bubble: Option<ChatBubbleSnapshot>,
}

/// One rasterized chat bubble, ready to upload as a texture.
///
/// The pixels are straight (non-premultiplied) RGBA8, top row first: both
/// overlay shaders multiply the color by alpha themselves, so the texture must
/// not arrive premultiplied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatBubbleTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where and how strongly the bubble shows in this frame.
///
/// `anchor` is the bubble's bottom-center in canvas space (the same space the
/// snapshot's drawables use), and `size` is the bubble rectangle in canvas
/// units, so the renderer can build the quad with the transform it already
/// applies to the model.
#[derive(Clone, Debug)]
pub struct ChatBubbleSnapshot {
    pub texture: Arc<ChatBubbleTexture>,
    pub anchor: [f32; 2],
    pub size: [f32; 2],
    pub opacity: f32,
}

impl PartialEq for ChatBubbleSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor
            && self.size == other.size
            && self.opacity == other.opacity
            && (Arc::ptr_eq(&self.texture, &other.texture)
                || self.texture.width == other.texture.width
                    && self.texture.height == other.texture.height
                    && self.texture.rgba == other.texture.rgba)
    }
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
