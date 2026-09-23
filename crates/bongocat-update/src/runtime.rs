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

/// The pipeline stage an update stopped in.
///
/// The stage is what lets the UI say *where* an update failed instead of only
/// *that* it failed, so it is part of the failure value rather than something a
/// caller has to infer from the error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStage {
    /// Reading and parsing the release manifest.
    Check,
    /// Transferring the payload.
    Download,
    /// Authenticating the downloaded payload.
    Verify,
    /// Writing the payload into the installation.
    Install,
}

impl UpdateStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Download => "download",
            Self::Verify => "verify",
            Self::Install => "install",
        }
    }
}

/// Upper bound on one payload transfer.
///
/// The transport has no timeout of its own, so without this a stalled connection
/// would leave the update worker blocked indefinitely. The bound is deliberately
/// generous — it covers a whole payload transfer, not one read — because the point is
/// to escape a dead connection, not to police a slow one. The update window is not
/// blocked by a transfer in progress and can be closed while it runs.
///
/// Manifest requests have their own, much shorter bound in
/// [`UPDATE_MANIFEST_REQUEST_TIMEOUT`]; this value is restored onto the
/// [`cargo_packager_updater::Update`] before its payload is downloaded.
pub const UPDATE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1800);

/// Upper bound on one manifest request to one source.
///
/// The manifest is a small document a healthy source serves in seconds, but the
/// sources are tried in sequence and a source that accepts the connection and never
/// answers would otherwise stack the transfer-sized [`UPDATE_REQUEST_TIMEOUT`] in
/// front of every later source. The bound is generous for a document this size — the
/// point is to retire a dead source quickly, not to police a slow one — and it is
/// deliberately shorter than [`UPDATE_REQUEST_TIMEOUT`], which keeps covering the
/// payload transfer itself.
pub const UPDATE_MANIFEST_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The GitHub proxy prefixes an update run tries, in order, before the official
/// endpoint.
///
/// Direct GitHub access is unreliable from mainland China, so the manifest request
/// is prefixed with each of these in turn (`<proxy>/<github-url>`) before the
/// official URL is tried last. A source counts as available only when its request
/// succeeds **and** the body parses as this release pipeline's manifest, and the
/// proxy that served the manifest is then used for that run's payload download too
/// (see `UpdateRuntime`'s download-URL conversion).
///
/// This is a reachability policy, not a trust decision: a proxy relays the request
/// and can stall a run, serve a stale or hostile manifest, or point the download
/// elsewhere — but it cannot forge the minisign signature the payload is checked
/// against, so nothing it tampers with can reach an install. The downgrade risk of
/// a manifest that names an older but validly signed release is the pre-existing
/// one recorded in ADR-0034, unchanged by proxying.
pub const GITHUB_PROXY_PREFIXES: &[&str] = &[
    "https://cdn.gh-proxy.org",
    "https://v6.gh-proxy.org",
    "https://axisnow.gh-proxy.org",
    "https://v4.gh-proxy.org",
    "https://gh-proxy.org",
];

/// A stable-coded update failure, tagged with the stage that produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateError {
    code: UpdateErrorCode,
    stage: UpdateStage,
}

impl UpdateError {
    const fn new(code: UpdateErrorCode) -> Self {
        Self {
            code,
            stage: UpdateStage::Check,
        }
    }

    /// Build a failure that stopped in a specific stage.
    ///
    /// Public because the stage is part of the value's meaning: a caller that
    /// classifies a failure itself — or a test that scripts one — has to be able to
    /// say which step it happened in.
    pub const fn at(stage: UpdateStage, code: UpdateErrorCode) -> Self {
        Self { code, stage }
    }

    pub const fn code(self) -> UpdateErrorCode {
        self.code
    }

