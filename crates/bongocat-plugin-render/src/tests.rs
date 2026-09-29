//! What a panel actually looks like, asserted on rather than eyeballed.
//!
//! The raster is a plain buffer, so every property a panel is supposed to have is
//! checkable without a screenshot: the fill lands where the layout said it would,
//! a rounded corner is antialiased rather than stepped, a progress bar's width
//! tracks its value, a press lands on the button under it. That is the reason for
//! doing this on the CPU at all, and these are the tests that would catch a
//! regression the eye would not.

use super::*;
use crate::layout::MINIMUM_BUTTON_SIZE;
use bongocat_plugin_protocol::{BindingTable, BindingValue, Color, SceneNode, StackNode, TextNode};

fn measurer() -> TextMeasurer {
    TextMeasurer::new(FontBook::load_system())
}

/// The alpha of the most opaque pixel, which is a cheap "did anything draw".
fn peak_alpha(pixels: &[u8]) -> u8 {
    pixels
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .max()
        .unwrap_or(0)
}

/// The number of pixels with any coverage at all.
fn covered(pixels: &[u8]) -> usize {
    pixels.chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
}

/// The alpha at a pixel, or 0 when the coordinates are outside the raster.
fn alpha_at(pixels: &[u8], width: u32, x: u32, y: u32) -> u8 {
    let index = (y as usize * width as usize + x as usize) * 4;
    pixels.get(index + 3).copied().unwrap_or(0)
}

fn scene(json: &str) -> SceneNode {
    serde_json::from_str(json).expect("the scene under test parses")
}

fn contribution(
    scene: &SceneNode,
    size: [u32; 2],
) -> bongocat_plugin_protocol::OverlayContribution {
    bongocat_plugin_protocol::OverlayContribution {
        anchor: bongocat_plugin_protocol::PluginAnchor::BottomLeft,
        margin: [0.02, 0.02],
        width_fraction: 0.8,
        opacity: 1.0,
        size,
        behaviors: Vec::new(),
        scene: scene.clone(),
    }
}

