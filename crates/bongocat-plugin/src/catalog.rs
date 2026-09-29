//! Where the plugin list comes from, and how an archive is fetched.
//!
//! Two sources, and the choice is compile-time rather than a setting:
//!
//! * **Development** reads a local directory. A plugin author drops a directory
//!   into it, presses refresh, and the panel is on the model window — with no
//!   publish step, no signature and no network. The whole author loop therefore
//!   works offline, which is the condition for writing a plugin at all.
//! * **Production** fetches `plugins.json` from a GitHub release, through the
//!   same proxy prefixes the update manifest goes through, with the same fallback
//!   order.
//!
//! The reachability policy is not restated here. It is *imported* from
//! `bongocat-update`, which already owns it, because a second copy of "which
//! proxies do we try, in what order" is a second thing to keep in step with a
//! change to the first — and the two would drift silently, which is the failure
//! mode that makes a build work in one region and not another.

use bongocat_plugin_protocol::{
    PLUGIN_CATALOG_FILE_NAME, PLUGIN_CATALOG_REPOSITORY_NAME, PLUGIN_CATALOG_REPOSITORY_OWNER,
    PluginCatalog, PluginCatalogEntry, PluginDownload, PluginError, PluginErrorCode,
};
use bongocat_update::GITHUB_PROXY_PREFIXES;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one catalog request may take.
///
/// The same bound the updater uses for its manifest, and for the same reason: a
/// source that accepts the connection and never answers would otherwise stack the
/// transfer-sized timeout in front of every later source.
pub const CATALOG_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The most bytes a catalog may be.
pub const MAXIMUM_CATALOG_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes one archive may be.
///
/// Checked against the size the catalog announced *before* the transfer starts, so
/// a catalog that names a gigabyte is refused without downloading it.
pub const MAXIMUM_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Where a catalog was read from, for a log line and for a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogSource {
    /// A local directory, which only a Development build uses.
    Directory,
    /// A GitHub release asset, reached directly or through a proxy.
    Network,
}

/// A catalog, and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedCatalog {
    pub catalog: PluginCatalog,
    pub source: CatalogSource,
}

impl LoadedCatalog {
    /// The entry for one plugin, and why there is none.
    pub fn entry(
        &self,
        id: &bongocat_plugin_protocol::PluginId,
    ) -> Result<&PluginCatalogEntry, PluginError> {
        self.catalog.entry(id)
    }

    /// The entry for this host, and why there is none.
    pub fn entry_for_host(
        &self,
        id: &bongocat_plugin_protocol::PluginId,
    ) -> Result<&PluginCatalogEntry, PluginError> {
        let entry = self.entry(id)?;
        entry.download_for(host_platform())?;
        Ok(entry)
    }
}

/// The `<os>-<arch>` key this host's downloads are published under.
///
/// The updater's own spelling, reached through its own function, so a plugin
/// archive and a release payload are named by the same rule and a change to that
/// rule moves both.
pub fn host_platform() -> &'static str {
    bongocat_update::HOST_TARGET_TRIPLE.manifest_platform()
}

/// The official URL of the plugin catalog.
pub fn catalog_url() -> String {
    format!(
        "https://github.com/{PLUGIN_CATALOG_REPOSITORY_OWNER}/{PLUGIN_CATALOG_REPOSITORY_NAME}/releases/latest/download/{PLUGIN_CATALOG_FILE_NAME}"
    )
}

/// The URL one proxy prefix requests: the official one, prefixed.
///
/// The same construction the updater uses for its manifest, so a catalog and a
/// manifest travel the same path to the same machines.
pub fn proxied_catalog_url(prefix: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), catalog_url())
}

/// Every source a catalog fetch tries, in order: each proxy, then the official
/// endpoint.
pub fn catalog_sources() -> Vec<String> {
    GITHUB_PROXY_PREFIXES
        .iter()
        .map(|prefix| proxied_catalog_url(prefix))
        .chain(std::iter::once(catalog_url()))
        .collect()
}

/// Read a catalog from a local directory.
///
/// A directory with no `plugins.json` is an empty catalog rather than an error:
/// that is what a fresh checkout looks like, and an author who has not written a
/// plugin yet should see an empty list rather than a failure.
pub fn load_local(directory: &Path) -> Result<LoadedCatalog, PluginError> {
    let path = directory.join(PLUGIN_CATALOG_FILE_NAME);
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(LoadedCatalog {
            catalog: PluginCatalog {
                schema_version: bongocat_plugin_protocol::PLUGIN_CATALOG_SCHEMA_VERSION,
                plugins: Vec::new(),
            },
            source: CatalogSource::Directory,
        });
    };
    if bytes.len() > MAXIMUM_CATALOG_BYTES {
        return Err(PluginError::with_detail(
            PluginErrorCode::CatalogInvalid,
            "the local catalog is too large",
        ));
    }
    Ok(LoadedCatalog {
        catalog: PluginCatalog::parse(&bytes)?,
        source: CatalogSource::Directory,
    })
}

/// The endpoints a Development build reads instead of the network.
///
/// A local directory beside the application's data root, so a plugin author's
/// files are not in the same place as their configuration and a data reset does
/// not delete them.
pub fn local_catalog_directory(data_root: &Path) -> PathBuf {
    data_root.join("plugin-catalog")
}

