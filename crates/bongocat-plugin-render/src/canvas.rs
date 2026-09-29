//! The raster a panel is drawn into.
//!
//! A straight-alpha RGBA8 buffer with compositing done by hand. It has to be
//! straight rather than premultiplied because that is what `bongocat-render`'s
//! layer raster declares and what both backends' shaders expect to sample — a
//! premultiplied texture sampled by a shader that premultiplies again would
//! darken every edge.
//!
//! Every primitive is antialiased. The model window is small, a panel's rounded
//! corners are at most a few pixels, and a hard edge there reads as a rendering
//! bug rather than as a design choice. Coverage is therefore computed per pixel
//! from the distance to the shape's edge, which costs a square root per pixel and
//! nothing else.

use crate::error::{PluginRenderError, PluginRenderErrorCode};
use crate::font::FontBook;
use crate::{Color, DecodedImage, TextLine};
use ab_glyph::{Font, Glyph, PxScale, ScaleFont};
use std::path::Path;

/// A rectangle, and the rounded corners it may have.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RoundedRect {
    /// Whether a point is inside, edges included.
    ///
    /// Edges count as inside deliberately: a press that lands exactly on a
    /// button's boundary belongs to the button, and at a model window's usual
    /// size the difference between "on the edge" and "one pixel inside" is the
    /// difference between a button that works and one that does not.
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    pub fn is_empty(self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    pub fn inset(self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }
}

/// One straight RGBA8 raster.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    width: u32,
    height: u32,
    scale: f32,
    pixels: Vec<u8>,
}

impl Canvas {
    /// A transparent raster of `width` by `height` logical pixels at `scale`
    /// device pixels per logical pixel.
    pub fn new(width: u32, height: u32, scale: f32) -> Result<Self, PluginRenderError> {
        let device_width = device_pixels(width, scale);
        let device_height = device_pixels(height, scale);
        if device_width == 0 || device_height == 0 {
            return Err(PluginRenderError::bare(
                PluginRenderErrorCode::RasterTooLarge,
            ));
        }
        // The same bound `bongocat-render` enforces on a layer, checked here so
        // the failure is a render error with a reason rather than a layer the
        // overlay silently drops.
        if u64::from(device_width) * u64::from(device_height)
            > u64::from(bongocat_render::MAXIMUM_OVERLAY_LAYER_PIXELS)
        {
            return Err(PluginRenderError::new(
                PluginRenderErrorCode::RasterTooLarge,
                format!("{device_width}x{device_height}"),
            ));
        }
        let length = device_width as usize * device_height as usize * 4;
        Ok(Self {
            width: device_width,
            height: device_height,
            scale,
            pixels: vec![0u8; length],
        })
    }

    /// The raster's device size.
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The raster's pixels, straight RGBA8, top row first.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// Fill a rounded rectangle with a straight-alpha color.
    ///
    /// `coverage` and the colour's alpha multiply, so a shape drawn at half alpha
    /// composites at half alpha exactly once rather than twice.
    pub fn fill_rect(&mut self, rect: RoundedRect, radius: f32, color: Color) {
        self.fill_rect_with(rect, radius, color, 1.0);
    }

    /// Fill a rounded rectangle at a partial coverage, for an antialiased edge.
    pub fn fill_rect_partial(
        &mut self,
        rect: RoundedRect,
        radius: f32,
        color: Color,
        coverage: f32,
    ) {
        self.fill_rect_with(rect, radius, color, coverage);
    }

    /// Fill a rounded rectangle's border, drawn *inside* its own edge.
    ///
    /// Inward, not straddling: `rect` is the box the node occupies, and a border
    /// that grew outward would be drawn over whatever is next to the node — over
    /// the model's own pixels at the panel's edge. Drawn as one ring with the
    /// interior punched out rather than as four rectangles, because four
    /// rectangles leave seams at the corners where their coverage does not sum
    /// to one.
    pub fn stroke_rect(&mut self, rect: RoundedRect, radius: f32, border_width: f32, color: Color) {
        if border_width <= 0.0 || color.alpha == 0 || rect.is_empty() {
            return;
        }
        let width = border_width.min(rect.width * 0.5).min(rect.height * 0.5);
        let inner = rect.inset(width);
        // The inner corner radius is the outer one less the width, which keeps
        // the border the same thickness all the way round; clamping at zero means
        // a very round shape with a wide border degenerates to a filled box rather
        // than to a ring with a negative radius.
        self.fill_even_odd(rect, radius, inner, (radius - width).max(0.0), color);
    }

