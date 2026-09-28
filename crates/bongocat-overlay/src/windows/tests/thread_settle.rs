//! What counts as a leak and what counts as a late driver worker.

use super::*;

#[test]
fn accepts_a_bounded_late_thread_worker_but_rejects_per_switch_growth() {
    // The warmup high-water mark is a snapshot, so a process-global D3D11,
    // DXGI, or thread-pool worker created after the warmup settle window may
    // lift the settled count by a bounded step without being an overlay
    // leak. Regression coverage for the model-switch smoke that failed with
    // `high-water mark 12 with 13 threads`.
    assert!(!thread_growth_exceeded(12, 12));
    assert!(!thread_growth_exceeded(12, 13));
    assert!(!thread_growth_exceeded(12, 12 + THREAD_GROWTH_LIMIT));
    assert!(!thread_growth_exceeded(12, 8));

    // A per-switch leak grows with the measured switch count, so it stays far
    // beyond the allowance and the gate still fails.
    assert!(thread_growth_exceeded(12, 12 + THREAD_GROWTH_LIMIT + 1));
    assert!(thread_growth_exceeded(12, 13 + 300));

    // The ceiling saturates rather than wrapping when a baseline cannot grow.
    assert!(!thread_growth_exceeded(u32::MAX, u32::MAX));
}
