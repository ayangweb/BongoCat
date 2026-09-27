//! The update runtime: the update pipeline bound to BongoCat's release policy.
//!
//! The library owns the transport and the trust decision. This module owns only
//! what is BongoCat-specific: which repository and channel an update may come
//! from, when to refuse before touching the network, where the manifest is
//! requested from, and the anonymous diagnostics contract.
//!
//! The pieces are the modules below; the runtime that drives them stays here.

//! The update runtime: the `cargo-packager-updater` pipeline bound to BongoCat's
//! release configuration, environment gating and signing-key policy.
//!
//! The library owns the transport and the trust decision. It fetches a release
//! manifest over HTTPS, compares versions, downloads the announced payload,
//! verifies it against an embedded minisign public key, and installs it: on macOS
//! by swapping the whole `.app` bundle, on Windows by running the NSIS installer the
//! manifest points at. This module owns only what is BongoCat-specific — which
//! repository and channel an update may come from, when to refuse before touching
//! the network, the GitHub proxy sources the manifest is requested through before
//! the official endpoint, and the anonymous diagnostics contract.

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

mod error;
mod event;
mod manifest;
mod progress;
mod release_identity;
mod stage;
#[cfg(test)]
mod tests;

pub(crate) use manifest::*;
// The rest are reached by the crate root's `pub use` below or by the test
// tree's own `use`. A glob over those would carry nothing: a `pub(crate)`
// glob narrows everything it holds, and they hold only `pub` items.

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use error::UpdateError;
pub use event::{UpdateEvent, UpdateOutcome};
pub use progress::{UpdateProgress, UpdateRelease};
pub use release_identity::{
    GITHUB_PROXY_PREFIXES, RELEASE_BINARY_NAME, RELEASE_BUNDLE_NAME, RELEASE_MANIFEST_NAME,
    RELEASE_REPOSITORY_NAME, RELEASE_REPOSITORY_OWNER, UPDATE_MANIFEST_REQUEST_TIMEOUT,
    UPDATE_REQUEST_TIMEOUT,
};
pub use stage::{UpdateStage, UpdateUnavailability};

/// Owns the update pipeline for one build.
///
/// Every update run re-derives its updater from the immutable
/// [`ReleaseConfiguration`], so no mutable state can retarget a later run.
pub struct UpdateRuntime {
    pub(crate) configuration: ReleaseConfiguration,
    pub(crate) current_version: &'static str,
    pub(crate) diagnostics: UpdateDiagnosticsTracker,
}

impl UpdateRuntime {
    pub fn new(
        configuration: ReleaseConfiguration,
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
    pub fn channel(&self) -> ReleaseChannel {
        self.configuration.channel
    }

    /// Whether this build can actually run an update.
    ///
    /// False when the build's channel is not allowed to update, or when no
    /// release signing key is provisioned. Callers use this to decide whether to
    /// offer an update entry point at all, rather than offering one that can only
    /// fail.
    pub fn is_available(&self) -> bool {
        self.unavailability().is_none()
    }

    /// Why this build cannot update, or `None` when it can.
    ///
    /// The order is the order the gates are applied in: the channel, then the
    /// signing key. Callers surface the first reason so the UI can explain the
    /// absence of the entry point instead of leaving it unexplained.
    pub fn unavailability(&self) -> Option<UpdateUnavailability> {
        if !self.configuration.channel.is_enabled() {
            return Some(UpdateUnavailability::DevelopmentChannel);
        }
        if configured_signing_key(RELEASE_SIGNING_KEY).is_none() {
            return Some(UpdateUnavailability::SigningKeyMissing);
        }
        None
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
    pub(crate) fn manifest_endpoint(
        configuration: ReleaseConfiguration,
    ) -> Result<Url, UpdateError> {
        let url = format!(
            "https://github.com/{}/{}/releases/latest/download/{RELEASE_MANIFEST_NAME}",
            configuration.repository_owner, configuration.repository_name,
        );
        Url::parse(&url).map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))
    }