    /// Draw text with its baseline at `(x, y)`, in logical pixels.
    ///
    /// The glyphs come from a measurement in logical pixels, so the outline is
    /// rasterized here at the canvas's own device scale. Doing it at draw time
    /// rather than at measure time is what lets one cached measurement serve
    /// every display scale — and it is why the face has to be reachable from the
    /// canvas rather than only from the measurer.
    pub fn draw_text(
        &mut self,
        book: &FontBook,
        line: &TextLine,
        x: f32,
        baseline: f32,
        color: Color,
    ) {
        let device_size = line.size * self.scale;
        if !device_size.is_finite() || device_size <= 0.0 {
            return;
        }
        let Some(face) = book.face(line.weight) else {
            return;
        };
        let scaled = face.as_scaled(PxScale::from(device_size));
        let factor = self.scale;
        for positioned in &line.glyphs {
            // The outline is placed at the origin and moved by its own bounds when
            // the coverage arrives, rather than by a `Glyph::position` offset.
            // Doing it the other way round would need the position in the
            // rasterizer's own units, which is where the two coordinate systems
            // get confused.
            let glyph = Glyph {
                id: positioned.id,
                scale: PxScale::from(device_size),
                position: ab_glyph::point(0.0, 0.0),
            };
            let Some(outlined) = scaled.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outlined.px_bounds();
            // The pen, in device pixels: the measurement is in logical pixels.
            let pen_x = (x + positioned.x) * factor;
            let pen_y = baseline * factor;
            // `draw` reports pixels of the outline's own box, indexed from its
            // top-left, with row 0 at the top. A glyph's box therefore hangs
            // *below* the baseline: its last row is the baseline and its first is
            // one box-height above it. The x offset is the outline's left side
            // bearing, which is signed and may be negative for a glyph that
            // overhangs to the left of its origin.
            let box_height = bounds.height();
            outlined.draw(|pixel_x, pixel_y, coverage| {
                let device_x = pen_x + pixel_x as f32 - bounds.min.x;
                let device_y = pen_y + pixel_y as f32 - box_height;
                self.blend_glyph_coverage(device_x, device_y, color, coverage);
            });
        }
    }

    /// Draw a decoded image into a box, preserving its aspect ratio when asked.
    pub fn draw_image(&mut self, image: &DecodedImage, rect: RoundedRect, preserve_aspect: bool) {
        if rect.is_empty() || image.width == 0 || image.height == 0 {
            return;
        }
        let device_rect = RoundedRect {
            x: rect.x * self.scale,
            y: rect.y * self.scale,
            width: rect.width * self.scale,
            height: rect.height * self.scale,
        };
        let (target, offset) = if preserve_aspect {
            let image_aspect = image.width as f32 / image.height as f32;
            let box_aspect = rect.width / rect.height;
            let (width, height) = if image_aspect > box_aspect {
                (rect.width, rect.width / image_aspect)
            } else {
                (rect.height * image_aspect, rect.height)
            };
            (
                RoundedRect {
                    width,
                    height,
                    ..device_rect
                },
                (
                    device_rect.x + (device_rect.width - width) * 0.5,
                    device_rect.y + (device_rect.height - height) * 0.5,
                ),
            )
        } else {
            (device_rect, (device_rect.x, device_rect.y))
        };
        // The image is resampled in *device* pixels, so a source pixel covers
        // `scale_x` by `scale_y` of them. Computing the ratio in logical pixels
        // and then multiplying by the canvas scale — the obvious form — is the
        // same number times the scale twice, and a 2x panel draws a 4x image.
        let scale_x = (target.width / image.width as f32).max(f32::MIN_POSITIVE);
        let scale_y = (target.height / image.height as f32).max(f32::MIN_POSITIVE);
        let left = offset.0.round() as i64;
        let top = offset.1.round() as i64;
        for row in 0..image.height {
            let destination_y = top + (row as f32 * scale_y).round() as i64;
            if destination_y < 0 || destination_y >= i64::from(self.height) {
                continue;
            }
            for column in 0..image.width {
                let destination_x = left + (column as f32 * scale_x).round() as i64;
                if destination_x < 0 || destination_x >= i64::from(self.width) {
                    continue;
                }
                let source = (row as usize * image.width as usize + column as usize) * 4;
                let color = crate::Color {
                    red: image.pixels[source],
                    green: image.pixels[source + 1],
                    blue: image.pixels[source + 2],
                    alpha: image.pixels[source + 3],
                };
                if color.alpha == 0 {
                    continue;
                }
                self.blend_pixel(destination_x as u32, destination_y as u32, color);
            }
        }
    }