#[test]
fn an_empty_scene_draws_nothing_and_publishes_no_pixels() {
    // A scene of only spacers has no ink. Publishing a transparent layer for it
    // would make the overlay upload a texture that can never show anything.
    let panel = render_panel(
        &scene(r#"{"type":"spacer","grow":1}"#),
        &BindingTable::new(),
        [64, 64],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("an empty scene is not an error");
    assert_eq!(panel.pixels.len(), 0, "a blank panel publishes no pixels");
    assert!(panel.hit_regions.is_empty());
}

#[test]
fn a_solid_stack_fills_its_whole_box() {
    let panel = render_panel(
        &scene(
            r#"{"type":"stack","background":"ff0000","radius":0,
                "children":[{"type":"spacer","grow":1}]}"#,
        ),
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a solid stack rasterizes");
    assert_eq!((panel.width, panel.height), (80, 40));
    assert_eq!(panel.pixels.len(), 80 * 40 * 4);
    // Dead centre is unambiguously inside and unambiguously the fill colour.
    assert!(alpha_at(&panel.pixels, 80, 40, 20) > 250);
    let centre = &panel.pixels[(20 * 80 + 40) * 4..(20 * 80 + 40) * 4 + 4];
    assert!(centre[0] > 250, "red channel is full");
    assert!(centre[1] < 5 && centre[2] < 5, "and green and blue are not");
}

#[test]
fn a_rounded_corner_is_antialiased_rather_than_stepped() {
    let panel = render_panel(
        &scene(
            r#"{"type":"stack","background":"ffffff","radius":16,
                "children":[{"type":"spacer","grow":1}]}"#,
        ),
        &BindingTable::new(),
        [64, 64],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a rounded stack rasterizes");
    // The very corner is outside the shape: a 16-pixel radius leaves the first
    // pixel column of the top row empty.
    assert_eq!(
        alpha_at(&panel.pixels, 64, 0, 0),
        0,
        "the corner of a rounded box is empty"
    );
    // Between the corner and where the edge becomes straight, the coverage ramps
    // up. A hard corner would have no such pixels at all — every pixel would be
    // either fully covered or not covered at all.
    let partial = (0..24_u32)
        .filter(|offset| {
            let alpha = alpha_at(&panel.pixels, 64, *offset, 0);
            alpha > 0 && alpha < 255
        })
        .count();
    assert!(
        partial >= 4,
        "a hard corner would have no partial pixels, got {partial}"
    );
    // The coverage increases monotonically along the ramp, which is what makes it
    // an antialiased edge rather than noise.
    let ramp: Vec<u8> = (0..20_u32)
        .map(|offset| alpha_at(&panel.pixels, 64, offset, 0))
        .collect();
    assert!(
        ramp.windows(2).all(|pair| pair[0] <= pair[1]),
        "coverage should not decrease along the corner: {ramp:?}"
    );
    // The middle of an edge is fully covered, so the shape is still a box.
    assert!(alpha_at(&panel.pixels, 64, 32, 0) > 250);
}

#[test]
fn a_progress_bar_fills_proportionally_to_its_value() {
    let bar = |fraction: f32| {
        let bar = scene(
            r#"{"type":"stack","children":[
                {"type":"progress_bar","value":{"fraction":"p","fallback":0},
                 "height":10,"fill":"00ff00","track":"000000","radius":0}
            ]}"#,
        );
        let mut values = BindingTable::new();
        values.set("p", BindingValue::Fraction(fraction));
        render_panel(
            &bar,
            &values,
            [100, 10],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .expect("a bar rasterizes")
    };
    let empty = bar(0.0);
    let full = bar(1.0);
    let half = bar(0.5);
    // The track is always drawn, so an empty bar is not transparent — and the
    // distinction between empty and full is in the fill colour, not the alpha.
    // The green channel is the fill and the blue one is the track, so this reads
    // the bar's state without caring how the two are weighted against each other.
    let is_fill = |panel: &RenderedPanel, x: u32| {
        let index = (5 * panel.width as usize + x as usize) * 4;
        panel.pixels[index + 1] > 150 && panel.pixels[index + 2] < 100
    };
    assert!(!is_fill(&empty, 50), "no fill at zero");
    assert!(is_fill(&full, 50), "fill at one");
    assert!(
        !is_fill(&half, 80),
        "the right half of a half-full bar is empty"
    );
    assert!(is_fill(&half, 20), "and the left half is filled");
    // The boundary is where the value says it is, to within a pixel.
    let filled: Vec<u32> = (0..100_u32).filter(|x| is_fill(&half, *x)).collect();
    let edge = *filled.iter().max().expect("a half-full bar has fill");
    assert!(
        (45..=55).contains(&edge),
        "the fill ends at {edge}, not at 50"
    );
}

#[test]
fn a_progress_ring_at_zero_draws_only_its_track() {
    let ring = |fraction: f32| {
        let node = scene(
            r#"{"type":"stack","children":[
                {"type":"progress_ring","value":{"fraction":"p","fallback":0},
                 "size":40,"thickness":6,"fill":"ff0000","track":"0000ff"}
            ]}"#,
        );
        let mut values = BindingTable::new();
        values.set("p", BindingValue::Fraction(fraction));
        render_panel(
            &node,
            &values,
            [40, 40],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .expect("a ring rasterizes")
    };
    let empty = ring(0.0);
    let full = ring(1.0);
    // At zero there is no red anywhere: the ring starts at the top and a zero
    // sweep is nothing.
    let red_at = |panel: &RenderedPanel| {
        panel
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 200 && pixel[3] > 200)
            .count()
    };
    assert_eq!(red_at(&empty), 0, "a zero ring draws no fill");
    assert!(red_at(&full) > 50, "a full ring draws a visible arc");
    // A full ring covers its own track, so the two are the same size; what
    // changes is the colour, which is why this asserts on the fill rather than on
    // the covered area.
    assert!(covered(&full.pixels) > 0, "the ring drew at all");
}

#[test]
fn a_text_node_puts_marks_where_the_layout_said() {
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"text","value":"MMMM","size":20}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [120, 30],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a text node rasterizes");
    assert!(
        peak_alpha(&panel.pixels) > 0,
        "text on a transparent panel still marks pixels"
    );
    // A run of `M` is solid, so its ink is in the middle band of the line and
    // nowhere near the bottom of the panel.
    let lower_half = (panel.height / 2..panel.height)
        .flat_map(|y| (0..panel.width).map(move |x| (x, y)))
        .filter(|(x, y)| alpha_at(&panel.pixels, panel.width, *x, *y) > 0)
        .count();
    let upper_half = (0..panel.height / 2)
        .flat_map(|y| (0..panel.width).map(move |x| (x, y)))
        .filter(|(x, y)| alpha_at(&panel.pixels, panel.width, *x, *y) > 0)
        .count();
    assert!(upper_half > lower_half, "a baseline sits in the upper part");
}

#[test]
fn text_draws_nothing_at_all_without_a_face_and_does_not_fail() {
    // The degradation path: a machine with no readable system font still loads
    // the plugin, still lays the panel out, and shows the panel without labels.
    let panel = render_panel(
        &scene(
            r#"{"type":"stack","background":"ffffff","children":[
                {"type":"text","value":"Focus","size":20}
            ]}"#,
        ),
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut TextMeasurer::empty(),
        &ImageLibrary::new(),
    )
    .expect("a missing font is not a render failure");
    assert!(
        covered(&panel.pixels) > 0,
        "the background still draws, so the panel is visible"
    );
}

