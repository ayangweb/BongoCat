//! The update runtime: the `cargo-packager-updater` pipeline bound to BongoCat's
//! release configuration, environment gating and signing-key policy.
//!
//! The library owns the transport and the trust decision. It fetches a release
//! manifest over HTTPS, compares versions, downloads the announced payload,
//! verifies it against an embedded minisign public key, and installs it: on macOS
//! by swapping the whole `.app` bundle, on Windows by running the NSIS installer the
//! manifest points at. This module owns only what is BongoCat-specific — which
//! repository and channel an update may come from, when to refuse before touching
//! the network, and the anonymous diagnostics contract.

use cargo_packager_updater::semver::Version;
use cargo_packager_updater::url::Url;
use cargo_packager_updater::{
    Config, Updater, UpdaterBuilder, WindowsConfig, WindowsUpdateInstallMode,
};

use crate::{
    UpdateErrorCode,
    diagnostics::UpdateDiagnosticsTracker,
    release::{ReleaseChannel, ReleaseConfiguration},
};

/// Release signing public key used to authenticate downloaded update payloads.
///
/// A minisign public key, encoded exactly as `cargo-packager`'s signer writes it to the
/// `.pub` file: base64 of the Minisign `PublicKeyBox` text, on one line. Generate the
/// pair with `just keygen <file>` (or `cargo run -p bongocat-packaging --
/// --generate-signing-key <file>`), keep the private half out of the repository, and
/// paste the **public** half here — a public key is not a secret.
///
/// This is the provisioned release key with ID `DF5E2C9D255DD85E`.
///
/// The runtime **fails closed** when this is absent, empty or whitespace: an update
/// check or install returns [`UpdateErrorCode::SignatureKeyMissing`] before any request
/// is made, so a build can never fetch, verify or install against an unconfigured key.
/// This guard is deliberate defence in depth — `cargo-packager-updater` already rejects
/// an empty public key, because it has to decode one — but it keeps the failure a
/// stable, path-free code instead of a library error.
pub const RELEASE_SIGNING_KEY: Option<&str> = Some(
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERGNUUyQzlEMjU1REQ4NUUKUldSZTJGMGxuU3hlMzIyMHdwUldMNTRvRStMb1hJZlI3T2w0TEdFRlI3YXhsa3k0NldGUW5EN20K",
);

/// Return the configured signing key, treating empty and whitespace-only values as
/// absent so the fail-closed policy can be tested independently of the shipped key.
fn configured_signing_key(key: Option<&str>) -> Option<&str> {
    key.filter(|key| !key.trim().is_empty())
}

/// The repository that publishes BongoCat releases.
pub const RELEASE_REPOSITORY_OWNER: &str = "ayangweb";
pub const RELEASE_REPOSITORY_NAME: &str = "BongoCat";

/// Name of the release manifest asset an update run requests.
///
/// One shared manifest for the whole release, carrying a `<os>-<arch>` entry per
/// shipped target. `crates/bongocat-packaging` writes one fragment per target and merges
/// them into this file; the agreement on both the name and the platform keys is pinned
/// by `tools/tests/test_update_release_contract.py`.
pub const RELEASE_MANIFEST_NAME: &str = "latest.json";

/// The executable the packaging pipeline ships.
///
/// Release identity, not a transport input: `cargo-packager-updater` locates the
/// installed application from the running executable and takes the payload location
/// from the manifest, so this name no longer finds a path inside an archive. It stays
/// the single source of truth for what the pipeline builds and installs.
pub const RELEASE_BINARY_NAME: &str = "bongocat-app";