    /// Stroke an arc from `start` to `sweep` radians, centerd on a point.
    ///
    /// Drawn as a fan of short quads, one antialiased pass each. A dedicated arc
    /// rasterizer would be faster and would produce a slightly different edge;
    /// this is a ring at most a few dozen logical pixels across, drawn at most
    /// once a second, and the visible result is the same.
    ///
    /// Angles run clockwise from the top in a y-down space, so a ring at zero
    /// sweep shows nothing rather than a full circle with a one-pixel gap.
    pub fn stroke_arc(
        &mut self,
        center: (f32, f32),
        radius: f32,
        thickness: f32,
        start: f32,
        sweep: f32,
        color: Color,
    ) {
        if radius <= 0.0 || thickness <= 0.0 || color.alpha == 0 || sweep == 0.0 {
            return;
        }
        let inner = (radius - thickness).max(0.0);
        // One segment every few degrees is enough that no facet is visible at a
        // ring this size, and the bound stops a huge radius from producing
        // millions of passes.
        let steps = ((sweep.abs() / 0.05).ceil() as usize).clamp(1, 512);
        for step in 0..steps {
            let from = start + sweep * step as f32 / steps as f32;
            let to = start + sweep * (step + 1) as f32 / steps as f32;
            let points = [
                (
                    center.0 + radius * from.cos(),
                    center.1 - radius * from.sin(),
                ),
                (center.0 + radius * to.cos(), center.1 - radius * to.sin()),
                (center.0 + inner * to.cos(), center.1 - inner * to.sin()),
                (center.0 + inner * from.cos(), center.1 - inner * from.sin()),
            ];
            self.fill_quad(&points, color);
        }
    }

