//! Shutdown aggregation across the runtime and the motion audio service.

use super::*;

#[test]
fn shutdown_results_preserve_single_failures_and_aggregate_dual_failures() {
    let runtime = combine_shutdown_results::<()>(Err(ShutdownError::TimedOut), Ok(()))
        .expect_err("runtime failure must be returned");
    assert!(matches!(
        runtime,
        ApplicationError::Shutdown(ShutdownError::TimedOut)
    ));

    let audio = combine_shutdown_results(Ok(()), Err(MotionAudioShutdownError::TimedOut))
        .expect_err("audio failure must be returned");
    assert!(matches!(audio, ApplicationError::MotionAudioShutdown(_)));

    let dual = combine_shutdown_results::<()>(
        Err(ShutdownError::WorkerPanicked),
        Err(MotionAudioShutdownError::TimedOut),
    )
    .expect_err("both service failures must be retained");
    match dual {
        ApplicationError::ShutdownAggregate(error) => {
            assert_eq!(error.runtime, ShutdownError::WorkerPanicked);
            assert_eq!(error.motion_audio, MotionAudioShutdownError::TimedOut);
            assert_eq!(
                error.to_string(),
                "runtime: runtime worker panicked; motion audio: motion audio shutdown timed out"
            );
        }
        other => panic!("unexpected shutdown error: {other}"),
    }
}
