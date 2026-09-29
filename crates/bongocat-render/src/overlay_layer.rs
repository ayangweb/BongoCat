//! Topmost layers the model window draws above the model and the pressed-key imagery.
//!
//! Everything the product already draws arrives through [`RenderSnapshot`], which
//! Cubism fills in and the runtime completes. A layer is the one thing that does
//! not: it is produced outside the real-time render path, changes on its own
//! cadence, and must never be able to stall a frame. So it travels on its own
//! bounded latest-wins channel instead of being squeezed into the frame the
//! runtime publishes at up to 240 Hz.
//!
//! The vocabulary here is deliberately platform-neutral and deliberately knows
//! nothing about plugins. Who produces a layer, what it means and when it
//! changes is decided above this crate; this file only says what a layer *is* and
//! where it sits, so a backend can draw it without a second opinion.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Which point of the model window a layer is pinned to.
///
/// Named after the window rather than the layer, so the same spelling reads the
/// same way in a plugin manifest, in the settings window and in a test.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum OverlayAnchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl OverlayAnchor {
    /// Every anchor, in the order the plugin center lists them.
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::CenterLeft,
        Self::Center,
        Self::CenterRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    /// The wire spelling, which is also the key a manifest uses.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::CenterLeft => "center_left",
            Self::Center => "center",
            Self::CenterRight => "center_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|anchor| anchor.as_str() == value)
    }

    /// The anchor as a fraction of the window box, x from the left and y from the
    /// top. Both are `0.0`, `0.5` or `1.0`, which is what makes a layer follow
    /// the window without knowing its size.
    pub const fn normalized(self) -> [f32; 2] {
        let (x, y) = match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::CenterLeft => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::CenterRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        };
        [x, y]
    }
}

/// Where a layer sits and how big it is, all in fractions of the window box.
///
/// Fractions rather than pixels on purpose. The model window is resizable at
/// runtime and its contents scale with the user's model-window scale setting, so
/// a layer described in pixels would need rewriting every time either changed. A
/// fraction of the box needs neither, and stays inside the box at every size
/// because both dimensions are clamped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayLayerPlacement {
    pub anchor: OverlayAnchor,
    /// Gap from the window box, as a fraction of its width and height.
    pub margin: [f32; 2],
    /// Additional gap along the axis the anchor does not sit on, as a fraction of
    /// the layer's own size. Positive is towards the window center, so a
    /// bottom-left layer can be nudged right without moving it down.
    pub nudge: [f32; 2],
    /// The layer's width as a fraction of the window box.
    pub width_fraction: f32,
    pub opacity: f32,
}

/// Bounds that keep a layer usable rather than merely accepted.
impl OverlayLayerPlacement {
    /// The largest width a layer may claim, so a plugin cannot cover the model
    /// it is supposed to sit beside.
    pub const MAXIMUM_WIDTH_FRACTION: f32 = 0.9;
    /// The largest gap from the window box, in fractions of its size.
    pub const MAXIMUM_MARGIN_FRACTION: f32 = 0.45;
    /// The largest nudge, in fractions of the layer's own size.
    pub const MAXIMUM_NUDGE: f32 = 4.0;

    /// Clamp every field into the range the overlay is able to honour.
    ///
    /// Validation reports a value outside these bounds; this exists so a backend
    /// drawing a layer that reached it some other way still cannot be handed a
    /// non-finite coordinate or a rectangle that has left the window.
    pub fn sanitized(mut self) -> Self {
        self.margin = [
            finite_fraction(self.margin[0], Self::MAXIMUM_MARGIN_FRACTION),
            finite_fraction(self.margin[1], Self::MAXIMUM_MARGIN_FRACTION),
        ];
        self.nudge = [
            finite_bounded(self.nudge[0], -Self::MAXIMUM_NUDGE, Self::MAXIMUM_NUDGE),
            finite_bounded(self.nudge[1], -Self::MAXIMUM_NUDGE, Self::MAXIMUM_NUDGE),
        ];
        self.width_fraction =
            finite_bounded(self.width_fraction, 0.05, Self::MAXIMUM_WIDTH_FRACTION);
        self.opacity = finite_bounded(self.opacity, 0.0, 1.0);
        self
    }
}

