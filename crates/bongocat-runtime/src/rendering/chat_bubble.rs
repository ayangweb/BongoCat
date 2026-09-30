//! The chat bubble floating above the model's head.
//!
//! The multiplayer worker sends a `ShowChatBubble` command when a room member
//! speaks; the runtime rasterizes the line once into a straight-alpha texture
//! and keeps showing it, fading, until its hold time is up. The renderer sees
//! only the finished texture and its placement — it never learns the text.

use super::*;

use bongocat_render::{ChatBubbleSnapshot, ChatBubbleTexture, ModelBounds};
use std::path::PathBuf;

/// Font size the bubble text is rasterized at, in texture pixels. The quad is
/// mapped from this by width ratio, so a larger raster only costs memory and
/// keeps the text crisp on high-scale displays.
pub(crate) const FONT_PIXELS: f32 = 40.0;
/// Horizontal padding around the text block.
pub(crate) const PADDING_X: f32 = 28.0;
/// Vertical padding above and below the text block.
pub(crate) const PADDING_Y: f32 = 18.0;
/// Corner radius of the bubble body.
pub(crate) const RADIUS: f32 = 18.0;
/// Width and height of the tail triangle at the bubble's bottom center.
pub(crate) const TAIL_WIDTH: f32 = 26.0;
pub(crate) const TAIL_HEIGHT: f32 = 14.0;
/// The widest line of text before wrapping, in texture pixels. The bubble quad
/// is sized so a line of this width fills [`DISPLAY_WIDTH_FRACTION`] of the
/// model canvas.
pub(crate) const MAXIMUM_TEXT_WIDTH: f32 = 720.0;
/// Full-scale raster width the display mapping is calibrated against.
pub(crate) const REFERENCE_RASTER_WIDTH: f32 = 800.0;
/// How wide a full-scale bubble may get, as a fraction of the canvas width.
pub(crate) const DISPLAY_WIDTH_FRACTION: f32 = 0.82;
/// Most lines the bubble will show before the text is cut short.
pub(crate) const MAXIMUM_LINES: usize = 4;
/// Line height in texture pixels, matched to the system fonts' metrics at
/// [`FONT_PIXELS`].
pub(crate) const LINE_HEIGHT: f32 = 48.0;
pub(crate) const FADE_IN: Duration = Duration::from_millis(180);
pub(crate) const FADE_OUT: Duration = Duration::from_millis(360);
pub(crate) const MINIMUM_HOLD: Duration = Duration::from_secs(4);
pub(crate) const MAXIMUM_HOLD: Duration = Duration::from_secs(10);
/// Extra hold per content character, so a longer line stays readable.
pub(crate) const HOLD_PER_CHAR: Duration = Duration::from_millis(70);
/// Space above the complete bubble, as a fraction of the canvas height.
/// The bottom-center anchor must also account for the bubble's height;
/// insetting only the anchor would place most of the quad outside the window.
pub(crate) const TOP_INSET_FRACTION: f32 = 0.03;
pub(crate) const BUBBLE_ALPHA: f32 = 226.0 / 255.0;
/// How many rasterized bubbles to keep. Chat repeats lines often enough that
/// the common case should not pay the raster again.
pub(crate) const CACHE_LIMIT: usize = 4;

/// One message being shown.
pub(crate) struct ActiveBubble {
    pub(crate) texture: Arc<ChatBubbleTexture>,
    pub(crate) shown_at: Duration,
    pub(crate) hold: Duration,
}

/// The renderer's bubble state: the system font, the active message and a tiny
/// cache so repeated lines do not rasterize twice.
///
/// The font loads lazily on the first message. Renderer startup runs in every
/// runtime worker, while a chat bubble only ever shows in a multiplayer room;
/// reading a multi-megabyte system font at startup would tax every launch and
/// every test for a feature most sessions never use.
pub(crate) struct ChatBubbleState {
    pub(crate) font: Option<fontdue::Font>,
    pub(crate) font_attempted: bool,
    pub(crate) active: Option<ActiveBubble>,
    pub(crate) cache: Vec<(String, Arc<ChatBubbleTexture>)>,
}

