//! The anonymous log and update diagnostics the application exposes.

use super::*;

use bongocat_model_store::{ModelStoreDiagnostic, ModelStoreError};

/// A store refusal has to say *which* refusal it was.
///
/// Twelve store codes reach this arm, and the issue template asks a reporter to
/// paste the `reason` field. A log that folded them all into one
/// `model_store_failed` answered nothing — the reported symptom ("模型包无效",
/// one message for an invalid package, a held lock, a source that moved) is
/// indistinguishable in it. The resource that would have narrowed it further
/// can be a user path, so it stays out of the log on purpose; the code is the
/// part that is safe to publish.
#[test]
fn a_store_refusal_logs_the_stores_own_code() {
    let error = ApplicationError::ModelStore(ModelStoreError::source_conversion_failed("detail"));
    assert_eq!(
        error.stable_code(),
        ModelStoreDiagnostic::SourceConversionFailed.as_str()
    );
    assert_ne!(error.stable_code(), "model_store_failed");
}

#[test]
fn application_reads_only_anonymous_core_log_diagnostics() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    assert_eq!(application.core_log_diagnostics(), None);

    application.set_core_log_diagnostics_provider(|| CoreLogDiagnostics {
        written: 3,
        dropped: 1,
        rotated: 2,
        pruned: 4,
        bytes: 128,
        retained_files: 2,
        retained_bytes: 192,
    });

    assert_eq!(
        application.core_log_diagnostics(),
        Some(CoreLogDiagnostics {
            written: 3,
            dropped: 1,
            rotated: 2,
            pruned: 4,
            bytes: 128,
            retained_files: 2,
            retained_bytes: 192,
        })
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_reads_only_anonymous_update_diagnostics() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    assert_eq!(application.update_diagnostics(), None);

    application.set_update_diagnostics_provider(|| bongocat_update::UpdateDiagnostics {
        last_error_code: Some("private_update_detail"),
        checks_started: 1,
        ..bongocat_update::UpdateDiagnostics::default()
    });
    assert_eq!(
        application
            .update_diagnostics()
            .expect("sanitized update diagnostics")
            .last_error_code,
        None
    );
    assert_eq!(
        application
            .update_diagnostics()
            .expect("sanitized update diagnostics")
            .checks_started,
        1
    );

    application.set_update_diagnostics_provider(|| bongocat_update::UpdateDiagnostics {
        last_error_code: Some("update_download_transport_failed"),
        checks_started: 3,
        checks_succeeded: 2,
        checks_failed: 1,
        downloads_started: 2,
        downloads_succeeded: 1,
        downloads_failed: 1,
        installs_started: 1,
        installs_succeeded: 0,
        installs_failed: 1,
    });

    assert_eq!(
        application.update_diagnostics(),
        Some(bongocat_update::UpdateDiagnostics {
            last_error_code: Some("update_download_transport_failed"),
            checks_started: 3,
            checks_succeeded: 2,
            checks_failed: 1,
            downloads_started: 2,
            downloads_succeeded: 1,
            downloads_failed: 1,
            installs_started: 1,
            installs_succeeded: 0,
            installs_failed: 1,
        })
    );
    application.shutdown().expect("clean shutdown");
}

#[test]
fn application_registers_shared_update_diagnostics_tracker() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout).expect("start application");
    let tracker = bongocat_update::UpdateDiagnosticsTracker::default();
    application.set_update_diagnostics_tracker(tracker.clone());

    tracker.record_check_started();
    tracker.record_check_succeeded();
    tracker.record_download_failed("update_download_transport_failed");

    assert_eq!(
        application.update_diagnostics(),
        Some(bongocat_update::UpdateDiagnostics {
            last_error_code: Some("update_download_transport_failed"),
            checks_started: 1,
            checks_succeeded: 1,
            checks_failed: 0,
            downloads_started: 0,
            downloads_succeeded: 0,
            downloads_failed: 1,
            installs_started: 0,
            installs_succeeded: 0,
            installs_failed: 0,
        })
    );
    application.shutdown().expect("clean shutdown");
}