/// The macOS bundle directory name.
///
/// Must equal the `.app` directory `crates/bongocat-packaging` produces, which is
/// `<product name>.app`. Also release identity rather than a transport input: the
/// updater extracts an archive's contents under the bundle path it derives from the
/// running executable.
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
    /// `cargo_packager_updater::Error` is `#[non_exhaustive]`, so an unrecognized
    /// variant degrades to [`UpdateErrorCode::Internal`] rather than leaking library
    /// text into the diagnostics export.
    fn from_library(error: cargo_packager_updater::Error) -> Self {
        use cargo_packager_updater::Error;

        let code = match error {
            // Configuration and build-time mismatches: nothing can be requested.
            Error::EmptyEndpoints
            | Error::UrlParse(_)
            | Error::UnsupportedArch
            | Error::UnsupportedOs
            | Error::Semver(_)
            | Error::Http(_) => UpdateErrorCode::NotConfigured,

            // The manifest could not be fetched or did not parse.
            Error::ReleaseNotFound | Error::Serialization(_) => UpdateErrorCode::ReleaseFetchFailed,

            // A well-formed manifest that says nothing about this host.
            Error::TargetNotFound(_) => UpdateErrorCode::NoMatchingAsset,

            Error::Network(_) | Error::Reqwest(_) => UpdateErrorCode::DownloadTransportFailed,

            // Authenticity of the downloaded payload.
            Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
                UpdateErrorCode::SignatureInvalid
            }

            // Payload shape and the install step itself.
            Error::UnsupportedUpdateFormat
            | Error::FailedToDetermineExtractPath
            | Error::TempDirNotOnSameMountPoint
            | Error::PersistError(_)
            | Error::Io(_) => UpdateErrorCode::InstallFailed,

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

/// Owns the update pipeline for one build.
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
    /// `None` on a host outside the shipped targets, where there is no release
    /// configuration at all.
    pub fn channel(&self) -> Option<ReleaseChannel> {
        self.configuration
            .map(|configuration| configuration.channel)
    }

    /// Whether this build can actually run an update.
    ///
    /// False when the host target is outside the shipped set, when the build's
    /// channel is not allowed to update, or when no release signing key is
    /// provisioned. Callers use this to decide whether to offer an update entry point
    /// at all, rather than offering one that can only fail.
    pub fn is_available(&self) -> bool {
        self.configuration
            .is_some_and(|configuration| configuration.channel.is_enabled())
            && configured_signing_key(RELEASE_SIGNING_KEY).is_some()
    }

    pub fn diagnostics(&self) -> &UpdateDiagnosticsTracker {
        &self.diagnostics
    }

    /// The manifest URL an update run reads, derived from the release identity.
    ///
    /// The manifest is one shared asset under the repository's latest release, so every
    /// platform requests the same URL; the library picks this host's entry out of its
    /// `platforms` map using the `<os>-<arch>` key it derives at runtime. The target
    /// triple is therefore release identity, not an input to the endpoint.
    fn manifest_endpoint(configuration: ReleaseConfiguration) -> Result<Url, UpdateError> {
        let url = format!(
            "https://github.com/{}/{}/releases/latest/download/{RELEASE_MANIFEST_NAME}",
            configuration.repository_owner, configuration.repository_name,
        );
        Url::parse(&url).map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))
    }

    /// Build the updater for this build, refusing before any request when the build is
    /// not allowed to update or cannot authenticate what it would download.
    fn updater(&self) -> Result<Updater, UpdateError> {
        let configuration = self
            .configuration
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::NotConfigured))?;
        if !configuration.channel.is_enabled() {
            return Err(UpdateError::new(UpdateErrorCode::EnvironmentDisabled));
        }
        let key = configured_signing_key(RELEASE_SIGNING_KEY)
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::SignatureKeyMissing))?;

        // The version is compiled in from `CARGO_PKG_VERSION`, so a parse failure means
        // this build cannot state its own version and cannot compare it either.
        let current_version = Version::parse(self.current_version)
            .map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))?;

        // A per-user NSIS install needs no elevation, and `/S` keeps the update
        // silent; `/R` asks the installer to relaunch the application, which the
        // Windows install path depends on because it exits the current process.
        let config = Config {
            endpoints: vec![Self::manifest_endpoint(configuration)?],
            pubkey: key.to_owned(),
            windows: Some(WindowsConfig {
                installer_args: None,
                install_mode: Some(WindowsUpdateInstallMode::Quiet),
            }),
        };

        UpdaterBuilder::new(current_version, config)
            .build()
            .map_err(UpdateError::from_library)
    }

    /// Query whether a newer release is published.
    pub fn check(&self) -> Result<UpdateOutcome, UpdateError> {
        self.diagnostics.record_check_started();

        match self.check_inner() {
            Ok(outcome) => {
                self.diagnostics.record_check_succeeded();
                Ok(outcome)
            }
            Err(error) => {
                self.diagnostics.record_check_failed(error.code_str());
                Err(error)
            }
        }
    }

    fn check_inner(&self) -> Result<UpdateOutcome, UpdateError> {
        let updater = self.updater()?;
        match updater.check().map_err(UpdateError::from_library)? {
            None => Ok(UpdateOutcome::UpToDate),
            Some(update) => Ok(UpdateOutcome::Available {
                version: update.version,
            }),
        }
    }

    /// Download, verify and install the newest release.
    ///
    /// The library performs the download and the install in one call, so the download
    /// and install counter families advance together.
    ///
    /// On Windows the NSIS install path terminates the process instead of returning:
    /// the installer replaces files the running application holds open. `Installed` is
    /// therefore only observable on macOS; on Windows a successful run ends in process
    /// exit, and the installer's `/R` argument relaunches the application.
    pub fn install(&self) -> Result<UpdateOutcome, UpdateError> {
        self.diagnostics.record_download_started();
        self.diagnostics.record_install_started();

        match self.install_inner() {
            Ok(outcome) => {
                self.diagnostics.record_download_succeeded();
                self.diagnostics.record_install_succeeded();
                Ok(outcome)
            }
            Err(error) => {
                self.diagnostics.record_download_failed(error.code_str());
                self.diagnostics.record_install_failed(error.code_str());
                Err(error)
            }
        }
    }

    fn install_inner(&self) -> Result<UpdateOutcome, UpdateError> {
        let updater = self.updater()?;
        let Some(update) = updater.check().map_err(UpdateError::from_library)? else {
            return Ok(UpdateOutcome::UpToDate);
        };
        let version = update.version.clone();
        update
            .download_and_install()
            .map_err(UpdateError::from_library)?;
        Ok(UpdateOutcome::Installed { version })
    }

    /// Relaunch the (already updated) executable with the same arguments.
    ///
    /// On success this does not return.
    pub fn restart(&self) -> Result<std::convert::Infallible, UpdateError> {
        restart_current_process()
    }
}

