//! The UI-facing update protocol.
//!
//! This module owns the shape of everything the update window renders and the
//! typed channel it drives the update worker with. Like every other UI protocol in
//! this crate it mirrors the producing subsystem rather than importing it, so the
//! update library and its types stay behind the application boundary: the UI sees a
//! stable stage, a stable code string and localized text, never a
//! `cargo-packager-updater` value.

use std::{
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_channel::{Receiver, Sender};

/// How often the update window re-reads the shared update state while it is open.
///
/// The worker publishes progress into shared memory instead of pushing events, so
/// the window decides its own refresh rate and a slow or hidden window cannot make
/// the worker block.
pub const UPDATE_STATE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Why this build cannot check for or install updates at all.
///
/// When a build reports one of these the update entry points stay hidden; the
/// variant only reaches the UI so a diagnostics surface can explain the absence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateUnavailableReason {
    /// The build carries the Development channel and never installs a release.
    DevelopmentBuild,
    /// No release signing key is provisioned, so nothing could be authenticated.
    SigningKeyMissing,
    /// The host is outside the shipped targets.
    UnsupportedHost,
}

/// The stage an update stopped in.
///
/// Mirrors `bongocat_update::UpdateStage`; the mapping is exhaustive in
/// `bongocat-app`, so a new stage cannot reach the UI unmapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateFailureStage {
    Check,
    Download,
    Verify,
    Install,
}

impl UpdateFailureStage {
    pub const ALL: [Self; 4] = [Self::Check, Self::Download, Self::Verify, Self::Install];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Download => "download",
            Self::Verify => "verify",
            Self::Install => "install",
        }
    }

    pub const fn from_str(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"check" => Some(Self::Check),
            b"download" => Some(Self::Download),
            b"verify" => Some(Self::Verify),
            b"install" => Some(Self::Install),
            _ => None,
        }
    }
}

/// The stable, path-free update error codes the UI can render.
///
/// Mirrors `bongocat_update::UpdateErrorCode`. The strings are the diagnostics
/// export contract, so they are reproduced here verbatim and pinned by a
/// uniqueness test rather than derived from Rust variant names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateErrorCode {
    NotConfigured,
    EnvironmentDisabled,
    SignatureKeyMissing,
    ReleaseFetchFailed,
    ReleaseManifestInvalid,
    NoMatchingAsset,
    DownloadTransportFailed,
    ChecksumMismatch,
    SignatureInvalid,
    ArchiveInvalid,
    InstallPathNotWritable,
    InstallFailed,
    RestartFailed,
    Internal,
}

impl UpdateErrorCode {
    pub const ALL: [Self; 14] = [
        Self::NotConfigured,
        Self::EnvironmentDisabled,
        Self::SignatureKeyMissing,
        Self::ReleaseFetchFailed,
        Self::ReleaseManifestInvalid,
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
            Self::ReleaseManifestInvalid => "update_release_manifest_invalid",
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

    /// The catalog entry for a stable code, or `None` for anything unrecognized.
    pub fn from_stable_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == code)
    }

    /// The i18n key for this code's user-facing message.
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::NotConfigured => "update.error.not_configured",
            Self::EnvironmentDisabled => "update.error.environment_disabled",
            Self::SignatureKeyMissing => "update.error.signature_key_missing",
            Self::ReleaseFetchFailed => "update.error.release_fetch_failed",
            Self::ReleaseManifestInvalid => "update.error.release_manifest_invalid",
            Self::NoMatchingAsset => "update.error.no_matching_asset",
            Self::DownloadTransportFailed => "update.error.download_transport_failed",
            Self::ChecksumMismatch => "update.error.checksum_mismatch",
            Self::SignatureInvalid => "update.error.signature_invalid",
            Self::ArchiveInvalid => "update.error.archive_invalid",
            Self::InstallPathNotWritable => "update.error.install_path_not_writable",
            Self::InstallFailed => "update.error.install_failed",
            Self::RestartFailed => "update.error.restart_failed",
            Self::Internal => "update.error.internal",
        }
    }
}

/// A published release the running build could move to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateReleaseInfo {
    pub version: String,
    /// The release changelog the manifest announced, when it announced one.
    pub notes: Option<String>,
    /// The page that shows the full release, when the release identity is known.
    pub release_page_url: Option<String>,
}

