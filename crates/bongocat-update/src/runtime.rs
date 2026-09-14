//! The update runtime: the `self_update` pipeline bound to BongoCat's release
//! configuration, environment gating and signing-key policy.

use crate::{
    UpdateErrorCode,
    diagnostics::UpdateDiagnosticsTracker,
    release::{ReleaseChannel, ReleaseConfiguration},
};
use self_update::backends::github;

/// zipsign ed25519 public key used to authenticate release archives.
///
/// `None` until a release signing key is provisioned. The runtime **fails closed**
/// while this is `None`: `self_update::verify_signature` returns `Ok(())` for an
/// empty key set, so an updater that merely forwarded an empty key list would
/// install unsigned archives without ever noticing.
///
/// To enable updates, generate a zipsign key pair, sign every release archive
/// with the private key, and set this constant to the 32-byte public key.
pub const RELEASE_SIGNING_KEY: Option<[u8; 32]> = None;

/// The repository that publishes BongoCat releases.
pub const RELEASE_REPOSITORY_OWNER: &str = "ayangweb";
pub const RELEASE_REPOSITORY_NAME: &str = "BongoCat";

/// The executable name inside a Windows release archive.
///
/// `self_update` derives the single path it extracts from a Windows archive as
/// `{RELEASE_BINARY_NAME}{EXE_SUFFIX}` — here `bongocat-app.exe` — and requires
/// it at the archive root. It must therefore equal the executable the packaging
/// pipeline actually ships, which is the same `bongocat-app` binary the Windows
/// installer installs. Release *asset names* are matched on the target triple
/// instead, so this constant governs only the path inside the archive.
pub const RELEASE_BINARY_NAME: &str = "bongocat-app";

/// The macOS bundle directory name inside a release archive.
///
/// Must equal the `.app` directory the packaging pipeline produces, which is
/// `<product name>.app` from `crates/bongocat-packaging`.
pub const RELEASE_BUNDLE_NAME: &str = "BongoCat.app";

/// A stable-coded update failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateError {
    code: UpdateErrorCode,
}

impl UpdateError {
    const fn new(code: UpdateErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> UpdateErrorCode {
        self.code
    }

    pub const fn code_str(self) -> &'static str {
        self.code.as_str()
    }

    /// Map a library failure onto the stable code catalog.
    ///
    /// `self_update::Error` is `#[non_exhaustive]`, so an unrecognized variant
    /// degrades to [`UpdateErrorCode::Internal`] rather than leaking library text.
    fn from_self_update(error: self_update::Error) -> Self {
        use self_update::Error;

        let code = match error {
            Error::ChecksumMismatch { .. } | Error::ChecksumSourceInvalid { .. } => {
                UpdateErrorCode::ChecksumMismatch
            }
            Error::Signature(_)
            | Error::NoSignatures(_)
            | Error::SignatureNonUTF8
            | Error::VerificationRejected { .. }
            | Error::ArchiveVerificationRejected { .. } => UpdateErrorCode::SignatureInvalid,
            Error::Transport(_)
            | Error::HttpStatus { .. }
            | Error::RateLimited { .. }
            | Error::Unauthorized { .. }
            | Error::NotFound { .. }
            | Error::InvalidCertificate { .. }
            | Error::InvalidProxy { .. } => UpdateErrorCode::DownloadTransportFailed,
            Error::NoReleaseFound { .. }
            | Error::MissingAssetField { .. }
            | Error::InvalidAssetName { .. } => UpdateErrorCode::NoMatchingAsset,
            Error::InvalidResponse { .. } | Error::MissingField { .. } | Error::Json(_) => {
                UpdateErrorCode::ReleaseFetchFailed
            }
            Error::InstallPathNotWritable { .. } => UpdateErrorCode::InstallPathNotWritable,
            Error::NoAppBundle { .. }
            | Error::AppTranslocated { .. }
            | Error::ConflictingConfig { .. }
            | Error::Io(_) => UpdateErrorCode::InstallFailed,
            Error::Zip(_) | Error::ArchiveNotEnabled(_) | Error::CompressionNotEnabled(_) => {
                UpdateErrorCode::ArchiveInvalid
            }
            Error::NoCurrentVersion
            | Error::SemVer(_)
            | Error::InvalidHeader { .. }
            | Error::InvalidAuthToken { .. } => UpdateErrorCode::NotConfigured,
            _ => UpdateErrorCode::Internal,
        };

        Self::new(code)
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateError {}

/// The outcome of a completed update check or install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateOutcome {
    /// The running build is already the newest release.
    UpToDate,
    /// A newer release exists; nothing was installed.
    Available { version: String },
    /// The release was installed.
    Installed { version: String },
}

/// Owns the `self_update` pipeline for one build.
///
/// Every update run re-derives its updater from the immutable
/// [`ReleaseConfiguration`], so no mutable state can retarget a later run.
pub struct UpdateRuntime {
    configuration: Option<ReleaseConfiguration>,
    current_version: &'static str,
    diagnostics: UpdateDiagnosticsTracker,
}

impl UpdateRuntime {
    pub fn new(
        configuration: Option<ReleaseConfiguration>,
        current_version: &'static str,
        diagnostics: UpdateDiagnosticsTracker,
    ) -> Self {
        Self {
            configuration,
            current_version,
            diagnostics,
        }
    }