    pub const fn code_str(self) -> &'static str {
        self.code.as_str()
    }

    pub const fn stage(self) -> UpdateStage {
        self.stage
    }

    /// The stage a library failure happened in once the transport has already been
    /// entered.
    ///
    /// `cargo_packager_updater::Update::download` reads the payload and verifies its
    /// signature in one call, so a single library error can come from either step;
    /// the code decides which. Anything unrecognized is reported as a transfer
    /// failure, which is the conservative choice: it does not claim the payload was
    /// authenticated.
    const fn download_stage(code: UpdateErrorCode) -> UpdateStage {
        match code {
            UpdateErrorCode::SignatureInvalid => UpdateStage::Verify,
            _ => UpdateStage::Download,
        }
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
            | Error::Http(_) => UpdateErrorCode::NotConfigured,

            // The published release carries no manifest this build could fetch.
            Error::ReleaseNotFound => UpdateErrorCode::ReleaseFetchFailed,

            // The manifest arrived but is not one this build can read.
            //
            // `Semver` belongs here rather than with the configuration failures: this
            // build's own version is parsed before any request is made, so a semver
            // error out of the library can only be the manifest's `version` field.
            Error::Serialization(_) | Error::Semver(_) => UpdateErrorCode::ReleaseManifestInvalid,

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

/// A step of the install pipeline a caller can observe while it runs.
///
/// The library verifies the payload immediately after reading it, so the three
/// events are the only points at which progress is knowable from outside: bytes
/// arrive, the transfer ends, and the payload is authenticated. Everything after
/// `Verified` is the install itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateEvent {
    /// Bytes arrived; `downloaded_bytes` is cumulative for this transfer.
    Progress(UpdateProgress),
    /// The payload has been read in full and its signature is about to be checked.
    DownloadFinished,
    /// The payload is authenticated and is about to be installed.
    Verified,
}

/// Why this build cannot check for or install updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateUnavailability {
    /// The build's channel is not allowed to update.
    DevelopmentChannel,
    /// No release signing key is provisioned.
    SigningKeyMissing,
}

/// One manifest source: an endpoint and the proxy prefix that produced it.
///
/// `proxy` is `None` for the official GitHub endpoint, which is tried last and
/// needs no download-URL conversion.
struct ManifestSource {
    proxy: Option<&'static str>,
    endpoint: Url,
}

/// What one successful manifest fetch produced.
#[derive(Debug)]
enum ManifestFetch<T> {
    /// The manifest is readable and announces nothing newer than this build.
    UpToDate,
    /// A newer release is offered, with its download URL already converted to the
    /// proxy that served the manifest (no conversion on the official endpoint).
    Offered(T),
}

/// A published release this build could move to.
///
/// `notes` is the release changelog the shared manifest announces. It is optional
/// because the manifest treats it as optional: a release published without notes
/// still offers a valid update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateRelease {
    pub version: String,
    pub notes: Option<String>,
}

/// Transfer progress of an in-flight update download.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateProgress {
    pub downloaded_bytes: u64,
    /// The payload size the server announced, when it announced one.
    pub total_bytes: Option<u64>,
}

impl UpdateProgress {
    /// The completed fraction of the transfer, when the total size is known.
    pub fn fraction(self) -> Option<f32> {
        let total = self.total_bytes.filter(|total| *total > 0)?;
        Some((self.downloaded_bytes as f64 / total as f64).min(1.0) as f32)
    }
}

/// The outcome of a completed update check or install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateOutcome {
    /// The running build is already the newest release.
    UpToDate,
    /// A newer release exists; nothing was installed.
    Available { release: UpdateRelease },
    /// The release was installed.
    Installed { version: String },
}

impl UpdateOutcome {
    /// The release this outcome is about, for callers that only need the metadata.
    pub fn release(&self) -> Option<UpdateRelease> {
        match self {
            Self::UpToDate => None,
            Self::Available { release } => Some(release.clone()),
            Self::Installed { version } => Some(UpdateRelease {
                version: version.clone(),
                notes: None,
            }),
        }
    }
}