/// Transfer progress of an in-flight download.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateProgressInfo {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl UpdateProgressInfo {
    /// The completed fraction, when the server announced a payload size.
    pub fn fraction(self) -> Option<f32> {
        let total = self.total_bytes.filter(|total| *total > 0)?;
        Some((self.downloaded_bytes as f64 / total as f64).min(1.0) as f32)
    }

    /// The completed percentage for display, when the server announced a size.
    pub fn percent(self) -> Option<u8> {
        Some((self.fraction()? * 100.0).round().clamp(0.0, 100.0) as u8)
    }
}

/// Everything the update window can be showing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdatePhase {
    /// The build cannot update; the window is not offered.
    Unavailable { reason: UpdateUnavailableReason },
    /// No check has run in this session.
    Idle,
    /// A check is in flight.
    Checking,
    /// The running build is the newest release.
    UpToDate,
    /// A newer release exists and nothing has been downloaded yet.
    Available { release: UpdateReleaseInfo },
    /// The payload is being transferred.
    Downloading {
        release: UpdateReleaseInfo,
        progress: UpdateProgressInfo,
    },
    /// The payload is fully transferred and its signature is being checked.
    Verifying { release: UpdateReleaseInfo },
    /// The verified payload is being written into the installation.
    Installing { release: UpdateReleaseInfo },
    /// The release was installed.
    ///
    /// `restart_required` is true on platforms where the running process keeps
    /// executing the previous build until it is replaced.
    Installed {
        version: String,
        restart_required: bool,
    },
    /// The update stopped in a stage.
    Failed {
        stage: UpdateFailureStage,
        code: UpdateErrorCode,
        /// The release the failure was about, when a check had already found one.
        release: Option<UpdateReleaseInfo>,
    },
}

impl UpdatePhase {
    /// Whether the worker is currently doing something for this phase.
    pub const fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::Downloading { .. }
                | Self::Verifying { .. }
                | Self::Installing { .. }
        )
    }

    /// The release this phase is about, when it is about one.
    pub fn release(&self) -> Option<&UpdateReleaseInfo> {
        match self {
            Self::Available { release }
            | Self::Downloading { release, .. }
            | Self::Verifying { release }
            | Self::Installing { release } => Some(release),
            Self::Failed { release, .. } => release.as_ref(),
            Self::Unavailable { .. }
            | Self::Idle
            | Self::Checking
            | Self::UpToDate
            | Self::Installed { .. } => None,
        }
    }

    /// Whether the window should offer to download and install.
    pub const fn offers_install(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Whether the window should offer to check again.
    pub const fn offers_check(&self) -> bool {
        matches!(self, Self::Idle | Self::UpToDate | Self::Failed { .. })
    }

    /// Whether the window should offer to restart into the installed build.
    pub const fn offers_restart(&self) -> bool {
        matches!(
            self,
            Self::Installed {
                restart_required: true,
                ..
            }
        )
    }
}

/// The update state one window renders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateSnapshot {
    pub revision: u64,
    /// The version of the running build.
    pub current_version: String,
    pub phase: UpdatePhase,
}

impl UpdateSnapshot {
    pub fn new(current_version: impl Into<String>, phase: UpdatePhase) -> Self {
        Self {
            revision: 0,
            current_version: current_version.into(),
            phase,
        }
    }
}

/// Shared update state between the worker and every window.
///
/// The worker is the only writer; readers clone a snapshot. The revision makes a
/// poll loop cheap and idempotent, and lets a window skip a repaint when nothing
/// changed.
#[derive(Clone)]
pub struct UpdateStateHandle {
    inner: Arc<Mutex<UpdateSnapshot>>,
}

impl UpdateStateHandle {
    pub fn new(initial: UpdateSnapshot) -> Self {
        Self {
            inner: Arc::new(Mutex::new(initial)),
        }
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replace the published phase and advance the revision.
    ///
    /// Returns the new revision. Publishing the same phase is not a change and does
    /// not advance the revision, so a poll loop does not repaint for nothing.
    pub fn publish(&self, phase: UpdatePhase) -> u64 {
        let mut current = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.phase == phase {
            return current.revision;
        }
        current.phase = phase;
        current.revision = current.revision.saturating_add(1);
        current.revision
    }

    /// The phase currently published.
    pub fn phase(&self) -> UpdatePhase {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .phase
            .clone()
    }
}

impl Default for UpdateStateHandle {
    fn default() -> Self {
        Self::new(UpdateSnapshot::new(String::new(), UpdatePhase::Idle))
    }
}

impl fmt::Debug for UpdateStateHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateStateHandle")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

/// A request from a window or the system menu to the update worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateCommand {
    /// Read the release manifest and report whether a newer release exists.
    Check,
    /// Download, verify and install the release the last check found.
    Install,
    /// Replace the running process with the installed build.
    Restart,
    /// Stop the worker.
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateServiceClosed;

impl fmt::Display for UpdateServiceClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("update command channel is closed")
    }
}

