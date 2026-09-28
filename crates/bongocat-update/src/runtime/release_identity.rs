//! What a release is called, and where it may come from.
//!
//! These are release identity rather than transport inputs: the names here are
//! what `crates/bongocat-packaging` writes and what this crate asks for, and the
//! agreement between the two is pinned by `tools/tests/test_update_release_contract.py`
//! rather than by either side reading the other. The proxy prefixes are here
//! because a manifest is requested through them before the official endpoint, and
//! a build behind a corporate proxy has to work without being configured twice.

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
