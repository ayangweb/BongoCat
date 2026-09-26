//! Two layers become one image, without losing a pixel.

use super::*;

#[test]
fn composite_is_the_porter_duff_over_operator() {
    // An opaque source replaces the destination outright.
    let mut destination = [1, 2, 3, 255];
    composite_pixel(&mut destination, &[9, 8, 7, 255]);
    assert_eq!(destination, [9, 8, 7, 255]);
    // A fully transparent source leaves it untouched.
    let mut destination = [1, 2, 3, 255];
    composite_pixel(&mut destination, &[9, 8, 7, 0]);
    assert_eq!(destination, [1, 2, 3, 255]);
    // Half-transparent white over opaque black is half grey.
    let mut destination = [0, 0, 0, 255];
    composite_pixel(&mut destination, &[255, 255, 255, 128]);
    assert_eq!(destination, [128, 128, 128, 255]);
    // A transparent destination keeps the source's own colour.
    let mut destination = [0, 0, 0, 0];
    composite_pixel(&mut destination, &[200, 100, 50, 64]);
    assert_eq!(destination, [200, 100, 50, 64]);
    // A transparent canvas survives a transparent source.
    let mut destination = [0, 0, 0, 0];
    composite_pixel(&mut destination, &[9, 9, 9, 0]);
    assert_eq!(destination, [0, 0, 0, 0]);
}

#[test]
fn the_png_recode_preserves_every_visible_pixel_and_shrinks_the_file() {
    // A composed-looking image: a transparent field, a filled shape and a
    // semi-transparent disc, which is what the real key images are made of.
    let encoded = encode_png(128, 96, |x, y| {
        let dx = i64::from(x) - 90;
        let dy = i64::from(y) - 48;
        if (20..60).contains(&x) && (20..60).contains(&y) {
            [200, 30, 40, 255]
        } else if dx * dx + dy * dy < 400 {
            [10, 10, 10, 128]
        } else {
            [0, 0, 0, 0]
        }
    });
    let optimized = optimize_png(encoded.clone());
    assert!(
        optimized.len() < encoded.len(),
        "recode must reduce {} bytes but produced {}",
        encoded.len(),
        optimized.len()
    );

    let before = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png)
        .expect("decode")
        .to_rgba8();
    let after = image::load_from_memory_with_format(&optimized, image::ImageFormat::Png)
        .expect("decode")
        .to_rgba8();
    assert_eq!(before.dimensions(), after.dimensions());
    for (index, (before, after)) in before
        .as_raw()
        .chunks_exact(4)
        .zip(after.as_raw().chunks_exact(4))
        .enumerate()
    {
        if before[3] == 0 {
            // Colour under a fully transparent pixel is not part of the
            // image; only its transparency has to survive.
            assert_eq!(after[3], 0, "pixel {index} must stay transparent");
            continue;
        }
        assert_eq!(before, after, "pixel {index} must be unchanged");
    }
}
