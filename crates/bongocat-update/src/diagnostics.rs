//! The anonymous update diagnostics contract exported to the application.
//!
//! This module is deliberately free of any `self_update` type. The application
//! diagnostics boundary (`ADR-0016`, `ADR-0027`) may only ever see the stable,
//! path-free counters and error codes defined here, so the underlying update
//! library can be replaced without changing the export shape.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

/// Stable, path-free error codes for the update subsystem.
///
/// These strings are part of the diagnostics export contract: they are the only
/// failure detail allowed to leave the update subsystem. Adding a variant is
/// backwards compatible; renaming or removing one is not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateErrorCode {
    /// The build has no release configuration (unsupported host target).
    NotConfigured,
    /// The build's release channel is not allowed to update.
    EnvironmentDisabled,
    /// No release signing key is provisioned, so nothing can be authenticated.
    SignatureKeyMissing,
    /// The release listing could not be fetched or parsed.
    ReleaseFetchFailed,
    /// No release asset matched this target.
    NoMatchingAsset,
    /// The artifact transfer failed (connection, TLS, HTTP status, rate limit).
    DownloadTransportFailed,
    /// The artifact did not match its expected checksum.
    ChecksumMismatch,
    /// The artifact's archive signature was absent or invalid.
    SignatureInvalid,
    /// The artifact archive could not be read or extracted.
    ArchiveInvalid,
    /// The install location is not writable.
    InstallPathNotWritable,
    /// The install step failed.
    InstallFailed,
    /// The updated process could not be relaunched.
    RestartFailed,
    /// An unexpected internal failure.
    Internal,
}

impl UpdateErrorCode {
    pub const ALL: [Self; 13] = [
        Self::NotConfigured,
        Self::EnvironmentDisabled,
        Self::SignatureKeyMissing,
        Self::ReleaseFetchFailed,
        Self::NoMatchingAsset,
        Self::DownloadTransportFailed,
        Self::ChecksumMismatch,
        Self::SignatureInvalid,
        Self::ArchiveInvalid,
        Self::InstallPathNotWritable,
        Self::InstallFailed,
        Self::RestartFailed,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "update_not_configured",
            Self::EnvironmentDisabled => "update_environment_disabled",
            Self::SignatureKeyMissing => "update_signature_key_missing",
            Self::ReleaseFetchFailed => "update_release_fetch_failed",
            Self::NoMatchingAsset => "update_no_matching_asset",
            Self::DownloadTransportFailed => "update_download_transport_failed",
            Self::ChecksumMismatch => "update_checksum_mismatch",
            Self::SignatureInvalid => "update_signature_invalid",
            Self::ArchiveInvalid => "update_archive_invalid",
            Self::InstallPathNotWritable => "update_install_path_not_writable",
            Self::InstallFailed => "update_install_failed",
            Self::RestartFailed => "update_restart_failed",
            Self::Internal => "update_internal_failed",
        }
    }
}

/// Returns whether `code` belongs to the update subsystem's stable, path-free
/// error-code catalog.
pub fn is_stable_error_code(code: &str) -> bool {
    UpdateErrorCode::ALL
        .iter()
        .any(|candidate| candidate.as_str() == code)
}

/// Anonymous counters exposed to the application diagnostics boundary.
///
/// The update runtime owns the source of these values; this crate only defines
/// the stable, path-free shape that can be sampled by the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateDiagnostics {
    pub last_error_code: Option<&'static str>,
    pub checks_started: u64,
    pub checks_succeeded: u64,
    pub checks_failed: u64,
    pub downloads_started: u64,
    pub downloads_succeeded: u64,
    pub downloads_failed: u64,
    pub installs_started: u64,
    pub installs_succeeded: u64,
    pub installs_failed: u64,
}

impl UpdateDiagnostics {
    /// Remove an unrecognized provider error without exposing arbitrary text
    /// through the diagnostics export contract.
    pub fn sanitized(self) -> Self {
        Self {
            last_error_code: self
                .last_error_code
                .filter(|code| is_stable_error_code(code)),
            ..self
        }
    }
}

#[derive(Default)]
struct UpdateDiagnosticsTrackerState {
    last_error_code: Mutex<Option<&'static str>>,
    checks_started: AtomicU64,
    checks_succeeded: AtomicU64,
    checks_failed: AtomicU64,
    downloads_started: AtomicU64,
    downloads_succeeded: AtomicU64,
    downloads_failed: AtomicU64,
    installs_started: AtomicU64,
    installs_succeeded: AtomicU64,
    installs_failed: AtomicU64,
}