    /// Build the runtime for the current build from the crate's release constants.
    pub fn for_current_build(
        environment: bongocat_config::BuildEnvironment,
        current_version: &'static str,
        diagnostics: UpdateDiagnosticsTracker,
    ) -> Self {
        Self::new(
            ReleaseConfiguration::for_current_build(
                environment,
                RELEASE_REPOSITORY_OWNER,
                RELEASE_REPOSITORY_NAME,
                RELEASE_BINARY_NAME,
                RELEASE_BUNDLE_NAME,
            ),
            current_version,
            diagnostics,
        )
    }

    /// The release channel this build is bound to.
    ///
    /// `None` on a host outside the four shipped targets, where there is no
    /// release configuration at all.
    pub fn channel(&self) -> Option<ReleaseChannel> {
        self.configuration
            .map(|configuration| configuration.channel)
    }

    /// Whether this build can actually run an update.
    ///
    /// False when the host target is outside the shipped set, when the build's
    /// channel is not allowed to update, or when no release signing key is
    /// provisioned. Callers use this to decide whether to offer an update entry
    /// point at all, rather than offering one that can only fail.
    pub fn is_available(&self) -> bool {
        self.configuration
            .is_some_and(|configuration| configuration.channel.is_enabled())
            && RELEASE_SIGNING_KEY.is_some()
    }

    pub fn diagnostics(&self) -> &UpdateDiagnosticsTracker {
        &self.diagnostics
    }

    fn updater(&self) -> Result<github::Update, UpdateError> {
        let configuration = self
            .configuration
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::NotConfigured))?;
        if !configuration.channel.is_enabled() {
            return Err(UpdateError::new(UpdateErrorCode::EnvironmentDisabled));
        }
        let key = RELEASE_SIGNING_KEY
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::SignatureKeyMissing))?;

        let mut builder = github::Update::configure();
        builder
            .repo_owner(configuration.repository_owner)
            .repo_name(configuration.repository_name)
            .bin_name(configuration.binary_name)
            .target(configuration.target.as_str())
            .current_version(self.current_version)
            .verifying_keys([key])
            .no_confirm(true)
            .show_output(false)
            .show_download_progress(false);
        if let Some(bundle_name) = configuration.bundle_name {
            builder.bundle_path_in_archive(bundle_name);
        }

        builder.build().map_err(UpdateError::from_self_update)
    }

    /// Query whether a newer release is published.
    pub fn check(&self) -> Result<UpdateOutcome, UpdateError> {
        self.diagnostics.record_check_started();

        let outcome = self.check_inner();
        match outcome {
            Ok(UpdateOutcome::UpToDate) => {
                self.diagnostics.record_check_succeeded();
                Ok(UpdateOutcome::UpToDate)
            }
            Ok(available) => {
                self.diagnostics.record_check_succeeded();
                Ok(available)
            }
            Err(error) => {
                self.diagnostics.record_check_failed(error.code_str());
                Err(error)
            }
        }
    }

    fn check_inner(&self) -> Result<UpdateOutcome, UpdateError> {
        let updater = self.updater()?;
        match updater
            .is_update_available()
            .map_err(UpdateError::from_self_update)?
        {
            None => Ok(UpdateOutcome::UpToDate),
            Some(release) => Ok(UpdateOutcome::Available {
                version: release.version().to_owned(),
            }),
        }
    }

    /// Download, verify, extract and install the newest release.
    ///
    /// `self_update` performs the download and the install in a single call, so
    /// the download and install counter families advance together.
    pub fn install(&self) -> Result<UpdateOutcome, UpdateError> {
        self.diagnostics.record_download_started();
        self.diagnostics.record_install_started();

        let outcome = self.install_inner();
        match outcome {
            Ok(UpdateOutcome::Installed { version }) => {
                self.diagnostics.record_download_succeeded();
                self.diagnostics.record_install_succeeded();
                Ok(UpdateOutcome::Installed { version })
            }
            Ok(UpdateOutcome::UpToDate) => {
                self.diagnostics.record_download_succeeded();
                self.diagnostics.record_install_succeeded();
                Ok(UpdateOutcome::UpToDate)
            }
            Ok(available @ UpdateOutcome::Available { .. }) => Ok(available),
            Err(error) => {
                self.diagnostics.record_download_failed(error.code_str());
                self.diagnostics.record_install_failed(error.code_str());
                Err(error)
            }
        }
    }

    fn install_inner(&self) -> Result<UpdateOutcome, UpdateError> {
        let updater = self.updater()?;
        let status = updater.update().map_err(UpdateError::from_self_update)?;
        let version = status.version().to_owned();
        if status.is_updated() {
            Ok(UpdateOutcome::Installed { version })
        } else {
            Ok(UpdateOutcome::UpToDate)
        }
    }

    /// Relaunch the (already updated) executable with the same arguments.
    ///
    /// On success this does not return.
    pub fn restart(&self) -> Result<std::convert::Infallible, UpdateError> {
        self_update::restart::restart()
            .map_err(|_| UpdateError::new(UpdateErrorCode::RestartFailed))
    }
}