impl ChatBubbleState {
    pub(crate) fn new() -> Self {
        Self {
            font: None,
            font_attempted: false,
            active: None,
            cache: Vec::new(),
        }
    }

    /// Show (or replace) the bubble for one chat line. Rasterization happens
    /// here, on the worker thread, once per new message: a command is the only
    /// path in, so the per-frame evaluation never touches the font.
    pub(crate) fn show(&mut self, sender: &str, content: &str, now: Duration) {
        let display = format!("{sender}: {content}");
        let Some(texture) = self.rasterize(&display) else {
            return;
        };
        // A longer line earns a longer hold, bounded so a wall of text cannot
        // park itself over the cat for half a minute.
        let hold =
            (MINIMUM_HOLD + HOLD_PER_CHAR * content.chars().count() as u32).min(MAXIMUM_HOLD);
        self.active = Some(ActiveBubble {
            texture,
            shown_at: now,
            hold,
        });
    }

    /// The bubble to draw this frame, fading in and out around its hold, or
    /// `None` once it expired. Expired bubbles are cleared here, so the state
    /// never grows and an expired bubble costs nothing.
    pub(crate) fn snapshot(
        &mut self,
        now: Duration,
        bounds: ModelBounds,
    ) -> Option<ChatBubbleSnapshot> {
        let active = self.active.as_mut()?;
        let shown_for = now.saturating_sub(active.shown_at);
        let total = FADE_IN + active.hold + FADE_OUT;
        if shown_for >= total {
            self.active = None;
            return None;
        }
        let opacity = if shown_for < FADE_IN {
            shown_for.as_secs_f32() / FADE_IN.as_secs_f32()
        } else {
            match shown_for.checked_sub(FADE_IN + active.hold) {
                None => 1.0,
                Some(since_hold_end) => {
                    (1.0 - since_hold_end.as_secs_f32() / FADE_OUT.as_secs_f32()).clamp(0.0, 1.0)
                }
            }
        };
        // The reference width is what a full text line rasterizes to; a shorter
        // line produces a narrower texture and therefore a narrower bubble.
        let texture = Arc::clone(&active.texture);
        let canvas_width = bounds.width();
        let canvas_height = bounds.height();
        let top_inset = canvas_height * TOP_INSET_FRACTION;
        let maximum_width = canvas_width * DISPLAY_WIDTH_FRACTION;
        let maximum_height = canvas_height - 2.0 * top_inset;
        // Keep the entire quad within the canvas, including multiline bubbles
        // on wide, short models. One scale preserves the texture's aspect ratio.
        let scale = (maximum_width / REFERENCE_RASTER_WIDTH)
            .min(maximum_width / texture.width as f32)
            .min(maximum_height / texture.height as f32);
        let display_width = texture.width as f32 * scale;
        let display_height = texture.height as f32 * scale;
        let center_x = (bounds.min_x + bounds.max_x) / 2.0;
        let bottom_y = bounds.max_y - top_inset - display_height;
        Some(ChatBubbleSnapshot {
            texture,
            anchor: [center_x, bottom_y],
            size: [display_width, display_height],
            opacity,
        })
    }

    fn rasterize(&mut self, display: &str) -> Option<Arc<ChatBubbleTexture>> {
        if let Some((_, texture)) = self.cache.iter().find(|(text, _)| text == display) {
            return Some(Arc::clone(texture));
        }
        if !self.font_attempted {
            self.font = load_system_font();
            self.font_attempted = true;
        }
        // Without a font the bubble still shows, as an empty pill: a missing
        // system font must not silently swallow the message, and the text is
        // still readable in the settings window.
        let texture = Arc::new(match self.font.as_ref() {
            Some(font) => rasterize_bubble(font, display),
            None => empty_bubble(),
        });
        self.cache.push((display.to_owned(), Arc::clone(&texture)));
        let overflow = self.cache.len().saturating_sub(CACHE_LIMIT);
        self.cache.drain(..overflow);
        Some(texture)
    }
}

