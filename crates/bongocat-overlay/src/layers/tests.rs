//! Placement, the click that decides a press, and the texture cache behind both.
//!
//! The y-axis reflection is the part worth pinning down: a drawable's origin is its
//! top left and NDC's is the center with y up, so the conversion is easy to write
//! backwards, and a layer cannot report the mistake — a button at the top of a panel
//! would simply stop responding.

use super::*;
use bongocat_render::{OverlayAnchor, OverlayLayerPlacement, OverlayLayerRaster};
use std::sync::Arc;

/// A square panel anchored to the top left of the window box.
fn panel(id: u64, side: u32) -> OverlayLayer {
    OverlayLayer {
        id,
        placement: OverlayLayerPlacement {
            anchor: OverlayAnchor::TopLeft,
            margin: [0.0, 0.0],
            nudge: [0.0, 0.0],
            width_fraction: 0.5,
            opacity: 1.0,
        },
        raster: OverlayLayerRaster {
            width: side,
            height: side,
            pixels: Arc::from(vec![0_u8; (side * side * 4) as usize]),
            content: id,
        },
    }
}

#[test]
fn place_drops_a_raster_no_backend_could_upload() {
    let empty = OverlayLayer {
        raster: OverlayLayerRaster {
            width: 0,
            height: 0,
            ..panel(2, 0).raster
        },
        ..panel(2, 0)
    };
    let placed = place(&[panel(1, 64), empty]);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].layer.id, 1);
}

#[test]
fn a_click_inside_a_panel_becomes_a_press_in_the_layers_own_pixels() {
    // A 200x200 drawable with a 100x100 panel in its top left quarter. The panel's
    // own raster is 20x20, so its center is 50% of the way across either axis.
    let placed = place(&[panel(7, 20)]);
    let press = press_at(&placed, 200, 200, 50.0, 50.0).expect("a click inside the panel");
    assert_eq!(press.layer_id, 7);
    assert!((press.x - 10.0).abs() < 0.01, "x was {}", press.x);
    assert!((press.y - 10.0).abs() < 0.01, "y was {}", press.y);
}

#[test]
fn the_y_axis_is_measured_from_the_top_of_the_drawable() {
    let placed = place(&[panel(7, 20)]);
    let press = press_at(&placed, 200, 200, 5.0, 5.0).expect("a click near the panel's top");
    assert!(press.y < 2.0, "y was {}", press.y);
    let press = press_at(&placed, 200, 200, 5.0, 95.0).expect("a click near the panel's bottom");
    assert!(press.y > 18.0, "y was {}", press.y);
}

#[test]
fn a_click_outside_every_panel_is_not_a_press() {
    let placed = place(&[panel(7, 20)]);
    // The bottom right of the drawable, where a top-left panel cannot reach.
    assert_eq!(press_at(&placed, 200, 200, 195.0, 195.0), None);
}

#[test]
fn the_later_layer_wins_where_two_overlap() {
    let placed = place(&[panel(1, 20), panel(2, 20)]);
    assert_eq!(
        press_at(&placed, 200, 200, 50.0, 50.0).map(|press| press.layer_id),
        Some(2)
    );
}

#[test]
fn a_fully_transparent_layer_is_not_clickable() {
    let mut hidden = panel(1, 20);
    hidden.placement.opacity = 0.0;
    let placed = place(&[hidden]);
    assert_eq!(press_at(&placed, 200, 200, 50.0, 50.0), None);
}

#[test]
fn a_drawable_that_does_not_exist_presses_nothing() {
    let placed = place(&[panel(1, 20)]);
    assert_eq!(press_at(&placed, 0, 200, 0.0, 0.0), None);
    assert_eq!(press_at(&placed, 200, 0, 0.0, 0.0), None);
}

#[test]
fn a_non_finite_click_presses_nothing() {
    let placed = place(&[panel(1, 20)]);
    assert_eq!(press_at(&placed, 200, 200, f32::NAN, 50.0), None);
    assert_eq!(press_at(&placed, 200, 200, 50.0, f32::INFINITY), None);
}

#[test]
fn a_texture_is_re_uploaded_only_when_its_content_changed() {
    let mut textures = LayerTextures::new();
    let layer = panel(1, 8);
    assert!(!textures.is_current(&layer));
    textures.insert(layer.id, "first", &layer);
    assert!(textures.is_current(&layer));
    assert_eq!(textures.get(layer.id), Some(&"first"));

    // A redraw that produced the same pixels is not a re-upload.
    textures.insert(layer.id, "second", &layer);
    assert!(textures.is_current(&layer));
    assert_eq!(textures.get(layer.id), Some(&"second"));

    let mut changed = layer.clone();
    changed.raster = OverlayLayerRaster {
        content: 99,
        ..changed.raster
    };
    assert!(!textures.is_current(&changed));

    // A different size is a re-creation rather than a stretch.
    let mut resized = layer.clone();
    resized.raster.width = 9;
    assert!(!textures.is_current(&resized));
}

#[test]
fn a_layer_that_left_the_frame_releases_its_texture() {
    let mut textures = LayerTextures::new();
    let gone = panel(1, 8);
    textures.insert(gone.id, "gone", &gone);
    let kept = panel(2, 8);
    textures.insert(kept.id, "kept", &kept);

    textures.retain(&place(std::slice::from_ref(&kept)));

    assert_eq!(textures.get(gone.id), None);
    assert_eq!(textures.get(kept.id), Some(&"kept"));
}