/// Owns the update pipeline for one build.
///
/// Every update run re-derives its updater from the immutable
/// [`ReleaseConfiguration`], so no mutable state can retarget a later run.
pub struct UpdateRuntime {
    configuration: ReleaseConfiguration,
    current_version: &'static str,
    diagnostics: UpdateDiagnosticsTracker,
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
    fn manifest_endpoint(configuration: ReleaseConfiguration) -> Result<Url, UpdateError> {
        let url = format!(
            "https://github.com/{}/{}/releases/latest/download/{RELEASE_MANIFEST_NAME}",
            configuration.repository_owner, configuration.repository_name,
        );
        Url::parse(&url).map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))
    }

    /// The manifest endpoint one proxy source requests: the official URL prefixed.
    fn proxied_manifest_endpoint(prefix: &str, official: &Url) -> Result<Url, UpdateError> {
        Url::parse(&format!("{}/{official}", prefix.trim_end_matches('/')))
            .map_err(|_| UpdateError::new(UpdateErrorCode::NotConfigured))
    }

    /// The sources one update run tries, proxies in order and the official endpoint
    /// last.
    fn manifest_sources(
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
    fn select_manifest_source<T>(
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
    fn proxied_download_url(proxy: Option<&str>, url: &Url) -> Url {
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
    fn updater(&self, endpoint: Url, timeout: std::time::Duration) -> Result<Updater, UpdateError> {
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
    fn check_source(
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
    fn fetch_manifest(&self) -> Result<ManifestFetch<cargo_packager_updater::Update>, UpdateError> {
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

    fn check_inner(&self) -> Result<UpdateOutcome, UpdateError> {
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

    fn install_inner(&self, observe: impl Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError> {
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
fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
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
fn restart_current_process() -> Result<std::convert::Infallible, UpdateError> {
    let executable = std::env::current_exe()
        .map_err(|_| UpdateError::at(UpdateStage::Install, UpdateErrorCode::RestartFailed))?;
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    command
        .spawn()
        .map_err(|_| UpdateError::at(UpdateStage::Install, UpdateErrorCode::RestartFailed))?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::{
        GITHUB_PROXY_PREFIXES, ManifestFetch, ManifestSource, RELEASE_BINARY_NAME,
        RELEASE_BUNDLE_NAME, RELEASE_MANIFEST_NAME, RELEASE_REPOSITORY_NAME,
        RELEASE_REPOSITORY_OWNER, RELEASE_SIGNING_KEY, UPDATE_MANIFEST_REQUEST_TIMEOUT,
        UPDATE_REQUEST_TIMEOUT, UpdateError, UpdateErrorCode, UpdateOutcome, UpdateProgress,
        UpdateRelease, UpdateRuntime, UpdateStage, UpdateUnavailability, configured_signing_key,
    };
    use crate::diagnostics::UpdateDiagnosticsTracker;
    use crate::release::{ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};
    use cargo_packager_updater::url::Url;

    /// A fixed release configuration.
    ///
    /// The gating tests must not go through `for_current_build`: a missing
    /// configuration would turn the channel and error-code assertions below into
    /// no-ops instead of real checks.
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
            configuration(channel),
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
        assert_eq!(runtime.channel().as_str(), "development");

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

    /// The transport has no timeout of its own, so the bound has to exist and has to
    /// stay generous enough for a whole payload on a slow link.
    #[test]
    fn a_transfer_is_bounded_but_not_tight() {
        assert!(UPDATE_REQUEST_TIMEOUT >= std::time::Duration::from_secs(600));
        assert!(UPDATE_REQUEST_TIMEOUT <= std::time::Duration::from_secs(3600));
    }

    #[test]
    fn outcome_variants_are_distinguishable() {
        assert_ne!(
            UpdateOutcome::UpToDate,
            UpdateOutcome::Available {
                release: UpdateRelease {
                    version: "1.0.0".to_owned(),
                    notes: None,
                },
            }
        );
    }

    #[test]
    fn an_available_release_carries_its_changelog() {
        let outcome = UpdateOutcome::Available {
            release: UpdateRelease {
                version: "1.2.0".to_owned(),
                notes: Some("- fixed the thing".to_owned()),
            },
        };
        assert_eq!(
            outcome.release(),
            Some(UpdateRelease {
                version: "1.2.0".to_owned(),
                notes: Some("- fixed the thing".to_owned()),
            })
        );
        assert_eq!(UpdateOutcome::UpToDate.release(), None);
    }

    /// A manifest source for the selection tests: a stable URL and the proxy that
    /// produced it.
    fn test_source(proxy: Option<&'static str>, tail: &str) -> ManifestSource {
        ManifestSource {
            proxy,
            endpoint: Url::parse(&format!("https://source.invalid/{tail}"))
                .expect("a test endpoint parses"),
        }
    }
    /// The proxy list is the product's ordered fallback policy, so its content and
    /// order are pinned the way the official endpoint's URL is: the literals are
    /// restated on purpose, so a change to either is an intentional edit.
    #[test]
    fn the_proxy_prefixes_are_the_ordered_fallback_policy() {
        assert_eq!(
            GITHUB_PROXY_PREFIXES,
            &[
                "https://cdn.gh-proxy.org",
                "https://v6.gh-proxy.org",
                "https://axisnow.gh-proxy.org",
                "https://v4.gh-proxy.org",
                "https://gh-proxy.org",
            ]
        );
        for prefix in GITHUB_PROXY_PREFIXES {
            let url = Url::parse(prefix).expect("a proxy prefix parses as a URL");
            assert_eq!(url.scheme(), "https", "{prefix} must be HTTPS");
            assert!(
                url.host_str().is_some_and(|host| !host.is_empty()),
                "{prefix} must name a host"
            );
            assert!(
                !prefix.ends_with('/'),
                "{prefix} must not end with a slash; prefixing adds its own"
            );
        }
    }

    /// The official URL prefixed by the first proxy is exactly the address the
    /// product expects a proxy check to hit.
    #[test]
    fn the_proxied_manifest_endpoint_prefixes_the_official_url() {
        let official = UpdateRuntime::manifest_endpoint(configuration(ReleaseChannel::Production))
            .expect("the release identity produces a valid URL");

        let proxied = UpdateRuntime::proxied_manifest_endpoint(
            GITHUB_PROXY_PREFIXES
                .first()
                .expect("the list is non-empty"),
            &official,
        )
        .expect("a proxy prefix and the official URL produce a valid endpoint");

        assert_eq!(
            proxied.as_str(),
            "https://cdn.gh-proxy.org/https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
        );
    }

    /// The source order is the fallback order: every proxy prefixed, official last.
    #[test]
    fn manifest_sources_try_proxies_in_order_then_the_official_endpoint() {
        let sources = UpdateRuntime::manifest_sources(configuration(ReleaseChannel::Production))
            .expect("every source endpoint parses");

        assert_eq!(sources.len(), GITHUB_PROXY_PREFIXES.len() + 1);
        for (source, prefix) in sources.iter().zip(GITHUB_PROXY_PREFIXES) {
            assert_eq!(source.proxy, Some(*prefix));
            assert_eq!(
                source.endpoint.as_str(),
                format!(
                    "{prefix}/https://github.com/{RELEASE_REPOSITORY_OWNER}/{RELEASE_REPOSITORY_NAME}/releases/latest/download/{RELEASE_MANIFEST_NAME}"
                )
            );
        }
        let official = sources.last().expect("the official source is last");
        assert_eq!(
            official.proxy, None,
            "the official endpoint needs no prefix"
        );
        assert_eq!(
            official.endpoint.as_str(),
            "https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
        );
    }

    /// A source is usable only when its request succeeds and the manifest parses;
    /// anything else moves on to the next source, and the first usable one wins.
    #[test]
    fn the_first_usable_source_wins_and_later_sources_are_not_consulted() {
        let sources = vec![
            test_source(Some("https://first.invalid"), "a"),
            test_source(Some("https://second.invalid"), "b"),
            test_source(None, "official"),
        ];

        let mut consulted = Vec::new();
        let fetch = UpdateRuntime::select_manifest_source(&sources, |source| {
            consulted.push(source.endpoint.as_str().to_owned());
            if source.proxy == Some("https://second.invalid") {
                Ok(ManifestFetch::Offered("second"))
            } else {
                Err(UpdateError::new(UpdateErrorCode::ReleaseFetchFailed))
            }
        })
        .expect("the second source is usable");

        assert_eq!(
            consulted,
            vec![
                "https://source.invalid/a".to_owned(),
                "https://source.invalid/b".to_owned(),
            ],
            "the official endpoint must not be requested once a proxy succeeded"
        );
        let ManifestFetch::Offered(update) = fetch else {
            panic!("expected an offered update, got {fetch:?}");
        };
        assert_eq!(update, "second");
    }

    /// Every proxy failing hands the run to the official endpoint.
    #[test]
    fn all_proxies_failing_falls_through_to_the_official_endpoint() {
        let sources = vec![
            test_source(Some("https://proxy.invalid"), "a"),
            test_source(None, "official"),
        ];

        let mut consulted = Vec::new();
        let fetch = UpdateRuntime::select_manifest_source(&sources, |source| {
            consulted.push(source.proxy);
            match source.proxy {
                Some(_) => Err(UpdateError::new(UpdateErrorCode::ReleaseFetchFailed)),
                None => Ok(ManifestFetch::Offered("official")),
            }
        })
        .expect("the official endpoint is usable");

        assert_eq!(consulted, vec![Some("https://proxy.invalid"), None]);
        let ManifestFetch::Offered(update) = fetch else {
            panic!("expected an offered update, got {fetch:?}");
        };
        assert_eq!(update, "official");
    }

    /// A run where every source fails reports the last source's error.
    #[test]
    fn all_sources_failing_reports_the_last_error() {
        let sources = vec![
            test_source(Some("https://first.invalid"), "a"),
            test_source(None, "official"),
        ];

        let error = UpdateRuntime::select_manifest_source::<&str>(&sources, |source| {
            Err(UpdateError::new(if source.proxy.is_none() {
                UpdateErrorCode::ReleaseManifestInvalid
            } else {
                UpdateErrorCode::ReleaseFetchFailed
            }))
        })
        .expect_err("no source is usable");

        assert_eq!(error.code(), UpdateErrorCode::ReleaseManifestInvalid);
    }

    /// A manifest that parses but announces no asset for this host stops the
    /// fallback: every source serves the same release asset, so another proxy
    /// cannot change the answer and the diagnostic must survive.
    #[test]
    fn a_manifest_without_this_platform_stops_the_source_fallback() {
        let sources = vec![
            test_source(Some("https://first.invalid"), "a"),
            test_source(None, "official"),
        ];

        let mut consulted = Vec::new();
        let error = UpdateRuntime::select_manifest_source::<&str>(&sources, |source| {
            consulted.push(source.proxy);
            Err(UpdateError::new(UpdateErrorCode::NoMatchingAsset))
        })
        .expect_err("no source can offer this host an asset");

        assert_eq!(error.code(), UpdateErrorCode::NoMatchingAsset);
        assert_eq!(consulted.len(), 1, "later sources must not be consulted");
    }

    /// The download-URL conversion rewrites exactly the official GitHub URLs, once.
    #[test]
    fn download_urls_are_proxied_only_when_official_github() {
        let proxy = Some("https://cdn.gh-proxy.org");
        let github = Url::parse(
            "https://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/BongoCat_x64-setup.exe",
        )
        .expect("the announced GitHub URL parses");
        assert_eq!(
            UpdateRuntime::proxied_download_url(proxy, &github).as_str(),
            "https://cdn.gh-proxy.org/https://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/BongoCat_x64-setup.exe"
        );

        // The official run keeps the announced URL untouched.
        assert_eq!(UpdateRuntime::proxied_download_url(None, &github), github);

        // An already-proxied URL is not prefixed again: its host is the proxy, not
        // github.com, so the conversion is idempotent.
        let already_proxied = UpdateRuntime::proxied_download_url(proxy, &github);
        assert_eq!(
            UpdateRuntime::proxied_download_url(proxy, &already_proxied),
            already_proxied
        );

        // Any other host is left alone.
        let elsewhere = Url::parse("https://example.invalid/BongoCat_x64-setup.exe")
            .expect("the foreign URL parses");
        assert_eq!(
            UpdateRuntime::proxied_download_url(proxy, &elsewhere),
            elsewhere
        );

        // And so is a GitHub URL that is not HTTPS.
        let insecure =
            Url::parse("http://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/a")
                .expect("the insecure URL parses");
        assert_eq!(
            UpdateRuntime::proxied_download_url(proxy, &insecure),
            insecure
        );
    }

    /// The per-source manifest bound has to be real (a dead source is retired, not
    /// waited out) and has to stay below the payload transfer bound.
    #[test]
    fn a_manifest_request_is_bounded_below_a_transfer() {
        assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT >= std::time::Duration::from_secs(10));
        assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT < UPDATE_REQUEST_TIMEOUT);
        assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT <= std::time::Duration::from_secs(300));
    }

    /// A failed update has to say which step it failed in, because the UI reports
    /// "could not download" and "could not install" as different problems.
    #[test]
    fn failures_carry_the_stage_they_happened_in() {
        assert_eq!(
            UpdateError::new(UpdateErrorCode::EnvironmentDisabled).stage(),
            UpdateStage::Check
        );
        assert_eq!(
            UpdateError::at(UpdateStage::Verify, UpdateErrorCode::SignatureInvalid).stage(),
            UpdateStage::Verify
        );
        assert_eq!(
            UpdateError::at(UpdateStage::Install, UpdateErrorCode::InstallFailed).stage(),
            UpdateStage::Install
        );
        assert_eq!(UpdateStage::Download.as_str(), "download");
    }

    /// The library reads and verifies the payload in one call, so the error code is
    /// what separates a transfer failure from an authentication failure.
    #[test]
    fn download_failures_are_split_by_their_code() {
        assert_eq!(
            UpdateError::download_stage(UpdateErrorCode::DownloadTransportFailed),
            UpdateStage::Download
        );
        assert_eq!(
            UpdateError::download_stage(UpdateErrorCode::SignatureInvalid),
            UpdateStage::Verify
        );
        assert_eq!(
            UpdateError::download_stage(UpdateErrorCode::Internal),
            UpdateStage::Download,
            "an unrecognized failure must not claim the payload was authenticated"
        );
    }

    /// A manifest that never arrived and one that arrived unreadable are different
    /// problems, and the diagnostics code has to say which happened.
    #[test]
    fn a_missing_manifest_is_not_an_unreadable_one() {
        assert_eq!(
            UpdateError::from_library(cargo_packager_updater::Error::ReleaseNotFound).code(),
            UpdateErrorCode::ReleaseFetchFailed
        );
        let unreadable = cargo_packager_updater::Error::Serialization(
            serde_json::from_str::<serde_json::Value>("not json").expect_err("invalid JSON"),
        );
        assert_eq!(
            UpdateError::from_library(unreadable).code(),
            UpdateErrorCode::ReleaseManifestInvalid
        );
        // This build's own version is parsed before any request, so a semver failure
        // out of the library can only be the manifest's `version` field.
        let bad_version = cargo_packager_updater::Error::Semver(
            cargo_packager_updater::semver::Version::parse("not a version")
                .expect_err("invalid version"),
        );
        assert_eq!(
            UpdateError::from_library(bad_version).code(),
            UpdateErrorCode::ReleaseManifestInvalid
        );
    }

    #[test]
    fn download_progress_reports_a_fraction_only_when_the_size_is_known() {
        assert_eq!(
            UpdateProgress {
                downloaded_bytes: 512,
                total_bytes: Some(1024),
            }
            .fraction(),
            Some(0.5)
        );
        assert_eq!(
            UpdateProgress {
                downloaded_bytes: 512,
                total_bytes: None,
            }
            .fraction(),
            None
        );
        assert_eq!(
            UpdateProgress {
                downloaded_bytes: 2048,
                total_bytes: Some(1024),
            }
            .fraction(),
            Some(1.0),
            "a server that under-reports the length must not exceed a full bar"
        );
    }

    #[test]
    fn the_release_page_url_follows_the_release_identity() {
        let runtime = runtime_for(ReleaseChannel::Production);
        assert_eq!(
            runtime.release_page_url("1.2.3").as_deref(),
            Some("https://github.com/ayangweb/BongoCat/releases/tag/v1.2.3")
        );
        assert_eq!(
            runtime.release_page_url("v1.2.3").as_deref(),
            Some("https://github.com/ayangweb/BongoCat/releases/tag/v1.2.3")
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

    /// The reason an entry point is missing has to be reportable, not just "no".
    #[test]
    fn unavailability_names_the_gate_that_closed() {
        assert_eq!(
            development_runtime().unavailability(),
            Some(UpdateUnavailability::DevelopmentChannel)
        );
        assert_eq!(
            runtime_for(ReleaseChannel::Production).unavailability(),
            None
        );
    }

    /// The release identity constants describe what the packaging pipeline ships.
    #[test]
    fn the_release_identity_matches_the_packaging_conventions() {
        assert_eq!(RELEASE_BINARY_NAME, "bongocat-app");
        assert_eq!(RELEASE_BUNDLE_NAME, "BongoCat.app");
    }
}