    /// The manifest endpoint one proxy source requests: the official URL prefixed.
    pub(crate) fn proxied_manifest_endpoint(
        prefix: &str,
        official: &Url,
    ) -> Result<Url, UpdateError> {
        Url::parse(&format!("{}/{official}", prefix.trim_end_matches('/')))
            .map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))
    }

    /// The sources one update run tries, proxies in order and the official endpoint
    /// last.
    pub(crate) fn manifest_sources(
        configuration: ReleaseConfiguration,
    ) -> Result<Vec<ManifestSource>, UpdateError> {
        let official = Self::manifest_endpoint(configuration)?;
        let mut sources = Vec::with_capacity(GITHUB_PROXY_PREFIXES.len() + 1);
        for prefix in GITHUB_PROXY_PREFIXES {
            sources.push(ManifestSource {
                proxy: Some(prefix),
                endpoint: Self::proxied_manifest_endpoint(prefix, &official)?,
            });
        }
        sources.push(ManifestSource {
            proxy: None,
            endpoint: official,
        });
        Ok(sources)
    }

    /// Try the manifest sources in order and stop at the first usable one.
    ///
    /// A source is usable when its request succeeds *and* the body parses as this
    /// pipeline's manifest — the checker decides that. A source that times out,
    /// errors or returns an unreadable body is skipped and the next one tried; the
    /// last error is reported when every source fails. `NoMatchingAsset` is
    /// different: it means a manifest was read but announces no asset for this host,
    /// and every source serves the same release asset, so no later source can change
    /// that answer — it is returned immediately.
    pub(crate) fn select_manifest_source<T>(
        sources: &[ManifestSource],
        mut check: impl FnMut(&ManifestSource) -> Result<ManifestFetch<T>, UpdateError>,
    ) -> Result<ManifestFetch<T>, UpdateError> {
        let mut last_error = None;
        for source in sources {
            match check(source) {
                Ok(fetch) => return Ok(fetch),
                Err(error) if error.code() == UpdateErrorCode::NoMatchingAsset => {
                    return Err(error);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| UpdateError::new(UpdateErrorCode::ReleaseFetchFailed)))
    }

    /// Prefix a GitHub release URL with the proxy that served the manifest.
    ///
    /// The manifest announces official GitHub URLs, so when its run came through a
    /// proxy the payload transfer goes through the same one. Only an HTTPS URL whose
    /// host is `github.com` is rewritten; anything else — including a URL that is
    /// already proxied, whose host is the proxy itself — is returned unchanged,
    /// which is what keeps the conversion idempotent. If the rewritten URL somehow
    /// fails to parse, the official URL is kept: a slower download beats a stopped
    /// one.
    pub(crate) fn proxied_download_url(proxy: Option<&str>, url: &Url) -> Url {
        let Some(proxy) = proxy else {
            return url.clone();
        };
        let is_official_github = url.scheme() == "https" && url.host_str() == Some("github.com");
        if !is_official_github {
            return url.clone();
        }
        Url::parse(&format!("{}/{url}", proxy.trim_end_matches('/')))
            .unwrap_or_else(|_| url.clone())
    }

    /// Build the updater for one manifest endpoint, refusing before any request when
    /// the build is not allowed to update or cannot authenticate what it would
    /// download.
    pub(crate) fn updater(
        &self,
        endpoint: Url,
        timeout: std::time::Duration,
    ) -> Result<Updater, UpdateError> {
        let configuration = self.configuration;
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
            endpoints: vec![endpoint],
            pubkey: key.to_owned(),
            windows: Some(WindowsConfig {
                installer_args: None,
                install_mode: Some(WindowsUpdateInstallMode::Quiet),
            }),
        };

        UpdaterBuilder::new(current_version, config)
            .timeout(timeout)
            .build()
            .map_err(UpdateError::from_library)
    }

    /// Request one manifest source with the short per-source bound.
    ///
    /// On success the payload transfer's own bound is restored onto the update and
    /// the announced download URL is converted to the proxy that served the
    /// manifest, so the rest of this update run stays on that source.
    pub(crate) fn check_source(
        &self,
        source: &ManifestSource,
    ) -> Result<ManifestFetch<cargo_packager_updater::Update>, UpdateError> {
        let updater = self.updater(source.endpoint.clone(), UPDATE_MANIFEST_REQUEST_TIMEOUT)?;
        match updater.check().map_err(UpdateError::from_library)? {
            None => Ok(ManifestFetch::UpToDate),
            Some(mut update) => {
                update.timeout = Some(UPDATE_REQUEST_TIMEOUT);
                update.download_url =
                    Self::proxied_download_url(source.proxy, &update.download_url);
                Ok(ManifestFetch::Offered(update))
            }
        }
    }

    /// Fetch the release manifest, trying the proxy sources before the official one.
    pub(crate) fn fetch_manifest(
        &self,
    ) -> Result<ManifestFetch<cargo_packager_updater::Update>, UpdateError> {
        let configuration = self.configuration;
        let sources = Self::manifest_sources(configuration)?;
        Self::select_manifest_source(&sources, |source| self.check_source(source))
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

    /// The page a user opens to read a release's full notes.
    ///
    /// Derived from the immutable release identity, so the update window can offer a
    /// link without owning the repository layout itself.
    pub fn release_page_url(&self, version: &str) -> Option<String> {
        let configuration = self.configuration;
        Some(format!(
            "https://github.com/{}/{}/releases/tag/v{}",
            configuration.repository_owner,
            configuration.repository_name,
            version.trim_start_matches('v'),
        ))
    }

    pub(crate) fn check_inner(&self) -> Result<UpdateOutcome, UpdateError> {
        match self.fetch_manifest()? {
            ManifestFetch::UpToDate => Ok(UpdateOutcome::UpToDate),
            ManifestFetch::Offered(update) => Ok(UpdateOutcome::Available {
                release: UpdateRelease {
                    version: update.version,
                    notes: update.body,
                },
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
        self.install_with_observer(|_| {})
    }

    /// Download, verify and install the newest release, reporting each step.
    ///
    /// `observe` runs on the calling thread, so the caller decides how the values
    /// reach the UI; this crate never touches a UI type. The download and the install
    /// are separate library calls on purpose: `download_extended` authenticates the
    /// payload before returning, so the caller can report "verifying" and "installing"
    /// as distinct states instead of collapsing them into one opaque call.
    ///
    /// There is deliberately no cancellation: the library reads and authenticates the
    /// payload inside one call and offers no abort hook, so a cancel that returned
    /// early would only abandon the result while the transfer kept running.
    pub fn install_with_observer(
        &self,
        observe: impl Fn(UpdateEvent),
    ) -> Result<UpdateOutcome, UpdateError> {
        self.diagnostics.record_download_started();
        self.diagnostics.record_install_started();

        match self.install_inner(observe) {
            Ok(outcome) => {
                self.diagnostics.record_download_succeeded();
                self.diagnostics.record_install_succeeded();
                Ok(outcome)
            }
            Err(error) => {
                // The library downloads and installs in one call, so a failure is
                // counted in both families; the error's own stage says which step it
                // actually stopped in.
                self.diagnostics.record_download_failed(error.code_str());
                self.diagnostics.record_install_failed(error.code_str());
                Err(error)
            }
        }
    }

    pub(crate) fn install_inner(
        &self,
        observe: impl Fn(UpdateEvent),
    ) -> Result<UpdateOutcome, UpdateError> {
        match self.fetch_manifest()? {
            ManifestFetch::UpToDate => Ok(UpdateOutcome::UpToDate),
            ManifestFetch::Offered(update) => {
                let version = update.version.clone();
                let downloaded_bytes = std::cell::Cell::new(0_u64);
                let payload = update
                    .download_extended(
                        |chunk, total| {
                            let downloaded = downloaded_bytes.get().saturating_add(chunk as u64);
                            downloaded_bytes.set(downloaded);
                            observe(UpdateEvent::Progress(UpdateProgress {
                                downloaded_bytes: downloaded,
                                total_bytes: total,
                            }));
                        },
                        || observe(UpdateEvent::DownloadFinished),
                    )
                    .map_err(|error| {
                        let error = UpdateError::from_library(error);
                        UpdateError::at(UpdateError::download_stage(error.code()), error.code())
                    })?;
                observe(UpdateEvent::Verified);
                update.install(payload).map_err(|error| {
                    let error = UpdateError::from_library(error);
                    UpdateError::at(UpdateStage::Install, error.code())
                })?;
                Ok(UpdateOutcome::Installed { version })
            }
        }
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
pub(crate) fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    use std::os::unix::process::CommandExt;

    let executable = std::env::current_exe()
        .map_err(|_| UpdateError::at(UpdateStage::Install, UpdateErrorCode::RestartFailed))?;
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    // `exec` replaces the current process image and only returns on failure.
    let _ = command.exec();
    Err(UpdateError::at(
        UpdateStage::Install,
        UpdateErrorCode::RestartFailed,
    ))
}

/// See the unix version above; Windows has no `exec`, so the updated executable is
/// spawned as a new process and the current one exits.
#[cfg(windows)]
pub(crate) fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    let executable = std::env::current_exe()
        .map_err(|_| UpdateError::at(UpdateStage::Install, UpdateErrorCode::RestartFailed))?;
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    command
        .spawn()
        .map_err(|_| UpdateError::at(UpdateStage::Install, UpdateErrorCode::RestartFailed))?;
    std::process::exit(0);
}
