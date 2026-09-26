//! Frame pacing, the frame interval and the work budget diagnostics.

use super::*;

#[test]
fn maximum_fps_interval_enforces_runtime_bounds() {
    assert_eq!(
        frame_interval_for_maximum_fps(60),
        Some(Duration::from_secs_f64(1.0 / 60.0))
    );
    assert!(frame_interval_for_maximum_fps(MINIMUM_FPS).is_some());
    assert!(frame_interval_for_maximum_fps(MAXIMUM_FPS).is_some());
    assert!(frame_interval_for_maximum_fps(MINIMUM_FPS - 1).is_none());
    assert!(frame_interval_for_maximum_fps(MAXIMUM_FPS + 1).is_none());
    assert_eq!(
        runtime_tick_work_budget(60),
        Duration::from_secs_f64(1.0 / 120.0)
    );
    let budget_120 = runtime_tick_work_budget(120);
    assert!(budget_120.abs_diff(Duration::from_secs_f64(1.0 / 240.0)) <= Duration::from_nanos(1));
    assert_eq!(runtime_tick_work_budget(0), Duration::from_millis(8));
    assert_eq!(
        frame_interval_for_runtime(MINIMUM_FPS, false),
        Some(HIDDEN_OVERLAY_FRAME_INTERVAL)
    );
    assert_eq!(
        frame_interval_for_runtime(MAXIMUM_FPS, false),
        Some(HIDDEN_OVERLAY_FRAME_INTERVAL)
    );
    assert!(frame_interval_for_runtime(MINIMUM_FPS - 1, false).is_none());
}

#[test]
fn work_budget_diagnostics_only_record_actual_overruns() {
    let mut diagnostics = RuntimeWorkDiagnostics::default();
    record_work_budget(
        &mut diagnostics,
        Duration::from_millis(10),
        Duration::from_millis(10),
    );
    assert_eq!(diagnostics, RuntimeWorkDiagnostics::default());

    record_work_budget(
        &mut diagnostics,
        Duration::from_millis(27),
        Duration::from_millis(10),
    );
    assert_eq!(
        diagnostics,
        RuntimeWorkDiagnostics {
            budget_exceeded: 1,
            last_over_budget_ms: 27,
        }
    );

    record_work_budget(
        &mut diagnostics,
        Duration::from_millis(9),
        Duration::from_millis(10),
    );
    assert_eq!(diagnostics.last_over_budget_ms, 27);

    diagnostics.budget_exceeded = u64::MAX;
    record_work_budget(
        &mut diagnostics,
        Duration::from_millis(30),
        Duration::from_millis(10),
    );
    assert_eq!(diagnostics.budget_exceeded, u64::MAX);
    assert_eq!(diagnostics.last_over_budget_ms, 30);
}

#[test]
fn frame_pacer_keeps_the_cadence_on_the_configured_rate() {
    let start = Instant::now();
    let interval = Duration::from_millis(10);
    let mut pacer = FramePacer::new(start, interval);
    assert_eq!(pacer.wait(start, interval), interval);

    // The first frame lands on its deadline. Whatever work it cost is
    // already spent, so the wait for the next frame is the remainder of the
    // interval rather than a fresh interval on top of that work — the
    // property that keeps the achieved cadence equal to `maximum_fps`.
    let first = start + interval;
    pacer.frame_produced(first, interval);
    assert_eq!(
        pacer.wait(first + Duration::from_millis(3), interval),
        Duration::from_millis(7)
    );

    // A command-driven frame ahead of its slot leaves the slot pending, so
    // it neither skips nor re-anchors the periodic frame.
    let early = first + Duration::from_millis(1);
    pacer.frame_produced(early, interval);
    assert_eq!(pacer.wait(early, interval), Duration::from_millis(9));

    // A frame that overran its slot skips the slots it missed instead of
    // producing a catch-up burst.
    let late = first + Duration::from_millis(35);
    pacer.frame_produced(late, interval);
    assert_eq!(pacer.wait(late, interval), interval);

    // A schedule change re-anchors the grid, which is how a `maximum_fps`
    // change and the hidden-overlay throttle take effect without a restart.
    let faster = Duration::from_millis(4);
    assert_eq!(pacer.wait(late, faster), faster);
    pacer.frame_produced(late + faster, faster);
    assert_eq!(pacer.wait(late + faster, faster), faster);
}

#[test]
fn runtime_worker_frame_pacing_reaches_the_configured_maximum_fps() {
    // A frame source that waits a whole interval after each frame only ever
    // reaches `interval + work`; at the 60 FPS default that measured 48.6
    // frames per second before `FramePacer`. The tolerance keeps the guard
    // off the scheduler's wake-up jitter while still rejecting that drift.
    const TARGET_FPS: u16 = 60;
    const WARM_UP: Duration = Duration::from_millis(300);
    const WINDOW: Duration = Duration::from_secs(2);
    const MINIMUM_RATIO: f64 = 0.9;

    let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let fps = client
        .send(RuntimeCommand::SetMaximumFps(TARGET_FPS))
        .expect("fps command");
    client.wait_for_command(fps, TIMEOUT).expect("fps applied");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let initial = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &initial);

    std::thread::sleep(WARM_UP);
    let before = consumer.diagnostics().published;
    let started = Instant::now();
    std::thread::sleep(WINDOW);
    let elapsed = started.elapsed();
    let published = consumer.diagnostics().published - before;
    let achieved = published as f64 / elapsed.as_secs_f64();
    assert!(
        achieved >= f64::from(TARGET_FPS) * MINIMUM_RATIO,
        "{published} frames in {elapsed:?} is {achieved:.1} FPS, below 90% of {TARGET_FPS}"
    );
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn hover_hide_delay_converts_whole_seconds_to_the_frame_clock() {
    assert_eq!(hover_hide_delay_ms(0), 0);
    assert_eq!(hover_hide_delay_ms(1), 1_000);
    assert_eq!(hover_hide_delay_ms(3), 3_000);
    assert_eq!(
        hover_hide_delay_ms(MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS),
        MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS
    );
    // The multiply saturates instead of wrapping, so an out-of-range second
    // value stays above the shared ceiling and the option validation keeps
    // rejecting it instead of seeing a small wrapped delay.
    assert!(
        hover_hide_delay_ms(MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS + 1)
            > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS
    );
    assert!(hover_hide_delay_ms(u32::MAX) > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS);
}