/// App-owned, thread-safe source for anonymous update diagnostics.
#[derive(Clone, Default)]
pub struct UpdateDiagnosticsTracker {
    state: Arc<UpdateDiagnosticsTrackerState>,
}

impl UpdateDiagnosticsTracker {
    pub fn snapshot(&self) -> UpdateDiagnostics {
        let last_error_code = self.state.last_error_code.lock().map_or(None, |code| *code);
        UpdateDiagnostics {
            last_error_code,
            checks_started: self.state.checks_started.load(Ordering::Relaxed),
            checks_succeeded: self.state.checks_succeeded.load(Ordering::Relaxed),
            checks_failed: self.state.checks_failed.load(Ordering::Relaxed),
            downloads_started: self.state.downloads_started.load(Ordering::Relaxed),
            downloads_succeeded: self.state.downloads_succeeded.load(Ordering::Relaxed),
            downloads_failed: self.state.downloads_failed.load(Ordering::Relaxed),
            installs_started: self.state.installs_started.load(Ordering::Relaxed),
            installs_succeeded: self.state.installs_succeeded.load(Ordering::Relaxed),
            installs_failed: self.state.installs_failed.load(Ordering::Relaxed),
        }
        .sanitized()
    }

    pub fn record_check_started(&self) {
        increment(&self.state.checks_started);
    }

    pub fn record_check_succeeded(&self) {
        increment(&self.state.checks_succeeded);
    }

    pub fn record_check_failed(&self, code: &'static str) {
        increment(&self.state.checks_failed);
        self.record_error(code);
    }

    pub fn record_download_started(&self) {
        increment(&self.state.downloads_started);
    }

    pub fn record_download_succeeded(&self) {
        increment(&self.state.downloads_succeeded);
    }

    pub fn record_download_failed(&self, code: &'static str) {
        increment(&self.state.downloads_failed);
        self.record_error(code);
    }

    pub fn record_install_started(&self) {
        increment(&self.state.installs_started);
    }

    pub fn record_install_succeeded(&self) {
        increment(&self.state.installs_succeeded);
    }

    pub fn record_install_failed(&self, code: &'static str) {
        increment(&self.state.installs_failed);
        self.record_error(code);
    }

    fn record_error(&self, code: &'static str) {
        if !is_stable_error_code(code) {
            return;
        }
        if let Ok(mut current) = self.state.last_error_code.lock() {
            *current = Some(code);
        }
    }
}

fn increment(counter: &AtomicU64) {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(1))
        })
        .expect("update diagnostic counter update always succeeds");
}

#[cfg(test)]
mod tests {
    use super::{
        UpdateDiagnostics, UpdateDiagnosticsTracker, UpdateErrorCode, is_stable_error_code,
    };

    #[test]
    fn transport_failure_code_remains_stable() {
        assert!(is_stable_error_code("update_download_transport_failed"));
        assert_eq!(
            UpdateErrorCode::DownloadTransportFailed.as_str(),
            "update_download_transport_failed"
        );
    }

    #[test]
    fn arbitrary_provider_text_is_not_a_stable_code() {
        assert!(!is_stable_error_code("private_update_detail"));
        assert!(!is_stable_error_code(""));
    }

    #[test]
    fn snapshot_sanitizes_an_unrecognized_last_error() {
        let sanitized = UpdateDiagnostics {
            last_error_code: Some("private_update_detail"),
            ..UpdateDiagnostics::default()
        }
        .sanitized();
        assert_eq!(sanitized.last_error_code, None);
    }

    #[test]
    fn tracker_counts_and_keeps_the_last_stable_error() {
        let tracker = UpdateDiagnosticsTracker::default();
        tracker.record_check_started();
        tracker.record_check_failed("update_download_transport_failed");
        tracker.record_check_failed("private_update_detail");
        tracker.record_download_started();
        tracker.record_install_started();

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.checks_started, 1);
        assert_eq!(snapshot.checks_failed, 2);
        assert_eq!(snapshot.downloads_started, 1);
        assert_eq!(snapshot.installs_started, 1);
        assert_eq!(
            snapshot.last_error_code,
            Some("update_download_transport_failed")
        );
    }

    #[test]
    fn every_code_in_the_catalog_is_stable() {
        for code in UpdateErrorCode::ALL {
            assert!(is_stable_error_code(code.as_str()));
        }
    }
}
