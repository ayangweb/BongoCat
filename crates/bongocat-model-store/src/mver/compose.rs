//! Composing a key cap and a paw into the one image the product draws.
//!
//! The legacy source keeps the two layers apart; the product draws one image per
//! key. Compositing is per pixel with the paw on top, and the result is
//! re-encoded only when that makes the file smaller, so a conversion never
//! costs the user image quality it did not have to.

use super::*;

/// Compose one key image: the paw drawn over the key cap.
///
/// Both layers are the same canvas in every real model, and the legacy
/// application draws them into a canvas no larger than the smaller of the two,
/// anchored at the origin — so a layer larger than the canvas is cropped rather
/// than scaled, and the canvas size is the minimum of the two.
pub(crate) fn compose_key_image(
    keyboard: &[u8],
    hand: &[u8],
    reference: &str,
) -> Result<Vec<u8>, ModelStoreError> {
    let keyboard = decode_png(keyboard, reference)?;
    let hand = decode_png(hand, reference)?;
    let width = keyboard.width().min(hand.width());
    let height = keyboard.height().min(hand.height());
    if width == 0 || height == 0 {
        return Err(conversion_error(
            Some(reference),
            "legacy key image has no pixels",
        ));
    }

    let mut canvas = vec![0_u8; width as usize * height as usize * 4];
    for layer in [&keyboard, &hand] {
        let stride = layer.width() as usize * 4;
        let source = layer.as_raw();
        for y in 0..height as usize {
            let source_row = y * stride;
            let canvas_row = y * (width as usize) * 4;
            for x in 0..width as usize {
                composite_pixel(
                    &mut canvas[canvas_row + x * 4..canvas_row + x * 4 + 4],
                    &source[source_row + x * 4..source_row + x * 4 + 4],
                );
            }
        }
    }

    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&canvas, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| {
            conversion_error(
                Some(reference),
                format!("composed key image cannot be encoded: {error}"),
            )
        })?;
    Ok(optimize_png(encoded))
}

pub(crate) fn decode_png(bytes: &[u8], reference: &str) -> Result<RgbaImage, ModelStoreError> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map(|image| image.to_rgba8())
        .map_err(|error| {
            conversion_error(
                Some(reference),
                format!("legacy key image is not a readable PNG: {error}"),
            )
        })
}

/// Composite one straight-alpha source pixel over one straight-alpha
/// destination pixel, the way a canvas `source-over` draw does.
///
/// The integer form of the Porter-Duff `over` operator is exact for the two
/// cases that carry all the visual weight — an opaque source replaces the
/// destination and an empty source leaves it untouched — and rounds once for
/// the antialiased edge pixels in between, where a floating-point form would
/// differ by at most one step per channel.
pub(crate) fn composite_pixel(destination: &mut [u8], source: &[u8]) {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    if source_alpha == 255 {
        destination.copy_from_slice(source);
        return;
    }
    let destination_alpha = u32::from(destination[3]);
    let inverse = 255 - source_alpha;
    let output_alpha = source_alpha + (destination_alpha * inverse + 127) / 255;
    if output_alpha == 0 {
        destination.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    let scale = 255 * output_alpha;
    for channel in 0..3 {
        let numerator = u32::from(source[channel]) * source_alpha * 255
            + u32::from(destination[channel]) * destination_alpha * inverse;
        destination[channel] = ((numerator + scale / 2) / scale) as u8;
    }
    destination[3] = output_alpha as u8;
}

/// Recode a composed PNG losslessly.
///
/// The composed pixels are produced by this module, so correctness never
/// depends on this step: every reduction the library applies by default (bit
/// depth, colour type, palette and greyscale) preserves the decoded pixels, and
/// `optimize_alpha` only rewrites the colour channels of pixels that are
/// already fully transparent. A recode the library refuses is written as the
/// plain encoding instead of failing the conversion, because the file is a
/// valid PNG either way and only its size is at stake.
pub(crate) fn optimize_png(encoded: Vec<u8>) -> Vec<u8> {
    let options = oxipng::Options {
        optimize_alpha: true,
        ..oxipng::Options::default()
    };
    match oxipng::optimize_from_memory(&encoded, &options) {
        Ok(optimized) if optimized.len() <= encoded.len() => optimized,
        _ => encoded,
    }
}