#[cfg(test)]
mod tests {
    use super::{RELEASE_SIGNING_KEY, UpdateError, UpdateErrorCode, UpdateOutcome, UpdateRuntime};
    use crate::diagnostics::UpdateDiagnosticsTracker;
    use crate::release::{ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};

    /// A fixed release configuration.
    ///
    /// The gating tests must not go through `for_current_build`: that returns
    /// `None` on any host outside the four shipped combinations (the Linux CI
    /// runner is one), which would turn the channel and error-code assertions
    /// below into no-ops there instead of real checks.
    fn configuration(channel: ReleaseChannel) -> ReleaseConfiguration {
        ReleaseConfiguration {
            channel,
            repository_owner: "ayangweb",
            repository_name: "BongoCat",
            binary_name: "BongoCat",
            bundle_name: Some("BongoCat.app"),
            target: UpdateTargetTriple::Aarch64AppleDarwin,
        }
    }

    fn runtime_for(channel: ReleaseChannel) -> UpdateRuntime {
        UpdateRuntime::new(
            Some(configuration(channel)),
            env!("CARGO_PKG_VERSION"),
            UpdateDiagnosticsTracker::default(),
        )
    }

    fn development_runtime() -> UpdateRuntime {
        runtime_for(ReleaseChannel::Development)
    }

    #[test]
    fn development_builds_never_reach_the_network() {
        let runtime = development_runtime();
        assert_eq!(runtime.channel().map(|c| c.as_str()), Some("development"));

        let error = runtime
            .check()
            .expect_err("development channel is disabled");
        assert_eq!(error.code(), UpdateErrorCode::EnvironmentDisabled);

        let snapshot = runtime.diagnostics().snapshot();
        assert_eq!(snapshot.checks_started, 1);
        assert_eq!(snapshot.checks_failed, 1);
        assert_eq!(
            snapshot.last_error_code,
            Some("update_environment_disabled")
        );
    }

    #[test]
    fn a_missing_signing_key_fails_closed_before_any_request() {
        assert_eq!(RELEASE_SIGNING_KEY, None);

        let runtime = runtime_for(ReleaseChannel::Production);
        let error = runtime
            .check()
            .expect_err("no signing key is provisioned in this build");
        assert_eq!(error.code(), UpdateErrorCode::SignatureKeyMissing);
    }

    #[test]
    fn a_host_outside_the_shipped_targets_has_no_release_configuration() {
        let runtime = UpdateRuntime::new(
            None,
            env!("CARGO_PKG_VERSION"),
            UpdateDiagnosticsTracker::default(),
        );

        assert_eq!(runtime.channel(), None);
        assert!(!runtime.is_available());

        let error = runtime
            .check()
            .expect_err("an unsupported host has no release configuration");
        assert_eq!(error.code(), UpdateErrorCode::NotConfigured);
    }

    #[test]
    fn error_codes_are_stable_strings() {
        let error = UpdateError::new(UpdateErrorCode::SignatureKeyMissing);
        assert_eq!(error.code_str(), "update_signature_key_missing");
        assert_eq!(error.to_string(), "update_signature_key_missing");
    }

    #[test]
    fn outcome_variants_are_distinguishable() {
        assert_ne!(
            UpdateOutcome::UpToDate,
            UpdateOutcome::Available {
                version: "1.0.0".to_owned()
            }
        );
    }

    #[test]
    fn availability_requires_a_production_channel_and_a_signing_key() {
        assert!(
            !development_runtime().is_available(),
            "the development channel is disabled"
        );
        assert!(
            !runtime_for(ReleaseChannel::Production).is_available(),
            "no signing key is provisioned"
        );
    }
}
