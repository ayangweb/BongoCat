//! Coalescing an overlay drag into one placement write.

use super::*;

#[test]
fn overlay_placement_debouncer_coalesces_drag_updates_and_flushes_latest() {
    let origin = Instant::now();
    let first = OverlayWindowBounds::new(0, 0, 420, 560);
    let middle = OverlayWindowBounds::new(12, 8, 420, 560);
    let latest = OverlayWindowBounds::new(24, 16, 420, 560);
    let mut debouncer = OverlayPlacementDebouncer::default();

    assert_eq!(debouncer.observe(first, origin), Some(first));
    debouncer.mark_sent(first);
    assert_eq!(
        debouncer.observe(middle, origin + Duration::from_millis(50)),
        None
    );
    assert_eq!(
        debouncer.observe(latest, origin + Duration::from_millis(100)),
        None
    );
    assert_eq!(
        debouncer.observe(latest, origin + Duration::from_millis(150)),
        Some(latest)
    );
    debouncer.mark_sent(latest);
    assert_eq!(
        debouncer.flush(origin + Duration::from_millis(200)),
        None,
        "the stable update was already submitted"
    );
}

#[test]
fn overlay_placement_debouncer_flushes_pending_update_on_shutdown() {
    let origin = Instant::now();
    let first = OverlayWindowBounds::new(0, 0, 420, 560);
    let latest = OverlayWindowBounds::new(24, 16, 420, 560);
    let mut debouncer = OverlayPlacementDebouncer::default();

    assert_eq!(debouncer.observe(first, origin), Some(first));
    debouncer.mark_sent(first);
    assert_eq!(
        debouncer.observe(latest, origin + Duration::from_millis(25)),
        None
    );
    assert_eq!(
        debouncer.flush(origin + Duration::from_millis(30)),
        Some(latest)
    );
    debouncer.mark_sent(latest);
    assert_eq!(debouncer.flush(origin + Duration::from_millis(31)), None);
}

#[test]
fn overlay_placement_debouncer_keeps_unsent_value_for_retry() {
    let origin = Instant::now();
    let first = OverlayWindowBounds::new(0, 0, 420, 560);
    let latest = OverlayWindowBounds::new(24, 16, 420, 560);
    let mut debouncer = OverlayPlacementDebouncer::default();

    assert_eq!(debouncer.observe(first, origin), Some(first));
    // Simulate a full settings queue: the producer did not acknowledge either send.
    assert_eq!(
        debouncer.observe(latest, origin + Duration::from_millis(25)),
        None
    );
    assert_eq!(
        debouncer.flush(origin + Duration::from_millis(30)),
        Some(latest)
    );
    // A failed shutdown send must leave the latest bounds available for a retry.
    assert_eq!(
        debouncer.flush(origin + Duration::from_millis(31)),
        Some(latest)
    );
    debouncer.mark_sent(latest);
    assert_eq!(debouncer.flush(origin + Duration::from_millis(32)), None);
}