fn finite_fraction(value: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, maximum)
    } else {
        0.0
    }
}

fn finite_bounded(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        minimum
    }
}

/// Rasterized pixels for one layer.
///
/// Straight (non-premultiplied) RGBA8, top row first — the same encoding the
/// model's own textures use, so a layer needs no separate blend path and cannot
/// disagree with the drawable's colour contract. The pixels are shared rather
/// than copied because the overlay may not take them for several frames while the
/// producer keeps writing new ones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayLayerRaster {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
    /// Changes whenever the pixels do.
    ///
    /// A backend compares this to decide whether a layer's texture still matches
    /// what it uploaded, so it has to change on *any* pixel difference and never
    /// on a redraw that produced identical pixels. A layer whose content is a
    /// one-second countdown is therefore uploaded once a second, not once a frame.
    pub content: u64,
}

/// Bounds on one rasterized layer, checked before a GPU ever sees it.
pub const MAXIMUM_OVERLAY_LAYER_PIXELS: u32 = 4_194_304;
pub const MAXIMUM_OVERLAY_LAYER_SIDE: u32 = 2048;

impl OverlayLayerRaster {
    /// Whether the raster is a shape a backend can allocate and upload.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAXIMUM_OVERLAY_LAYER_SIDE
            && self.height <= MAXIMUM_OVERLAY_LAYER_SIDE
            && self.width.saturating_mul(self.height) <= MAXIMUM_OVERLAY_LAYER_PIXELS
            && self
                .width
                .checked_mul(self.height)
                .and_then(|total| usize::try_from(total).ok())
                .and_then(|total| total.checked_mul(4))
                .is_some_and(|expected| expected == self.pixels.len())
    }
}

/// One layer, as the model window draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayLayer {
    /// Identifies the layer across content changes.
    ///
    /// Stable for as long as the layer exists, so a backend can keep one texture
    /// and upload into it, and so a pointer can name the layer it landed on
    /// without the pointer and the layer having to agree on an index.
    pub id: u64,
    pub placement: OverlayLayerPlacement,
    pub raster: OverlayLayerRaster,
}

/// A rectangle in normalized device coordinates, y up from the center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    pub min_x: f32,
    pub max_x: f32,
    /// The smaller y, which is the *bottom* edge in NDC.
    pub min_y: f32,
    /// The larger y, which is the *top* edge in NDC.
    pub max_y: f32,
}

impl ClipRect {
    /// Whether a point in NDC falls inside, edges included.
    pub fn contains(self, x: f32, y: f32) -> bool {
        x.is_finite()
            && y.is_finite()
            && x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
    }

    /// The four corners in the order the shared quad layout uses: top-left,
    /// top-right, bottom-right, bottom-left, with the same uv assignment as the
    /// model background quad so a backend reuses that layout verbatim.
    pub const fn vertices(self) -> [Vertex; 4] {
        [
            Vertex {
                position: [self.min_x, self.max_y],
                uv: [0.0, 0.0],
            },
            Vertex {
                position: [self.max_x, self.max_y],
                uv: [1.0, 0.0],
            },
            Vertex {
                position: [self.max_x, self.min_y],
                uv: [1.0, 1.0],
            },
            Vertex {
                position: [self.min_x, self.min_y],
                uv: [0.0, 1.0],
            },
        ]
    }
}

