//! A presented frame is a picture, and a GPU that is not ready yet backs off.

use super::*;

#[test]
fn frame_smoke_requires_transparent_and_antialiased_model_coverage() {
    let statistics = validate_frame_smoke([[0, 0, 0, 0], [32, 64, 96, 127], [200, 160, 120, 255]])
        .expect("transparent, translucent, and colorful model samples should pass");
    assert_eq!(statistics.transparent_pixels, 1);
    assert_eq!(statistics.translucent_pixels, 1);
    assert_eq!(statistics.opaque_pixels, 1);
    assert_eq!(statistics.distinct_visible_colors, 2);
}

/// A frame that never lands on a semi-transparent pixel is still a picture.
///
/// The fixed sample grid can miss anti-aliased model coverage entirely — a
/// one-texture model with hard alpha edges, drawn small in a large canvas,
/// composites to transparent-or-opaque at every sampled point. That frame used
/// to be rejected, and because the check gates a model commit the user saw a
/// correctly drawn model reported as "the selected model could not be
/// activated" while the same model activated fine on other poses.
#[test]
fn frame_smoke_accepts_a_frame_without_a_semi_transparent_sample() {
    let statistics = validate_frame_smoke([[0, 0, 0, 0], [10, 20, 30, 255], [200, 160, 120, 255]])
        .expect("opaque model coverage over transparent background should pass");
    assert_eq!(statistics.transparent_pixels, 1);
    assert_eq!(statistics.translucent_pixels, 0);
    assert_eq!(statistics.opaque_pixels, 2);
    assert_eq!(statistics.distinct_visible_colors, 2);
}

#[test]
fn frame_smoke_rejects_missing_coverage_and_color_variation() {
    assert_eq!(
        validate_frame_smoke([[10, 20, 30, 255]]),
        Err("renderer readback found no transparent overlay pixels")
    );
    assert_eq!(
        validate_frame_smoke([[0, 0, 0, 0], [0, 0, 0, 0]]),
        Err("renderer readback found no model pixels")
    );
    assert_eq!(
        validate_frame_smoke([[0, 0, 0, 0], [10, 20, 30, 127]]),
        Err("renderer readback found insufficient visible color variation")
    );
}

#[test]
fn temporary_drawable_failures_back_off_without_accumulating_error_reports() {
    let mut backoff = FrameRetryBackoff::default();
    let delays = (0..6)
        .map(|_| backoff.register_temporary_failure())
        .collect::<Vec<_>>();
    assert_eq!(
        delays,
        vec![
            Duration::from_millis(100),
            Duration::from_millis(200),
            Duration::from_millis(400),
            Duration::from_millis(800),
            Duration::from_secs(1),
            Duration::from_secs(1),
        ]
    );
    assert_eq!(
        OverlayTickOutcome::Deferred(delays[0]).retry_after(),
        Some(Duration::from_millis(100))
    );

    backoff.record_success();
    assert_eq!(
        backoff.register_temporary_failure(),
        Duration::from_millis(100)
    );
}
