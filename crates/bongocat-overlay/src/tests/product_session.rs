//! A denied permission is reported once, as a degraded state.

use super::*;

#[test]
fn input_start_failures_publish_one_anonymous_degraded_attempt() {
    for (error, expected) in [
        (
            PlatformInputError::PermissionDenied,
            PlatformInputServiceStatus::PermissionDenied,
        ),
        (
            PlatformInputError::BackendUnavailable,
            PlatformInputServiceStatus::BackendUnavailable,
        ),
        (
            PlatformInputError::TapCreateFailed,
            PlatformInputServiceStatus::Failed,
        ),
    ] {
        let diagnostics = input_start_failure_diagnostics(error);
        assert_eq!(diagnostics.service_status, expected);
        assert_eq!(diagnostics.service_error_code, Some(error.as_str()));
        assert_eq!(diagnostics.service_start_attempts, 1);
        assert_eq!(diagnostics.captured_edges, 0);
    }
}

#[test]
fn platform_input_owner_attempts_a_denied_start_only_once() {
    let attempts = AtomicUsize::new(0);
    let producer = PlatformInputDiagnosticsProducer::default();
    let (service, error) = start_platform_input(&producer, || {
        attempts.fetch_add(1, Ordering::Relaxed);
        Err::<(), _>(PlatformInputError::PermissionDenied)
    });

    assert_eq!(service, None);
    assert_eq!(error, Some(PlatformInputError::PermissionDenied));
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
    assert_eq!(
        producer.diagnostics(),
        PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::PermissionDenied,
            service_error_code: Some("platform_input_permission_denied"),
            service_start_attempts: 1,
            ..PlatformInputDiagnostics::default()
        }
    );
}
