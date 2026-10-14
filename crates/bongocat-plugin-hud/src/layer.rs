//! Composing every running plugin's panel into one layer.
//!
//! The model window draws plugin UI as a single textured quad, so several
//! plugins' panels have to become one surface before they reach a backend. This
//! module does that composition once, on the platform-neutral side: it rasterizes
//! each frame, blits it into a shared bitmap, and collects the interactive
//! rectangles in the device-pixel space the native window reports pointer
//! positions in.
//!
//! That is also what keeps a plugin's panel from costing a texture, a draw call
//! and a code path per plugin on both backends: N plugins are one layer, one
//! upload and one quad.

use crate::{HudBitmap, HudFont, HudRect, HudRenderer, HudRendererError};
use bongocat_plugin_sdk::PluginView;
use std::sync::{Arc, Mutex};

/// One plugin's panel, named so the host can route its interactions back.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginHudFrame {
    /// The plugin id the frame belongs to. This is the only thing the host needs
    /// to know about a frame's origin, so composition stays plugin-agnostic.
    pub plugin: String,
    pub view: PluginView,
}

impl PluginHudFrame {
    pub fn new(plugin: impl Into<String>, view: PluginView) -> Self {
        Self {
            plugin: plugin.into(),
            view,
        }
    }
}

/// The model window's size and backing scale, as the panel layer needs it.
///
/// `width`/`height` are device pixels, because that is the space a native window
/// reports pointer positions in and the space the layer's bitmap is rasterized
/// in; `scale` is device pixels per logical pixel, which is what keeps a panel's
/// text the same physical size on a 200% display as on a 100% one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudViewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl HudViewport {
    pub const fn new(width: u32, height: u32, scale: f32) -> Self {
        Self {
            width,
            height,
            scale,
        }
    }

    /// The placeholder a channel starts with, and what a session that has no
    /// window yet reports.
    pub const fn unusable() -> Self {
        Self {
            width: 0,
            height: 0,
            scale: 1.0,
        }
    }

    /// Whether this viewport can carry a layer at all.
    pub fn is_usable(&self) -> bool {
        self.width > 0 && self.height > 0 && self.scale.is_finite() && self.scale > 0.0
    }

    fn logical_width(&self) -> u32 {
        ((self.width as f32) / self.scale).round().max(1.0) as u32
    }

    fn logical_height(&self) -> u32 {
        ((self.height as f32) / self.scale).round().max(1.0) as u32
    }
}

/// One interactive widget a plugin drew, in device pixels inside the window.
#[derive(Clone, Debug, PartialEq)]
pub struct HudTarget {
    pub plugin: String,
    pub id: String,
    pub rect: HudRect,
}

/// What the user did to one of a plugin's widgets.
///
/// The host reports which control fired and never what it means: the plugin
/// decides that when it next handles the action.
#[derive(Clone, Debug, PartialEq)]
pub struct HudInteraction {
    pub plugin: String,
    pub id: String,
}

/// The composed panel layer for one window size.
#[derive(Clone, Debug, PartialEq)]
pub struct HudLayer {
    pub width: u32,
    pub height: u32,
    /// Row-major premultiplied RGBA8, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    /// The layer's top-left corner inside the window, in device pixels.
    pub origin_x: f32,
    pub origin_y: f32,
    /// The window size the layer was composed for. A backend that sees a
    /// different size drops the layer rather than stretching it; the next
    /// composed frame matches.
    pub window_width: u32,
    pub window_height: u32,
    /// Every interactive widget, in draw order, in the window's device pixels.
    ///
    /// The rectangles are window-relative, not bitmap-relative, because that is
    /// the space a native window reports a pointer position in on both
    /// backends. A control the compositor drew into the shared bitmap at
    /// `(origin_x, origin_y)` is hit-tested at that same window coordinate, not
    /// at the coordinate it occupies inside the bitmap.
    pub targets: Vec<HudTarget>,
}

