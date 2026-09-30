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

use bongocat_plugin_protocol::{Color, SceneNode};
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

/// Rasterize one panel from the update a plugin sent.
///
/// This is the entry point the product uses: it takes the placement and the scene
/// exactly as the plugin declared them, so nothing about a panel's appearance
/// passes through this crate's judgement. The placement is *sanitized* rather than
/// trusted because it came from another process — but the protocol already
/// validated it, so this is the second line rather than the first.
pub fn render_update(
    update: &bongocat_plugin_protocol::PanelUpdate,
    scale: f32,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
) -> Result<RenderedPanel, PluginRenderError> {
    let placement = update.placement.sanitized();
    let panel = render_panel(&update.scene, placement.size, scale, measurer, images)?;
    Ok(RenderedPanel {
        anchor: placement.anchor.to_overlay_anchor(),
        margin: placement.margin,
        width_fraction: placement.width_fraction,
        opacity: placement.opacity,
        ..panel
    })
}

/// Rasterize a bare scene into a fixed-size panel.
///
/// `scale` is device pixels per logical pixel; see [`MINIMUM_RASTER_SCALE`].
/// `images` supplies the plugin's own PNGs by the relative path its scene named,
/// so this function performs no file access and is fully testable from a map.
///
/// A scene carries concrete values rather than bindings, so there is nothing to
/// resolve here: the plugin computed every number before it sent the tree.
pub fn render_panel(
    scene: &SceneNode,
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
    // The placement is applied by `render_update`; a bare panel carries none, so
    // the renderer's own defaults stand in here.
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

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_protocol::{
        ButtonNode, ButtonVariant, Color, PanelPlacement, PanelUpdate, PluginAnchor, SceneNode,
        SpacerNode, StackNode, TextNode,
    };

    /// A measurer with no face, so a test needs no font file and no machine.
    ///
    /// Text then contributes no marks and no width, which is the renderer's own
    /// documented degradation path — a machine with no readable system font. Every
    /// geometry below comes from the node's own box rather than from a glyph, so
    /// these tests are testing layout and hit regions rather than the font book,
    /// which `font`'s own tests cover.
    fn measurer() -> TextMeasurer {
        TextMeasurer::empty()
    }

    fn render(scene: SceneNode, size: [u32; 2]) -> Result<RenderedPanel, PluginRenderError> {
        render_panel(&scene, size, 1.0, &mut measurer(), &ImageLibrary::new())
    }

    #[test]
    fn a_panel_rasterizes_to_exactly_the_size_it_was_asked_for() {
        let scene = SceneNode::Stack(StackNode {
            background: Some(Color::rgb(20, 20, 24)),
            padding: [8.0, 8.0],
            children: vec![SceneNode::Text(TextNode {
                value: "25:00".to_string(),
                size: 24.0,
                ..TextNode::default()
            })],
            ..StackNode::default()
        });
        let panel = render(scene, [200, 60]).expect("a panel rasterizes");
        assert_eq!(panel.width, 200);
        assert_eq!(panel.height, 60);
        assert_eq!(panel.pixels.len(), 200 * 60 * 4);
        assert!(
            panel.pixels.chunks(4).any(|pixel| pixel[3] > 0),
            "the background reached the canvas, so the stack drew something"
        );
    }

    #[test]
    fn a_panel_with_no_pressables_reports_no_hit_regions() {
        let scene = SceneNode::Text(TextNode {
            value: "hi".to_string(),
            ..TextNode::default()
        });
        let panel = render(scene, [120, 40]).expect("a panel rasterizes");
        assert!(panel.hit_regions.is_empty());
        assert!(panel.hit_test(10.0, 10.0).is_none());
    }

    #[test]
    fn a_button_is_hit_only_inside_the_rectangle_it_was_drawn_in() {
        let scene = SceneNode::Stack(StackNode {
            padding: [4.0, 4.0],
            children: vec![SceneNode::Button(ButtonNode {
                id: "toggle".to_string(),
                label: "Start".to_string(),
                variant: ButtonVariant::Primary,
                radius: 6.0,
                ..ButtonNode::default()
            })],
            ..StackNode::default()
        });
        let panel = render(scene, [200, 60]).expect("a panel rasterizes");
        let region = panel
            .hit_regions
            .first()
            .expect("the button was drawn, so it has a region");
        assert_eq!(region.button, "toggle");
        let (cx, cy) = region.rect.center();
        assert_eq!(panel.hit_test(cx, cy), Some("toggle"));
        assert_eq!(
            panel.hit_test(region.rect.x - 1.0, cy),
            None,
            "a pixel outside the rectangle is not the button"
        );
    }

    #[test]
    fn a_button_still_gets_a_press_target_when_no_font_loaded() {
        // The minimum press size is a floor rather than a function of the label, so
        // a panel on a machine with no readable font is still pressable. That is
        // what makes the empty measurer a legitimate thing for a test to use.
        let scene = SceneNode::Button(ButtonNode {
            id: "toggle".to_string(),
            ..ButtonNode::default()
        });
        let panel = render(scene, [120, 40]).expect("a panel rasterizes");
        let region = panel.hit_regions.first().expect("a region");
        assert!(region.rect.width >= MINIMUM_BUTTON_SIZE);
        assert!(region.rect.height >= MINIMUM_BUTTON_SIZE);
    }

    #[test]
    fn a_disabled_button_is_drawn_greyed_and_reported_separately() {
        let scene = SceneNode::Stack(StackNode {
            children: vec![SceneNode::Button(ButtonNode {
                id: "toggle".to_string(),
                label: "Start".to_string(),
                disabled: true,
                ..ButtonNode::default()
            })],
            ..StackNode::default()
        });
        let panel = render(scene, [200, 60]).expect("a panel rasterizes");
        assert!(
            panel
                .disabled_regions
                .iter()
                .any(|region| region.button == "toggle"),
            "a plugin that greys a button has to be able to see that it did"
        );
    }

    #[test]
    fn a_later_button_wins_where_two_overlap() {
        // Layout never overlaps two buttons, so this is built directly: the
        // ordering rule is about what a press *means* when regions do share a
        // point, and a press that resolved to the button underneath would be a
        // silent wrong answer rather than a visible failure.
        let panel = RenderedPanel {
            pixels: Vec::new(),
            width: 100,
            height: 100,
            anchor: OverlayAnchor::TopLeft,
            margin: [0.0, 0.0],
            width_fraction: 0.5,
            opacity: 1.0,
            hit_regions: vec![
                HitRegion {
                    button: "under".to_string(),
                    rect: RoundedRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                },
                HitRegion {
                    button: "over".to_string(),
                    rect: RoundedRect {
                        x: 50.0,
                        y: 50.0,
                        width: 50.0,
                        height: 50.0,
                    },
                },
            ],
            disabled_regions: Vec::new(),
        };
        assert_eq!(
            panel.hit_test(75.0, 75.0),
            Some("over"),
            "the region drawn last is the one the press belongs to"
        );
        assert_eq!(panel.hit_test(10.0, 10.0), Some("under"));
        assert_eq!(panel.hit_test(200.0, 200.0), None);
    }

    #[test]
    fn a_raster_past_the_layer_pixel_bound_is_refused_rather_than_allocated() {
        // The per-side bound belongs to the protocol, which is the only place a
        // document arrives from; what this asserts is the renderer's own bound —
        // a canvas bigger than a layer may carry is refused here, with a reason,
        // rather than becoming a texture the overlay has to drop.
        let scene = SceneNode::Spacer(SpacerNode { grow: 1.0 });
        let side = bongocat_render::MAXIMUM_OVERLAY_LAYER_SIDE;
        assert!(
            render(scene.clone(), [side + 1, side]).is_err(),
            "the bound is exclusive, and a raster one pixel past it is refused"
        );
        assert!(
            render(scene, [side, side]).is_ok(),
            "and the largest raster the bound allows is exactly at it"
        );
    }

    #[test]
    fn a_raster_carries_a_content_hash_that_tracks_the_pixels() {
        let one = SceneNode::Stack(StackNode {
            background: Some(Color::rgb(10, 20, 30)),
            children: vec![SceneNode::Spacer(SpacerNode { grow: 1.0 })],
            ..StackNode::default()
        });
        let two = SceneNode::Stack(StackNode {
            background: Some(Color::rgb(30, 20, 10)),
            children: vec![SceneNode::Spacer(SpacerNode { grow: 1.0 })],
            ..StackNode::default()
        });
        let a = render(one.clone(), [120, 40]).expect("rasterizes").to_raster();
        let b = render(one, [120, 40]).expect("rasterizes").to_raster();
        let c = render(two, [120, 40]).expect("rasterizes").to_raster();
        assert_eq!(
            a.content, b.content,
            "the same scene rasterizes to the same hash, which is what keeps an unchanged panel from being re-uploaded"
        );
        assert_ne!(a.content, c.content);
    }

    #[test]
    fn an_update_carries_its_placement_into_the_rendered_panel() {
        let update = PanelUpdate {
            placement: PanelPlacement {
                anchor: PluginAnchor::BottomRight,
                margin: [0.03, 0.05],
                width_fraction: 0.5,
                opacity: 0.8,
                size: [200, 80],
            },
            scene: SceneNode::Text(TextNode {
                value: "hi".to_string(),
                ..TextNode::default()
            }),
        };
        let panel =
            render_update(&update, 1.0, &mut measurer(), &ImageLibrary::new()).expect("rasterizes");
        assert_eq!(panel.anchor, OverlayAnchor::BottomRight);
        assert_eq!(panel.margin, [0.03, 0.05]);
        assert_eq!(panel.width_fraction, 0.5);
        assert_eq!(panel.opacity, 0.8);
        assert_eq!((panel.width, panel.height), (200, 80));
    }

    #[test]
    fn a_non_finite_scale_becomes_the_minimum_rather_than_reaching_every_coordinate() {
        let scene = SceneNode::Stack(StackNode {
            background: Some(Color::rgb(1, 2, 3)),
            children: vec![SceneNode::Spacer(SpacerNode { grow: 1.0 })],
            ..StackNode::default()
        });
        let panel = render_panel(
            &scene,
            [100, 50],
            f32::NAN,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .expect("rasterizes");
        assert_eq!(
            (panel.width, panel.height),
            (100, 50),
            "the logical size still decides the panel; the scale only multiplies"
        );
    }
}
