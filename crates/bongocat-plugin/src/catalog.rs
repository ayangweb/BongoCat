//! Where the plugin list comes from, and how an archive is fetched.
//!
//! Two sources, and the choice is compile-time rather than a setting:
//!
//! * **Development** reads a local directory of plugin sources and *derives* the list
//!   from them: every name, description, icon and version on a card comes from the
//!   plugin's own `plugin.json`, and the archive is the one the packer wrote beside it.
//!   A plugin author drops a directory in, presses refresh, and the panel is on the model
//!   window — with no publish step, no signature and no network, and with no list anywhere
//!   to add a line to. The whole author loop therefore works offline, which is the
//!   condition for writing a plugin at all.
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
    PLUGIN_MANIFEST_FILE_NAME, PluginCatalog, PluginCatalogEntry, PluginDownload, PluginError,
    PluginErrorCode, PluginManifest,
};
use bongocat_update::GITHUB_PROXY_PREFIXES;
use std::collections::BTreeMap;
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

/// The directory a packed archive is written to, relative to a catalog directory.
///
/// The name the packer uses, restated rather than imported because the packer is the
/// *product's* build entry point and this is the host: a published catalog names an
/// archive by URL and a development catalog names it by path, and the two have to agree
/// on one directory name or a freshly packed plugin is not found.
pub const LOCAL_BUILD_DIRECTORY: &str = "build";

/// The plugin directories a catalog directory holds, in id order.
///
/// A plugin is a directory with a `plugin.json` in it, and nothing else in the tree is
/// one — so the manifest is the whole of the discovery rule, and this is the same rule
/// the packer builds by. An author who adds a plugin adds a directory, and there is no
/// list anywhere for them to add a line to.
pub fn local_plugin_directories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    !name.starts_with('.') && name != LOCAL_BUILD_DIRECTORY && name != "target"
                })
        })
        .filter(|path| path.join(PLUGIN_MANIFEST_FILE_NAME).is_file())
        .collect();
    found.sort();
    found
}

/// Read a catalog from a local directory of plugin sources.
///
/// The list is *derived* rather than read from a file, which is the whole point: every
/// name, description, icon and version on a card comes from the plugin's own
/// `plugin.json`, so a plugin cannot be added, renamed or removed without editing a
/// second document somewhere else, and a card cannot show one sentence before the plugin
/// is installed and another after.
///
/// An entry is offered only when the archive the packer writes is there, so the plugin
/// center lists what can actually be installed. A directory that is not a plugin — a
/// scratch folder, Cargo's own output — is skipped rather than refused, because a
/// development directory is the author's own tree and refusing the whole catalog over
/// one folder they left there would make the failure about housekeeping rather than
/// about a plugin.
pub fn load_local(directory: &Path) -> Result<LoadedCatalog, PluginError> {
    let mut entries = Vec::new();
    for source in local_plugin_directories(directory) {
        let Some(id) = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let archive = format!("{LOCAL_BUILD_DIRECTORY}/{id}.zip");
        if !directory.join(&archive).is_file() {
            continue;
        }
        let manifest = read_local_manifest(&source)?;
        entries.push(entry_from_manifest(&manifest, &archive)?);
    }
    // One document out of many, checked as a whole rather than entry by entry: the bounds
    // that matter — how many, and no id twice — are properties of the list, and this is
    // the same validation a published catalog gets for free.
    let catalog = PluginCatalog {
        schema_version: bongocat_plugin_protocol::PLUGIN_CATALOG_SCHEMA_VERSION,
        plugins: entries,
    };
    catalog.validate()?;
    Ok(LoadedCatalog {
        catalog,
        source: CatalogSource::Directory,
    })
}

/// Read one plugin's own manifest.
///
/// The protocol's own parser, so a development plugin is held to exactly the rules a
/// published one is: the same field types, the same path checks on the icon and the
/// executable, the same refusal for a `schema_version` this host does not speak.
fn read_local_manifest(directory: &Path) -> Result<PluginManifest, PluginError> {
    let path = directory.join(PLUGIN_MANIFEST_FILE_NAME);
    let bytes = std::fs::read(&path).map_err(|error| {
        PluginError::with_detail(
            PluginErrorCode::CatalogInvalid,
            format!("{}: {error}", path.display()),
        )
    })?;
    if bytes.len() > MAXIMUM_CATALOG_BYTES {
        return Err(PluginError::with_detail(
            PluginErrorCode::CatalogInvalid,
            "a plugin's own manifest is larger than the bound",
        ));
    }
    PluginManifest::parse(&bytes).map_err(|error| {
        // The file named in the message, because a development plugin's manifest is one of
        // several in a tree and "key must be a string at line 1 column 3" is a message an
        // author cannot act on without knowing which of their plugins it came from.
        PluginError::with_detail(error.code(), format!("{}: {error}", path.display()))
    })
}