/// Place one layer inside a model window whose drawable is `size` device pixels.
///
/// The layer is described in fractions of the drawable, so the result is the same
/// rectangle at every window size and needs no re-derivation when the window is
/// resized or the model window scale changes. The rectangle is built directly in
/// normalized device coordinates rather than in the model's own space, because a
/// layer is interface chrome: a mirrored model must not mirror the panel beside
/// it.
///
/// `aspect` is the layer's width divided by its height. It is clamped against
/// `1 / 64` because a degenerate aspect would divide by zero here, and a layer
/// that reports one is refused earlier, before a backend could act on it.
pub fn overlay_layer_clip_rect(placement: OverlayLayerPlacement, aspect: f32) -> Option<ClipRect> {
    let aspect = if aspect.is_finite() && aspect > 1.0 / 64.0 {
        aspect
    } else {
        return None;
    };
    let placement = placement.sanitized();
    // NDC spans two units across, so a fraction `f` of the window is `2f - 1`
    // from the center. Doing the whole placement in the 0..=1 window fraction
    // first keeps the anchor arithmetic in one place and the conversion in one
    // line.
    let anchor = placement.anchor.normalized();
    let width = placement.width_fraction;
    let height = width / aspect;
    // The anchor names the window *edge* the layer is pinned to, so the layer's
    // own leading edge sits on it: `anchor * (1 - size)` is 0, the centerd value
    // and `1 - size` for a left, center and right anchor alike. Working in the
    // window's 0..=1 fraction keeps that one expression true for all nine
    // positions, and the conversion to device space stays a single line below.
    //
    // `y` grows downward here, matching the window, so a top edge is the smaller
    // of the two and becomes the *larger* NDC y in [`ClipRect`].
    let left = anchor[0] * (1.0 - width)
        + placement.margin[0] * if anchor[0] <= 0.5 { 1.0 } else { -1.0 }
        + placement.nudge[0] * width;
    let right = left + width;
    let top = anchor[1] * (1.0 - height)
        + placement.margin[1] * if anchor[1] <= 0.5 { 1.0 } else { -1.0 }
        + placement.nudge[1] * height;
    let bottom = top + height;
    Some(ClipRect {
        min_x: to_clip(left),
        max_x: to_clip(right),
        min_y: to_clip_down(bottom),
        max_y: to_clip_down(top),
    })
}

