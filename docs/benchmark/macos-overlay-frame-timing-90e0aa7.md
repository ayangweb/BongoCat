# macOS Metal Overlay Frame Timing: 90e0aa7

Date: 2026-09-06

This is a single-device diagnostic baseline for the macOS Live2D overlay. It
validates the newly added bounded preview timing output; it is not a stable
release performance claim or a replacement for the Phase 8 platform matrix.

## Build and Environment

- Commit: `90e0aa7a219889c4106966555be54de91c6d8321`
- Command: `cargo run -p bongocat-overlay --release --locked -- standard 30`
- Build: release (`opt-level=3`, thin LTO, symbols stripped)
- OS: macOS 26.5.2 (25F84)
- CPU/GPU: Apple M1 Pro, 16 GPU cores, Metal 4
- Memory: 16 GiB
- Display: built-in Liquid Retina XDR, 3456x2234 physical pixels, Retina
- Model: bundled `standard` preset (`cat.model3.json`, 21 drawables, 5 masked,
  3 textures)
- Overlay geometry: default preview options, 350 logical-pixel width and height
  derived from the model canvas; no persisted bounds or explicit scale override
- Target cadence: 60 FPS (`16,667 us` interval)
- Input: the non-interactive preview's deterministic model-specific keyboard and
  cursor script. No CGEventTap, physical keyboard, mouse, or controller input.

## Method

The release binary was already built locally, then launched as a fresh preview
process for 30 seconds. The timing collector began before the initial Metal
draw. For every successful `NativeOverlay::draw` call it stored the elapsed
microseconds through the return from the renderer; it does not include AppKit
event pumping, deterministic input generation, runtime handoff, or the pacing
sleep. Percentiles use nearest-rank selection over sorted samples.

After each full paced-loop iteration, the preview counted a missed deadline when
that complete main-thread iteration had already reached its next 60 FPS deadline
and could not sleep. The collector retains at most 4,096 samples and reports
overflow separately. Raw values are in
[`data/macos-overlay-frame-timing-90e0aa7.csv`](data/macos-overlay-frame-timing-90e0aa7.csv).

No CPU, RSS, GPU-utilization, power, input-latency, cold-start, or
runtime-to-present measurement was collected. Instruments, Metal System Trace,
and os_signpost were not used for this run. Display brightness and concurrent
desktop activity were not controlled.

## Result

| Metric                 |    Value |
| ---------------------- | -------: |
| Frames presented       |    1,798 |
| Draw timing samples    |    1,798 |
| Dropped timing samples |        0 |
| Draw p50               | 1.741 ms |
| Draw p95               | 3.148 ms |
| Draw p99               | 4.274 ms |
| Missed deadlines       |        2 |
| Render frames consumed |    1,337 |
| Dynamic snapshots      |    1,334 |

For this one warm local run, the measured `draw` p95 was below the 60 FPS
16.7 ms target. That does not satisfy the release exit metric: it omits
cross-device, Windows, long-running, instrumented, and input-latency evidence.
