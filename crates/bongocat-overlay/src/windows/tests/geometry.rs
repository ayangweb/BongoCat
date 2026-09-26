//! Where the overlay lands, and how a logical size becomes a physical one.

use super::*;

#[test]
fn enumerated_displays_are_non_empty_and_cover_their_own_center() {
    let screens = screen_bounds_all();
    assert!(!screens.is_empty(), "the desktop has at least one display");
    for screen in &screens {
        assert!(screen.width >= 128 && screen.height >= 128);
        let center = OverlayWindowBounds::new(
            screen.x + (screen.width / 2) as i32 - 32,
            screen.y + (screen.height / 2) as i32 - 32,
            64,
            64,
        );
        assert!(bounds_inside_screens(&screens, center));
    }
    // A box starting one display-width past the right-most display cannot
    // intersect any of them.
    let right_most = screens
        .iter()
        .max_by_key(|screen| screen.x + screen.width as i32)
        .expect("at least one display");
    let off_desktop = OverlayWindowBounds::new(
        right_most.x + right_most.width as i32 + 1_000,
        right_most.y,
        350,
        350,
    );
    assert!(!bounds_inside_screens(&screens, off_desktop));
}

#[test]
fn the_resize_base_is_the_logical_size_scaled_by_the_window_dpi() {
    // 100% is defined in logical pixels, while the drag works in the
    // physical pixels `SetWindowPos` takes.
    let base = resize_base_for_dpi(350, 350, 96).expect("96 DPI base");
    assert_eq!(base, ResizeBase::new(350.0, 350.0).expect("square base"));

    let scaled = resize_base_for_dpi(350, 350, 192).expect("192 DPI base");
    assert_eq!(scaled, ResizeBase::new(700.0, 700.0).expect("doubled base"));

    // 150% is not an exact multiple of 96, so the rounding is what the
    // window creation path uses as well.
    let fractional = resize_base_for_dpi(350, 200, 144).expect("144 DPI base");
    assert_eq!(
        fractional,
        ResizeBase::new(
            f64::from(logical_to_physical(350, 144).expect("scaled width")),
            f64::from(logical_to_physical(200, 144).expect("scaled height")),
        )
        .expect("scaled base")
    );
}
