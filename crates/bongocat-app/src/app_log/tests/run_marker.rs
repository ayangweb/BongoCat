//! A marker says how the last run ended, and nothing more.

use super::*;
#[test]
fn run_marker_survives_unclean_drop_and_is_removed_on_completion() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    let (marker, previous) = handle.begin_run().expect("begin first run");
    assert_eq!(previous, None);
    let marker_path = directory.path().join(RUN_MARKER_NAME);
    assert_eq!(
        fs::read(&marker_path).expect("marker bytes"),
        RUN_MARKER_RUNNING
    );
    drop(marker);

    let (marker, previous) = handle.begin_run().expect("begin recovered run");
    assert_eq!(previous, Some(PreviousRunState::ForcedOrUnknown));
    marker.complete().expect("complete run");
    assert!(!marker_path.exists());
}

#[test]
fn run_marker_classifies_panic_and_interrupted_shutdown_without_retaining_them() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    let marker_path = directory.path().join(RUN_MARKER_NAME);

    fs::write(&marker_path, RUN_MARKER_PANICKED).expect("panic marker");
    let (marker, previous) = handle.begin_run().expect("recover panic");
    assert_eq!(previous, Some(PreviousRunState::Panic));
    assert_eq!(
        fs::read(&marker_path).expect("new running marker"),
        RUN_MARKER_RUNNING
    );
    marker.mark_shutdown_started().expect("mark shutdown start");
    drop(marker);

    let (marker, previous) = handle.begin_run().expect("recover interrupted shutdown");
    assert_eq!(previous, Some(PreviousRunState::ShutdownInterrupted));
    marker.complete().expect("complete recovered run");
}
