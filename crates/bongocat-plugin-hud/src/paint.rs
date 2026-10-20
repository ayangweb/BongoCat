//! Rasterizing a laid-out view into an RGBA bitmap.
//!
//! The host lays out a plugin's panel itself; inline status icons are confined
//! to their bounded slot. This is where the panel's appearance is decided. The output is a plain
//! premultiplied RGBA buffer the two backends upload unchanged: D3D11 and Metal
//! differ in almost everything, but they agree on a texture, which is what lets
//! one rasterizer serve both.

use crate::layout::{placed_nodes, text_size};
use crate::{
    BUTTON_PADDING, CORNER_RADIUS, DEFAULT_BUTTON_FILL, DEFAULT_PANEL_BORDER, DEFAULT_PANEL_FILL,
    DEFAULT_TEXT_COLOR, HudFont, HudRect, LABEL_SIZE, PlacedNode,
};
use bongocat_plugin_sdk::{PluginView, StatusIcon, UiNode};

/// The largest panel the host will rasterize on either axis, in device pixels.
///
/// A panel is drawn over a model window that a user can resize, so a bound here
/// is what keeps a malicious or merely mistaken layout from asking for a
/// gigapixel allocation.
pub const MAX_HUD_DIMENSION: u32 = 4096;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct HudRendererError {
    pub message: String,
}

impl HudRendererError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A rasterized panel, ready to upload as one texture.
///
/// The pixels are premultiplied RGBA8, matching what the model texture path
/// already expects, so a backend uploads this exactly like a key image.
#[derive(Clone, Debug, PartialEq)]
pub struct HudBitmap {
    pub width: u32,
    pub height: u32,
    /// Row-major premultiplied RGBA8, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    /// Where the bitmap sits inside the model window, in logical pixels.
    pub origin: HudRect,
}

impl HudBitmap {
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }
}

/// Draws a plugin's view tree into a bitmap.
pub struct HudRenderer {
    font: HudFont,
}

impl HudRenderer {
    pub fn new(font: HudFont) -> Self {
        Self { font }
    }

    pub fn font(&self) -> &HudFont {
        &self.font
    }

    /// Rasterize one view into a bitmap at the given device scale.
    ///
    /// `viewport_width`/`viewport_height` are logical pixels: the panel is placed
    /// inside them, then the whole panel is scaled by `scale` into device pixels.
    pub fn rasterize(
        &self,
        view: &PluginView,
        viewport_width: u32,
        viewport_height: u32,
        scale: f32,
    ) -> Result<HudBitmap, HudRendererError> {
        if view.nodes.is_empty() {
            return Ok(HudBitmap {
                width: 0,
                height: 0,
                pixels: Vec::new(),
                origin: HudRect::default(),
            });
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(HudRendererError::new("panel scale is not usable"));
        }
        let viewport = HudRect::new(0.0, 0.0, viewport_width as f32, viewport_height as f32);
        let layout = crate::layout(view, viewport, &self.font);
        if layout.panel.is_empty() {
            return Ok(HudBitmap {
                width: 0,
                height: 0,
                pixels: Vec::new(),
                origin: HudRect::default(),
            });
        }
        let width = (layout.panel.width * scale).ceil().max(1.0) as u32;
        let height = (layout.panel.height * scale).ceil().max(1.0) as u32;
        if width > MAX_HUD_DIMENSION || height > MAX_HUD_DIMENSION {
            return Err(HudRendererError::new("panel exceeds the maximum size"));
        }
        let mut canvas = Canvas::new(width, height);
        // Everything is painted in device pixels, so the panel's own logical
        // origin is the offset that turns a laid-out rectangle into a bitmap
        // coordinate.
        let offset = layout.panel;
        canvas.fill_rounded_rect(
            HudRect::new(0.0, 0.0, width as f32, height as f32),
            CORNER_RADIUS * scale,
            DEFAULT_PANEL_FILL,
        );
        canvas.stroke_rounded_rect(
            HudRect::new(0.0, 0.0, width as f32, height as f32),
            CORNER_RADIUS * scale,
            DEFAULT_PANEL_BORDER,
            (scale).max(1.0),
        );
        for placed in placed_nodes(view, layout.panel, &self.font) {
            self.paint_node(&mut canvas, &placed, offset, scale);
        }
        Ok(HudBitmap {
            width,
            height,
            pixels: canvas.into_pixels(),
            origin: HudRect::new(offset.x, offset.y, offset.width, offset.height),
        })
    }

