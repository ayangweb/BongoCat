use crate::{StagedUpdateArtifact, UpdateDiagnosticsTracker};
use std::{fmt, path::Path};

/// Stable, path-free outcomes for the pre-install coordination boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateInstallErrorCode {
    Cancelled,
    ShutdownFailed,
    InstallFailed,
    RollbackFailed,
}

impl UpdateInstallErrorCode {
    pub const ALL: [Self; 4] = [
        Self::Cancelled,
        Self::ShutdownFailed,
        Self::InstallFailed,
        Self::RollbackFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "update_install_cancelled",
            Self::ShutdownFailed => "update_install_shutdown_failed",
            Self::InstallFailed => "update_install_failed",
            Self::RollbackFailed => "update_install_rollback_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateInstallError {
    code: UpdateInstallErrorCode,
}

impl UpdateInstallError {
    const fn new(code: UpdateInstallErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> UpdateInstallErrorCode {
        self.code
    }
}

impl fmt::Display for UpdateInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateInstallError {}

/// Enforces the update install ordering without owning a runtime, renderer, or
/// platform installer. Each callback must keep platform errors private and
/// return only whether its phase succeeded.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateInstallCoordinator;

impl UpdateInstallCoordinator {
    pub fn install<Cancel, Shutdown, Install, Rollback>(
        &self,
        artifact: &StagedUpdateArtifact,
        mut cancelled: Cancel,
        mut shutdown: Shutdown,
        mut install: Install,
        mut rollback: Rollback,
    ) -> Result<(), UpdateInstallError>
    where
        Cancel: FnMut() -> bool,
        Shutdown: FnMut() -> bool,
        Install: FnMut(&Path) -> bool,
        Rollback: FnMut() -> bool,
    {
        if cancelled() {
            return Err(UpdateInstallError::new(UpdateInstallErrorCode::Cancelled));
        }
        if !shutdown() {
            return Err(UpdateInstallError::new(
                UpdateInstallErrorCode::ShutdownFailed,
            ));
        }
        if cancelled() {
            return Err(UpdateInstallError::new(UpdateInstallErrorCode::Cancelled));
        }
        if install(artifact.path()) {
            return Ok(());
        }
        if rollback() {
            Err(UpdateInstallError::new(
                UpdateInstallErrorCode::InstallFailed,
            ))
        } else {
            Err(UpdateInstallError::new(
                UpdateInstallErrorCode::RollbackFailed,
            ))
        }
    }

    pub fn install_with_diagnostics<Cancel, Shutdown, Install, Rollback>(
        &self,
        artifact: &StagedUpdateArtifact,
        tracker: &UpdateDiagnosticsTracker,
        cancelled: Cancel,
        shutdown: Shutdown,
        install: Install,
        rollback: Rollback,
    ) -> Result<(), UpdateInstallError>
    where
        Cancel: FnMut() -> bool,
        Shutdown: FnMut() -> bool,
        Install: FnMut(&Path) -> bool,
        Rollback: FnMut() -> bool,
    {
        tracker.record_install_started();
        let result = self.install(artifact, cancelled, shutdown, install, rollback);
        match &result {
            Ok(()) => tracker.record_install_succeeded(),
            Err(error) => tracker.record_install_failed(error.code().as_str()),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TargetTriple, UpdateChannel, UpdateTarget, VerifiedArtifact};
    use bongocat_config::{BuildEnvironment, StorageLayout};
    use std::{cell::Cell, io::Cursor};
    use tempfile::tempdir;

    fn staged_artifact() -> (tempfile::TempDir, StagedUpdateArtifact) {
        let directory = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(directory.path(), BuildEnvironment::Development);
        let artifact = VerifiedArtifact::from_test_bytes(
            UpdateChannel::Development,
            UpdateTarget::new(TargetTriple::Aarch64AppleDarwin),
            "https://updates.example.invalid/app",
            b"payload",
        );
        let staged = artifact
            .stage_reader(&layout, Cursor::new(b"payload"), || false)
            .expect("staged artifact");
        (directory, staged)
    }

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes = UpdateInstallErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UpdateInstallErrorCode::ALL.len());
    }

    #[test]
    fn cancellation_precedes_shutdown_and_install() {
        let (_directory, artifact) = staged_artifact();
        let called = Cell::new(false);
        let error = UpdateInstallCoordinator
            .install(
                &artifact,
                || true,
                || {
                    called.set(true);
                    true
                },
                |_| true,
                || true,
            )
            .expect_err("cancelled install");
        assert_eq!(error.code(), UpdateInstallErrorCode::Cancelled);
        assert!(!called.get());
    }

    #[test]
    fn shutdown_must_succeed_before_install() {
        let (_directory, artifact) = staged_artifact();
        let called = Cell::new(false);
        let error = UpdateInstallCoordinator
            .install(
                &artifact,
                || false,
                || false,
                |_| {
                    called.set(true);
                    true
                },
                || true,
            )
            .expect_err("shutdown failure");
        assert_eq!(error.code(), UpdateInstallErrorCode::ShutdownFailed);
        assert!(!called.get());
    }

    #[test]
    fn failed_install_rolls_back_and_classifies_rollback_result() {
        let (_directory, artifact) = staged_artifact();
        let rollback_calls = Cell::new(0);
        let error = UpdateInstallCoordinator
            .install(
                &artifact,
                || false,
                || true,
                |_| false,
                || {
                    rollback_calls.set(rollback_calls.get() + 1);
                    true
                },
            )
            .expect_err("install failure");
        assert_eq!(error.code(), UpdateInstallErrorCode::InstallFailed);
        assert_eq!(rollback_calls.get(), 1);

        let error = UpdateInstallCoordinator
            .install(&artifact, || false, || true, |_| false, || false)
            .expect_err("rollback failure");
        assert_eq!(error.code(), UpdateInstallErrorCode::RollbackFailed);
    }

    #[test]
    fn diagnostics_wrapper_records_install_success() {
        let (_directory, artifact) = staged_artifact();
        let tracker = crate::UpdateDiagnosticsTracker::default();
        UpdateInstallCoordinator
            .install_with_diagnostics(&artifact, &tracker, || false, || true, |_| true, || true)
            .expect("successful install");

        assert_eq!(tracker.snapshot().installs_started, 1);
        assert_eq!(tracker.snapshot().installs_succeeded, 1);
        assert_eq!(tracker.snapshot().installs_failed, 0);
    }
}
