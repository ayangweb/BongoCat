#![forbid(unsafe_code)]

//! Laying out and rasterizing a plugin's model-window panel.
//!
//! This crate is the one implementation of "what a plugin's panel looks like",
//! shared by both native renderers. It is platform-neutral and holds no GPU,
//! window or overlay dependency: it turns a plugin's bounded [`PluginView`] tree
//! into positioned rectangles and an RGBA bitmap, and the two backends only
//! upload the bitmap and draw one quad. That is what keeps D3D11 and Metal from
//! drifting, and it is why a plugin cannot produce a pixel the host did not lay
//! out.
//!
//! Text is rasterized with `fontdue` from a font the platform supplies. The host
//! does not bundle a font: a panel is a label, a count or a timer, and the
//! platform's own text face keeps the binary free of a font license obligation
//! while matching what the user sees elsewhere in the system.
//!
//! A panel's palette and metrics come from the gpui-kit theme the settings
//! window uses — the same `secondary`/`border`/`foreground` tones, the same
//! `Theme::radius` and the same medium `Button` height and padding. The panel
//! itself cannot be a gpui-kit `Button`: it is rasterized here into one bitmap
//! for the overlay quad rather than composed by GPUI, and no GPUI element ever
//! reaches this crate. Sharing the tokens is what makes the two surfaces look
//! like one product without a second widget implementation.

use bongocat_plugin_sdk::PluginView;
use fontdue::{Font, FontSettings};

mod font;
mod layer;
mod layout;
mod paint;

pub use font::load_platform_font;
pub use layer::{
    HudCompositor, HudConsumer, HudInteraction, HudLayer, HudProducer, HudTarget, HudViewport,
    HudViewportReader, HudViewportReporter, PluginHudFrame, latest_hud_channel,
    latest_viewport_channel,
};
pub use layout::{collect_interactive, layout_view, measure_view};
pub use paint::{HudBitmap, HudRenderer, HudRendererError, MAX_HUD_DIMENSION};

pub use layout::PlacedNode;

/// A logical rectangle: an origin and a size, in the model window's logical
/// pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HudRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl HudRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// A font the host loaded, and the sizes it can be drawn at.
pub struct HudFont {
    font: Font,
}

impl HudFont {
    /// Load a font from bytes. `collection_index` selects a face inside a
    /// collection such as a `.ttc`; `0` is the first.
    pub fn from_bytes(bytes: &[u8], collection_index: u32) -> Result<Self, HudRendererError> {
        let settings = FontSettings {
            collection_index,
            ..FontSettings::default()
        };
        let font = Font::from_bytes(bytes, settings)
            .map_err(|error| HudRendererError::new(format!("cannot load panel font: {error}")))?;
        Ok(Self { font })
    }

    pub(crate) fn rasterize(&self, character: char, size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.font.rasterize(character, size)
    }

    /// Whether this face can draw `character`.
    ///
    /// `fontdue` happily rasterizes a character the face has no glyph for, and
    /// what comes back is the font's own "missing glyph" box. A plugin's label
    /// is arbitrary UTF-8, so that box would appear as a placeholder next to the
    /// real text instead of the character the plugin asked for.
    pub fn has_glyph(&self, character: char) -> bool {
        self.font.has_glyph(character)
    }

    pub(crate) fn line_metrics(&self, size: f32) -> fontdue::LineMetrics {
        self.font
            .horizontal_line_metrics(size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: size * 0.8,
                descent: -size * 0.2,
                line_gap: 0.0,
                new_line_size: size,
            })
    }

    /// The advance width of a string at `size`, in logical pixels.
    ///
    /// A character the face cannot draw measures zero and takes no room, which
    /// is what keeps a skipped character from leaving a hole in a label.
    pub fn measure_text(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .filter(|character| self.has_glyph(*character))
            .map(|character| self.font.metrics(character, size).advance_width)
            .sum()
    }

    /// The line height of `size`, in logical pixels.
    pub fn line_height(&self, size: f32) -> f32 {
        self.line_metrics(size).new_line_size
    }
}

