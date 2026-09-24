//! UI-side compatibility module for the update protocol.
//!
//! The protocol implementation is owned by `bongocat-ui-protocol`; this module
//! keeps the historical `crate::update` path used by GPUI render tests.

#[cfg(test)]
use bongocat_ui_protocol::{
    UpdateErrorCode, UpdateFailureStage, UpdatePhase, UpdateProgressInfo, UpdateReleaseInfo,
    UpdateUnavailableReason,
};

#[cfg(test)]
pub(crate) fn every_renderable_phase() -> Vec<UpdatePhase> {
    let notes = "## What's new\n\n- a change\n";
    let release = |notes: Option<&str>| UpdateReleaseInfo {
        version: "9.9.9".to_owned(),
        notes: notes.map(str::to_owned),
        release_page_url: Some("https://example.invalid/v9.9.9".to_owned()),
    };
    let progress = |downloaded: u64, total: Option<u64>| UpdateProgressInfo {
        downloaded_bytes: downloaded,
        total_bytes: total,
    };
    vec![
        UpdatePhase::Unavailable {
            reason: UpdateUnavailableReason::DevelopmentBuild,
        },
        UpdatePhase::Unavailable {
            reason: UpdateUnavailableReason::SigningKeyMissing,
        },
        UpdatePhase::Idle,
        UpdatePhase::Checking,
        UpdatePhase::UpToDate,
        UpdatePhase::Available {
            release: release(Some(notes)),
        },
        UpdatePhase::Available {
            release: release(None),
        },
        UpdatePhase::Downloading {
            release: release(Some(notes)),
            progress: progress(48 * 1024 * 1024, Some(120 * 1024 * 1024)),
        },
        UpdatePhase::Downloading {
            release: release(Some(notes)),
            progress: progress(48 * 1024 * 1024, None),
        },
        UpdatePhase::Verifying {
            release: release(Some(notes)),
        },
        UpdatePhase::Installing {
            release: release(Some(notes)),
        },
        UpdatePhase::Installed {
            version: "9.9.9".to_owned(),
            restart_required: true,
        },
        UpdatePhase::Installed {
            version: "9.9.9".to_owned(),
            restart_required: false,
        },
        UpdatePhase::Failed {
            stage: UpdateFailureStage::Check,
            code: UpdateErrorCode::ReleaseManifestInvalid,
            release: None,
        },
        UpdatePhase::Failed {
            stage: UpdateFailureStage::Download,
            code: UpdateErrorCode::DownloadTransportFailed,
            release: Some(release(Some(notes))),
        },
        UpdatePhase::Failed {
            stage: UpdateFailureStage::Verify,
            code: UpdateErrorCode::SignatureInvalid,
            release: Some(release(Some(notes))),
        },
        UpdatePhase::Failed {
            stage: UpdateFailureStage::Install,
            code: UpdateErrorCode::InstallPathNotWritable,
            release: Some(release(Some(notes))),
        },
    ]
}