impl std::error::Error for UpdateServiceClosed {}

/// The window's handle on the update worker.
#[derive(Clone)]
pub struct UpdateClient {
    commands: Sender<UpdateCommand>,
    state: UpdateStateHandle,
}

pub struct UpdateServiceEndpoint {
    commands: Receiver<UpdateCommand>,
}

impl UpdateClient {
    pub fn bounded(capacity: usize) -> (Self, UpdateServiceEndpoint) {
        assert!(capacity > 0, "update command capacity must be positive");
        let (commands, receiver) = async_channel::bounded(capacity);
        (
            Self {
                commands,
                state: UpdateStateHandle::default(),
            },
            UpdateServiceEndpoint { commands: receiver },
        )
    }

    /// Attach the worker's shared state so this client observes published progress.
    pub fn track_state(&self, state: UpdateStateHandle) -> Self {
        Self {
            commands: self.commands.clone(),
            state,
        }
    }

    pub fn state(&self) -> UpdateStateHandle {
        self.state.clone()
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.state.snapshot()
    }

    pub fn request_check(&self) -> Result<(), UpdateServiceClosed> {
        self.send(UpdateCommand::Check)
    }

    pub fn request_install(&self) -> Result<(), UpdateServiceClosed> {
        self.send(UpdateCommand::Install)
    }

    pub fn request_restart(&self) -> Result<(), UpdateServiceClosed> {
        self.send(UpdateCommand::Restart)
    }

    pub fn request_shutdown(&self) -> Result<(), UpdateServiceClosed> {
        self.send(UpdateCommand::Shutdown)
    }

    fn send(&self, command: UpdateCommand) -> Result<(), UpdateServiceClosed> {
        self.commands
            .try_send(command)
            .map_err(|_| UpdateServiceClosed)
    }
}

impl UpdateServiceEndpoint {
    pub fn recv_blocking(&self) -> Result<UpdateCommand, UpdateServiceClosed> {
        self.commands
            .recv_blocking()
            .map_err(|_| UpdateServiceClosed)
    }
}