/// The default text color when a plugin does not choose one.
///
/// The settings window draws its labels with gpui-kit's `foreground` token, and
/// a panel that sits over the same product should not read as a different
/// application, so the palette below is taken from that theme rather than
/// invented here.
pub(crate) const DEFAULT_TEXT_COLOR: [f32; 4] = [0.98, 0.98, 0.98, 1.0];

/// The default panel fill: a translucent dark card that stays legible over any
/// model art without hiding it. This is gpui-kit's `popover.background` /
/// `secondary.background` step for the dark theme, held translucent so the
/// model behind it still shows.
pub(crate) const DEFAULT_PANEL_FILL: [f32; 4] = [0.078, 0.078, 0.078, 0.82];

/// The default button fill: gpui-kit's `secondary.background` for the dark
/// theme, so a panel button and a settings-window button share one tone.
pub(crate) const DEFAULT_BUTTON_FILL: [f32; 4] = [0.271, 0.271, 0.271, 0.95];

/// The default panel border: gpui-kit's `border` token for the dark theme,
/// which is what separates a card from the surface behind it. The tone is the
/// token's; the alpha is held back so a one-pixel outline does not read as a
/// hard outline over arbitrary model art.
pub(crate) const DEFAULT_PANEL_BORDER: [f32; 4] = [0.204, 0.204, 0.204, 0.55];

/// The corner radius of a button and of the panel card.
///
/// This is gpui-kit's `Theme::radius` (`px(6.)`), the value its own `Button`
/// rounds a medium button by. The panel is rasterized into a bitmap by this
/// crate rather than composed by GPUI, so it cannot call the component — the
/// shared constant is what keeps the two looking like one product.
pub const CORNER_RADIUS: f32 = 6.0;

/// The padding inside a panel, in logical pixels.
pub const PANEL_PADDING: f32 = 10.0;
/// The gap between a panel's rows, in logical pixels.
pub const PANEL_GAP: f32 = 6.0;
/// The text size a plugin's labels are drawn at, in logical pixels.
pub const LABEL_SIZE: f32 = 15.0;
/// The text size a plugin's primary value is drawn at, in logical pixels.
pub const VALUE_SIZE: f32 = 22.0;
/// The height of a plugin's button, in logical pixels.
///
/// gpui-kit's medium `Button` is `h_8()`, and a panel that draws a visibly
/// shorter button than the settings window does not look like the same
/// control, so the medium height is what a panel button uses.
pub const BUTTON_HEIGHT: f32 = 32.0;
/// The horizontal padding inside a button, in logical pixels.
///
/// gpui-kit's medium `Button` is `px_2p5()` (10 logical pixels) on each side.
pub const BUTTON_PADDING: f32 = 10.0;

/// A node a user can activate, and the rectangle it occupies.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractiveNode {
    pub id: String,
    pub rect: HudRect,
}

/// The result of laying out one view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudLayout {
    /// The panel's rectangle inside the viewport.
    pub panel: HudRect,
    /// Every interactive node, in draw order.
    pub interactive: Vec<InteractiveNode>,
}

/// Lay a view out and return the panel rectangle and its interactive nodes.
///
/// This is the pure half of the renderer: it needs the font only to measure
/// text, so a test can lay a tree out and assert on hit testing without a GPU.
pub fn layout(view: &PluginView, viewport: HudRect, font: &HudFont) -> HudLayout {
    if view.nodes.is_empty() {
        return HudLayout::default();
    }
    let size = layout::measure_view(view, font);
    let panel = layout::anchored_rect(view.anchor, viewport, size);
    let mut interactive = Vec::new();
    layout::collect_interactive(view, panel, font, &mut interactive);
    HudLayout { panel, interactive }
}

/// Which interactive node a point activates, if any.
///
/// The last match wins because later nodes are drawn on top, so a point inside
/// two overlapping buttons activates the one the user can see.
pub fn hit_test(layout: &HudLayout, x: f32, y: f32) -> Option<&InteractiveNode> {
    layout
        .interactive
        .iter()
        .rev()
        .find(|node| node.rect.contains(x, y))
}

#[cfg(test)]
mod tests;
