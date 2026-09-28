//! How long a frame took, and how the overlay's own frame timing is summarised.
//!
//! The collector is bounded, so a machine that renders for a week does not grow
//! an unbounded sample list; what it drops is the oldest, which is the part a
//! reader of a percentile is least interested in. Percentiles are nearest-rank
//! rather than interpolated, so a reported number is a frame that actually
//! happened.

#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewReport {
    pub frames_presented: u64,
    pub dynamic_snapshots: u64,
    pub runtime_input_events: u64,
    pub platform_input_edges: u64,
    pub runtime_cursor_published: u64,
    pub runtime_cursor_coalesced: u64,
    pub runtime_cursor_consumed: u64,
    pub platform_cursor_samples: u64,
    pub render_frames_published: u64,
    pub render_frames_coalesced: u64,
    pub render_frames_consumed: u64,
    pub model_switches: u64,
    pub failed_gpu_prepare_preserved: bool,
    pub gpu_bytes_before: u64,
    pub gpu_bytes_after: u64,
    pub drawable_count: usize,
    pub masked_drawable_count: usize,
    pub texture_count: usize,
    /// Windows switch-probe thread observation: the warmup high-water mark the
    /// settled count is measured against. The macOS probe does not sample
    /// process thread counts and leaves it absent rather than reporting zero.
    pub warmup_thread_high_water: Option<u32>,
    /// Windows switch-probe settled thread count after the measurement interval,
    /// reported even on success so the remaining allowance is visible.
    pub threads_after: Option<u32>,
    /// Timing from the diagnostic preview's renderer call only. Product frame
    /// sources do not collect this data, and previews without a paced loop
    /// leave it absent rather than reporting zeroes as measurements.
    pub frame_timing: Option<FrameTimingSummary>,
}

/// Bounded diagnostic timing data for a paced preview loop.
///
/// `draw_*_us` measures the interval around `NativeOverlay::draw`, including
/// the backend's submit/present work but excluding input, runtime handoff, and
/// the preview loop's sleep. `missed_deadlines` counts paced-loop iterations
/// whose complete main-thread work reached the next 60 FPS deadline before the
/// loop could sleep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameTimingSummary {
    pub sample_count: u32,
    pub samples_dropped: u64,
    pub draw_p50_us: u64,
    pub draw_p95_us: u64,
    pub draw_p99_us: u64,
    pub missed_deadlines: u64,
}

#[cfg(any(target_os = "macos", test))]
pub(crate) const MAX_FRAME_TIMING_SAMPLES: usize = 4_096;

/// Collect a fixed maximum number of exact microsecond samples so diagnostic
/// previews cannot grow memory use during long-running benchmark sessions.
#[derive(Debug)]
#[cfg(any(target_os = "macos", test))]
pub(crate) struct FrameTimingCollector {
    pub(crate) draw_samples_us: Vec<u64>,
    pub(crate) samples_dropped: u64,
    pub(crate) missed_deadlines: u64,
}

#[cfg(any(target_os = "macos", test))]
impl FrameTimingCollector {
    pub(crate) fn new() -> Self {
        Self {
            draw_samples_us: Vec::with_capacity(MAX_FRAME_TIMING_SAMPLES),
            samples_dropped: 0,
            missed_deadlines: 0,
        }
    }

    pub(crate) fn record_draw(&mut self, elapsed: Duration) {
        let elapsed_us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        if self.draw_samples_us.len() < MAX_FRAME_TIMING_SAMPLES {
            self.draw_samples_us.push(elapsed_us);
        } else {
            self.samples_dropped = self.samples_dropped.saturating_add(1);
        }
    }

    pub(crate) fn record_missed_deadline(&mut self) {
        self.missed_deadlines = self.missed_deadlines.saturating_add(1);
    }

    pub(crate) fn summary(mut self) -> FrameTimingSummary {
        self.draw_samples_us.sort_unstable();
        let sample_count = u32::try_from(self.draw_samples_us.len())
            .expect("frame timing collector capacity fits in u32");
        FrameTimingSummary {
            sample_count,
            samples_dropped: self.samples_dropped,
            draw_p50_us: percentile_nearest_rank(&self.draw_samples_us, 50),
            draw_p95_us: percentile_nearest_rank(&self.draw_samples_us, 95),
            draw_p99_us: percentile_nearest_rank(&self.draw_samples_us, 99),
            missed_deadlines: self.missed_deadlines,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn percentile_nearest_rank(sorted_samples: &[u64], percentile: u8) -> u64 {
    if sorted_samples.is_empty() {
        return 0;
    }
    let rank = (sorted_samples.len() * usize::from(percentile)).div_ceil(100);
    sorted_samples[rank.saturating_sub(1)]
}