    fn paint_node(
        &self,
        canvas: &mut Canvas,
        placed: &PlacedNode<'_>,
        offset: HudRect,
        scale: f32,
    ) {
        let rect = bitmap_rect(placed.rect, offset, scale);
        match placed.node {
            UiNode::IconText { icon, text } => {
                let side = 24.0 * scale;
                let icon_rect = centered_icon_rect(rect, side);
                match icon {
                    StatusIcon::Rgba(pixels) if icon.validate() => {
                        canvas.draw_icon(pixels, icon_rect, icon.is_template());
                    }
                    StatusIcon::Badge(badge) => {
                        canvas.fill_rounded_rect(
                            icon_rect,
                            CORNER_RADIUS * scale,
                            DEFAULT_BUTTON_FILL,
                        );
                        let size =
                            LABEL_SIZE * scale * if badge.chars().count() > 1 { 0.75 } else { 1.0 };
                        let width = self.font.measure_text(badge, size);
                        canvas.draw_text(
                            &self.font,
                            badge,
                            rect.x + (side - width) / 2.0,
                            rect.y + (side - self.font.line_height(size)) / 2.0,
                            size,
                            DEFAULT_TEXT_COLOR,
                        );
                    }
                    _ => {}
                }
                canvas.draw_text(
                    &self.font,
                    text,
                    rect.x + side + crate::PANEL_GAP * scale,
                    centered_text_y(&self.font, text, LABEL_SIZE * scale, rect),
                    LABEL_SIZE * scale,
                    DEFAULT_TEXT_COLOR,
                );
            }
            UiNode::Text { text } => {
                let size = text_size(text) * scale;
                let x = if text_size(text) == crate::VALUE_SIZE {
                    (offset.width * scale - self.font.measure_text(text, size)) / 2.0
                } else {
                    rect.x
                };
                canvas.draw_text(&self.font, text, x, rect.y, size, DEFAULT_TEXT_COLOR);
            }
            UiNode::Button {
                label,
                icon,
                loading,
                ..
            } => {
                let primary = matches!(
                    icon,
                    bongocat_plugin_sdk::ActionIcon::Play | bongocat_plugin_sdk::ActionIcon::Pause
                );
                let fill = if primary {
                    [0.97, 0.975, 0.98, 0.98]
                } else {
                    [0.19, 0.195, 0.20, 0.98]
                };
                let border = if primary {
                    [1.0, 1.0, 1.0, 0.65]
                } else {
                    [0.38, 0.39, 0.40, 0.65]
                };
                canvas.fill_rounded_rect(rect, CORNER_RADIUS * scale, fill);
                canvas.stroke_rounded_rect(rect, CORNER_RADIUS * scale, border, scale);
                let color = if *loading {
                    [0.65, 0.7, 0.72, 1.0]
                } else if primary {
                    [0.13, 0.15, 0.18, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                };
                canvas.draw_text(
                    &self.font,
                    label,
                    rect.x + (rect.width - self.font.measure_text(label, LABEL_SIZE * scale)) / 2.0,
                    centered_text_y(&self.font, label, LABEL_SIZE * scale, rect),
                    LABEL_SIZE * scale,
                    color,
                );
            }
            UiNode::Toggle { label, value, .. } => {
                canvas.fill_rounded_rect(rect, CORNER_RADIUS * scale, DEFAULT_PANEL_FILL);
                let track = HudRect::new(
                    rect.x + rect.width - 30.0 * scale,
                    rect.y + (rect.height - 16.0 * scale) / 2.0,
                    24.0 * scale,
                    16.0 * scale,
                );
                let fill = if *value {
                    DEFAULT_BUTTON_FILL
                } else {
                    [0.3, 0.32, 0.38, 0.9]
                };
                canvas.fill_rounded_rect(track, 8.0 * scale, fill);
                canvas.draw_text(
                    &self.font,
                    label,
                    rect.x + BUTTON_PADDING * scale,
                    rect.y + (rect.height - self.font.line_height(LABEL_SIZE * scale)) / 2.0,
                    LABEL_SIZE * scale,
                    DEFAULT_TEXT_COLOR,
                );
            }
            UiNode::Integer { label, value, .. } => {
                let text = format!("{label}: {value}");
                canvas.draw_text(
                    &self.font,
                    &text,
                    rect.x,
                    rect.y,
                    LABEL_SIZE * scale,
                    DEFAULT_TEXT_COLOR,
                );
            }
            UiNode::Select {
                label,
                value,
                options,
                ..
            } => {
                let text = format!(
                    "{label}: {}",
                    crate::layout::select_option_label(value, options)
                );
                canvas.draw_text(
                    &self.font,
                    &text,
                    rect.x,
                    rect.y,
                    LABEL_SIZE * scale,
                    DEFAULT_TEXT_COLOR,
                );
            }
        }
    }
}

fn bitmap_rect(rect: HudRect, offset: HudRect, scale: f32) -> HudRect {
    HudRect::new(
        (rect.x - offset.x) * scale,
        (rect.y - offset.y) * scale,
        rect.width * scale,
        rect.height * scale,
    )
}

fn centered_icon_rect(row: HudRect, side: f32) -> HudRect {
    HudRect::new(row.x, row.y + (row.height - side) / 2.0, side, side)
}

fn centered_text_y(font: &HudFont, text: &str, size: f32, row: HudRect) -> f32 {
    let ascent = font.line_metrics(size).ascent;
    let mut ink_top = f32::INFINITY;
    let mut ink_bottom = f32::NEG_INFINITY;
    for character in text.chars().filter(|character| font.has_glyph(*character)) {
        let (metrics, _) = font.rasterize(character, size);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        ink_top = ink_top.min(ascent - metrics.ymin as f32 - metrics.height as f32);
        ink_bottom = ink_bottom.max(ascent - metrics.ymin as f32);
    }
    if !ink_top.is_finite() {
        return row.y + (row.height - font.line_height(size)) / 2.0;
    }
    row.y + (row.height - (ink_bottom - ink_top)) / 2.0 - ink_top
}

/// A premultiplied RGBA8 surface the panel is painted into.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn draw_icon(&mut self, pixels: &[u8], rect: HudRect, template: bool) {
        // Supersampling the source and interpolating premultiplied colour keeps
        // transparent edges clean when a 32px native icon is resized for the
        // panel. Nearest-neighbour sampling made circles and diagonal strokes
        // visibly stair-step at 100% and 125% display scales.
        const SAMPLES_PER_AXIS: usize = 2;
        const SAMPLE_COUNT: f32 = (SAMPLES_PER_AXIS * SAMPLES_PER_AXIS) as f32;
        for y in 0..rect.height.ceil() as u32 {
            for x in 0..rect.width.ceil() as u32 {
                let mut color = [0.0; 4];
                for sample_y in 0..SAMPLES_PER_AXIS {
                    for sample_x in 0..SAMPLES_PER_AXIS {
                        let local_x = (x as f32
                            + (sample_x as f32 + 0.5) / SAMPLES_PER_AXIS as f32)
                            / rect.width;
                        let local_y = (y as f32
                            + (sample_y as f32 + 0.5) / SAMPLES_PER_AXIS as f32)
                            / rect.height;
                        let sample = Self::sample_icon(
                            pixels,
                            local_x * 32.0 - 0.5,
                            local_y * 32.0 - 0.5,
                            template,
                        );
                        color[0] += sample[0] * sample[3] / SAMPLE_COUNT;
                        color[1] += sample[1] * sample[3] / SAMPLE_COUNT;
                        color[2] += sample[2] * sample[3] / SAMPLE_COUNT;
                        color[3] += sample[3] / SAMPLE_COUNT;
                    }
                }
                if color[3] > f32::EPSILON {
                    color[0] /= color[3];
                    color[1] /= color[3];
                    color[2] /= color[3];
                }
                self.blend(
                    rect.x as i64 + i64::from(x),
                    rect.y as i64 + i64::from(y),
                    color,
                );
            }
        }
    }

    fn sample_icon(pixels: &[u8], x: f32, y: f32, template: bool) -> [f32; 4] {
        const SIZE: usize = 32;
        let x = x.clamp(0.0, (SIZE - 1) as f32);
        let y = y.clamp(0.0, (SIZE - 1) as f32);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(SIZE - 1);
        let y1 = (y0 + 1).min(SIZE - 1);
        let tx = x - x0 as f32;
        let ty = y - y0 as f32;
        let mut premultiplied = [0.0; 4];

        for (row, row_weight) in [(y0, 1.0 - ty), (y1, ty)] {
            for (column, column_weight) in [(x0, 1.0 - tx), (x1, tx)] {
                let weight = row_weight * column_weight;
                let index = (row * SIZE + column) * 4;
                let alpha = f32::from(pixels[index + 3]) / 255.0;
                premultiplied[3] += alpha * weight;
                for channel in 0..3 {
                    let value = if template {
                        DEFAULT_TEXT_COLOR[channel]
                    } else {
                        f32::from(pixels[index + channel]) / 255.0
                    };
                    premultiplied[channel] += value * alpha * weight;
                }
            }
        }

        if premultiplied[3] > f32::EPSILON {
            premultiplied[0] /= premultiplied[3];
            premultiplied[1] /= premultiplied[3];
            premultiplied[2] /= premultiplied[3];
        }
        premultiplied
    }

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

    fn blend(&mut self, x: i64, y: i64, color: [f32; 4]) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let alpha = color[3].clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        // Source-over with premultiplied source: `dst = src + dst * (1 - a)`.
        let inverse = 1.0 - alpha;
        for (channel, component) in color.iter().take(3).enumerate() {
            let source = component.clamp(0.0, 1.0) * alpha;
            let destination = f32::from(self.pixels[index + channel]) / 255.0;
            self.pixels[index + channel] =
                ((source + destination * inverse).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        let destination_alpha = f32::from(self.pixels[index + 3]) / 255.0;
        self.pixels[index + 3] =
            ((alpha + destination_alpha * inverse).clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    fn fill_rounded_rect(&mut self, rect: HudRect, radius: f32, color: [f32; 4]) {
        let x0 = rect.x.floor() as i64;
        let y0 = rect.y.floor() as i64;
        let x1 = rect.right().ceil() as i64;
        let y1 = rect.bottom().ceil() as i64;
        let radius = radius.max(0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let coverage = rounded_coverage(x as f32 + 0.5, y as f32 + 0.5, rect, radius);
                if coverage <= 0.0 {
                    continue;
                }
                let mut painted = color;
                painted[3] *= coverage;
                self.blend(x, y, painted);
            }
        }
    }

    fn stroke_rounded_rect(&mut self, rect: HudRect, radius: f32, color: [f32; 4], width: f32) {
        let inner = HudRect::new(
            rect.x + width,
            rect.y + width,
            (rect.width - width * 2.0).max(0.0),
            (rect.height - width * 2.0).max(0.0),
        );
        let x0 = rect.x.floor() as i64;
        let y0 = rect.y.floor() as i64;
        let x1 = rect.right().ceil() as i64;
        let y1 = rect.bottom().ceil() as i64;
        let radius = radius.max(0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let outer = rounded_coverage(px, py, rect, radius);
                let hole = rounded_coverage(px, py, inner, (radius - width).max(0.0));
                let coverage = (outer - hole).max(0.0);
                if coverage <= 0.0 {
                    continue;
                }
                let mut painted = color;
                painted[3] *= coverage;
                self.blend(x, y, painted);
            }
        }
    }

    fn draw_text(
        &mut self,
        font: &HudFont,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
    ) {
        let metrics = font.line_metrics(size);
        let baseline = y + metrics.ascent;
        let mut pen = x;
        for character in text.chars() {
            // A character the face has no glyph for rasterizes to the font's
            // own placeholder box. Drawing it would put a row of boxes next to
            // the label, so it is skipped instead and the rest of the string
            // closes up over the gap.
            if !font.has_glyph(character) {
                continue;
            }
            let (glyph, bitmap) = font.rasterize(character, size);
            if !bitmap.is_empty() {
                for row in 0..glyph.height {
                    for column in 0..glyph.width {
                        let coverage = f32::from(bitmap[row * glyph.width + column]) / 255.0;
                        if coverage <= 0.0 {
                            continue;
                        }
                        let px = pen + glyph.xmin as f32 + column as f32;
                        let py = baseline - glyph.ymin as f32 - glyph.height as f32 + row as f32;
                        let mut painted = color;
                        painted[3] *= coverage;
                        self.blend(px.round() as i64, py.round() as i64, painted);
                    }
                }
            }
            pen += glyph.advance_width;
        }
    }
}

