//! Turning one captured overlay frame into a model cover.
//!
//! The per-platform renderers produce the pixels (see the `CoverCaptureSession`
//! in `macos.rs` and `windows.rs`); everything that does not depend on a GPU
//! backend lives here, so it is covered by ordinary unit tests on any supported
//! platform instead of only by the platform smoke.
//!
//! A captured frame is what the overlay window shows: the model's own background
//! image, the Live2D drawables, and transparent pixels wherever the window has no
//! content. The cover keeps that composition — it is what identifies the model in
//! the settings window, and a converted source gives each input mode its own
//! background — but trims the empty margin so the artwork fills the card.

use crate::OverlayError;
use image::ImageEncoder;
use std::time::Duration;

/// The cover capture renders at twice the overlay's default box. Framing does not
/// depend on it — the window box and the model are both fitted from the canvas
/// aspect, so the composition is the same at any scale — but the readback size
/// does: at 200% a 350-pixel window reads back 700 pixels wide on a 1x display
/// and 1400 on Retina, which is what the crop and downscale below start from.
/// Capturing at 100% would produce 350 pixels and let the card upscale them.
pub(crate) const COVER_CAPTURE_SCALE_PERCENT: u16 = 200;

/// Frames drawn before the capture is taken. The runtime animates the model from
/// its first published frame, so a handful of frames let the idle pose and its
/// physics settle into the state a user would see in the overlay.
pub(crate) const COVER_CAPTURE_FRAMES: u32 = 30;

/// Bound on the frame loop above. A runtime that stops publishing frames must not
/// leave the thread that owns the capture window drawing forever, so the loop
/// ends here and the most recent frame is the one encoded.
pub(crate) const COVER_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest side of a written cover, in pixels.
///
/// The settings window draws a cover at roughly 400 logical pixels wide and
/// scales it with `object_fit: Cover`, so a 640-pixel capture stays crisp on a
/// Retina display without writing a multi-megabyte image into the model package.
const COVER_MAX_SIDE: u32 = 640;

/// A captured frame with fewer opaque pixels than this is treated as empty. It
/// is also the floor the crop keeps, so a degenerate capture cannot produce a
/// zero-sized image.
const COVER_MIN_CONTENT_SIDE: u32 = 8;

/// Highest capture dimension accepted before the readback is rejected. It is the
/// same ceiling the overlay window geometry accepts, so a backend that reports
/// an impossible size fails loudly instead of allocating for it.
const CAPTURE_MAX_SIDE: u32 = 16_384;

/// A channel value at or below this counts as transparent when the content
/// bounds are computed, so anti-aliased edges and a black shadow do not stretch
/// the crop to the whole window.
const CONTENT_ALPHA_THRESHOLD: u8 = 8;

/// One captured overlay frame in straight-alpha RGBA8, top row first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapturedFrame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl CapturedFrame {
    /// Convert a backend readback of premultiplied BGRA8 into straight-alpha RGBA8.
    ///
    /// Both backends composite premultiplied — D3D11 through its blend state and
    /// Metal through the same factors — and both present BGRA, so the conversion
    /// is shared rather than repeated per platform. `row_pitch` is the backend's
    /// row stride: D3D11's staging texture pads rows, Metal's `get_bytes` does
    /// not, and both are expressed the same way here.
    ///
    /// Alpha is un-premultiplied because PNG carries straight alpha: writing the
    /// premultiplied values would darken every semi-transparent edge.
    pub(crate) fn from_premultiplied_bgra(
        bytes: &[u8],
        row_pitch: usize,
        width: u32,
        height: u32,
    ) -> Result<Self, OverlayError> {
        validate_dimensions(width, height)?;
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| OverlayError::new("captured frame width overflows its row size"))?;
        if row_pitch < row_bytes {
            return Err(OverlayError::new(
                "captured frame row pitch is smaller than one row",
            ));
        }
        let required = usize::try_from(height)
            .ok()
            .and_then(|height| height.checked_mul(row_pitch))
            .ok_or_else(|| OverlayError::new("captured frame size overflows"))?;
        if bytes.len() < required {
            return Err(OverlayError::new(
                "captured frame is shorter than its pitch",
            ));
        }

        let mut pixels = vec![0_u8; pixel_len(width, height)?];
        for (row, destination) in pixels.chunks_exact_mut(row_bytes).enumerate() {
            let source = &bytes[row * row_pitch..row * row_pitch + row_bytes];
            for (pixel, destination) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4))
            {
                let alpha = pixel[3];
                let (red, green, blue) = if alpha == 0 {
                    (0, 0, 0)
                } else if alpha == 255 {
                    (pixel[2], pixel[1], pixel[0])
                } else {
                    let unpremultiply = |channel: u8| {
                        let scaled = u32::from(channel) * 255 + u32::from(alpha) / 2;
                        u8::try_from((scaled / u32::from(alpha)).min(255))
                            .expect("un-premultiplied channel stays in range")
                    };
                    (
                        unpremultiply(pixel[2]),
                        unpremultiply(pixel[1]),
                        unpremultiply(pixel[0]),
                    )
                };
                destination.copy_from_slice(&[red, green, blue, alpha]);
            }
        }
        Ok(Self {
            pixels,
            width,
            height,
        })
    }

    pub(crate) fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// A model cover authored as PNG, ready to be written into a model package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCoverCapture {
    png: Vec<u8>,
    width: u32,
    height: u32,
}