/// System fonts that cover the chat languages the product ships, most
/// preferred first. A missing or unreadable font is not fatal: the bubble
/// rasterizes as an empty pill, and the chat text is still readable in the
/// settings window.
fn load_system_font() -> Option<fontdue::Font> {
    candidate_font_paths()
        .into_iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find_map(|bytes| {
            fontdue::Font::from_bytes(
                bytes,
                fontdue::FontSettings {
                    collection_index: 0,
                    scale: FONT_PIXELS,
                    load_substitutions: false,
                },
            )
            .ok()
        })
}

#[cfg(windows)]
fn candidate_font_paths() -> Vec<PathBuf> {
    let fonts = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("Fonts");
    vec![fonts.join("msyh.ttc"), fonts.join("simsun.ttc")]
}

#[cfg(target_os = "macos")]
fn candidate_font_paths() -> Vec<PathBuf> {
    let fonts = PathBuf::from("/System/Library/Fonts");
    vec![fonts.join("PingFang.ttc"), fonts.join("Helvetica.ttc")]
}

/// One laid-out line: glyph indices with pen offsets from the line start.
#[derive(Clone)]
struct LaidLine {
    glyphs: Vec<(u16, f32)>,
    width: f32,
}

struct BubbleLayout {
    lines: Vec<LaidLine>,
}

/// Wrap `display` into at most [`MAXIMUM_LINES`] lines of
/// [`MAXIMUM_TEXT_WIDTH`] pixels, cutting the tail with an ellipsis when it
/// does not fit. Wrapping prefers a space inside the current line and falls
/// back to breaking between characters, which is what CJK text needs anyway.
fn layout_text(font: &fontdue::Font, display: &str) -> BubbleLayout {
    let mut lines: Vec<LaidLine> = Vec::new();
    // One pending line: glyph indices with pen offsets from the line start.
    let mut current: Vec<(u16, f32)> = Vec::new();
    let mut current_width = 0.0_f32;
    let mut last_space: Option<usize> = None;
    for character in display.chars() {
        let index = font.lookup_glyph_index(character);
        let advance = font
            .metrics_indexed(index, FONT_PIXELS)
            .advance_width
            .max(0.0);
        if character == ' ' {
            last_space = Some(current.len());
        }
        if current_width + advance > MAXIMUM_TEXT_WIDTH && !current.is_empty() {
            // Break after the last space when there is one; the glyphs after
            // it start the next line with a re-zeroed pen.
            let break_at = last_space.map_or(current.len(), |space| space + 1);
            let rest: Vec<(u16, f32)> = current.split_off(break_at.min(current.len()));
            let rest_offset = rest.first().map_or(0.0, |(_, pen)| *pen);
            lines.push(LaidLine {
                width: current_width,
                glyphs: std::mem::take(&mut current),
            });
            current = rest
                .into_iter()
                .map(|(index, pen)| (index, pen - rest_offset))
                .collect();
            current_width -= rest_offset;
            last_space = None;
        }
        current.push((index, current_width));
        current_width += advance;
    }
    if !current.is_empty() {
        lines.push(LaidLine {
            width: current_width,
            glyphs: current,
        });
    }
    // The text does not fit: the earlier lines stay whole, the last one fills
    // as far as it can and ends in an ellipsis.
    let ellipsis = build_ellipsis(font);
    if lines.len() > MAXIMUM_LINES {
        let mut last: Vec<(u16, f32)> = Vec::new();
        let mut last_width = 0.0_f32;
        'outer: for line in &lines[MAXIMUM_LINES - 1..] {
            for (index, _) in &line.glyphs {
                let advance = font
                    .metrics_indexed(*index, FONT_PIXELS)
                    .advance_width
                    .max(0.0);
                if last_width + advance > MAXIMUM_TEXT_WIDTH {
                    break 'outer;
                }
                last.push((*index, last_width));
                last_width += advance;
            }
        }
        for (index, pen) in &ellipsis {
            last.push((*index, last_width + pen));
        }
        if let Some((_, pen)) = ellipsis.last() {
            last_width += pen;
        }
        let mut kept: Vec<LaidLine> = lines[..MAXIMUM_LINES - 1].to_vec();
        kept.push(LaidLine {
            width: last_width,
            glyphs: last,
        });
        lines = kept;
    }
    BubbleLayout { lines }
}

