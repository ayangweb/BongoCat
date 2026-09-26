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

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}