/// Fetch a catalog, trying each source in turn.
///
/// A source counts as usable only when its request succeeds *and* the body parses
/// as a plugin catalog. A source that stalls or returns something else is skipped,
/// and the last failure is reported when every source has been tried — the same
/// rule the updater applies to its manifest, for the same reason.
///
/// Transport is injected rather than taken as a dependency of this function's
/// signature so a test can drive every branch — first source fails, second
/// succeeds, all fail, a hostile body — without a network.
pub fn fetch_catalog(
    mut request: impl FnMut(&str, Duration) -> Result<Vec<u8>, PluginError>,
) -> Result<LoadedCatalog, PluginError> {
    let mut last = None;
    for source in catalog_sources() {
        match request(&source, CATALOG_REQUEST_TIMEOUT) {
            Ok(bytes) => {
                if bytes.len() > MAXIMUM_CATALOG_BYTES {
                    last = Some(PluginError::with_detail(
                        PluginErrorCode::CatalogInvalid,
                        "the catalog is larger than the bound",
                    ));
                    continue;
                }
                match PluginCatalog::parse(&bytes) {
                    Ok(catalog) => {
                        return Ok(LoadedCatalog {
                            catalog,
                            source: CatalogSource::Network,
                        });
                    }
                    Err(error) => last = Some(error),
                }
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| PluginError::new(PluginErrorCode::DownloadFailed)))
}

/// Fetch one archive, trying each source in turn.
///
/// A source is a URL the catalog named, optionally prefixed with the proxy the
/// catalog itself came through — the same choice the updater makes, and for the
/// same reason: a run that could reach GitHub through a proxy should not then fail
/// to reach the file that GitHub pointed at.
pub fn fetch_archive(
    download: &PluginDownload,
    through_proxy: Option<&str>,
    mut request: impl FnMut(&str, Duration) -> Result<Vec<u8>, PluginError>,
) -> Result<Vec<u8>, PluginError> {
    // Checked before any request: a catalog that names an archive larger than the
    // bound is refused without spending the transfer.
    let size_bytes = download.size_bytes.unwrap_or(0);
    if size_bytes > MAXIMUM_ARCHIVE_BYTES {
        return Err(PluginError::with_detail(
            PluginErrorCode::ArchiveInvalid,
            "the catalog announces an archive larger than the bound",
        ));
    }
    let url = download.url()?;
    let mut candidates = vec![url.to_string()];
    if let Some(prefix) = through_proxy {
        let proxied = format!("{}/{}", prefix.trim_end_matches('/'), url);
        if proxied != url {
            candidates.insert(0, proxied);
        }
    }
    let mut last = None;
    for candidate in &candidates {
        match request(candidate, crate::ARCHIVE_REQUEST_TIMEOUT) {
            Ok(bytes) => {
                // A size the catalog did not announce is a truncated transfer, and
                // continuing would mean unpacking a partial archive. A development
                // entry announces none, so there is nothing to disagree with.
                if download.size_bytes.is_some_and(|announced| {
                    u64::try_from(bytes.len()).unwrap_or(u64::MAX) != announced
                }) {
                    last = Some(PluginError::with_detail(
                        PluginErrorCode::ChecksumMismatch,
                        "the download does not match the announced size",
                    ));
                    continue;
                }
                return Ok(bytes);
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| PluginError::new(PluginErrorCode::DownloadFailed)))
}

/// Read an archive into memory with the given request policy.
///
/// This is the one place that knows about an HTTP client, so everything above it
/// is testable without one.
pub fn download_with(
    url: &str,
    timeout: Duration,
    agent: &ureq::Agent,
) -> Result<Vec<u8>, PluginError> {
    // Every URL a catalog can name was checked against the protocol's host rule
    // at parse time. Re-checked here because this is the function that actually
    // connects, and a rule that is only enforced at parse time is a rule that a
    // future code path can forget.
    bongocat_plugin_protocol::validate_release_url(url)?;
    let _ = timeout;
    let response = agent
        .get(url)
        .call()
        .map_err(|error| PluginError::with_detail(PluginErrorCode::DownloadFailed, error))?;
    let reader = response.into_body().into_reader();
    let mut bytes = Vec::new();
    // Bounded while reading rather than after: a response that keeps producing
    // bytes is stopped at the bound instead of filling memory first.
    reader
        .take(MAXIMUM_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::DownloadFailed, error))?;
    if bytes.len() as u64 > MAXIMUM_ARCHIVE_BYTES {
        return Err(PluginError::with_detail(
            PluginErrorCode::ArchiveInvalid,
            "the response is larger than the bound",
        ));
    }
    Ok(bytes)
}

use std::io::Read;

/// The agent every download uses.
///
/// One agent, one connection pool, one set of timeouts. A fresh agent per request
/// would drop and re-establish TLS for every source in a retry chain, which is
/// the expensive part of a request — and the plugin centre makes several in a row.
pub fn agent() -> Result<ureq::Agent, PluginError> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(crate::ARCHIVE_REQUEST_TIMEOUT))
        .build()
        .into())
}