/// The ellipsis glyph run, preferring the single-character ellipsis and
/// falling back to three periods for a font without it.
fn build_ellipsis(font: &fontdue::Font) -> Vec<(u16, f32)> {
    let ellipsis = font.lookup_glyph_index('…');
    if ellipsis != 0 {
        return vec![(ellipsis, 0.0)];
    }
    let mut glyphs = Vec::new();
    let mut pen = 0.0;
    for _ in 0..3 {
        let period = font.lookup_glyph_index('.');
        glyphs.push((period, pen));
        pen += font
            .metrics_indexed(period, FONT_PIXELS)
            .advance_width
            .max(0.0);
    }
    glyphs
}

/// Rasterize one bubble: a translucent rounded pill with a tail, the wrapped
/// text in dark ink, straight alpha — the overlay shaders premultiply at draw.
fn rasterize_bubble(font: &fontdue::Font, display: &str) -> ChatBubbleTexture {
    let layout = layout_text(font, display);
    let text_width = layout
        .lines
        .iter()
        .map(|line| line.width)
        .fold(0.0_f32, f32::max)
        .max(1.0);
    let line_count = layout.lines.len().max(1);
    let body_height = PADDING_Y * 2.0 + LINE_HEIGHT * line_count as f32;
    let width = (text_width + PADDING_X * 2.0).ceil().max(RADIUS * 2.0) as u32;
    let height = (body_height + TAIL_HEIGHT).ceil() as u32;
    let mut rgba = vec![0_u8; (width as usize) * (height as usize) * 4];
    let half_w = width as f32 / 2.0;
    let body_half_h = body_height / 2.0;
    let body_bottom = body_height;
    for row in 0..height {
        for column in 0..width {
            let x = column as f32 + 0.5 - half_w;
            let y = row as f32 + 0.5;
            let coverage = body_coverage(x, y, half_w, body_half_h).max(tail_coverage(
                x,
                y,
                body_bottom,
                height as f32,
            ));
            if coverage <= 0.0 {
                continue;
            }
            let pixel = (row as usize * width as usize + column as usize) * 4;
            rgba[pixel] = 255;
            rgba[pixel + 1] = 255;
            rgba[pixel + 2] = 255;
            rgba[pixel + 3] = (coverage.min(1.0) * BUBBLE_ALPHA * 255.0) as u8;
        }
    }
    let ascent = font
        .vertical_line_metrics(FONT_PIXELS)
        .map_or(FONT_PIXELS * 0.8, |metrics| metrics.ascent);
    for (line_index, line) in layout.lines.iter().enumerate() {
        let line_left = (width as f32 - line.width) / 2.0;
        let baseline = PADDING_Y + ascent + LINE_HEIGHT * line_index as f32;
        for (index, pen) in &line.glyphs {
            blit_glyph(
                font,
                &mut rgba,
                width,
                height,
                *index,
                line_left + pen,
                baseline,
            );
        }
    }
    ChatBubbleTexture {
        width,
        height,
        rgba,
    }
}

/// The fallback pill for when no system font loaded: same shape as a one-line
/// bubble, no glyphs.
fn empty_bubble() -> ChatBubbleTexture {
    let width = 160_u32;
    let height = (PADDING_Y * 2.0 + LINE_HEIGHT + TAIL_HEIGHT).ceil() as u32;
    let mut rgba = vec![0_u8; (width as usize) * (height as usize) * 4];
    let half_w = width as f32 / 2.0;
    let body_half_h = (height as f32 - TAIL_HEIGHT) / 2.0;
    let body_bottom = height as f32 - TAIL_HEIGHT;
    for row in 0..height {
        for column in 0..width {
            let x = column as f32 + 0.5 - half_w;
            let y = row as f32 + 0.5;
            let coverage = body_coverage(x, y, half_w, body_half_h).max(tail_coverage(
                x,
                y,
                body_bottom,
                height as f32,
            ));
            if coverage <= 0.0 {
                continue;
            }
            let pixel = (row as usize * width as usize + column as usize) * 4;
            rgba[pixel] = 255;
            rgba[pixel + 1] = 255;
            rgba[pixel + 2] = 255;
            rgba[pixel + 3] = (coverage.min(1.0) * BUBBLE_ALPHA * 255.0) as u8;
        }
    }
    ChatBubbleTexture {
        width,
        height,
        rgba,
    }
}

