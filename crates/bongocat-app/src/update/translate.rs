//! The update subsystem's vocabulary, as the window's.
//!
//! Translation happens once, here, rather than at each place the window reads a
//! value. That is what makes the mappings testable as a table: every source code
//! has a UI code, every source stage has a UI stage, and a reason a build cannot
//! update is preserved rather than collapsed into "unavailable" — a development
//! build and a build with no signing key are different problems with different
//! fixes.

use super::*;

pub(crate) fn failure_phase(error: UpdateError, release: Option<UpdateReleaseInfo>) -> UpdatePhase {
    UpdatePhase::Failed {
        stage: failure_stage(error.stage()),
        code: error_code(error.code()),
        release,
    }
}

pub(crate) fn release_info(engine: &dyn UpdateEngine, release: UpdateRelease) -> UpdateReleaseInfo {
    UpdateReleaseInfo {
        release_page_url: engine.release_page_url(&release.version),
        version: release.version,
        notes: release.notes,
    }
}

pub(crate) const fn progress_info(progress: bongocat_update::UpdateProgress) -> UpdateProgressInfo {
    UpdateProgressInfo {
        downloaded_bytes: progress.downloaded_bytes,
        total_bytes: progress.total_bytes,
    }
}

/// Map the update subsystem's stage onto the UI protocol.
///
/// The match is exhaustive on purpose: a new stage must be given a user-facing
/// meaning before it can reach a window.
pub(crate) const fn failure_stage(stage: bongocat_update::UpdateStage) -> UpdateFailureStage {
    match stage {
        bongocat_update::UpdateStage::Check => UpdateFailureStage::Check,
        bongocat_update::UpdateStage::Download => UpdateFailureStage::Download,
        bongocat_update::UpdateStage::Verify => UpdateFailureStage::Verify,
        bongocat_update::UpdateStage::Install => UpdateFailureStage::Install,
    }
}

pub(crate) const fn error_code(code: bongocat_update::UpdateErrorCode) -> UpdateErrorCode {
    use bongocat_update::UpdateErrorCode as Source;
    match code {
        Source::NotConfigured => UpdateErrorCode::NotConfigured,
        Source::EnvironmentDisabled => UpdateErrorCode::EnvironmentDisabled,
        Source::SignatureKeyMissing => UpdateErrorCode::SignatureKeyMissing,
        Source::ReleaseFetchFailed => UpdateErrorCode::ReleaseFetchFailed,
        Source::ReleaseManifestInvalid => UpdateErrorCode::ReleaseManifestInvalid,
        Source::NoMatchingAsset => UpdateErrorCode::NoMatchingAsset,
        Source::DownloadTransportFailed => UpdateErrorCode::DownloadTransportFailed,
        Source::ChecksumMismatch => UpdateErrorCode::ChecksumMismatch,
        Source::SignatureInvalid => UpdateErrorCode::SignatureInvalid,
        Source::ArchiveInvalid => UpdateErrorCode::ArchiveInvalid,
        Source::InstallPathNotWritable => UpdateErrorCode::InstallPathNotWritable,
        Source::InstallFailed => UpdateErrorCode::InstallFailed,
        Source::RestartFailed => UpdateErrorCode::RestartFailed,
        Source::Internal => UpdateErrorCode::Internal,
    }
}

pub(crate) const fn unavailable_reason(reason: UpdateUnavailability) -> UpdateUnavailableReason {
    match reason {
        UpdateUnavailability::DevelopmentChannel => UpdateUnavailableReason::DevelopmentBuild,
        UpdateUnavailability::UnsupportedTarget => UpdateUnavailableReason::UnsupportedPlatform,
        UpdateUnavailability::SigningKeyMissing => UpdateUnavailableReason::SigningKeyMissing,
    }
}
