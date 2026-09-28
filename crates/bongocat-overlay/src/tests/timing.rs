//! The collector is bounded, and its percentiles are frames that happened.

use super::*;

#[test]
fn frame_timing_uses_nearest_rank_percentiles_and_counts_misses() {
    let mut timing = FrameTimingCollector::new();
    for sample_us in [10, 90, 20, 80, 30, 70, 40, 60, 50, 100] {
        timing.record_draw(Duration::from_micros(sample_us));
    }
    timing.record_missed_deadline();
    timing.record_missed_deadline();

    assert_eq!(
        timing.summary(),
        FrameTimingSummary {
            sample_count: 10,
            samples_dropped: 0,
            draw_p50_us: 50,
            draw_p95_us: 100,
            draw_p99_us: 100,
            missed_deadlines: 2,
        }
    );
}

#[test]
fn frame_timing_empty_collector_reports_zero_quantiles() {
    assert_eq!(
        FrameTimingCollector::new().summary(),
        FrameTimingSummary {
            sample_count: 0,
            samples_dropped: 0,
            draw_p50_us: 0,
            draw_p95_us: 0,
            draw_p99_us: 0,
            missed_deadlines: 0,
        }
    );
}

#[test]
fn frame_timing_drops_samples_after_its_fixed_capacity() {
    let mut timing = FrameTimingCollector::new();
    for _ in 0..=MAX_FRAME_TIMING_SAMPLES {
        timing.record_draw(Duration::from_micros(7));
    }

    let summary = timing.summary();
    assert_eq!(summary.sample_count as usize, MAX_FRAME_TIMING_SAMPLES);
    assert_eq!(summary.samples_dropped, 1);
    assert_eq!(summary.draw_p50_us, 7);
    assert_eq!(summary.draw_p95_us, 7);
    assert_eq!(summary.draw_p99_us, 7);
}
