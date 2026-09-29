//! Turning a plugin's declared scene into the pixels the model window draws.
//!
//! A plugin declares a tree of nodes. This crate lays that tree out, resolves
//! every bound value against the state the host wrote, rasterizes the result to
//! straight RGBA8, and hands back pixels plus the rectangles a press would land
//! in. The overlay then uploads the pixels once and draws one positioned quad, so
//! neither Metal nor D3D11 learns anything about panels, text or buttons.
//!
//! Everything here is platform-neutral and CPU-only. That is the reason the whole
//! design works: the two render backends stay one layer each, and a panel cannot
//! fail in a way that is specific to one of them.
//!
//! # The layout pass
//!
//! Two passes, both recursive, both bounded by the same constants the manifest
//! was validated against:
//!
//! 1. **Measure.** Every node is asked how big it wants to be, given the space
//!    available on its axis. A stack's answer is its largest child's extent plus
//!    its own padding and spacing. Nothing is clipped and nothing is refused: a
//!    panel is a fixed size and a node that does not fit is a node that is drawn
//!    smaller or not at all, which is a picture the user can see is wrong rather
//!    than a load failure.
//! 2. **Draw.** Every node is given a rectangle and produces pixels, plus — for a
//!    button — the rectangle a press is tested against.
//!
//! The second pass emits the hit rectangles, so what the user sees and what the
//! user can press are produced by the same pass and cannot disagree.

#![forbid(unsafe_code)]

mod canvas;
mod error;
mod font;
mod layout;
#[cfg(test)]
mod tests;

use bongocat_plugin_protocol::{BindingTable, Color, SceneNode};
use bongocat_render::{OverlayAnchor, OverlayLayerPlacement, OverlayLayerRaster};
use std::sync::Arc;

pub use canvas::{Canvas, RoundedRect};
pub use error::{PluginRenderError, PluginRenderErrorCode};
pub use font::{DecodedImage, FontBook, FontWeight, TextLine, TextMeasurer, TextStyle};
pub use layout::MINIMUM_BUTTON_SIZE;

/// How many device pixels one logical pixel becomes.
///
/// A panel is authored in logical pixels so it is the same size relative to the
/// window at every display scale. This is where that becomes device pixels: the
/// overlay's drawable size divided by the panel's logical width, clamped so a
/// huge window cannot ask for an unbounded raster and a tiny one does not produce
/// a panel too small to read.
pub const MINIMUM_RASTER_SCALE: f32 = 1.0;
pub const MAXIMUM_RASTER_SCALE: f32 = 4.0;

/// One pressable region, in the panel's own logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct HitRegion {
    /// The button's id, as the scene declared it.
    pub button: String,
    pub rect: RoundedRect,
}

/// A panel, rasterized and ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderedPanel {
    /// Straight RGBA8, top row first, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The scene's anchors and margins, ready to become a placement.
    pub anchor: OverlayAnchor,
    pub margin: [f32; 2],
    pub width_fraction: f32,
    pub opacity: f32,
    /// Pressable regions, in logical pixels, in the order the scene declared
    /// them.
    pub hit_regions: Vec<HitRegion>,
    /// The pressables that were drawn greyed out, so a host that wants to ignore
    /// a press while a behavior is busy can ask which ones those were.
    pub disabled_regions: Vec<HitRegion>,
}

impl RenderedPanel {
    /// The layer texture the overlay uploads, stamped with a content hash.
    ///
    /// The hash covers the pixels and nothing else. A panel that is laid out and
    /// rasterized again to the same result must not be re-uploaded, and a hash
    /// that included the scene would change on every re-parse even when the
    /// picture did not.
    pub fn to_raster(&self) -> OverlayLayerRaster {
        OverlayLayerRaster {
            width: self.width,
            height: self.height,
            pixels: Arc::from(self.pixels.clone().into_boxed_slice()),
            content: content_hash(&self.pixels),
        }
    }

    /// Where this panel sits in a model window.
    pub fn to_placement(&self) -> OverlayLayerPlacement {
        OverlayLayerPlacement {
            anchor: self.anchor,
            margin: self.margin,
            nudge: [0.0, 0.0],
            width_fraction: self.width_fraction,
            opacity: self.opacity,
        }
    }

    /// Whether a press at a point in the panel's own logical pixels hits a
    /// pressable, and which one.
    ///
    /// Later regions win, matching the order they were drawn: a button that
    /// overlaps another is the one the press belongs to.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&str> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| region.rect.contains(x, y))
            .map(|region| region.button.as_str())
    }
}