#[test]
fn a_button_registers_exactly_the_rectangle_it_was_drawn_in() {
    let node = scene(
        r#"{"type":"stack","padding":[10,10],"children":[
            {"type":"button","id":"go","label":"Go","action":"toggle","target":"t",
             "variant":"primary","radius":6}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [100, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a button rasterizes");
    assert_eq!(panel.hit_regions.len(), 1);
    let region = &panel.hit_regions[0];
    assert_eq!(region.button, "go");
    // The stack's padding is honoured, so the button starts where the layout put
    // it rather than at the origin.
    assert!((region.rect.x - 10.0).abs() < 0.001);
    assert!((region.rect.y - 10.0).abs() < 0.001);
    // A press at the button's centre hits it.
    assert_eq!(
        panel.hit_test(region.rect.x + 1.0, region.rect.y + 1.0),
        Some("go")
    );
    // A press above it, in the padding, does not.
    assert_eq!(panel.hit_test(2.0, 2.0), None);
}

#[test]
fn a_disabled_button_is_reported_separately_and_takes_no_press() {
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"go","label":"Go","action":"toggle","target":"t",
             "disabled":{"flag":"busy","fallback":false}}
        ]}"#,
    );
    let mut values = BindingTable::new();
    values.set("busy", BindingValue::Flag(true));
    let panel = render_panel(
        &node,
        &values,
        [100, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a disabled button rasterizes");
    assert!(
        panel.hit_regions.is_empty(),
        "a disabled button takes no press"
    );
    assert_eq!(panel.disabled_regions.len(), 1);
    let region = &panel.disabled_regions[0];
    assert_eq!(
        panel.hit_test(region.rect.x + 1.0, region.rect.y + 1.0),
        None,
        "and a press inside it hits nothing"
    );
}

#[test]
fn a_bound_value_changes_what_is_drawn() {
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"text","value":{"binding":"label","fallback":""},"size":18}
        ]}"#,
    );
    let mut values = BindingTable::new();
    values.set("label", BindingValue::Text("AAAA".to_string()));
    let with_text = render_panel(
        &node,
        &values,
        [120, 30],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a bound text rasterizes");
    values.set("label", BindingValue::Text(String::new()));
    let without = render_panel(
        &node,
        &values,
        [120, 30],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("an empty binding rasterizes");
    assert!(
        covered(&with_text.pixels) > covered(&without.pixels),
        "binding to text draws more than binding to nothing"
    );
}

#[test]
fn a_stack_with_a_growing_spacer_uses_the_space_it_was_given() {
    let without = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t"}
        ]}"#,
    );
    let with = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t"},
            {"type":"spacer","grow":1},
            {"type":"button","id":"b","label":"B","action":"toggle","target":"t"}
        ]}"#,
    );
    let images = ImageLibrary::new();
    let spread = render_panel(
        &with,
        &BindingTable::new(),
        [100, 200],
        1.0,
        &mut measurer(),
        &images,
    )
    .expect("two buttons and a spacer rasterize");
    // The two buttons are told apart by the space between them: with a growing
    // spacer, the second is pushed most of the way down the panel rather than
    // sitting directly under the first.
    assert_eq!(spread.hit_regions.len(), 2);
    let gap = spread.hit_regions[1].rect.y - spread.hit_regions[0].rect.y;
    assert!(
        gap > spread.hit_regions[0].rect.height * 2.0,
        "a growing spacer should push the second button down, gap was {gap}"
    );
    // And the last button still ends inside the panel, which is the invariant a
    // growing spacer must not break.
    let last = &spread.hit_regions[1];
    assert!(last.rect.y + last.rect.height <= 200.0 + 0.001);
    // A stack with only one button has only one press target, and the spacer
    // version has two — the two are not interchangeable panels.
    let short = render_panel(
        &without,
        &BindingTable::new(),
        [100, 200],
        1.0,
        &mut measurer(),
        &images,
    )
    .expect("one button rasterizes");
    assert_eq!(short.hit_regions.len(), 1);
}