/// The catalog entry one plugin's own manifest describes.
///
/// A projection rather than a copy: the entry carries the manifest's name, description,
/// icon, version and author, so the card a user reads before installing and the card they
/// read afterwards are the same words from the same document. The only thing this step
/// adds is where the archive is, which is a fact about this machine rather than about the
/// plugin.
fn entry_from_manifest(
    manifest: &PluginManifest,
    archive: &str,
) -> Result<PluginCatalogEntry, PluginError> {
    let mut downloads = BTreeMap::new();
    downloads.insert(
        host_platform().to_owned(),
        PluginDownload {
            url: None,
            path: Some(archive.to_owned()),
            sha256: None,
            size_bytes: None,
            signature: None,
        },
    );
    Ok(PluginCatalogEntry {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version,
        author: manifest.author.clone(),
        description: manifest.description.clone(),
        api_version: manifest.api_version,
        min_app_version: manifest.min_app_version,
        downloads,
        icon: manifest.display_icon(),
        icon_url: None,
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
///
/// `timeout` is applied **per call** rather than being read off `agent`, because
/// one agent serves both kinds of request and the two bounds differ by a factor of
/// sixty. A bound that lives only on the agent is the *archive* bound, so a hung
/// mirror would hold the worker thread for the transfer timeout instead of the
/// catalog one — once per source, across every source in the retry chain — and the
/// worker is the thread that answers panel presses and takes the stop request.
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
    let response = agent
        .get(url)
        .timeout(timeout)
        .call()
        .map_err(|error| PluginError::with_detail(PluginErrorCode::DownloadFailed, error))?;
    let reader = response.into_reader();
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
/// One agent, one connection pool, so the retry chain does not re-establish TLS for
/// every source it tries. The agent's own global timeout is a ceiling that no
/// caller can raise; the *effective* bound is the smaller of it and the per-call
/// timeout [`download_with`] applies, which is how the catalog's 30 s bound and the
/// archive's transfer bound both hold from one agent.
pub fn agent() -> Result<ureq::Agent, PluginError> {
    Ok(ureq::builder()
        .timeout(crate::ARCHIVE_REQUEST_TIMEOUT)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// One plugin directory, with a manifest this host accepts.
    fn written_plugin(catalog: &Path, id: &str) -> PathBuf {
        let directory = catalog.join(id);
        fs::create_dir_all(&directory).expect("a plugin directory");
        fs::write(
            directory.join(PLUGIN_MANIFEST_FILE_NAME),
            format!(
                r#"{{"schema_version":1,"api_version":1,"id":"{id}","name":"{id}",
                    "version":"1.0.0","author":"BongoCat","executable":"{id}",
                    "description":"A focus timer.",
                    "icon":{{"emoji":"🧩"}}}}"#
            ),
        )
        .expect("a manifest is written");
        directory
    }

    /// The packed archive beside a plugin, which is what makes it installable.
    fn packed(catalog: &Path, id: &str) {
        fs::create_dir_all(catalog.join(LOCAL_BUILD_DIRECTORY)).expect("a build directory");
        fs::write(
            catalog
                .join(LOCAL_BUILD_DIRECTORY)
                .join(format!("{id}.zip")),
            b"pk",
        )
        .expect("an archive is written");
    }

    fn a_catalog_with_one_plugin() -> tempfile::TempDir {
        let catalog = tempfile::tempdir().expect("a temporary directory");
        written_plugin(catalog.path(), "pomodoro");
        packed(catalog.path(), "pomodoro");
        catalog
    }

    #[test]
    fn a_plugin_directory_is_a_catalog_entry_with_no_list_anywhere() {
        // The whole of "adding a plugin is adding a directory": every word on the card
        // comes out of the plugin's own manifest, and there is no second document to add a
        // line to. A plugin that is added, renamed or removed is changed in one place.
        let catalog = a_catalog_with_one_plugin();
        let loaded = load_local(catalog.path()).expect("reads");
        assert_eq!(loaded.source, CatalogSource::Directory);
        assert_eq!(loaded.catalog.plugins.len(), 1);
        let entry = &loaded.catalog.plugins[0];
        assert_eq!(entry.id.as_str(), "pomodoro");
        assert_eq!(
            entry.version,
            bongocat_plugin_protocol::PluginVersion::new(1, 0, 0)
        );
        assert_eq!(entry.author, "BongoCat");
        assert_eq!(entry.icon.emoji_text().as_deref(), Some("🧩"));
        assert_eq!(entry.name.resolve("en-US"), "pomodoro");
    }

    #[test]
    fn the_entry_points_at_the_archive_the_packer_wrote() {
        let catalog = a_catalog_with_one_plugin();
        let loaded = load_local(catalog.path()).expect("reads");
        let download = loaded
            .entry_for_host(&bongocat_plugin_protocol::PluginId::new("pomodoro").expect("an id"))
            .expect("this host's platform is named")
            .download_for(host_platform())
            .expect("one download");
        assert!(
            download.is_local(),
            "because a development archive is a file on this machine, not a fetch"
        );
        assert_eq!(download.path.as_deref(), Some("build/pomodoro.zip"));
        assert!(
            download.sha256.is_none() && download.signature.is_none(),
            "and an author iterating on a plugin has no release artifact to sign yet — which \\
             is the whole of what 'no publish step' means"
        );
    }

    #[test]
    fn a_plugin_with_no_packed_archive_is_not_offered() {
        // The card says "Install", and the install would fail because the archive is not
        // there. Listing only what can be installed is the honest list, and it is also why
        // `just dev` builds the plugins before it launches the product.
        let catalog = tempfile::tempdir().expect("a temporary directory");
        written_plugin(catalog.path(), "pomodoro");
        let loaded = load_local(catalog.path()).expect("reads");
        assert!(
            loaded.catalog.plugins.is_empty(),
            "so a plugin that has not been packed yet is not on a card inviting an install"
        );
    }

    #[test]
    fn an_empty_directory_is_an_empty_list_rather_than_a_failure() {
        // What a fresh checkout looks like: no plugins, nothing to install, and nothing
        // broken. A developer who has not written a plugin should see an empty page rather
        // than an error they have to work out.
        let catalog = tempfile::tempdir().expect("a temporary directory");
        let loaded = load_local(catalog.path()).expect("reads");
        assert!(loaded.catalog.plugins.is_empty());
        assert_eq!(loaded.source, CatalogSource::Directory);
    }

    #[test]
    fn a_directory_that_is_not_there_is_an_empty_list_rather_than_a_failure() {
        let missing = std::env::temp_dir().join("bongocat-no-such-plugin-catalog");
        let _ = fs::remove_dir_all(&missing);
        assert!(
            load_local(&missing)
                .expect("reads")
                .catalog
                .plugins
                .is_empty()
        );
    }

    #[test]
    fn a_directory_that_is_not_a_plugin_is_skipped_rather_than_refusing_the_catalog() {
        // A development catalog directory is the author's own tree, and it will hold things
        // that are not plugins. Refusing the whole list over one of them would make the
        // failure about housekeeping rather than about a plugin.
        let catalog = a_catalog_with_one_plugin();
        fs::create_dir_all(catalog.path().join("scratch")).expect("an unrelated directory");
        fs::create_dir_all(catalog.path().join(LOCAL_BUILD_DIRECTORY)).expect("a build directory");
        fs::create_dir_all(catalog.path().join("target")).expect("a target directory");
        let loaded = load_local(catalog.path()).expect("reads");
        assert_eq!(
            loaded
                .catalog
                .plugins
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            ["pomodoro"]
        );
    }

    #[test]
    fn a_manifest_this_host_will_not_accept_is_refused_rather_than_skipped() {
        // The other half of the rule above: a directory that *is* a plugin and whose
        // manifest is broken is a plugin the author needs to hear about, and a card that
        // silently omitted it would be a plugin that exists and cannot be installed with
        // no message. Refused, and the message names the file.
        let catalog = a_catalog_with_one_plugin();
        written_plugin(catalog.path(), "broken");
        fs::write(
            catalog
                .path()
                .join("broken")
                .join(PLUGIN_MANIFEST_FILE_NAME),
            "{ not json",
        )
        .expect("a broken manifest is written");
        packed(catalog.path(), "broken");
        let error = load_local(catalog.path()).expect_err("a broken manifest is refused");
        assert!(
            error.to_string().contains("plugin.json"),
            "and the refusal names the file rather than saying 'a parse error': {error}"
        );
    }

    #[test]
    fn a_manifest_naming_a_schema_version_this_host_does_not_speak_is_refused() {
        let catalog = a_catalog_with_one_plugin();
        let path = catalog
            .path()
            .join("pomodoro")
            .join(PLUGIN_MANIFEST_FILE_NAME);
        let document = fs::read_to_string(&path).expect("reads");
        fs::write(
            &path,
            document.replace(r#""schema_version":1"#, r#""schema_version":99"#),
        )
        .expect("writes");
        assert_eq!(
            load_local(catalog.path()).unwrap_err().code(),
            PluginErrorCode::UnsupportedSchemaVersion,
            "because a development plugin is held to the same rules as a published one"
        );
    }

    #[test]
    fn two_plugin_directories_are_two_entries_in_id_order() {
        // Order is id order rather than directory-listing order, so a refresh does not
        // reshuffle the page between two runs that added the same plugin.
        let catalog = tempfile::tempdir().expect("a temporary directory");
        for id in ["typing-sound", "keyboard-display", "pomodoro"] {
            written_plugin(catalog.path(), id);
            packed(catalog.path(), id);
        }
        let loaded = load_local(catalog.path()).expect("reads");
        assert_eq!(
            loaded
                .catalog
                .plugins
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            ["keyboard-display", "pomodoro", "typing-sound"]
        );
    }

    #[test]
    fn the_derived_list_is_held_to_the_same_bounds_as_a_published_one() {
        // Building a catalog in memory and shipping it unchecked would be the same document
        // a published one is refused for, so the list-level checks run here too.
        let catalog = tempfile::tempdir().expect("a temporary directory");
        written_plugin(catalog.path(), "pomodoro");
        packed(catalog.path(), "pomodoro");
        let loaded = load_local(catalog.path()).expect("reads");
        loaded
            .catalog
            .validate()
            .expect("a derived list is a valid catalog");
    }
}
