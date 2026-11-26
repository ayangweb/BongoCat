//! The render error codes, renderer health and frame scheduling.

use super::*;

#[test]
fn render_error_codes_are_stable_and_unique() {
    let mut codes = RuntimeRenderErrorCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.iter().all(|code| !code.is_empty()));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), RuntimeRenderErrorCode::ALL.len());
    assert_eq!(
        RuntimeRenderErrorCode::GpuPreparationFailed.to_string(),
        "gpu_preparation_failed"
    );
}

#[test]
fn renderer_health_transitions_degrade_once_and_recover() {
    let snapshot = SnapshotCell {
        value: Mutex::new(RuntimeSnapshot::starting(
            true,
            true,
            MotionAudioClient::unavailable().diagnostics(),
        )),
        changed: Condvar::new(),
        activity: Default::default(),
    };
    publish(&snapshot, |current| current.state = RuntimeState::Ready);
    let ready_revision = snapshot
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .revision;

    update_renderer_health(
        &snapshot,
        Err(RuntimeRenderErrorCode::ModelEvaluationFailed),
    );
    let degraded = snapshot
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(degraded.state, RuntimeState::Degraded);
    assert_eq!(
        degraded.render_error,
        Some(RuntimeRenderErrorCode::ModelEvaluationFailed)
    );
    assert_eq!(degraded.revision, ready_revision + 1);

    update_renderer_health(
        &snapshot,
        Err(RuntimeRenderErrorCode::ModelEvaluationFailed),
    );
    assert_eq!(
        snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revision,
        degraded.revision
    );

    update_renderer_health(&snapshot, Ok(()));
    let recovered = snapshot
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(recovered.state, RuntimeState::Ready);
    assert_eq!(recovered.render_error, None);
    assert_eq!(recovered.revision, degraded.revision + 1);
}

#[test]
fn frame_scheduling_agrees_with_the_published_snapshot() {
    // A frame source paces itself from `frame_scheduling()` while the overlay
    // session reads the full snapshot. The two must never disagree, or the
    // frame loop would pace against a different cadence than the one the
    // runtime stored.
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    let ready = client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    assert_eq!(
        client.frame_scheduling(),
        FrameScheduling {
            maximum_fps: ready.maximum_fps,
            overlay_visible: ready.overlay_visible,
            video_output_active: ready.video_output_active,
        }
    );

    client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("command accepted");
    let changed = client
        .wait_for_revision(ready.revision + 1, TIMEOUT)
        .expect("updated snapshot");
    let scheduling = client.frame_scheduling();
    assert_eq!(scheduling.overlay_visible, changed.overlay_visible);
    assert!(!scheduling.overlay_visible);
    assert_eq!(scheduling.maximum_fps, changed.maximum_fps);
    assert_eq!(
        frame_interval_for_runtime(scheduling.maximum_fps, scheduling.overlay_visible),
        frame_interval_for_runtime(changed.maximum_fps, changed.overlay_visible)
    );
}

#[test]
fn video_output_keeps_hidden_model_frames_advancing() {
    let (owner, consumer) = RuntimeOwner::start_with_rendering(false, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let sequence = client
        .send(RuntimeCommand::SetVideoOutputStatus(
            VideoOutputStatus::Running,
        ))
        .expect("enable output");
    let active = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("output enabled");
    assert!(!active.overlay_visible);
    assert!(active.video_output_active);
    assert!(client.frame_scheduling().video_output_active);
    let activation = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activate model");
    let prepared = wait_for_prepared_model(&client, &consumer, activation);
    report_model_prepared(&client, &consumer, &prepared);
    let first = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == prepared.model_generation
            && frame.frame_number > prepared.frame_number
    });
    let later = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == first.model_generation && frame.frame_number > first.frame_number
    });
    assert!(later.frame_number > first.frame_number);
    let sequence = client
        .send(RuntimeCommand::SetVideoOutputStatus(
            VideoOutputStatus::Disabled,
        ))
        .expect("disable output");
    let inactive = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("output disabled");
    assert!(!inactive.overlay_visible);
    assert!(!inactive.video_output_active);
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}
