//! Where the model is and how big it is.
//!
//! Cubism's origin is at the top left and its y axis points down, which is the
//! opposite of what most of Rust assumes. The conversion happens here, once, so
//! every consumer above works in the orientation it was written for rather than
//! each one re-deriving the flip.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInfo {
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub pixels_per_unit: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

impl ModelBounds {
    pub const fn from_canvas(canvas: CanvasInfo) -> Self {
        let half_width = canvas.width / canvas.pixels_per_unit * 0.5;
        let half_height = canvas.height / canvas.pixels_per_unit * 0.5;
        let center_x = (canvas.width * 0.5 - canvas.origin_x) / canvas.pixels_per_unit;
        let center_y = (canvas.origin_y - canvas.height * 0.5) / canvas.pixels_per_unit;
        Self {
            min_x: center_x - half_width,
            max_x: center_x + half_width,
            min_y: center_y - half_height,
            max_y: center_y + half_height,
        }
    }

    pub fn width(self) -> f32 {
        (self.max_x - self.min_x).max(f32::EPSILON)
    }

    pub fn height(self) -> f32 {
        (self.max_y - self.min_y).max(f32::EPSILON)
    }

    pub fn center(self) -> [f32; 2] {
        [
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        ]
    }
}

/// The horizontal margin, in key-image pixels, between the left edge of a
/// BongoCatMver window and the model canvas drawn inside it.
///
/// The legacy application draws a key image at its window's top-left corner, one
/// image pixel per window pixel, and draws the Live2D canvas centred in the same
/// window. A key image is exactly as large as the canvas is inside that window —
/// it is authored that way — so the canvas starts `(window_width - image_width) /
/// 2` pixels to the right of the image, and that margin is what a converted
/// package carries with it.
///
/// The window is never narrower than the canvas it shows, and a model author who
/// does not size the window to the canvas uses the square that contains it, so
/// the margin is `(image_height - image_width) / 2` for an image taller than it
/// is wide and zero otherwise: an image at least as wide as it is tall already
/// sits in a window that hugs it, which is the case the shipped models are in.
pub const fn legacy_key_frame_margin(image_width: u32, image_height: u32) -> u32 {
    if image_width == 0 || image_width >= image_height {
        return 0;
    }
    (image_height - image_width) / 2
}

/// The quad a legacy key image must be drawn on, in `bounds`' own model space.
///
/// [`ModelBounds::from_canvas`] is the quad the product draws the model on, and a
/// key image authored for BongoCatMver is offset from that quad by
/// [`legacy_key_frame_margin`] — one image pixel is one legacy window pixel, and
/// the canvas spans the image's width, so the margin converts to model units by
/// the canvas' own width per image pixel. An image that needs no margin is
/// returned as the canvas quad itself, which is what every model that already
/// ships its key images in the product's own frame gets.
pub fn legacy_key_overlay_bounds(
    bounds: ModelBounds,
    image_width: u32,
    image_height: u32,
) -> ModelBounds {
    let margin = legacy_key_frame_margin(image_width, image_height);
    if margin == 0 {
        return bounds;
    }
    let shift = margin as f32 * bounds.width() / image_width as f32;
    ModelBounds {
        min_x: bounds.min_x - shift,
        max_x: bounds.max_x - shift,
        ..bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

/// The four corners of one textured quad, wound as the two triangles the shared
/// index buffer names.
///
/// The product draws the model's background and every key overlay as a quad in
/// model space, and the only thing that differs between them is which bounds
/// that quad covers. Building the vertices in one place keeps the winding and
/// the uv orientation identical for all of them.
pub const fn quad_vertices(bounds: ModelBounds) -> [Vertex; 4] {
    [
        Vertex {
            position: [bounds.min_x, bounds.min_y],
            uv: [0.0, 0.0],
        },
        Vertex {
            position: [bounds.max_x, bounds.min_y],
            uv: [1.0, 0.0],
        },
        Vertex {
            position: [bounds.max_x, bounds.max_y],
            uv: [1.0, 1.0],
        },
        Vertex {
            position: [bounds.min_x, bounds.max_y],
            uv: [0.0, 1.0],
        },
    ]
}