impl ModelCoverCapture {
    pub fn png(&self) -> &[u8] {
        &self.png
    }

    /// The size the cover was written with, for the capture's own log line.
    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }
}

/// The crop a frame's visible content occupies, as `(x, y, width, height)`.
///
/// `None` means nothing in the frame is visible, which is a capture failure
/// rather than a cover: the caller falls back to whatever cover the package
/// already shipped.
pub(crate) fn content_bounds(frame: &CapturedFrame) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = frame.width;
    let mut min_y = frame.height;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    let mut found = false;
    for y in 0..frame.height {
        let row = &frame.pixels[y as usize * frame.width as usize * 4..];
        for x in 0..frame.width {
            if row[x as usize * 4 + 3] > CONTENT_ALPHA_THRESHOLD {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !found {
        return None;
    }
    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    if width < COVER_MIN_CONTENT_SIDE || height < COVER_MIN_CONTENT_SIDE {
        return None;
    }
    Some((min_x, min_y, width, height))
}

/// Crop `frame` to `bounds`, keeping every pixel inside the requested box.
pub(crate) fn crop(frame: &CapturedFrame, bounds: (u32, u32, u32, u32)) -> CapturedFrame {
    let (x, y, width, height) = bounds;
    let row_bytes = frame.width as usize * 4;
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in y..y + height {
        let start = row as usize * row_bytes + x as usize * 4;
        pixels.extend_from_slice(&frame.pixels[start..start + width as usize * 4]);
    }
    CapturedFrame {
        pixels,
        width,
        height,
    }
}

/// Scale `frame` down so its longest side is at most `maximum_side`.
///
/// A capture already smaller than the limit is returned unchanged: upscaling a
/// small window would only blur it, and the settings window scales the cover to
/// its card anyway.
pub(crate) fn downscale(frame: CapturedFrame, maximum_side: u32) -> CapturedFrame {
    let longest = frame.width.max(frame.height);
    if longest <= maximum_side {
        return frame;
    }
    let ratio = f64::from(maximum_side) / f64::from(longest);
    let width = ((f64::from(frame.width) * ratio).round() as u32).max(1);
    let height = ((f64::from(frame.height) * ratio).round() as u32).max(1);
    let Some(source) = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels.clone())
    else {
        return frame;
    };
    let resized = image::imageops::resize(
        &source,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    CapturedFrame {
        pixels: resized.into_raw(),
        width,
        height,
    }
}

/// Turn a captured frame into the PNG a model package stores as its cover.
pub(crate) fn encode_cover(frame: CapturedFrame) -> Result<ModelCoverCapture, OverlayError> {
    let bounds = content_bounds(&frame).ok_or_else(|| {
        OverlayError::new("captured frame has no visible content to use as a cover")
    })?;
    let frame = downscale(crop(&frame, bounds), COVER_MAX_SIDE);
    let png = encode_png(frame.pixels(), frame.width, frame.height)?;
    Ok(ModelCoverCapture {
        png,
        width: frame.width,
        height: frame.height,
    })
}

fn encode_png(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, OverlayError> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(pixels, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| OverlayError::new(format!("model cover cannot be encoded: {error}")))?;
    Ok(png)
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), OverlayError> {
    if width == 0 || height == 0 || width > CAPTURE_MAX_SIDE || height > CAPTURE_MAX_SIDE {
        return Err(OverlayError::new("captured frame dimensions are invalid"));
    }
    Ok(())
}

fn pixel_len(width: u32, height: u32) -> Result<usize, OverlayError> {
    let row = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| OverlayError::new("captured frame width overflows its row size"))?;
    usize::try_from(height)
        .ok()
        .and_then(|height| row.checked_mul(height))
        .ok_or_else(|| OverlayError::new("captured frame size overflows"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> CapturedFrame {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&pixel(x, y));
            }
        }
        CapturedFrame {
            pixels,
            width,
            height,
        }
    }

    #[test]
    fn un_premultiplied_alpha_survives_the_bgra_conversion() {
        let premultiplied = [
            0, 0, 0, 0, // transparent
            0, 0, 255, 255, // opaque red (BGRA)
            0, 0, 128, 128, // half-covered red
            64, 0, 0, 128, // half-covered blue
        ];
        let converted =
            CapturedFrame::from_premultiplied_bgra(&premultiplied, 16, 4, 1).expect("conversion");
        assert_eq!(
            converted.pixels,
            vec![0, 0, 0, 0, 255, 0, 0, 255, 255, 0, 0, 128, 0, 0, 128, 128]
        );
    }

    #[test]
    fn padded_rows_are_read_at_the_backend_pitch() {
        // Two 1x1 rows with four bytes of D3D11 padding after each one.
        let padded = [0, 0, 255, 255, 9, 9, 9, 9, 0, 255, 0, 255, 9, 9, 9, 9];
        let converted =
            CapturedFrame::from_premultiplied_bgra(&padded, 8, 1, 2).expect("conversion");
        assert_eq!(converted.width, 1);
        assert_eq!(converted.height, 2);
        assert_eq!(converted.pixels, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn a_short_readback_is_rejected_instead_of_read_out_of_bounds() {
        assert!(CapturedFrame::from_premultiplied_bgra(&[0; 4], 4, 2, 2).is_err());
        assert!(CapturedFrame::from_premultiplied_bgra(&[0; 16], 2, 2, 2).is_err());
        assert!(CapturedFrame::from_premultiplied_bgra(&[], 4, 0, 4).is_err());
    }

    #[test]
    fn the_crop_keeps_only_the_visible_content() {
        let frame = frame(16, 16, |x, y| {
            if (4..12).contains(&x) && (6..14).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert_eq!(content_bounds(&frame), Some((4, 6, 8, 8)));
        let cropped = crop(&frame, content_bounds(&frame).expect("content"));
        assert_eq!((cropped.width, cropped.height), (8, 8));
        assert!(cropped.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn faint_edges_do_not_stretch_the_crop() {
        let frame = frame(16, 16, |x, y| {
            if (4..12).contains(&x) && (4..12).contains(&y) {
                [255, 0, 0, 255]
            } else if x == 0 || y == 15 {
                [255, 0, 0, CONTENT_ALPHA_THRESHOLD]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert_eq!(content_bounds(&frame), Some((4, 4, 8, 8)));
    }

    #[test]
    fn an_empty_or_degenerate_frame_is_not_a_cover() {
        let empty = frame(16, 16, |_, _| [0, 0, 0, 0]);
        assert_eq!(content_bounds(&empty), None);
        let sliver = frame(16, 16, |x, _| {
            if x < 4 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert_eq!(content_bounds(&sliver), None);
    }

    #[test]
    fn a_capture_is_only_ever_scaled_down() {
        let small = frame(320, 200, |_, _| [10, 20, 30, 255]);
        let unchanged = downscale(small.clone(), COVER_MAX_SIDE);
        assert_eq!((unchanged.width, unchanged.height), (320, 200));
        assert_eq!(unchanged.pixels, small.pixels);

        let large = downscale(frame(1600, 1200, |_, _| [10, 20, 30, 255]), 640);
        assert_eq!((large.width, large.height), (640, 480));
        assert_eq!(large.pixels.len(), 640 * 480 * 4);
    }

    #[test]
    fn the_encoded_cover_is_a_png_of_the_cropped_capture() {
        let frame = frame(64, 48, |x, y| {
            if (8..56).contains(&x) && (4..44).contains(&y) {
                [200, 30, 40, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let cover = encode_cover(frame).expect("cover");
        assert_eq!((cover.width(), cover.height()), (48, 40));
        assert_eq!(&cover.png()[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(cover.png()).expect("cover decodes");
        assert_eq!((decoded.width(), decoded.height()), (48, 40));
    }

    #[test]
    fn a_frame_without_content_is_reported_rather_than_written() {
        let empty = frame(32, 32, |_, _| [0, 0, 0, 0]);
        assert!(encode_cover(empty).is_err());
    }
}