impl HudLayer {
    /// A layer with nothing to draw.
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            origin_x: 0.0,
            origin_y: 0.0,
            window_width: 0,
            window_height: 0,
            targets: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }

    /// The interactive widget at a device-pixel point, if any.
    ///
    /// Later widgets win, because they are drawn on top: a point inside two
    /// overlapping buttons activates the one the user can see. The point is in
    /// the window's device pixels, the same space [`Self::targets`] is in.
    pub fn target_at(&self, x: f32, y: f32) -> Option<&HudTarget> {
        self.targets
            .iter()
            .rev()
            .find(|target| target.rect.contains(x, y))
    }

    /// The four corners of the layer's quad, in the window's device pixels.
    ///
    /// The winding and the index buffer are the ones the model quads already
    /// use, so both backends can draw this with a pipeline of their own. The uv
    /// pairs are chosen so the layer's first row of pixels lands on the top edge
    /// of the quad after the shared vertex shader flips v.
    pub fn quad_vertices(&self) -> [HudVertex; 4] {
        let x0 = self.origin_x;
        let y0 = self.origin_y;
        let x1 = x0 + self.width as f32;
        let y1 = y0 + self.height as f32;
        [
            HudVertex {
                position: [x0, y0],
                uv: [0.0, 1.0],
            },
            HudVertex {
                position: [x1, y0],
                uv: [1.0, 1.0],
            },
            HudVertex {
                position: [x1, y1],
                uv: [1.0, 0.0],
            },
            HudVertex {
                position: [x0, y1],
                uv: [0.0, 0.0],
            },
        ]
    }

    /// The vertex transform that maps the layer's device pixels to clip space.
    ///
    /// The window's pixel space has its origin at the top left and its y axis
    /// pointing down; clip space has its origin at the centre with y up. This is
    /// the same relation the model quads use, written for a pixel-space quad
    /// rather than a model-space one.
    pub fn clip_transform(&self) -> [f32; 4] {
        let width = self.window_width.max(1) as f32;
        let height = self.window_height.max(1) as f32;
        [2.0 / width, -2.0 / height, -1.0, 1.0]
    }
}

/// One corner of a panel layer's quad, in the window's device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct HudVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

/// Turns a set of plugin frames into one layer.
pub struct HudCompositor {
    renderer: HudRenderer,
}

impl HudCompositor {
    pub fn new(font: HudFont) -> Self {
        Self {
            renderer: HudRenderer::new(font),
        }
    }

    pub fn font(&self) -> &HudFont {
        self.renderer.font()
    }

