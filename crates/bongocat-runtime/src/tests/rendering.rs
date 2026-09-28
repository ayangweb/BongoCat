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