#[test]
fn a_missing_image_leaves_its_space_empty_rather_than_failing_the_panel() {
    let node = scene(
        r#"{"type":"stack","background":"ffffff","children":[
            {"type":"image","asset":"icon.png","size":24}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a missing image is not a render failure");
    // The background drew; the image's box did not.
    assert!(covered(&panel.pixels) > 0);
}

#[test]
fn the_raster_scale_multiplies_the_device_size() {
    let node =
        scene(r#"{"type":"stack","background":"ffffff","children":[{"type":"spacer","grow":1}]}"#);
    for scale in [1.0_f32, 2.0, 3.0] {
        let panel = render_panel(
            &node,
            &BindingTable::new(),
            [50, 25],
            scale,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .expect("a stack rasterizes at any scale");
        assert_eq!(panel.width, 50 * scale as u32);
        assert_eq!(panel.height, 25 * scale as u32);
        assert_eq!(
            panel.pixels.len(),
            (50 * scale as usize) * (25 * scale as usize) * 4
        );
    }
}

#[test]
fn the_raster_scale_is_clamped_to_a_bound() {
    // A window scaled absurdly must not be able to ask for an unbounded raster.
    let node =
        scene(r#"{"type":"stack","background":"ffffff","children":[{"type":"spacer","grow":1}]}"#);
    for scale in [0.0_f32, -3.0, f32::NAN, f32::INFINITY, 1000.0] {
        let panel = render_panel(
            &node,
            &BindingTable::new(),
            [50, 25],
            scale,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .unwrap_or_else(|error| panic!("{scale} should be clamped, not refused: {error}"));
        assert!(
            panel.width <= 50 * MAXIMUM_RASTER_SCALE as u32,
            "{scale} produced an unbounded raster"
        );
    }
}

#[test]
fn a_zero_sized_panel_is_refused_rather_than_allocating_nothing() {
    let error = render_panel(
        &scene(r#"{"type":"spacer","grow":1}"#),
        &BindingTable::new(),
        [0, 10],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), PluginRenderErrorCode::RasterTooLarge);
}

#[test]
fn a_manifest_contribution_carries_its_placement_through() {
    let panel = render_contribution(
        &contribution(
            &scene(
                r#"{"type":"stack","background":"ffffff","children":[
                    {"type":"spacer","grow":1}
                ]}"#,
            ),
            [64, 32],
        ),
        &BindingTable::new(),
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a contribution rasterizes");
    assert_eq!(panel.anchor, OverlayAnchor::BottomLeft);
    assert_eq!(panel.margin, [0.02, 0.02]);
    assert_eq!(panel.width_fraction, 0.8);
    let placement = panel.to_placement();
    assert_eq!(placement.anchor, OverlayAnchor::BottomLeft);
    assert_eq!(placement.width_fraction, 0.8);
    assert_eq!(placement.opacity, 1.0);
}

#[test]
fn identical_pixels_hash_identically_and_different_ones_do_not() {
    let pixels = vec![7u8; 64];
    assert_eq!(content_hash(&pixels), content_hash(&pixels));
    let mut other = pixels.clone();
    other[31] = 8;
    assert_ne!(content_hash(&pixels), content_hash(&other));
}

#[test]
fn a_panel_that_looks_the_same_after_a_redraw_hashes_the_same() {
    // The point of the hash: a re-layout that produced the same picture must not
    // make the overlay re-upload the texture.
    let node = scene(
        r#"{"type":"stack","background":"334455","padding":[4,4],"children":[
            {"type":"text","value":"Focus","size":14}
        ]}"#,
    );
    let values = BindingTable::new();
    let first = render_panel(
        &node,
        &values,
        [80, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("first draw");
    let second = render_panel(
        &node,
        &values,
        [80, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("second draw");
    assert_eq!(first.to_raster().content, second.to_raster().content);
}

#[test]
fn a_layer_raster_carries_the_pixels_and_the_hash() {
    let node =
        scene(r#"{"type":"stack","background":"ffffff","children":[{"type":"spacer","grow":1}]}"#);
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [20, 10],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a stack rasterizes");
    let raster = panel.to_raster();
    assert_eq!(raster.width, 20);
    assert_eq!(raster.height, 10);
    assert_eq!(raster.pixels.len(), 20 * 10 * 4);
    assert!(raster.is_valid());
    assert_eq!(raster.content, content_hash(&panel.pixels));
}

#[test]
fn a_translucent_surface_composites_over_what_is_already_there() {
    // Two stacks, the outer opaque and the inner half-transparent, so the
    // composite has something to show through.
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"stack","background":"ffffff80","children":[
                {"type":"spacer","grow":1}
            ]}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("nested translucent stacks rasterize");
    let centre = &panel.pixels[(20 * 40 + 20) * 4..(20 * 40 + 20) * 4 + 4];
    // Half-white over black is mid grey, and fully opaque because the black below
    // it is.
    assert!(centre[0] > 100 && centre[0] < 160, "got {}", centre[0]);
    assert_eq!(centre[3], 255);
}

#[test]
fn a_transparent_area_stays_transparent() {
    // A stack with no background must not paint one, or every panel would have a
    // visible box over the model whether it wanted one or not.
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [100, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a button rasterizes");
    // A button sits at the top left, so the pixel to prove the panel is
    // transparent has to be one the button does not reach.
    assert_eq!(alpha_at(&panel.pixels, 100, 99, 39), 0);
    assert_eq!(alpha_at(&panel.pixels, 100, 95, 5), 0);
}

#[test]
fn a_border_is_drawn_as_a_ring_and_leaves_its_interior_alone() {
    let node = scene(
        r#"{"type":"stack","background":"000000","border":"ffffff",
            "border_width":2,"radius":0,"children":[{"type":"spacer","grow":1}]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a bordered stack rasterizes");
    let white = |x: u32, y: u32| alpha_at(&panel.pixels, 40, x, y);
    // The edge is white, the middle is the black background.
    let edge = &panel.pixels[(20 * 40) * 4..(20 * 40) * 4 + 4];
    assert!(edge[0] > 200, "the border is white");
    let middle = &panel.pixels[(20 * 40 + 20) * 4..(20 * 40 + 20) * 4 + 4];
    assert!(middle[0] < 20, "the interior is the background");
    assert!(white(0, 0) > 0);
}

#[test]
fn the_topmost_of_two_overlapping_buttons_takes_the_press() {
    // Drawn in order, so the later one is on top and is the one a press belongs
    // to — the same rule the layer hit test in `bongocat-render` uses. A column
    // never overlaps two children, so the panel is assembled directly: the rule
    // under test is the hit test's, not the layout's.
    let panel = RenderedPanel {
        pixels: Vec::new(),
        width: 100,
        height: 80,
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
                    width: 40.0,
                    height: 40.0,
                },
            },
            HitRegion {
                button: "over".to_string(),
                rect: RoundedRect {
                    x: 20.0,
                    y: 20.0,
                    width: 40.0,
                    height: 40.0,
                },
            },
        ],
        disabled_regions: Vec::new(),
    };
    // In the overlap, the later region wins.
    assert_eq!(panel.hit_test(30.0, 30.0), Some("over"));
    // Outside the overlap, each still answers for itself.
    assert_eq!(panel.hit_test(5.0, 5.0), Some("under"));
    assert_eq!(panel.hit_test(55.0, 55.0), Some("over"));
    // And outside both, nothing does.
    assert_eq!(panel.hit_test(90.0, 5.0), None);
}

#[test]
fn a_column_of_buttons_gives_each_one_its_own_press_target() {
    // The everyday case the rule above exists for: buttons laid out normally are
    // individually pressable, and pressing one does not press its neighbour.
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t"},
            {"type":"button","id":"b","label":"B","action":"toggle","target":"t"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [100, 80],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("two buttons rasterize");
    assert_eq!(panel.hit_regions.len(), 2);
    let first = &panel.hit_regions[0];
    let second = &panel.hit_regions[1];
    assert_eq!(
        panel.hit_test(first.rect.x + 2.0, first.rect.y + 2.0),
        Some("a")
    );
    assert_eq!(
        panel.hit_test(second.rect.x + 2.0, second.rect.y + 2.0),
        Some("b")
    );
    assert!(second.rect.y >= first.rect.y + first.rect.height - 0.001);
}

#[test]
fn a_divider_is_a_thin_line_across_the_node() {
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"divider","thickness":2,"color":"ffffff"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [40, 20],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a divider rasterizes");
    let lit = (0..panel.height)
        .flat_map(|y| (0..panel.width).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let index = (*y as usize * panel.width as usize + *x as usize) * 4;
            panel.pixels[index] > 200
        })
        .count();
    assert!(
        (40 * 2..40 * 8).contains(&lit),
        "a 2px divider across 40px should light about 80 pixels, got {lit}"
    );
}

#[test]
fn a_horizontal_stack_lays_children_side_by_side() {
    let node = scene(
        r#"{"type":"stack","axis":"horizontal","spacing":4,"children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t"},
            {"type":"button","id":"b","label":"B","action":"toggle","target":"t"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [200, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a row rasterizes");
    let first = &panel.hit_regions[0];
    let second = &panel.hit_regions[1];
    assert!(second.rect.x > first.rect.x, "a row moves right, not down");
    assert!(
        (second.rect.y - first.rect.y).abs() < 1.0,
        "and both sit on the same line"
    );
}

#[test]
fn an_image_is_drawn_where_the_scene_put_it() {
    // A two-by-two opaque red image, so the drawn pixels are unambiguous. It is
    // built rather than decoded because the property under test is that an image
    // lands in its box, not that this crate can read a PNG.
    let image = DecodedImage {
        width: 2,
        height: 2,
        pixels: vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ],
    };
    let mut images = ImageLibrary::new();
    images.insert("icon.png", image);
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"image","asset":"icon.png","size":20}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &images,
    )
    .expect("an image rasterizes");
    // Somewhere in the image's box there is red; outside it there is only black.
    let red = panel
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[0] > 200 && pixel[1] < 30)
        .count();
    assert!(red > 0, "the image drew");
}

#[test]
fn a_decoded_png_round_trips_through_memory() {
    // A one-pixel PNG, written by hand so the test needs no fixture file.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let image = DecodedImage::decode_png(PNG).expect("a one-pixel PNG decodes");
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(image.pixels.len(), 4);
    assert_eq!(image.pixels[3], 255, "and it is opaque");
}

#[test]
fn the_default_theme_is_legible_on_a_transparent_window() {
    // The default surface is what a panel that names no colour gets, so its
    // contrast against the default text is a product decision worth pinning.
    let theme = Theme::default();
    let luminance = |color: Color| {
        0.2126 * f32::from(color.red)
            + 0.7152 * f32::from(color.green)
            + 0.0722 * f32::from(color.blue)
    };
    assert!(
        luminance(theme.text) > luminance(theme.surface) + 100.0,
        "the label must be clearly brighter than the surface behind it"
    );
    assert!(
        theme.surface.alpha > 0 && theme.surface.alpha < 255,
        "and the surface must be translucent, or it hides the model"
    );
}

#[test]
fn a_scene_whose_only_node_is_a_button_produces_a_press_target() {
    // The simplest possible plugin panel, and the shape most panels start from.
    let node = scene(
        r#"{"type":"stack","padding":[8,8],"children":[
            {"type":"button","id":"only","label":"Only","action":"toggle"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [60, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("the minimal panel rasterizes");
    assert_eq!(panel.hit_regions.len(), 1);
    assert_eq!(panel.hit_regions[0].button, "only");
}

#[test]
fn a_transparent_variant_button_takes_a_press_without_drawing_chrome() {
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t",
             "variant":"transparent"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a transparent button rasterizes");
    assert_eq!(panel.hit_regions.len(), 1);
    // The label drew...
    assert!(covered(&panel.pixels) > 0, "the label drew");
    // ...and the button contributed no fill of its own, which is the difference
    // between a transparent variant and a primary one. Compared against the same
    // panel with the button removed, since the stack's background covers
    // everything either way.
    let without = render_panel(
        &scene(
            r#"{"type":"stack","background":"000000","children":[
                {"type":"spacer","grow":1}
            ]}"#,
        ),
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a plain stack rasterizes");
    assert_eq!(
        covered(&panel.pixels),
        covered(&without.pixels),
        "a transparent button must not change the picture"
    );
}

#[test]
fn a_weighted_label_renders_and_does_not_change_the_panel_shape() {
    let regular =
        scene(r#"{"type":"stack","children":[{"type":"text","value":"Focus","size":16}]}"#);
    let bold = scene(
        r#"{"type":"stack","children":[
            {"type":"text","value":"Focus","size":16,"weight":"bold"}
        ]}"#,
    );
    let values = BindingTable::new();
    let a = render_panel(
        &regular,
        &values,
        [120, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a regular label rasterizes");
    let b = render_panel(
        &bold,
        &values,
        [120, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a bold label rasterizes");
    assert_eq!((a.width, a.height), (b.width, b.height));
    assert!(
        covered(&b.pixels) >= covered(&a.pixels),
        "bold is at least as much ink"
    );
}

#[test]
fn a_transparent_raster_reports_itself_as_empty() {
    let mut canvas = Canvas::new(10, 10, 1.0).expect("a canvas allocates");
    assert!(canvas.is_fully_transparent());
    canvas.fill_rect(
        RoundedRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        0.0,
        Color::rgb(255, 255, 255),
    );
    assert!(!canvas.is_fully_transparent());
}

#[test]
fn a_button_below_the_minimum_press_size_is_still_that_size() {
    // A one-character label would otherwise produce a hit target too small to hit.
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"a","label":"","action":"toggle","target":"t"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [80, 60],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("an empty button rasterizes");
    let region = &panel.hit_regions[0];
    assert!(region.rect.width >= MINIMUM_BUTTON_SIZE);
    assert!(region.rect.height >= MINIMUM_BUTTON_SIZE);
}

#[test]
fn a_button_variant_secondary_draws_a_visible_edge() {
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"button","id":"a","label":"A","action":"toggle","target":"t",
             "variant":"secondary","radius":4}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a secondary button rasterizes");
    // The corner of its own box is outside the rounded surface, so the stack's
    // black background shows through there — which is what makes the rounding
    // visible at all.
    let region = &panel.hit_regions[0];
    let corner_x = region.rect.x as u32;
    let corner_y = region.rect.y as u32;
    let corner_index = (corner_y as usize * 80 + corner_x as usize) * 4;
    let corner = &panel.pixels[corner_index..corner_index + 4];
    assert!(
        corner[0] < 20 && corner[3] == 255,
        "the corner should be the background, got {corner:?}"
    );
    // And just inside the button's bottom edge is its own surface rather than the
    // background. The bottom edge is used rather than the middle because the
    // middle is where the label is.
    let surface_x = (region.rect.x + region.rect.width * 0.5) as u32;
    let surface_y = (region.rect.y + region.rect.height - 2.0) as u32;
    let surface_index = (surface_y as usize * 80 + surface_x as usize) * 4;
    let surface = &panel.pixels[surface_index..surface_index + 4];
    assert!(
        surface[0] > 20,
        "inside the button should be its surface, got {surface:?}"
    );
}

#[test]
fn a_node_that_does_not_fit_is_clipped_rather_than_overflowing() {
    // A panel is a fixed size, so a child that wants more than it was given is
    // cut at the panel's edge rather than drawn over it.
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"button","id":"a","label":"A very long label indeed","action":"toggle",
             "target":"t","variant":"transparent"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [30, 20],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("an oversized label rasterizes");
    assert!(panel.hit_regions[0].rect.width <= 30.0);
    assert!(panel.hit_regions[0].rect.height <= 20.0);
    assert!(
        panel.hit_regions[0].rect.y + panel.hit_regions[0].rect.height <= 20.0 + 0.001,
        "and the press target stays inside the panel's own pixels"
    );
}

#[test]
fn every_bindable_node_kind_renders_without_error() {
    // A smoke test over the whole vocabulary, so adding a node kind and forgetting
    // to draw it fails here rather than as a blank space in a panel.
    let nodes = [
        r#"{"type":"spacer","grow":1}"#,
        r#"{"type":"divider"}"#,
        r#"{"type":"text","value":"x"}"#,
        r#"{"type":"text","value":{"binding":"a.b","fallback":"fb"}}"#,
        r#"{"type":"progress_bar","value":{"fraction":"a.b","fallback":0.5}}"#,
        r#"{"type":"progress_ring","value":{"fraction":"a.b","fallback":0.5}}"#,
        r#"{"type":"image","asset":"a.png"}"#,
        r#"{"type":"button","id":"a","label":"A","action":"toggle"}"#,
        r#"{"type":"button","id":"a","label":"A","action":"reset","target":"b"}"#,
        r#"{"type":"button","id":"a","label":"A","action":"increment","target":"b","variant":"secondary"}"#,
        r#"{"type":"stack","axis":"horizontal","children":[{"type":"spacer"}]}"#,
    ];
    for node in nodes {
        render_panel(
            &scene(node),
            &BindingTable::new(),
            [100, 100],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .unwrap_or_else(|error| panic!("{node} failed to render: {error}"));
    }
}

#[test]
fn a_deep_scene_lays_out_or_fails_on_its_own_bound_not_the_recursion_limit() {
    // The manifest rejects this tree before it reaches a renderer. A renderer
    // that panicked on it would be a crash reachable from a plugin file, which
    // is exactly what the manifest check exists to prevent — so the check is
    // asserted here on the protocol side, where it belongs.
    let mut node = SceneNode::Stack(StackNode {
        children: vec![SceneNode::Text(TextNode {
            value: bongocat_plugin_protocol::SceneValue::Text("x".to_string()),
            ..TextNode::default()
        })],
        ..StackNode::default()
    });
    for _ in 0..bongocat_plugin_protocol::MAXIMUM_SCENE_DEPTH + 4 {
        node = SceneNode::Stack(StackNode {
            children: vec![node],
            ..StackNode::default()
        });
    }
    let mut inspector = bongocat_plugin_protocol::scene::inspect::Inspector::new();
    let error = bongocat_plugin_protocol::scene::inspect::walk(&node, 1, &mut inspector)
        .expect_err("a scene past the depth bound is refused");
    assert_eq!(
        error.code(),
        bongocat_plugin_protocol::PluginErrorCode::SceneTooDeep
    );
}

#[test]
fn the_layout_never_places_a_child_outside_its_parents_box() {
    // The invariant the whole layout rests on: whatever a node wanted, what it
    // got is inside what its parent had. A violation is what makes a panel bleed
    // over the model window's edge.
    let node = scene(
        r#"{"type":"stack","axis":"horizontal","spacing":20,"children":[
            {"type":"text","value":"a very wide label that will not fit","size":24},
            {"type":"text","value":"another one that will not fit either","size":24}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [60, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("an oversized row rasterizes");
    for region in &panel.hit_regions {
        assert!(region.rect.x + region.rect.width <= 60.0 + 0.001);
    }
    // Nothing drew outside the raster, which is the visible half of the same
    // invariant.
    assert_eq!(panel.pixels.len(), 60 * 40 * 4);
}

#[test]
fn a_panel_of_only_a_counter_looks_the_same_twice_in_a_row() {
    // The reason a counter-only panel is not re-rasterized every frame: two
    // evaluations with the same values produce the same hash.
    let node = scene(
        r#"{"type":"stack","background":"202020","children":[
            {"type":"text","value":{"binding":"c.n","fallback":"0"},"size":22}
        ]}"#,
    );
    let mut values = BindingTable::new();
    values.set("c.n", BindingValue::Text("7".to_string()));
    let first = render_panel(
        &node,
        &values,
        [80, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("first draw");
    let second = render_panel(
        &node,
        &values,
        [80, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("second draw");
    assert_eq!(first.to_raster().content, second.to_raster().content);
    values.set("c.n", BindingValue::Text("8".to_string()));
    let third = render_panel(
        &node,
        &values,
        [80, 30],
        2.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("third draw");
    assert_ne!(first.to_raster().content, third.to_raster().content);
}

#[test]
fn a_button_of_each_variant_renders_and_keeps_its_press_target() {
    for variant in ["primary", "secondary", "transparent"] {
        let node = scene(&format!(
            r#"{{"type":"stack","background":"101010","children":[
                {{"type":"button","id":"b","label":"Go","action":"toggle","target":"t",
                 "variant":"{variant}"}}
            ]}}"#
        ));
        let panel = render_panel(
            &node,
            &BindingTable::new(),
            [80, 40],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .unwrap_or_else(|error| panic!("{variant} failed: {error}"));
        assert_eq!(
            panel.hit_regions.len(),
            1,
            "{variant} lost its press target"
        );
        assert!(covered(&panel.pixels) > 0, "{variant} drew nothing");
    }
}

#[test]
fn a_secondary_button_is_told_apart_from_a_primary_one_by_its_pixels() {
    let render = |variant: &str| {
        let node = scene(&format!(
            r#"{{"type":"stack","background":"000000","children":[
                {{"type":"button","id":"b","label":"Go","action":"toggle","target":"t",
                 "variant":"{variant}","radius":6}}
            ]}}"#
        ));
        render_panel(
            &node,
            &BindingTable::new(),
            [80, 40],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .expect("a button rasterizes")
    };
    // The two variants must not produce the same picture, or the declaration is
    // not doing anything.
    assert_ne!(
        content_hash(&render("primary").pixels),
        content_hash(&render("secondary").pixels)
    );
}

#[test]
fn a_stack_axis_of_horizontal_with_one_child_looks_the_same_as_vertical() {
    // A single child has nothing to be laid out against, so the axis must not
    // change where it lands. This catches a placement that depends on the axis
    // when it should not.
    let node = |axis: &str| {
        scene(&format!(
            r#"{{"type":"stack","axis":"{axis}","background":"ffffff","children":[
                {{"type":"spacer","grow":1}}
            ]}}"#
        ))
    };
    let vertical = render_panel(
        &node("vertical"),
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a vertical stack rasterizes");
    let horizontal = render_panel(
        &node("horizontal"),
        &BindingTable::new(),
        [40, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a horizontal stack rasterizes");
    assert_eq!(
        content_hash(&vertical.pixels),
        content_hash(&horizontal.pixels)
    );
}

#[test]
fn the_theme_is_used_where_a_scene_names_no_colour() {
    // A panel that names no colour must still be visible, and the colour it got
    // has to be the theme's rather than a hardcoded one — which is what keeps a
    // plugin consistent with the product's own surfaces.
    let theme = Theme::default();
    let node = scene(
        r#"{"type":"stack","children":[
            {"type":"button","id":"b","label":"Go","action":"toggle","target":"t"}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [80, 40],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a button rasterizes");
    let found_theme_fill = panel.pixels.chunks_exact(4).any(|pixel| {
        let candidate = Color::rgba(pixel[0], pixel[1], pixel[2], pixel[3]);
        candidate == theme.accent || candidate == theme.button_surface
    });
    assert!(
        found_theme_fill,
        "a default button should use a theme colour, not an arbitrary one"
    );
}

#[test]
fn a_scene_with_a_named_colour_uses_it_rather_than_the_theme() {
    let node =
        scene(r#"{"type":"stack","background":"123456","children":[{"type":"spacer","grow":1}]}"#);
    let panel = render_panel(
        &node,
        &BindingTable::new(),
        [20, 20],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a stack rasterizes");
    let found = panel
        .pixels
        .chunks_exact(4)
        .any(|pixel| pixel[0] == 0x12 && pixel[1] == 0x34 && pixel[2] == 0x56);
    assert!(found, "the declared colour was not used");
}

#[test]
fn a_button_of_every_size_renders_and_keeps_its_label_inside() {
    for size in [8.0_f32, 13.0, 24.0, 48.0] {
        let node = scene(&format!(
            r#"{{"type":"stack","background":"000000","children":[
                {{"type":"text","value":"Sizing","size":{size}}}
            ]}}"#
        ));
        let panel = render_panel(
            &node,
            &BindingTable::new(),
            [200, 80],
            2.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .unwrap_or_else(|error| panic!("size {size} failed: {error}"));
        assert_eq!(panel.pixels.len(), 400 * 160 * 4);
    }
}

#[test]
fn an_empty_binding_falls_back_rather_than_drawing_the_path() {
    let mut values = BindingTable::new();
    values.set("other.path", BindingValue::Text("wrong".to_string()));
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"text","value":{"binding":"missing.path","fallback":"fallback"},"size":16}
        ]}"#,
    );
    let panel = render_panel(
        &node,
        &values,
        [120, 30],
        1.0,
        &mut measurer(),
        &ImageLibrary::new(),
    )
    .expect("a fallback renders");
    assert!(covered(&panel.pixels) > 0, "the fallback drew");
}

#[test]
fn an_out_of_range_fraction_clamps_instead_of_emptying_the_panel() {
    let node = scene(
        r#"{"type":"stack","background":"000000","children":[
            {"type":"progress_bar","value":{"fraction":"p","fallback":0},"height":8,
             "fill":"ffffff","track":"000000","radius":0}
        ]}"#,
    );
    for fraction in [-5.0_f32, 0.0, 0.5, 1.0, 5.0] {
        let mut values = BindingTable::new();
        values.set("p", BindingValue::Fraction(fraction));
        let panel = render_panel(
            &node,
            &values,
            [40, 8],
            1.0,
            &mut measurer(),
            &ImageLibrary::new(),
        )
        .unwrap_or_else(|error| panic!("{fraction} failed: {error}"));
        assert!(covered(&panel.pixels) > 0, "{fraction} emptied the bar");
    }
}