    /// Rasterize every frame and compose them into one layer.
    ///
    /// A frame that cannot be rasterized is skipped rather than failing the
    /// whole layer: one plugin's oversized panel must not stop another plugin's
    /// from being drawn, which is the same isolation the runtime gives a plugin
    /// that traps.
    pub fn compose(
        &self,
        viewport: HudViewport,
        frames: &[PluginHudFrame],
    ) -> Result<HudLayer, HudRendererError> {
        if !viewport.is_usable() {
            return Ok(HudLayer::empty());
        }
        let mut panels = Vec::with_capacity(frames.len());
        for frame in frames {
            let Ok(bitmap) = self.renderer.rasterize(
                &frame.view,
                viewport.logical_width(),
                viewport.logical_height(),
                viewport.scale,
            ) else {
                continue;
            };
            if bitmap.is_empty() {
                continue;
            }
            let logical_viewport = HudRect::new(
                0.0,
                0.0,
                viewport.logical_width() as f32,
                viewport.logical_height() as f32,
            );
            let layout = crate::layout(&frame.view, logical_viewport, self.renderer.font());
            let targets = layout
                .interactive
                .into_iter()
                .map(|node| HudTarget {
                    plugin: frame.plugin.clone(),
                    id: node.id,
                    rect: scale_rect(node.rect, viewport.scale),
                })
                .collect::<Vec<_>>();
            panels.push((bitmap, targets));
        }
        if panels.is_empty() {
            return Ok(HudLayer::empty());
        }
        let (origin_x, origin_y, width, height) = bounding_box(&panels, viewport);
        if width == 0 || height == 0 {
            return Ok(HudLayer::empty());
        }
        let mut canvas = LayerCanvas::new(width, height);
        let mut targets = Vec::new();
        for (bitmap, panel_targets) in &panels {
            let x = (bitmap.origin.x * viewport.scale - origin_x)
                .round()
                .max(0.0) as u32;
            let y = (bitmap.origin.y * viewport.scale - origin_y)
                .round()
                .max(0.0) as u32;
            canvas.blit(x, y, bitmap);
            for target in panel_targets {
                // Targets stay in the window's device pixels rather than being
                // shifted into the composed bitmap: a native host hit-tests the
                // pointer position it reports, which is a window coordinate, and
                // a layer-relative rectangle would put every control
                // `origin` away from where it is actually drawn.
                targets.push(HudTarget {
                    plugin: target.plugin.clone(),
                    id: target.id.clone(),
                    rect: target.rect,
                });
            }
        }
        Ok(HudLayer {
            width,
            height,
            pixels: canvas.into_pixels(),
            origin_x,
            origin_y,
            window_width: viewport.width,
            window_height: viewport.height,
            targets,
        })
    }
}

/// The device-pixel box every panel fits in, clamped to the window.
fn bounding_box(
    panels: &[(HudBitmap, Vec<HudTarget>)],
    viewport: HudViewport,
) -> (f32, f32, u32, u32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x: f32 = 0.0;
    let mut max_y: f32 = 0.0;
    for (bitmap, _) in panels {
        let left = (bitmap.origin.x * viewport.scale).floor().max(0.0);
        let top = (bitmap.origin.y * viewport.scale).floor().max(0.0);
        let right = (left + bitmap.width as f32).min(viewport.width as f32);
        let bottom = (top + bitmap.height as f32).min(viewport.height as f32);
        if right <= left || bottom <= top {
            continue;
        }
        min_x = min_x.min(left);
        min_y = min_y.min(top);
        max_x = max_x.max(right);
        max_y = max_y.max(bottom);
    }
    if !min_x.is_finite() || !min_y.is_finite() || max_x <= min_x || max_y <= min_y {
        return (0.0, 0.0, 0, 0);
    }
    (
        min_x,
        min_y,
        (max_x - min_x).ceil().max(0.0) as u32,
        (max_y - min_y).ceil().max(0.0) as u32,
    )
}

fn scale_rect(rect: HudRect, scale: f32) -> HudRect {
    HudRect::new(
        rect.x * scale,
        rect.y * scale,
        rect.width * scale,
        rect.height * scale,
    )
}