/// A hash of the pixels, for the renderer's upload check.
///
/// FNV-1a rather than a cryptographic digest: this is a change detector for a few
/// hundred kilobytes a second, and a hash collision would mean one stale panel
/// until the next change. It is not, and must not be used as, an integrity check
/// — that is `sha256` in the store, on the archive.
pub fn content_hash(pixels: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in pixels {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Rasterize one panel from a validated manifest's contribution.
///
/// This is the entry point the product uses: it takes the anchor, size and scene
/// exactly as the manifest declared them, so nothing about a panel's placement
/// passes through this crate's judgement.
pub fn render_contribution(
    contribution: &bongocat_plugin_protocol::OverlayContribution,
    values: &BindingTable,
    scale: f32,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
) -> Result<RenderedPanel, PluginRenderError> {
    let panel = render_panel(
        &contribution.scene,
        values,
        contribution.size,
        scale,
        measurer,
        images,
    )?;
    Ok(RenderedPanel {
        anchor: contribution.anchor.to_overlay_anchor(),
        margin: contribution.margin,
        width_fraction: contribution.width_fraction,
        opacity: contribution.opacity,
        ..panel
    })
}

/// Rasterize a bare scene into a fixed-size panel.
///
/// `scale` is device pixels per logical pixel; see [`MINIMUM_RASTER_SCALE`].
/// `images` supplies the plugin's own PNGs by the relative path its scene named,
/// so this function performs no file access and is fully testable from a map.
pub fn render_panel(
    scene: &SceneNode,
    values: &BindingTable,
    logical_size: [u32; 2],
    scale: f32,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
) -> Result<RenderedPanel, PluginRenderError> {
    // A non-finite scale — which a caller can only produce by dividing by zero —
    // becomes the minimum rather than propagating into every coordinate.
    let scale = if scale.is_finite() {
        scale.clamp(MINIMUM_RASTER_SCALE, MAXIMUM_RASTER_SCALE)
    } else {
        MINIMUM_RASTER_SCALE
    };
    let width = logical_size[0] as f32;
    let height = logical_size[1] as f32;
    let mut canvas = Canvas::new(logical_size[0], logical_size[1], scale)?;
    let root = layout::draw(
        &mut canvas,
        scene,
        values,
        RoundedRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        measurer,
        images,
        &Theme::default(),
    )?;
    let mut panel = layout::finish(canvas, root, OverlayAnchor::TopLeft, [0.0, 0.0], 0.72, 1.0)?;
    panel.hit_regions.shrink_to_fit();
    panel.disabled_regions.shrink_to_fit();
    Ok(panel)
}

/// The colors and metrics a panel draws with when it names none.
///
/// A panel that names every color gets exactly what it asked for. One that names
/// none gets the product's own reading of a small overlay panel: a dark
/// translucent surface, a bright label, a muted secondary. The values live here
/// rather than in the settings window's theme because a panel is drawn on the
/// model window's transparent surface, not on a window background, and the two
/// need different defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub surface: Color,
    pub surface_border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub track: Color,
    pub button_surface: Color,
    pub button_text: Color,
    pub button_border: Color,
    pub divider: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // A dark translucent surface is the one choice that reads on both a
            // light and a dark desktop, and it is the same choice the update
            // window and the tray menu make.
            surface: Color::rgba(24, 24, 27, 204),
            surface_border: Color::rgba(255, 255, 255, 26),
            text: Color::rgb(245, 245, 247),
            text_muted: Color::rgb(161, 161, 170),
            accent: Color::rgb(232, 86, 78),
            track: Color::rgba(255, 255, 255, 40),
            button_surface: Color::rgba(255, 255, 255, 28),
            button_text: Color::rgb(245, 245, 247),
            button_border: Color::rgba(255, 255, 255, 40),
            divider: Color::rgba(255, 255, 255, 32),
        }
    }
}

/// The plugin's own images, by the relative path a scene named.
#[derive(Debug, Default)]
pub struct ImageLibrary {
    images: std::collections::BTreeMap<String, DecodedImage>,
}

impl ImageLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, image: DecodedImage) {
        self.images.insert(path.into(), image);
    }

    pub fn get(&self, path: &str) -> Option<&DecodedImage> {
        self.images.get(path)
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// Map a protocol error onto a render error.
///
/// The two error enums answer different questions — see `error` — so this is the
/// one place they meet. A manifest that validated should not reach the render
/// pass in a bad state, so almost every protocol code maps to
/// [`PluginRenderErrorCode::SceneInvalid`]: the point is to name the fact that the
/// two disagreed, not to guess which was wrong.
pub fn from_protocol(error: bongocat_plugin_protocol::PluginError) -> PluginRenderError {
    use bongocat_plugin_protocol::PluginErrorCode as Code;
    let code = match error.code() {
        Code::InvalidAssetPath => PluginRenderErrorCode::ImageUnreadable,
        _ => PluginRenderErrorCode::SceneInvalid,
    };
    let code_name = error.code().as_str();
    let detail = error.detail.unwrap_or_else(|| code_name.to_string());
    PluginRenderError::new(code, detail)
}