fn to_clip(fraction: f32) -> f32 {
    (fraction * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// The vertical counterpart of [`to_clip`]: NDC counts up from the center while
/// the window's own fraction counts down from the top, so this one is the
/// reflection. Getting it wrong is silent — a layer is still inside the window,
/// just at the opposite edge — which is why the two are separate names rather
/// than one sign argument.
fn to_clip_down(fraction: f32) -> f32 {
    (1.0 - fraction * 2.0).clamp(-1.0, 1.0)
}

/// The bounded channel between a layer producer and the model window.
///
/// Latest-wins, like the frame channel, and for the same reason: a layer that
/// arrives late is a layer nobody looks at, and blocking a producer behind a
/// window that is not presenting would trade a stale panel for a stalled frame.
/// Unlike the frame channel, nothing here is a control-plane message, so there is
/// no reliable case to carve out.
pub struct OverlayLayerProducer {
    slot: Arc<OverlayLayerSlot>,
}

impl OverlayLayerProducer {
    pub fn close(&self) {
        self.slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
    }

    pub fn diagnostics(&self) -> OverlayLayerTransportDiagnostics {
        self.slot.diagnostics()
    }
}

pub struct OverlayLayerConsumer {
    slot: Arc<OverlayLayerSlot>,
}

impl OverlayLayerConsumer {
    /// Take the most recent layer set, or an empty set when a producer has
    /// published nothing yet. A window that is not presenting still drains the
    /// channel, so a producer is never blocked by a hidden overlay.
    pub fn take_latest(&self) -> Vec<OverlayLayer> {
        self.slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .layers
            .take()
            .unwrap_or_default()
    }

    pub fn diagnostics(&self) -> OverlayLayerTransportDiagnostics {
        self.slot.diagnostics()
    }
}

#[derive(Default)]
struct OverlayLayerState {
    layers: Option<Vec<OverlayLayer>>,
    closed: bool,
    published: u64,
    coalesced: u64,
    rejected_after_close: u64,
    unuploadable: u64,
}

#[derive(Default)]
struct OverlayLayerSlot {
    state: Mutex<OverlayLayerState>,
}

impl OverlayLayerSlot {
    fn diagnostics(&self) -> OverlayLayerTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        OverlayLayerTransportDiagnostics {
            pending: u64::from(state.layers.is_some()),
            published: state.published,
            coalesced: state.coalesced,
            rejected_after_close: state.rejected_after_close,
            unuploadable: state.unuploadable,
        }
    }
}

pub fn overlay_layer_channel() -> (OverlayLayerProducer, OverlayLayerConsumer) {
    let slot = Arc::new(OverlayLayerSlot::default());
    (
        OverlayLayerProducer {
            slot: Arc::clone(&slot),
        },
        OverlayLayerConsumer { slot },
    )
}

impl OverlayLayerProducer {
    /// Publish a layer set, dropping and counting any layer a GPU could not take.
    ///
    /// Refusing a layer here rather than in a backend is deliberate: the pixels
    /// came from outside the real-time path, so a malformed one has to be a
    /// dropped, counted layer rather than an error on the frame thread.
    pub fn publish_checked(
        &self,
        layers: Vec<OverlayLayer>,
    ) -> Result<(), OverlayLayerPublishError> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.rejected_after_close = state.rejected_after_close.saturating_add(1);
            return Err(OverlayLayerPublishError::Closed);
        }
        let refused = layers
            .iter()
            .filter(|layer| !layer.raster.is_valid())
            .count() as u64;
        let layers: Vec<OverlayLayer> = layers
            .into_iter()
            .filter(|layer| layer.raster.is_valid())
            .collect();
        state.unuploadable = state.unuploadable.saturating_add(refused);
        if state.layers.replace(layers).is_some() {
            state.coalesced = state.coalesced.saturating_add(1);
        }
        state.published = state.published.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OverlayLayerPublishError {
    #[error("the overlay layer channel is closed")]
    Closed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverlayLayerTransportDiagnostics {
    pub pending: u64,
    pub published: u64,
    pub coalesced: u64,
    pub rejected_after_close: u64,
    pub unuploadable: u64,
}

/// Hands out the layer ids that identify a layer across content changes.
///
/// A producer owns one, so a layer keeps its id when its pixels change and two
/// layers can never collide. Ids are never reused: a backend that kept a texture
/// for a removed layer must not find it already describing a different one.
#[derive(Debug, Default)]
pub struct OverlayLayerIds {
    next: AtomicU64,
}

impl OverlayLayerIds {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    pub fn allocate(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
}

/// One pointer event addressed to the layer it landed on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayLayerPointer {
    pub layer_id: u64,
    /// Position inside the layer, in its own raster's pixels, origin top left.
    pub x: f32,
    pub y: f32,
}

impl OverlayLayerPointer {
    /// Where the pointer landed, as a fraction of the layer with y from the top.
    ///
    /// This is what a layer's own hit test compares against, because a layer
    /// describes its interactive regions in its own coordinates and knows nothing
    /// about the window it was placed in.
    pub fn normalized(self, raster: &OverlayLayerRaster) -> Option<[f32; 2]> {
        if raster.width == 0 || raster.height == 0 {
            return None;
        }
        let x = self.x / raster.width as f32;
        let y = self.y / raster.height as f32;
        (x.is_finite() && y.is_finite()).then_some([x, y])
    }
}

/// Where a press inside a layer goes.
///
/// The layer's producer implements it, and the overlay is handed it at start-up.
/// A trait rather than a channel because the producer and the overlay are
/// separate objects with separate shutdowns: a channel would need a second thread
/// to be read from between them, and this is a single `try_send`-worth of work per
/// click.
///
/// Implementations must not block. The overlay calls this from its frame loop.
pub trait OverlayPressSink: Send + Sync + std::fmt::Debug {
    /// A press at a position inside `layer_id`'s own raster.
    fn press(&self, layer_id: u64, x: f32, y: f32);
}

/// The rectangle a raster occupies in the window, and the inverse mapping a
/// backend needs to turn a window click into a layer-local position.
///
/// Built once per placed layer so a backend computes it the same way on both
/// sides of the click rather than re-deriving the arithmetic in two places.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedOverlayLayer {
    pub layer: OverlayLayer,
    pub rect: ClipRect,
}

impl PlacedOverlayLayer {
    pub fn new(layer: OverlayLayer) -> Option<Self> {
        let raster = &layer.raster;
        if !raster.is_valid() {
            return None;
        }
        let aspect = raster.width as f32 / raster.height as f32;
        let rect = overlay_layer_clip_rect(layer.placement, aspect)?;
        Some(Self { layer, rect })
    }

    /// The topmost layer containing a point in NDC, or `None`.
    ///
    /// Later layers win, which is the same order they are drawn in: a panel that
    /// overlaps another is the one a click belongs to.
    pub fn hit_test(layers: &[Self], x: f32, y: f32) -> Option<&Self> {
        layers
            .iter()
            .rev()
            .find(|placed| placed.layer.placement.opacity > 0.0 && placed.rect.contains(x, y))
    }

    /// The position of a point in NDC inside this layer, in raster pixels.
    pub fn to_layer_pixels(&self, x: f32, y: f32) -> Option<OverlayLayerPointer> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let width = self.layer.raster.width as f32;
        let height = self.layer.raster.height as f32;
        // NDC y points up while a raster's first row is at the top, which is why
        // the y axis is measured downward from the top edge.
        let u = (x - self.rect.min_x) / (self.rect.max_x - self.rect.min_x);
        let v = (self.rect.max_y - y) / (self.rect.max_y - self.rect.min_y);
        Some(OverlayLayerPointer {
            layer_id: self.layer.id,
            x: (u * width).clamp(0.0, width),
            y: (v * height).clamp(0.0, height),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(width: u32, height: u32, content: u64) -> OverlayLayerRaster {
        let len = width as usize * height as usize * 4;
        OverlayLayerRaster {
            width,
            height,
            pixels: Arc::from(vec![0u8; len].into_boxed_slice()),
            content,
        }
    }

    fn placement(anchor: OverlayAnchor) -> OverlayLayerPlacement {
        OverlayLayerPlacement {
            anchor,
            margin: [0.0, 0.0],
            nudge: [0.0, 0.0],
            width_fraction: 0.5,
            opacity: 1.0,
        }
    }

    #[test]
    fn anchor_spelling_round_trips() {
        for anchor in OverlayAnchor::ALL {
            assert_eq!(OverlayAnchor::parse(anchor.as_str()), Some(anchor));
        }
        assert_eq!(OverlayAnchor::parse("middle"), None);
    }

    #[test]
    fn anchor_normalization_covers_the_nine_positions() {
        assert_eq!(OverlayAnchor::TopLeft.normalized(), [0.0, 0.0]);
        assert_eq!(OverlayAnchor::Center.normalized(), [0.5, 0.5]);
        assert_eq!(OverlayAnchor::BottomRight.normalized(), [1.0, 1.0]);
    }

    #[test]
    fn a_top_left_layer_occupies_the_upper_left_quarter() {
        let rect = overlay_layer_clip_rect(placement(OverlayAnchor::TopLeft), 1.0).unwrap();
        assert!((rect.min_x - -1.0).abs() < f32::EPSILON);
        assert!((rect.max_x - 0.0).abs() < f32::EPSILON);
        assert!((rect.max_y - 1.0).abs() < f32::EPSILON);
        assert!((rect.min_y - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_bottom_right_layer_is_the_diagonal_opposite() {
        let rect = overlay_layer_clip_rect(placement(OverlayAnchor::BottomRight), 1.0).unwrap();
        assert!((rect.min_x - 0.0).abs() < f32::EPSILON);
        assert!((rect.max_x - 1.0).abs() < f32::EPSILON);
        assert!((rect.max_y - 0.0).abs() < f32::EPSILON);
        assert!((rect.min_y - -1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn margin_pushes_a_layer_inwards_from_the_edges_it_is_pinned_to() {
        let mut placement = placement(OverlayAnchor::BottomLeft);
        placement.margin = [0.1, 0.1];
        let rect = overlay_layer_clip_rect(placement, 1.0).unwrap();
        assert!(
            rect.min_x > -1.0,
            "a left margin must move the left edge in"
        );
        assert!(
            rect.min_y > -1.0,
            "a bottom margin must move the bottom edge in"
        );
    }

    #[test]
    fn a_wide_layer_is_shorter_than_a_square_one() {
        let wide = overlay_layer_clip_rect(placement(OverlayAnchor::Center), 2.0).unwrap();
        let square = overlay_layer_clip_rect(placement(OverlayAnchor::Center), 1.0).unwrap();
        let height = |rect: ClipRect| rect.max_y - rect.min_y;
        assert!(height(wide) < height(square));
    }

    #[test]
    fn a_degenerate_aspect_is_refused_rather_than_divided_by() {
        assert!(overlay_layer_clip_rect(placement(OverlayAnchor::Center), 0.0).is_none());
        assert!(overlay_layer_clip_rect(placement(OverlayAnchor::Center), f32::NAN).is_none());
    }

    #[test]
    fn non_finite_placement_fields_are_sanitized_not_propagated() {
        let placement = OverlayLayerPlacement {
            anchor: OverlayAnchor::Center,
            margin: [f32::NAN, f32::INFINITY],
            nudge: [f32::NAN, 100.0],
            width_fraction: f32::NAN,
            opacity: 2.0,
        };
        let rect = overlay_layer_clip_rect(placement, 1.0).unwrap();
        assert!(rect.min_x.is_finite() && rect.max_x.is_finite());
        assert!(rect.min_y.is_finite() && rect.max_y.is_finite());
        assert_eq!(placement.sanitized().opacity, 1.0);
    }

    #[test]
    fn quad_uvs_match_the_model_background_layout() {
        let rect = overlay_layer_clip_rect(placement(OverlayAnchor::TopLeft), 1.0).unwrap();
        let vertices = rect.vertices();
        assert_eq!(vertices[0].uv, [0.0, 0.0]);
        assert_eq!(vertices[1].uv, [1.0, 0.0]);
        assert_eq!(vertices[2].uv, [1.0, 1.0]);
        assert_eq!(vertices[3].uv, [0.0, 1.0]);
    }

    #[test]
    fn a_pointer_maps_back_to_the_pixel_it_landed_on() {
        let placed = PlacedOverlayLayer::new(OverlayLayer {
            id: 7,
            placement: placement(OverlayAnchor::TopLeft),
            raster: raster(200, 100, 1),
        })
        .unwrap();
        // The top-left corner of this layer is the window's top-left corner, so a
        // click there is pixel (0, 0) and one on its far edge is (200, 100).
        let corner = placed.to_layer_pixels(-1.0, 1.0).unwrap();
        assert_eq!(corner.layer_id, 7);
        assert!((corner.x - 0.0).abs() < 0.001);
        assert!((corner.y - 0.0).abs() < 0.001);
        let center = placed.to_layer_pixels(-0.5, 0.75).unwrap();
        assert!((center.x - 100.0).abs() < 0.001);
        assert!((center.y - 50.0).abs() < 0.001);
        assert!(placed.to_layer_pixels(0.9, 0.9).is_none());
    }

    #[test]
    fn the_topmost_layer_wins_a_hit() {
        let under = PlacedOverlayLayer::new(OverlayLayer {
            id: 1,
            placement: placement(OverlayAnchor::Center),
            raster: raster(10, 10, 1),
        })
        .unwrap();
        let mut over_placement = placement(OverlayAnchor::Center);
        over_placement.width_fraction = 0.2;
        let over = PlacedOverlayLayer::new(OverlayLayer {
            id: 2,
            placement: over_placement,
            raster: raster(10, 10, 1),
        })
        .unwrap();
        let layers = [under, over];
        let hit = PlacedOverlayLayer::hit_test(&layers, 0.0, 0.0).unwrap();
        assert_eq!(hit.layer.id, 2);
    }

    #[test]
    fn a_fully_transparent_layer_is_not_hit() {
        let mut invisible = placement(OverlayAnchor::Center);
        invisible.opacity = 0.0;
        let layer = PlacedOverlayLayer::new(OverlayLayer {
            id: 1,
            placement: invisible,
            raster: raster(10, 10, 1),
        })
        .unwrap();
        assert!(PlacedOverlayLayer::hit_test(&[layer], 0.0, 0.0).is_none());
    }

    #[test]
    fn an_oversized_raster_is_refused_before_placement() {
        let layer = OverlayLayer {
            id: 1,
            placement: placement(OverlayAnchor::Center),
            raster: raster(4096, 4096, 1),
        };
        assert!(PlacedOverlayLayer::new(layer).is_none());
    }

    #[test]
    fn a_raster_whose_length_disagrees_with_its_size_is_refused() {
        let raster = OverlayLayerRaster {
            width: 4,
            height: 4,
            pixels: Arc::from([0u8; 8].as_slice()),
            content: 1,
        };
        assert!(!raster.is_valid());
    }

    #[test]
    fn publishing_coalesces_and_counts_refused_layers() {
        let (producer, consumer) = overlay_layer_channel();
        let good = OverlayLayer {
            id: 1,
            placement: placement(OverlayAnchor::Center),
            raster: raster(4, 4, 1),
        };
        let mut bad = good.clone();
        bad.id = 2;
        bad.raster = raster(4, 4, 0);
        bad.raster.pixels = Arc::from(Vec::new().into_boxed_slice());

        producer.publish_checked(vec![good.clone(), bad]).unwrap();
        producer.publish_checked(vec![good]).unwrap();
        let diagnostics = producer.diagnostics();
        assert_eq!(diagnostics.published, 2);
        assert_eq!(diagnostics.coalesced, 1);
        assert_eq!(diagnostics.unuploadable, 1);
        assert_eq!(consumer.take_latest().len(), 1);
        assert!(consumer.take_latest().is_empty());
    }

    #[test]
    fn publishing_after_close_is_refused_and_counted() {
        let (producer, _consumer) = overlay_layer_channel();
        producer.close();
        assert!(producer.publish_checked(Vec::new()).is_err());
        assert_eq!(producer.diagnostics().rejected_after_close, 1);
    }

    #[test]
    fn layer_ids_are_unique_and_never_repeat() {
        let ids = OverlayLayerIds::new();
        let first = ids.allocate();
        let second = ids.allocate();
        assert_ne!(first, second);
        assert_ne!(first, 0, "zero is what an absent layer reads as");
    }

    #[test]
    fn a_normalized_pointer_is_relative_to_the_raster() {
        let raster = raster(100, 50, 1);
        let pointer = OverlayLayerPointer {
            layer_id: 1,
            x: 25.0,
            y: 10.0,
        };
        let normalized = pointer.normalized(&raster).unwrap();
        assert!((normalized[0] - 0.25).abs() < f32::EPSILON);
        assert!((normalized[1] - 0.2).abs() < f32::EPSILON);
    }
}