/// A premultiplied RGBA8 surface the composed panels are blitted into.
struct LayerCanvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl LayerCanvas {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; width as usize * height as usize * 4],
        }
    }

    fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Blend one panel bitmap into the layer at `(x, y)`, clipped to the layer.
    fn blit(&mut self, x: u32, y: u32, bitmap: &HudBitmap) {
        for row in 0..bitmap.height {
            let target_y = y + row;
            if target_y >= self.height {
                break;
            }
            for column in 0..bitmap.width {
                let target_x = x + column;
                if target_x >= self.width {
                    break;
                }
                let source = (row as usize * bitmap.width as usize + column as usize) * 4;
                let destination = (target_y as usize * self.width as usize + target_x as usize) * 4;
                let source_alpha = f32::from(bitmap.pixels[source + 3]) / 255.0;
                if source_alpha <= 0.0 {
                    continue;
                }
                // Both sides are premultiplied, so source-over is
                // `dst = src + dst * (1 - src.a)` per channel.
                let inverse = 1.0 - source_alpha;
                for channel in 0..4 {
                    let value = f32::from(bitmap.pixels[source + channel])
                        + f32::from(self.pixels[destination + channel]) * inverse;
                    self.pixels[destination + channel] = value.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// The producer end of the latest-wins panel-layer channel.
pub struct HudProducer {
    slot: Arc<Mutex<HudSlot>>,
}

/// The consumer end of the latest-wins panel-layer channel.
#[derive(Clone)]
pub struct HudConsumer {
    slot: Arc<Mutex<HudSlot>>,
}

#[derive(Default)]
struct HudSlot {
    pending: Option<HudLayer>,
    published: u64,
    consumed: u64,
}

impl HudProducer {
    /// Publish the newest composed layer, replacing one the consumer has not
    /// taken yet: a panel that arrives late is a panel nobody looks at.
    pub fn publish(&self, layer: HudLayer) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.pending = Some(layer);
        slot.published = slot.published.saturating_add(1);
    }
}

impl HudConsumer {
    /// Take the newest layer, if one has arrived since the last call.
    pub fn take_latest(&self) -> Option<HudLayer> {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let layer = slot.pending.take();
        if layer.is_some() {
            slot.consumed = slot.consumed.saturating_add(1);
        }
        layer
    }
}

/// Create a latest-wins panel-layer channel.
pub fn latest_hud_channel() -> (HudProducer, HudConsumer) {
    let slot = Arc::new(Mutex::new(HudSlot::default()));
    (
        HudProducer {
            slot: Arc::clone(&slot),
        },
        HudConsumer { slot },
    )
}

/// The window half of the viewport channel.
///
/// A composed layer has to match the window it is drawn into, so the window's
/// size and backing scale are the window's facts to report rather than the
/// plugin service's to guess. Like the layer channel this is a latest value, not
/// a queue: a size the window has since left is not worth composing a frame for.
#[derive(Clone)]
pub struct HudViewportReporter {
    slot: Arc<Mutex<HudViewport>>,
}

impl HudViewportReporter {
    /// Report the window's current size and backing scale.
    pub fn publish(&self, viewport: HudViewport) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = viewport;
    }
}

/// The plugin-service half of the viewport channel.
#[derive(Clone)]
pub struct HudViewportReader {
    slot: Arc<Mutex<HudViewport>>,
}

impl HudViewportReader {
    /// The most recent window report, or a viewport that composes nothing.
    pub fn latest(&self) -> HudViewport {
        *self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A latest-value channel carrying the model window's size and backing scale.
pub fn latest_viewport_channel() -> (HudViewportReporter, HudViewportReader) {
    let slot = Arc::new(Mutex::new(HudViewport::unusable()));
    (
        HudViewportReporter {
            slot: Arc::clone(&slot),
        },
        HudViewportReader { slot },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::{Anchor, UiNode};

    fn view() -> PluginView {
        PluginView {
            anchor: Anchor::TopLeft,
            nodes: vec![
                UiNode::Text {
                    text: "05:00".into(),
                },
                UiNode::Button {
                    id: "toggle".into(),
                    label: "Start".into(),
                },
            ],
        }
    }

    fn compositor() -> HudCompositor {
        HudCompositor::new(crate::load_platform_font().expect("platform font"))
    }

    #[test]
    fn composing_one_panel_produces_a_layer_and_a_target() {
        let frames = [PluginHudFrame::new("dev.example.timer", view())];
        let layer = compositor()
            .compose(HudViewport::new(400, 300, 2.0), &frames)
            .unwrap();
        assert!(!layer.is_empty());
        assert_eq!(layer.window_width, 400);
        assert_eq!(layer.targets.len(), 1);
        let target = &layer.targets[0];
        assert_eq!(target.plugin, "dev.example.timer");
        assert_eq!(target.id, "toggle");
        assert!(
            layer
                .target_at(target.rect.x + 1.0, target.rect.y + 1.0)
                .is_some()
        );
    }

    /// A control is hit-tested where the window draws it.
    ///
    /// The composed bitmap is only the union of the panels' boxes, so its top
    /// left corner is not the window's origin: an anchored panel sits at
    /// `origin_x` inside the window. A target rectangle expressed relative to
    /// that corner is offset from the pointer position a native window reports,
    /// and every click on a panel that is not at the window's origin then lands
    /// on nothing.
    ///
    /// The expectation comes from the layout rather than from the layer, so the
    /// check is against where the panel is drawn and not against whatever the
    /// compositor happened to record.
    #[test]
    fn a_targets_rectangle_is_in_window_pixels_not_bitmap_pixels() {
        let frames = [PluginHudFrame::new("dev.example.timer", view())];
        let viewport = HudViewport::new(800, 600, 2.0);
        let layer = compositor().compose(viewport, &frames).unwrap();
        assert!(
            layer.origin_x > 1.0,
            "the layer is not at the window origin"
        );

        // Where the layout puts the button, in the window's device pixels.
        let drawn = {
            let logical = HudRect::new(
                0.0,
                0.0,
                viewport.logical_width() as f32,
                viewport.logical_height() as f32,
            );
            let font = crate::load_platform_font().expect("platform font");
            let mut interactive = Vec::new();
            crate::layout::collect_interactive(
                &frames[0].view,
                crate::layout(&frames[0].view, logical, &font).panel,
                &font,
                &mut interactive,
            );
            let button = interactive
                .into_iter()
                .find(|node| node.id == "toggle")
                .expect("the view publishes a button");
            scale_rect(button.rect, viewport.scale)
        };

        let target = layer
            .targets
            .iter()
            .find(|target| target.id == "toggle")
            .expect("the button is a target");
        assert_eq!(
            target.rect, drawn,
            "the recorded rectangle must be the one the panel was drawn at"
        );
        assert!(
            drawn.x >= layer.origin_x,
            "a window-relative rectangle starts at or after the layer's origin, \
             got {drawn:?} for an origin of {}",
            layer.origin_x
        );
    }

    #[test]
    fn several_panels_are_composed_into_one_layer() {
        let frames = [
            PluginHudFrame::new("dev.example.timer", view()),
            PluginHudFrame::new("dev.example.clock", view()),
        ];
        let layer = compositor()
            .compose(HudViewport::new(400, 300, 1.0), &frames)
            .unwrap();
        assert_eq!(layer.targets.len(), 2);
        assert!(layer.width > 0);
    }

    #[test]
    fn an_unusable_viewport_yields_an_empty_layer() {
        let frames = [PluginHudFrame::new("dev.example.timer", view())];
        let layer = compositor()
            .compose(HudViewport::new(0, 300, 1.0), &frames)
            .unwrap();
        assert!(layer.is_empty());
        assert!(layer.targets.is_empty());
    }

    #[test]
    fn the_channel_keeps_only_the_newest_layer() {
        let (producer, consumer) = latest_hud_channel();
        producer.publish(HudLayer::empty());
        let mut second = HudLayer::empty();
        second.width = 10;
        second.height = 10;
        second.pixels = vec![0; 400];
        producer.publish(second.clone());
        assert_eq!(consumer.take_latest(), Some(second));
        assert_eq!(consumer.take_latest(), None);
    }

    #[test]
    fn the_viewport_channel_reports_the_newest_window_size() {
        let (reporter, reader) = latest_viewport_channel();
        assert_eq!(reader.latest(), HudViewport::unusable());
        reporter.publish(HudViewport::new(350, 350, 2.0));
        assert_eq!(reader.latest(), HudViewport::new(350, 350, 2.0));
        reporter.publish(HudViewport::new(700, 400, 1.0));
        assert_eq!(reader.latest(), HudViewport::new(700, 400, 1.0));
    }
}