    /// Fill a convex quadrilateral with an antialiased edge on all four sides.
    ///
    /// Separate from the general polygon because a quad has a cheap exact
    /// coverage answer: a pixel's coverage is the fraction of it inside the shape,
    /// computed by clamping along each axis. The general case needs a scanline
    /// crossing count, and the only caller of the general case is a path this
    /// crate no longer draws.
    fn fill_quad(&mut self, points: &[(f32, f32)], color: Color) {
        let min_x = points.iter().map(|point| point.0).fold(f32::MAX, f32::min);
        let max_x = points.iter().map(|point| point.0).fold(f32::MIN, f32::max);
        let min_y = points.iter().map(|point| point.1).fold(f32::MAX, f32::min);
        let max_y = points.iter().map(|point| point.1).fold(f32::MIN, f32::max);
        if !(max_x > min_x && max_y > min_y) {
            return;
        }
        let left = ((min_x * self.scale).floor() as i64).max(0);
        let top = ((min_y * self.scale).floor() as i64).max(0);
        let right = ((max_x * self.scale).ceil() as i64).min(i64::from(self.width));
        let bottom = ((max_y * self.scale).ceil() as i64).min(i64::from(self.height));
        for y in top..bottom {
            for x in left..right {
                let px = (x as f32 + 0.5) / self.scale;
                let py = (y as f32 + 0.5) / self.scale;
                // The signed distance to the nearest edge, positive inside. The
                // band is one device pixel wide either side of the boundary, which
                // is what makes a thin shape a thin shape rather than a shape
                // with a one-pixel fringe and a hollow middle: a pixel more than
                // half a pixel inside is fully covered, and a pixel further out
                // than that is not covered at all.
                let distance = points
                    .iter()
                    .zip(points.iter().cycle().skip(1))
                    .map(|(from, to)| distance_to_segment(px, py, *from, *to))
                    .fold(f32::MAX, f32::min);
                let signed = if point_in_quad(px, py, points) {
                    distance
                } else {
                    -distance
                };
                let coverage = (0.5 + signed * self.scale).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend_pixel_coverage(x as u32, y as u32, color, coverage);
                }
            }
        }
    }

    /// Whether every pixel is fully transparent.
    ///
    /// Used to skip publishing a layer for a panel that drew nothing, which
    /// happens when a scene's every node is invisible — a case a manifest cannot
    /// detect and a render pass can.
    pub fn is_fully_transparent(&self) -> bool {
        self.pixels.chunks_exact(4).all(|pixel| pixel[3] == 0)
    }

    fn fill_rect_with(&mut self, rect: RoundedRect, radius: f32, color: Color, coverage: f32) {
        if color.alpha == 0 || rect.is_empty() {
            return;
        }
        let radius = radius.min(rect.width * 0.5).min(rect.height * 0.5);
        if radius <= 0.0 {
            self.fill_bounds(rect, color, coverage);
            return;
        }
        let left = ((rect.x * self.scale).floor() as i64).max(0);
        let top = ((rect.y * self.scale).floor() as i64).max(0);
        let right = (((rect.x + rect.width) * self.scale).ceil() as i64).min(i64::from(self.width));
        let bottom =
            (((rect.y + rect.height) * self.scale).ceil() as i64).min(i64::from(self.height));
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                // `rounded_rect_distance` is a signed distance: negative inside
                // the shape, positive outside, so it is already what
                // `rounded_coverage` expects.
                let distance =
                    rounded_rect_distance(px / self.scale, py / self.scale, rect, radius);
                let edge = rounded_coverage(distance, self.scale);
                if edge <= 0.0 {
                    continue;
                }
                self.blend_pixel_coverage(
                    x as u32,
                    y as u32,
                    color,
                    edge * coverage.clamp(0.0, 1.0),
                );
            }
        }
    }

    fn fill_even_odd(
        &mut self,
        outer: RoundedRect,
        outer_radius: f32,
        inner: RoundedRect,
        inner_radius: f32,
        color: Color,
    ) {
        if color.alpha == 0 {
            return;
        }
        let left = ((outer.x * self.scale).floor() as i64).max(0);
        let top = ((outer.y * self.scale).floor() as i64).max(0);
        let right =
            (((outer.x + outer.width) * self.scale).ceil() as i64).min(i64::from(self.width));
        let bottom =
            (((outer.y + outer.height) * self.scale).ceil() as i64).min(i64::from(self.height));
        let outer_radius = outer_radius.min(outer.width * 0.5).min(outer.height * 0.5);
        let inner_radius = inner_radius.min(inner.width * 0.5).min(inner.height * 0.5);
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let outside = rounded_coverage(
                    rounded_rect_distance(px / self.scale, py / self.scale, outer, outer_radius),
                    self.scale,
                );
                let inside = rounded_coverage(
                    rounded_rect_distance(px / self.scale, py / self.scale, inner, inner_radius),
                    self.scale,
                );
                // A border is the ring between the two shapes: inside the outer
                // one and outside the inner one.
                let coverage = outside * (1.0 - inside);
                if coverage <= 0.0 {
                    continue;
                }
                self.blend_pixel_coverage(x as u32, y as u32, color, coverage);
            }
        }
    }

    fn fill_bounds(&mut self, rect: RoundedRect, color: crate::Color, coverage: f32) {
        let left = ((rect.x * self.scale).floor() as i64).max(0);
        let top = ((rect.y * self.scale).floor() as i64).max(0);
        let right = (((rect.x + rect.width) * self.scale).ceil() as i64).min(i64::from(self.width));
        let bottom =
            (((rect.y + rect.height) * self.scale).ceil() as i64).min(i64::from(self.height));
        let coverage = coverage.clamp(0.0, 1.0);
        for y in top..bottom {
            for x in left..right {
                self.blend_pixel_coverage(x as u32, y as u32, color, coverage);
            }
        }
    }

    fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        self.blend_pixel_coverage(x, y, color, 1.0);
    }

    /// Composite a glyph's antialiased coverage, given in device pixels.
    ///
    /// Coverage from the rasterizer is already the fraction of the pixel the
    /// glyph covers, so it multiplies the colour's alpha directly. Clamping the
    /// coordinate first is what keeps an off-canvas glyph — a label wider than
    /// its node, which a panel is allowed to declare — from wrapping into a row
    /// it does not belong to.
    fn blend_glyph_coverage(&mut self, device_x: f32, device_y: f32, color: Color, coverage: f32) {
        if !device_x.is_finite() || !device_y.is_finite() {
            return;
        }
        let x = device_x.floor();
        let y = device_y.floor();
        if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
            return;
        }
        self.blend_pixel_coverage(x as u32, y as u32, color, coverage);
    }

    /// Composite one straight-alpha source pixel over the destination.
    fn blend_pixel_coverage(&mut self, x: u32, y: u32, color: crate::Color, coverage: f32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let alpha = f32::from(color.alpha) / 255.0 * coverage.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        let destination_alpha = f32::from(self.pixels[index + 3]) / 255.0;
        // Standard straight-alpha "over". The destination's colour is not
        // divided out first, so a translucent source over a translucent
        // destination darkens slightly; that is the standard approximation and
        // the difference is invisible at these alphas, whereas the correct
        // formula needs a division that a fully transparent destination makes
        // undefined.
        let out_alpha = alpha + destination_alpha * (1.0 - alpha);
        if out_alpha <= 0.0 {
            self.pixels[index] = 0;
            self.pixels[index + 1] = 0;
            self.pixels[index + 2] = 0;
            self.pixels[index + 3] = 0;
            return;
        }
        let mix = |source: u8, destination: u8| {
            let destination = f32::from(destination) / 255.0;
            let out = (f32::from(source) / 255.0 * alpha + destination * (1.0 - alpha)) / out_alpha;
            (out * 255.0).round().clamp(0.0, 255.0) as u8
        };
        self.pixels[index] = mix(color.red, self.pixels[index]);
        self.pixels[index + 1] = mix(color.green, self.pixels[index + 1]);
        self.pixels[index + 2] = mix(color.blue, self.pixels[index + 2]);
        self.pixels[index + 3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

/// Whether a point is inside a convex quad given in order.
fn point_in_quad(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut positive = false;
    let mut negative = false;
    for index in 0..points.len() {
        let from = points[index];
        let to = points[(index + 1) % points.len()];
        let cross = (to.0 - from.0) * (y - from.1) - (to.1 - from.1) * (x - from.0);
        if cross > 0.0 {
            positive = true;
        } else if cross < 0.0 {
            negative = true;
        }
    }
    !(positive && negative)
}

/// The distance from a point to a line segment.
fn distance_to_segment(x: f32, y: f32, from: (f32, f32), to: (f32, f32)) -> f32 {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((x - from.0).powi(2) + (y - from.1).powi(2)).sqrt();
    }
    let t = (((x - from.0) * dx + (y - from.1) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest = (from.0 + t * dx, from.1 + t * dy);
    ((x - nearest.0).powi(2) + (y - nearest.1).powi(2)).sqrt()
}

fn device_pixels(logical: u32, scale: f32) -> u32 {
    if !scale.is_finite() || scale <= 0.0 || logical == 0 {
        return 0;
    }
    (f64::from(logical) * f64::from(scale))
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The signed distance from a point to a rounded rectangle's edge.
///
/// Negative inside, positive outside, and measured in logical pixels so the
/// antialiasing band is one device pixel wide whatever the scale.
///
/// The `- radius` term is not optional. `max(q, 0).length()` measures the distance
/// to the *core* box — the shape inset by the corner radius — so without it every
/// point inside that core box would report a distance of about zero, and a
/// rounded box would have no interior at all: the fill would be exactly the shape
/// of its four corner arcs and nothing between them.
fn rounded_rect_distance(x: f32, y: f32, rect: RoundedRect, radius: f32) -> f32 {
    let half_width = rect.width * 0.5;
    let half_height = rect.height * 0.5;
    let center_x = rect.x + half_width;
    let center_y = rect.y + half_height;
    let dx = (x - center_x).abs() - (half_width - radius);
    let dy = (y - center_y).abs() - (half_height - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    outside + inside - radius
}

/// Coverage at a signed distance, one device pixel wide.
fn rounded_coverage(distance: f32, scale: f32) -> f32 {
    (0.5 - distance * scale).clamp(0.0, 1.0)
}

impl crate::DecodedImage {
    /// Read a PNG from a path, for a caller that has one.
    pub fn from_path(path: &Path) -> Result<Self, PluginRenderError> {
        Self::read_png(path)
    }
}
