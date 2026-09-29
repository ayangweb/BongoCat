//! The topmost layers the model window draws, in one place per backend.
//!
//! Everything about a layer — what it is, where it sits, which layer a click
//! landed on — is decided in `bongocat-render` and platform-neutral. What is left
//! here is the only part that has to differ between Metal and D3D11: holding a
//! texture, keeping it in step with a raster whose content hash has changed, and
//! uploading it.
//!
//! The hash is what makes this cheap. A panel is re-rasterized whenever a value it
//! binds to changes, which for a one-second countdown is once a second; without the
//! hash that would be a texture upload a second per panel, and for a stopwatch it
//! would be sixty. With it, a redraw that produced identical pixels costs a
//! comparison.
//!
//! No backend holds more than one texture per layer id. A layer that disappears
//! releases its texture immediately, and a layer whose id comes back is a new layer
//! even if the id repeats — which is why ids are never reused by a producer.

use bongocat_render::{OverlayLayer, OverlayLayerPointer, PlacedOverlayLayer};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[cfg(test)]
mod tests;

/// Place a layer set, dropping what cannot be drawn.
///
/// A layer with a malformed raster or a degenerate aspect is dropped here rather
/// than handed to a backend, so both backends see the same list and neither has to
/// make the same decision twice. No drawable size is needed: placement is in
/// normalized device coordinates, so the same list is correct for whatever the
/// drawable is currently sized.
pub fn place(layers: &[OverlayLayer]) -> Vec<PlacedOverlayLayer> {
    layers
        .iter()
        .filter_map(|layer| PlacedOverlayLayer::new(layer.clone()))
        .collect()
}

/// The press a click at a point in drawable device pixels belongs to, if any.
///
/// The point is converted to normalized device coordinates here rather than in each
/// backend, so the two agree on which layer wins when they overlap — later in the
/// list, because that is the order they were drawn in. The result is a position in
/// the *layer's* raster, because that is the coordinate space a layer describes its
/// buttons in and knows nothing else about.
pub fn press_at(
    placed: &[PlacedOverlayLayer],
    drawable_width: u32,
    drawable_height: u32,
    x: f32,
    y: f32,
) -> Option<OverlayLayerPointer> {
    if drawable_width == 0 || drawable_height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    // Device pixels to NDC, with y up from the center. The drawable's origin is
    // its top left, which is the opposite of NDC's y, so y is reflected.
    let ndc_x = (x / drawable_width as f32) * 2.0 - 1.0;
    let ndc_y = 1.0 - (y / drawable_height as f32) * 2.0;
    PlacedOverlayLayer::hit_test(placed, ndc_x, ndc_y)
        .and_then(|layer| layer.to_layer_pixels(ndc_x, ndc_y))
}

/// The placement the native window hit-tests a click against.
///
/// Published by the frame loop once a tick and read by the window procedure (or the
/// AppKit event monitor), which is the only place that can decide what a click
/// *means*: on Windows a left click over a panel has to be answered `HTCLIENT`
/// rather than `HTCAPTION`, and that decision is made in `WM_NCHITTEST`, long
/// before the frame loop would see a message.
///
/// Copy-on-write behind a mutex, and the mutex is deliberate: its critical section
/// is one `Arc` clone, so the worst a preempted writer can cost the UI thread is
/// the length of one atomic increment. The alternative — no lock at all — would
/// need the placement to be single-threaded, and the window procedure is reached
/// through a raw pointer that no compiler can tie to a thread.
#[derive(Clone, Debug, Default)]
pub(crate) struct PlacedLayers(Arc<Mutex<Arc<[PlacedOverlayLayer]>>>);

impl PlacedLayers {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(Arc::from([]))))
    }

    /// Replace the placement, once per tick, after the textures are prepared.
    pub(crate) fn publish(&self, placed: Vec<PlacedOverlayLayer>) {
        let mut current = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = Arc::from(placed);
    }

    /// The press a click at a point in drawable device pixels belongs to, if any.
    pub(crate) fn press(
        &self,
        drawable_width: u32,
        drawable_height: u32,
        x: f32,
        y: f32,
    ) -> Option<OverlayLayerPointer> {
        let current = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        press_at(&current, drawable_width, drawable_height, x, y)
    }
}

/// A per-layer record of what was uploaded, so an unchanged raster costs nothing.
#[derive(Debug)]
pub(crate) struct LayerTextures<T> {
    entries: BTreeMap<u64, Entry<T>>,
}

#[derive(Debug)]
struct Entry<T> {
    texture: T,
    /// The content hash the texture was uploaded from.
    content: u64,
    /// The raster's size, so a panel that changed size is re-created rather than
    /// stretched.
    width: u32,
    height: u32,
}

impl<T> LayerTextures<T> {
    pub(crate) fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// The texture for a layer, whatever its content.
    pub(crate) fn get(&self, id: u64) -> Option<&T> {
        self.entries.get(&id).map(|entry| &entry.texture)
    }

    /// Release one layer's texture.
    ///
    /// Dropped before a replacement is created rather than after: a size change
    /// means the old texture cannot be reused, and holding both at once is a
    /// texture's worth of memory for no frame.
    pub(crate) fn remove(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    /// Whether a layer's texture is already what its raster needs.
    ///
    /// `false` for a size change as well as for a content change, because a
    /// texture cannot be resized and a stretched one is a visible defect rather
    /// than a stale one.
    pub(crate) fn is_current(&self, layer: &OverlayLayer) -> bool {
        self.entries.get(&layer.id).is_some_and(|entry| {
            entry.content == layer.raster.content
                && entry.width == layer.raster.width
                && entry.height == layer.raster.height
        })
    }

    /// Record a texture as the current one for a layer.
    pub(crate) fn insert(&mut self, id: u64, texture: T, layer: &OverlayLayer) {
        self.entries.insert(
            id,
            Entry {
                texture,
                content: layer.raster.content,
                width: layer.raster.width,
                height: layer.raster.height,
            },
        );
    }

    /// Drop every texture whose layer is not in this frame.
    ///
    /// Called once per presented frame rather than once per publish, so a layer
    /// that arrives and is immediately superseded by a resize does not leak a
    /// texture. A layer that is absent from one frame is gone: the producer
    /// publishes the whole set every time, so absence means "not drawn".
    pub(crate) fn retain(&mut self, present: &[PlacedOverlayLayer]) {
        self.entries
            .retain(|id, _| present.iter().any(|layer| layer.layer.id == *id));
    }
}