/// Anti-aliased coverage of the bubble's rounded body. Pixels are addressed in
/// x-from-center, y-from-top; the rounded-rect signed distance does the edges.
fn body_coverage(x: f32, y: f32, half_w: f32, body_half_h: f32) -> f32 {
    let dx = x.abs() - (half_w - RADIUS);
    let dy = (y - body_half_h).abs() - (body_half_h - RADIUS);
    let outside_x = dx.max(0.0);
    let outside_y = dy.max(0.0);
    let distance = outside_x.hypot(outside_y) + dx.max(dy).min(0.0) - RADIUS;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Anti-aliased coverage of the tail triangle under the body.
fn tail_coverage(x: f32, y: f32, body_bottom: f32, height: f32) -> f32 {
    if y < body_bottom || y > height {
        return 0.0;
    }
    // x is already relative to the texture's center, not its left edge.
    let apex = 0.0;
    let half_spread = TAIL_WIDTH / 2.0 * (height - y) / TAIL_HEIGHT;
    let left = apex - half_spread;
    let right = apex + half_spread;
    (0.5 - (left - x).max(x - right)).clamp(0.0, 1.0)
}

/// Blend one glyph's coverage into the buffer. The ink is near-black; the
/// alpha channel carries the shape until the final premultiply.
fn blit_glyph(
    font: &fontdue::Font,
    rgba: &mut [u8],
    width: u32,
    height: u32,
    index: u16,
    pen_x: f32,
    baseline: f32,
) {
    let (glyph_metrics, coverage) = font.rasterize_indexed(index, FONT_PIXELS);
    if glyph_metrics.width == 0 || glyph_metrics.height == 0 {
        return;
    }
    // `ymin` is the bitmap's bottom edge relative to the baseline in y-down
    // screen coordinates, so the top row sits at `baseline + ymin - height`.
    let left = pen_x as i64 + glyph_metrics.xmin as i64;
    let top = baseline as i64 + glyph_metrics.ymin as i64 - glyph_metrics.height as i64;
    for row in 0..glyph_metrics.height {
        for column in 0..glyph_metrics.width {
            let alpha = coverage[row * glyph_metrics.width + column] as f32 / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            let x = left + column as i64;
            let y = top + row as i64;
            if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                continue;
            }
            let pixel = (y as usize * width as usize + x as usize) * 4;
            let existing = rgba[pixel + 3] as f32 / 255.0;
            rgba[pixel + 3] = ((existing + alpha * (1.0 - existing)) * 255.0) as u8;
            // Ink color, composited over the bubble fill.
            rgba[pixel] = (rgba[pixel] as f32 * (1.0 - alpha) + 24.0 * alpha) as u8;
            rgba[pixel + 1] = (rgba[pixel + 1] as f32 * (1.0 - alpha) + 26.0 * alpha) as u8;
            rgba[pixel + 2] = (rgba[pixel + 2] as f32 * (1.0 - alpha) + 29.0 * alpha) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_is_centered_under_the_body() {
        let body_bottom = 84.0;
        let height = body_bottom + TAIL_HEIGHT;
        let y = body_bottom + TAIL_HEIGHT / 2.0;
        assert!(tail_coverage(0.0, y, body_bottom, height) > 0.99);
        assert_eq!(
            tail_coverage(-4.0, y, body_bottom, height),
            tail_coverage(4.0, y, body_bottom, height)
        );
        assert_eq!(tail_coverage(80.0, y, body_bottom, height), 0.0);
    }
}