/// The fraction of a pixel inside a rounded rectangle.
///
/// A single sample per pixel is enough for a panel: it is small, it sits over
/// model art rather than over other text, and the alternative — four samples and
/// a box filter — would cost four times the rasterization for a difference the
/// user cannot see at this size.
fn rounded_coverage(px: f32, py: f32, rect: HudRect, radius: f32) -> f32 {
    if px < rect.x || py < rect.y || px >= rect.right() || py >= rect.bottom() {
        return 0.0;
    }
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0);
    if radius <= 0.0 {
        return 1.0;
    }
    let corners = [
        (rect.x + radius, rect.y + radius, -1.0, -1.0),
        (rect.right() - radius, rect.y + radius, 1.0, -1.0),
        (rect.x + radius, rect.bottom() - radius, -1.0, 1.0),
        (rect.right() - radius, rect.bottom() - radius, 1.0, 1.0),
    ];
    for (cx, cy, sign_x, sign_y) in corners {
        let inside_x = if sign_x < 0.0 { px < cx } else { px > cx };
        let inside_y = if sign_y < 0.0 { py < cy } else { py > cy };
        if inside_x && inside_y {
            let dx = px - cx;
            let dy = py - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            return (radius + 0.5 - distance).clamp(0.0, 1.0);
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HudFont;
    use bongocat_plugin_sdk::{ActionIcon, Anchor, UiNode};

    fn renderer() -> HudRenderer {
        HudRenderer::new(crate::load_platform_font().expect("platform font"))
    }

    fn view() -> PluginView {
        PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![
                UiNode::Text {
                    text: "05:00".into(),
                },
                UiNode::button("toggle", "Start", ActionIcon::Play),
            ],
        }
    }

    #[test]
    fn a_panel_rasterizes_to_premultiplied_pixels_inside_its_bounds() {
        let bitmap = renderer().rasterize(&view(), 300, 300, 2.0).unwrap();
        assert!(bitmap.width > 0 && bitmap.height > 0);
        assert_eq!(
            bitmap.pixels.len(),
            bitmap.width as usize * bitmap.height as usize * 4
        );
        // Premultiplied means no channel may exceed the alpha it belongs to.
        for pixel in bitmap.pixels.chunks_exact(4) {
            for channel in &pixel[..3] {
                assert!(*channel <= pixel[3]);
            }
        }
        assert!(bitmap.pixels.iter().any(|byte| *byte > 0));
    }

    #[test]
    fn an_empty_view_draws_nothing_and_an_oversized_panel_is_refused() {
        let renderer = renderer();
        let empty = PluginView::default();
        let bitmap = renderer.rasterize(&empty, 300, 300, 1.0).unwrap();
        assert!(bitmap.is_empty());
        assert!(renderer.rasterize(&view(), 300, 300, 0.0).is_err());
    }

    #[test]
    fn icon_is_centered_in_a_taller_text_row() {
        let row = HudRect::new(0.0, 0.0, 120.0, 32.0);
        assert_eq!(
            centered_icon_rect(row, 24.0),
            HudRect::new(0.0, 4.0, 24.0, 24.0)
        );
    }

    #[test]
    fn icon_and_chinese_label_share_the_same_visible_vertical_center() {
        let font = crate::load_platform_font().expect("platform font");
        let row = HudRect::new(0.0, 0.0, 180.0, 24.0);
        let label = "微信输入法";
        let size = LABEL_SIZE;
        let y = centered_text_y(&font, label, size, row);
        let ascent = font.line_metrics(size).ascent;
        let mut ink_top = f32::INFINITY;
        let mut ink_bottom = f32::NEG_INFINITY;
        for character in label.chars() {
            let (metrics, _) = font.rasterize(character, size);
            ink_top = ink_top.min(y + ascent - metrics.ymin as f32 - metrics.height as f32);
            ink_bottom = ink_bottom.max(y + ascent - metrics.ymin as f32);
        }
        let text_center = (ink_top + ink_bottom) / 2.0;
        let icon = centered_icon_rect(row, 24.0);
        let icon_center = icon.y + icon.height / 2.0;
        assert!((text_center - icon_center).abs() < 0.01);
    }

    #[test]
    fn icon_sampling_interpolates_transparent_edges_without_rgb_fringes() {
        let mut pixels = vec![0; 32 * 32 * 4];
        let index = (16 * 32 + 16) * 4;
        pixels[index..index + 4].copy_from_slice(&[255, 0, 0, 255]);
        let sample = Canvas::sample_icon(&pixels, 15.5, 16.0, false);
        assert!(sample[3] > 0.0 && sample[3] < 1.0);
        assert!((sample[0] - 1.0).abs() < f32::EPSILON);
        assert!(sample[1].abs() < f32::EPSILON && sample[2].abs() < f32::EPSILON);
    }

    #[test]
    fn the_panel_is_placed_in_the_anchor_corner() {
        let bitmap = renderer().rasterize(&view(), 400, 200, 1.0).unwrap();
        // Top-right: the bitmap's right edge sits near the viewport's right edge.
        assert!(bitmap.origin.right() <= 400.0);
        assert!(bitmap.origin.x > 200.0);
        assert!(bitmap.origin.y < 100.0);
    }

    #[test]
    fn the_font_measures_and_rasterizes_text() {
        let font: HudFont = crate::load_platform_font().expect("platform font");
        assert!(font.measure_text("00:00", LABEL_SIZE) > 0.0);
        assert!(font.line_height(LABEL_SIZE) > 0.0);
    }

    /// A plugin's label is arbitrary UTF-8, so the face the host picked has to
    /// be able to draw what a plugin actually publishes.
    ///
    /// A Latin-only face does not fail here — it rasterizes a placeholder box
    /// for every character it lacks, which is what put a row of squares next to
    /// the timer's labels.
    #[test]
    fn the_platform_face_draws_the_scripts_a_plugin_may_publish() {
        let font = crate::load_platform_font().expect("platform font");
        for text in ["Ready", "Start / 开始", "Restart / 重新计时", "25:00"] {
            for character in text.chars() {
                assert!(
                    font.has_glyph(character),
                    "the loaded face cannot draw {character:?} from {text:?}"
                );
            }
        }
    }

    /// A character the face lacks takes no space rather than a placeholder box.
    #[test]
    fn a_character_the_face_lacks_is_skipped_rather_than_drawn_as_a_box() {
        let font = crate::load_platform_font().expect("platform font");
        let missing = ['\u{10FFFD}', '\u{1F600}']
            .into_iter()
            .find(|character| !font.has_glyph(*character));
        let Some(missing) = missing else {
            // Nothing outside the face's coverage to test with.
            return;
        };
        assert_eq!(font.measure_text(&missing.to_string(), LABEL_SIZE), 0.0);
    }
}