/// Every phase the window can be asked to render, including the sub-states whose
/// rendering branches differ.
///
/// Test-only, and deliberately the single source for "all of them": the render tests
/// iterate it, so a new `UpdatePhase` variant cannot be added without a rendering
/// branch being exercised.
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
        UpdatePhase::Unavailable {
            reason: UpdateUnavailableReason::UnsupportedHost,
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

#[cfg(test)]
mod tests {
    use super::{
        UpdateClient, UpdateCommand, UpdateErrorCode, UpdateFailureStage, UpdatePhase,
        UpdateProgressInfo, UpdateReleaseInfo, UpdateSnapshot, UpdateStateHandle,
        UpdateUnavailableReason,
    };

    fn release() -> UpdateReleaseInfo {
        UpdateReleaseInfo {
            version: "1.2.0".to_owned(),
            notes: Some("- a change".to_owned()),
            release_page_url: Some("https://example.invalid/v1.2.0".to_owned()),
        }
    }

    #[test]
    fn error_codes_are_unique_and_stable() {
        let mut codes: Vec<&str> = UpdateErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect();
        codes.sort_unstable();
        let unique = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), unique, "two update codes share a string");
        assert_eq!(UpdateErrorCode::ALL.len(), 14);

        for code in UpdateErrorCode::ALL {
            assert_eq!(UpdateErrorCode::from_stable_code(code.as_str()), Some(code));
            assert!(code.message_key().starts_with("update.error."));
        }
        assert_eq!(UpdateErrorCode::from_stable_code("private_detail"), None);
    }

    /// Every code has to resolve to real text, not to its own key.
    ///
    /// `rust_i18n` falls back to `en-US`, so a key that exists in only one locale still
    /// renders real text and is caught by `bongocat-i18n`'s cross-locale parity test
    /// instead. What that fallback *cannot* cover is a key missing from **every**
    /// locale: then `text` returns the key itself and the window would print
    /// `update.error.something` to the user. Comparing against the key is what turns
    /// that into a failure here.
    ///
    /// Verified by removing `release_manifest_invalid` from both catalogs (this test
    /// fails) and from `zh-CN` only (this test passes, the parity test fails).
    #[test]
    fn every_error_code_resolves_to_text_rather_than_to_its_own_key() {
        for code in UpdateErrorCode::ALL {
            for locale in ["en-US", "zh-CN"] {
                let message = bongocat_i18n::text(locale, code.message_key());
                assert_ne!(
                    message,
                    code.message_key(),
                    "{locale} has no message for {} in any locale, so the key itself                      would be rendered",
                    code.message_key()
                );
                assert!(
                    !message.trim().is_empty(),
                    "{locale} has an empty message for {}",
                    code.message_key()
                );
            }
        }
    }

    #[test]
    fn failure_stages_round_trip_through_their_stable_names() {
        for stage in UpdateFailureStage::ALL {
            assert_eq!(UpdateFailureStage::from_str(stage.as_str()), Some(stage));
        }
        assert_eq!(UpdateFailureStage::from_str("unknown"), None);
    }

    #[test]
    fn progress_only_reports_a_percentage_when_the_size_is_known() {
        assert_eq!(
            UpdateProgressInfo {
                downloaded_bytes: 256,
                total_bytes: Some(1024),
            }
            .percent(),
            Some(25)
        );
        assert_eq!(
            UpdateProgressInfo {
                downloaded_bytes: 256,
                total_bytes: None,
            }
            .percent(),
            None
        );
    }

    #[test]
    fn phases_describe_which_actions_the_window_offers() {
        assert!(UpdatePhase::Idle.offers_check());
        assert!(UpdatePhase::UpToDate.offers_check());
        assert!(!UpdatePhase::Checking.offers_check());
        assert!(UpdatePhase::Available { release: release() }.offers_install());
        assert!(
            !UpdatePhase::Downloading {
                release: release(),
                progress: UpdateProgressInfo::default(),
            }
            .offers_install()
        );
        assert!(
            UpdatePhase::Installed {
                version: "1.2.0".to_owned(),
                restart_required: true,
            }
            .offers_restart()
        );
        assert!(
            !UpdatePhase::Installed {
                version: "1.2.0".to_owned(),
                restart_required: false,
            }
            .offers_restart()
        );
        assert!(UpdatePhase::Checking.is_busy());
        assert!(UpdatePhase::Installing { release: release() }.is_busy());
        assert!(!UpdatePhase::Idle.is_busy());
    }

    #[test]
    fn a_failure_keeps_the_release_it_was_about() {
        let phase = UpdatePhase::Failed {
            stage: UpdateFailureStage::Download,
            code: UpdateErrorCode::DownloadTransportFailed,
            release: Some(release()),
        };
        assert_eq!(
            phase.release().map(|release| release.version.as_str()),
            Some("1.2.0")
        );
        assert!(phase.offers_check());
    }

    #[test]
    fn publishing_the_same_phase_does_not_advance_the_revision() {
        let state = UpdateStateHandle::new(UpdateSnapshot::new("1.0.0", UpdatePhase::Idle));
        assert_eq!(state.publish(UpdatePhase::Idle), 0);
        assert_eq!(state.publish(UpdatePhase::Checking), 1);
        assert_eq!(state.publish(UpdatePhase::Checking), 1);
        assert_eq!(state.publish(UpdatePhase::UpToDate), 2);
        assert_eq!(state.snapshot().revision, 2);
        assert_eq!(state.snapshot().current_version, "1.0.0");
    }

    #[test]
    fn the_client_publishes_through_the_shared_state() {
        let (client, endpoint) = UpdateClient::bounded(4);
        let state = UpdateStateHandle::new(UpdateSnapshot::new("1.0.0", UpdatePhase::Idle));
        let client = client.track_state(state.clone());

        client.request_check().expect("queued check");
        client.request_install().expect("queued install");
        client.request_shutdown().expect("queued shutdown");
        assert_eq!(endpoint.recv_blocking(), Ok(UpdateCommand::Check));
        assert_eq!(endpoint.recv_blocking(), Ok(UpdateCommand::Install));
        assert_eq!(endpoint.recv_blocking(), Ok(UpdateCommand::Shutdown));

        state.publish(UpdatePhase::Checking);
        assert_eq!(client.snapshot().phase, UpdatePhase::Checking);
    }

    #[test]
    fn an_unavailable_build_reports_why_without_a_release() {
        let phase = UpdatePhase::Unavailable {
            reason: UpdateUnavailableReason::DevelopmentBuild,
        };
        assert_eq!(phase.release(), None);
        assert!(!phase.offers_check());
        assert!(!phase.offers_install());
        assert!(!phase.is_busy());
    }
}