/// Re-run the current executable with the same arguments, replacing the process.
#[cfg(unix)]
fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    use std::os::unix::process::CommandExt;

    let executable =
        std::env::current_exe().map_err(|_| UpdateError::new(UpdateErrorCode::RestartFailed))?;
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    // `exec` replaces the current process image and only returns on failure.
    let _ = command.exec();
    Err(UpdateError::new(UpdateErrorCode::RestartFailed))
}

/// See the unix version above; Windows has no `exec`, so the updated executable is
/// spawned as a new process and the current one exits.
#[cfg(windows)]
fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    let executable =
        std::env::current_exe().map_err(|_| UpdateError::new(UpdateErrorCode::RestartFailed))?;
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    command
        .spawn()
        .map_err(|_| UpdateError::new(UpdateErrorCode::RestartFailed))?;
    std::process::exit(0);
}

#[cfg(not(any(unix, windows)))]
fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    Err(UpdateError::new(UpdateErrorCode::RestartFailed))
}

#[cfg(test)]
mod tests {
    use super::{
        RELEASE_BINARY_NAME, RELEASE_BUNDLE_NAME, RELEASE_MANIFEST_NAME, RELEASE_REPOSITORY_NAME,
        RELEASE_REPOSITORY_OWNER, RELEASE_SIGNING_KEY, UpdateError, UpdateErrorCode, UpdateOutcome,
        UpdateRuntime, configured_signing_key,
    };
    use crate::diagnostics::UpdateDiagnosticsTracker;
    use crate::release::{ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};

    /// A fixed release configuration.
    ///
    /// The gating tests must not go through `for_current_build`: that returns `None` on
    /// any host outside the shipped combinations (the Linux CI runner is one), which
    /// would turn the channel and error-code assertions below into no-ops there instead
    /// of real checks.
    fn configuration(channel: ReleaseChannel) -> ReleaseConfiguration {
        ReleaseConfiguration {
            channel,
            repository_owner: "ayangweb",
            repository_name: "BongoCat",
            binary_name: "bongocat-app",
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
    fn the_release_signing_key_is_provisioned() {
        assert_eq!(
            RELEASE_SIGNING_KEY,
            Some(
                "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERGNUUyQzlEMjU1REQ4NUUKUldSZTJGMGxuU3hlMzIyMHdwUldMNTRvRStMb1hJZlI3T2w0TEdFRlI3YXhsa3k0NldGUW5EN20K"
            )
        );
        assert!(configured_signing_key(RELEASE_SIGNING_KEY).is_some());
    }

    #[test]
    fn absent_or_blank_signing_keys_fail_closed() {
        assert!(configured_signing_key(None).is_none());
        assert!(configured_signing_key(Some("")).is_none());
        assert!(configured_signing_key(Some("   \n\t")).is_none());
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

    /// The endpoint is the repository's shared release manifest.
    ///
    /// The literal URL is restated on purpose: it is the address the product is
    /// expected to check, so a change to the repository identity or the asset name has
    /// to be an intentional edit here rather than a silent consequence of a constant.
    #[test]
    fn the_manifest_endpoint_is_the_shared_release_manifest() {
        let endpoint = UpdateRuntime::manifest_endpoint(configuration(ReleaseChannel::Production))
            .expect("the release identity produces a valid URL");

        assert_eq!(
            endpoint.as_str(),
            format!(
                "https://github.com/{RELEASE_REPOSITORY_OWNER}/{RELEASE_REPOSITORY_NAME}/releases/latest/download/{RELEASE_MANIFEST_NAME}"
            )
        );
        assert_eq!(
            endpoint.as_str(),
            "https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
        );
        assert_eq!(endpoint.scheme(), "https");

        // One shared manifest serves every target, so the build's triple must not
        // change the endpoint. The library still needs it to pick this host's entry out
        // of the manifest's `platforms` map.
        for target in [
            UpdateTargetTriple::Aarch64AppleDarwin,
            UpdateTargetTriple::X86_64AppleDarwin,
            UpdateTargetTriple::X86_64PcWindowsMsvc,
        ] {
            let mut configuration = configuration(ReleaseChannel::Production);
            configuration.target = target;
            assert_eq!(
                UpdateRuntime::manifest_endpoint(configuration)
                    .expect("every shipped target produces a valid URL"),
                endpoint,
                "{} must request the shared manifest",
                target.as_str()
            );
        }
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
            runtime_for(ReleaseChannel::Production).is_available(),
            "the provisioned signing key makes production updates available"
        );
    }

    /// The release identity constants describe what the packaging pipeline ships.
    #[test]
    fn the_release_identity_matches_the_packaging_conventions() {
        assert_eq!(RELEASE_BINARY_NAME, "bongocat-app");
        assert_eq!(RELEASE_BUNDLE_NAME, "BongoCat.app");
    }
}
