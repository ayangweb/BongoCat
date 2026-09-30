//! The published list of plugins.
//!
//! One JSON document, fetched from a GitHub release exactly the way the update
//! manifest is: through the same proxy prefixes, with the same fall back to the
//! official endpoint, and under the same signature rule. That reuse is the point.
//! BongoCat already has a trust model for "a document on GitHub that names a
//! file and a digest", and a second one for plugins would be a second thing to
//! get right.
//!
//! What is in an entry is deliberately not what is in a manifest. The catalog
//! answers three questions the plugin center has to answer before a user can act:
//! what is this plugin called, who made it, and where is the archive for *this*
//! platform. Everything else — the panel, the behaviors, the features needed — is
//! inside the archive, so a catalog can be generated from a plugin's own metadata
//! without a second file to keep in step.

use super::identity::{PluginId, PluginVersion};
use super::{PluginError, PluginErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The shape of a `plugin.json`. One accepted version, no migration.
pub const PLUGIN_SCHEMA_VERSION: u32 = 1;

/// The shape of the catalog. One accepted version, no migration.
pub const PLUGIN_CATALOG_SCHEMA_VERSION: u32 = 1;

/// The feature level this build implements.
///
/// A plugin declaring a higher `api_version` is not loaded. It is not loaded
/// partially either: a plugin whose panel uses a feature the host does not have
/// would draw a hole where that feature was, which is worse than not appearing.
pub const SUPPORTED_PLUGIN_API_VERSION: u32 = 1;

/// The release asset the catalog is published as.
///
/// The same repository as the application releases and the same
/// `<owner>/<repo>/releases/latest/download/<file>` shape the updater requests,
/// so one proxy list and one fallback policy serve both.
pub const PLUGIN_CATALOG_FILE_NAME: &str = "plugins.json";

/// The repository the catalog is published from.
pub const PLUGIN_CATALOG_REPOSITORY_OWNER: &str = "ayangweb";

/// The repository the catalog is published from.
pub const PLUGIN_CATALOG_REPOSITORY_NAME: &str = "BongoCat";

/// The most entries a catalog may announce.
///
/// A bound rather than a guess: a catalog is parsed into memory and rendered as a
/// list, and an unbounded document from a reachable endpoint is a way to make the
/// plugin center allocate without limit. A few hundred plugins is far past what
/// this product will ever list.
pub const MAXIMUM_CATALOG_ENTRIES: usize = 512;

/// One downloadable artifact for one platform.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDownload {
    /// The archive URL. Must be HTTPS, and must be on a host this product is
    /// willing to fetch from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// An archive on the machine running a Development build, relative to the
    /// catalog's own directory.
    ///
    /// The reason the author–reload–see loop needs no publish step and no signature.
    /// It is refused in a catalog that came from the network — see
    /// [`PluginCatalog::validate`] — so a published catalog can never point a user's
    /// product at a file on their disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Lowercase hex SHA-256 of the archive, as published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The archive's size, so a truncated transfer is caught before it is unzipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// A detached Minisign signature over the archive, in the same `.minisig`
    /// text form the release manifest uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl PluginDownload {
    /// Whether this entry names an archive on this machine rather than one to
    /// fetch.
    pub const fn is_local(&self) -> bool {
        self.path.is_some()
    }

    /// The HTTPS URL, or why there is none.
    ///
    /// Split from [`Self::is_local`] so a caller decides what to do about a local
    /// entry rather than discovering it by fetching and failing.
    pub fn url(&self) -> Result<&str, PluginError> {
        match (&self.url, &self.path) {
            (Some(url), _) => Ok(url.as_str()),
            (None, Some(_)) => Err(PluginError::new(PluginErrorCode::PluginNotPublished)),
            (None, None) => Err(PluginError::new(PluginErrorCode::CatalogInvalid)),
        }
    }

    /// Whether this entry names exactly one archive, in one of the two forms.
    fn validate(&self) -> Result<(), PluginError> {
        match (&self.url, &self.path) {
            // Both: a local entry that also names a URL would install whichever the
            // reader preferred, and the answer would depend on the build.
            (Some(_), Some(_)) => Err(PluginError::new(PluginErrorCode::CatalogInvalid)),
            (Some(url), None) => {
                super::validate_release_url(url)?;
                self.validate_fingerprint(true)
            }
            (None, Some(path)) => {
                // Relative, and no way upwards: a development path is still a path,
                // and a catalog that could name `../../..` would be a catalog that
                // could read anywhere the user can.
                if path.is_empty()
                    || Path::new(path).is_absolute()
                    || path.split(['/', '\\']).any(|part| part == "..")
                {
                    return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
                }
                // A digest is optional here — an author iterating on a plugin has
                // not built a release artifact yet — but a signature is not asked
                // for and could not be checked, so neither is invented.
                self.validate_fingerprint(false)
            }
            (None, None) => Err(PluginError::new(PluginErrorCode::CatalogInvalid)),
        }
    }

    /// The digest, and the signature an entry a network catalog may carry needs.
    fn validate_fingerprint(&self, require_signature: bool) -> Result<(), PluginError> {
        if let Some(sha256) = &self.sha256
            && (sha256.len() != 64
                || !sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
        {
            return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
        }
        if require_signature
            && self
                .signature
                .as_ref()
                .is_none_or(|signature| signature.trim().is_empty())
        {
            return Err(PluginError::new(PluginErrorCode::SignatureInvalid));
        }
        Ok(())
    }
}

/// One plugin, as the center lists it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCatalogEntry {
    pub id: PluginId,
    pub name: String,
    pub version: PluginVersion,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// The feature level the archive's manifest needs. Checked against the
    /// installed manifest as well as the catalog, so an entry cannot offer a
    /// plugin this build cannot run.
    pub api_version: u32,
    /// The oldest BongoCat that can run the archive's manifest.
    #[serde(default)]
    pub min_app_version: Option<PluginVersion>,
    /// One download per `<os>-<arch>` key, spelled the way the update manifest
    /// spells its platform keys so one target triple names both.
    pub downloads: BTreeMap<String, PluginDownload>,
    /// The icon shown in the center, over HTTPS, on the same host rule as an
    /// archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

impl PluginCatalogEntry {
    /// The download for one platform key, or why there is none.
    pub fn download_for(&self, platform: &str) -> Result<&PluginDownload, PluginError> {
        self.downloads.get(platform).ok_or(PluginError::with_detail(
            PluginErrorCode::PluginNotPublished,
            format!("no download for platform {platform}"),
        ))
    }
}

/// The published list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCatalog {
    pub schema_version: u32,
    pub plugins: Vec<PluginCatalogEntry>,
}

impl PluginCatalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, PluginError> {
        let catalog: Self = serde_json::from_slice(bytes)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::CatalogInvalid, error))?;
        catalog.validate()?;
        Ok(catalog)
    }

    fn validate(&self) -> Result<(), PluginError> {
        if self.schema_version != PLUGIN_CATALOG_SCHEMA_VERSION {
            return Err(PluginError::new(PluginErrorCode::UnsupportedSchemaVersion));
        }
        if self.plugins.len() > MAXIMUM_CATALOG_ENTRIES {
            return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
        }
        for entry in &self.plugins {
            entry.validate()?;
        }
        // Two entries for one id would make "install" ambiguous, and a user
        // cannot tell which of the two the button they pressed would have got.
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.plugins {
            if !seen.insert(entry.id.clone()) {
                return Err(PluginError::new(PluginErrorCode::CatalogInvalid));
            }
        }
        Ok(())
    }

    /// The entry for one plugin, or why there is none.
    pub fn entry(&self, id: &PluginId) -> Result<&PluginCatalogEntry, PluginError> {
        self.plugins
            .iter()
            .find(|entry| &entry.id == id)
            .ok_or_else(|| {
                PluginError::with_detail(
                    PluginErrorCode::PluginNotPublished,
                    format!("{id} is not in the catalog"),
                )
            })
    }
}

impl PluginCatalogEntry {
    fn validate(&self) -> Result<(), PluginError> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > super::descriptor::MAXIMUM_PLUGIN_NAME_CHARS
        {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginName));
        }
        if self.description.chars().count() > super::descriptor::MAXIMUM_PLUGIN_DESCRIPTION_CHARS {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginDescription));
        }
        if self.api_version == 0 || self.api_version > SUPPORTED_PLUGIN_API_VERSION {
            return Err(PluginError::new(PluginErrorCode::UnsupportedApiVersion));
        }
        for download in self.downloads.values() {
            download.validate()?;
        }
        if let Some(icon) = &self.icon_url {
            super::validate_release_url(icon)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download() -> PluginDownload {
        PluginDownload {
            url: Some("https://github.com/ayangweb/BongoCat/releases/download/p/v.zip".to_string()),
            path: None,
            sha256: Some("a".repeat(64)),
            size_bytes: Some(1024),
            signature: Some(
                "untrusted comment: test\nRUQZ\nAAAA\ntrusted comment: test\nAAAA\nAAAA\n"
                    .to_string(),
            ),
        }
    }

    fn catalog_json(extra: &str) -> String {
        format!(
            r#"{{"schema_version":1,"plugins":[{{
                "id":"pomodoro","name":"Pomodoro","version":"1.2.0","api_version":1,
                "description":"A focus timer.",
                "downloads":{{"macos-aarch64":{}}}{}
            }}]}}"#,
            serde_json::to_string(&download()).unwrap(),
            extra
        )
    }

    #[test]
    fn a_catalog_reads_and_finds_its_entry() {
        let catalog = PluginCatalog::parse(catalog_json("").as_bytes()).unwrap();
        let id = PluginId::new("pomodoro").unwrap();
        assert_eq!(
            catalog.entry(&id).unwrap().version,
            PluginVersion::new(1, 2, 0)
        );
        assert!(catalog.plugins[0].downloads.contains_key("macos-aarch64"));
    }

    #[test]
    fn a_missing_platform_is_reported_rather_than_defaulted() {
        let catalog = PluginCatalog::parse(catalog_json("").as_bytes()).unwrap();
        let error = catalog.plugins[0]
            .download_for("windows-x86_64")
            .unwrap_err();
        assert_eq!(error.code(), PluginErrorCode::PluginNotPublished);
    }

    #[test]
    fn a_plain_http_url_is_refused() {
        let mut download = download();
        download.url = Some("http://example.invalid/v.zip".to_string());
        let json = format!(
            r#"{{"schema_version":1,"plugins":[{{"id":"a-b","name":"A","version":"1.0.0",
                "api_version":1,"downloads":{{"macos-aarch64":{}}}}}]}}"#,
            serde_json::to_string(&download).unwrap()
        );
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::CatalogInvalid
        );
    }

    #[test]
    fn a_digest_that_is_not_a_sha256_is_refused() {
        let mut download = download();
        download.sha256 = Some("abc".to_string());
        let json = format!(
            r#"{{"schema_version":1,"plugins":[{{"id":"a-b","name":"A","version":"1.0.0",
                "api_version":1,"downloads":{{"macos-aarch64":{}}}}}]}}"#,
            serde_json::to_string(&download).unwrap()
        );
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::CatalogInvalid
        );
    }

    #[test]
    fn an_empty_signature_is_refused_before_anything_is_fetched() {
        let mut download = download();
        download.signature = Some("   ".to_string());
        let json = format!(
            r#"{{"schema_version":1,"plugins":[{{"id":"a-b","name":"A","version":"1.0.0",
                "api_version":1,"downloads":{{"macos-aarch64":{}}}}}]}}"#,
            serde_json::to_string(&download).unwrap()
        );
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::SignatureInvalid
        );
    }

    #[test]
    fn a_catalog_needing_a_newer_api_version_is_refused() {
        let json = catalog_json("").replace("\"api_version\":1", "\"api_version\":7");
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::UnsupportedApiVersion
        );
    }

    #[test]
    fn two_entries_for_one_id_are_refused() {
        let entry = r#"{"id":"a-b","name":"A","version":"1.0.0","api_version":1,"downloads":{}}"#;
        let json = format!(r#"{{"schema_version":1,"plugins":[{entry},{entry}]}}"#);
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::CatalogInvalid
        );
    }

    #[test]
    fn a_catalog_with_another_schema_version_is_refused() {
        let json = catalog_json("").replace("\"schema_version\":1", "\"schema_version\":2");
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::UnsupportedSchemaVersion
        );
    }

    /// A development entry: an archive on this machine, with nothing signed.
    fn local_download(path: &str) -> PluginDownload {
        PluginDownload {
            url: None,
            path: Some(path.to_string()),
            sha256: None,
            size_bytes: None,
            signature: None,
        }
    }

    fn local_catalog_json(download: &PluginDownload) -> String {
        format!(
            r#"{{"schema_version":1,"plugins":[{{"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","api_version":1,
                "downloads":{{"macos-aarch64":{}}}}}]}}"#,
            serde_json::to_string(download).unwrap()
        )
    }

    #[test]
    fn a_development_catalog_may_name_an_archive_on_this_machine() {
        let catalog = PluginCatalog::parse(
            local_catalog_json(&local_download("build/pomodoro.zip")).as_bytes(),
        )
        .expect("a local development entry is the whole point of the local catalog");
        let download = catalog.plugins[0]
            .download_for("macos-aarch64")
            .expect("the entry names this platform");
        assert!(download.is_local());
        assert_eq!(download.path.as_deref(), Some("build/pomodoro.zip"));
    }

    #[test]
    fn a_development_path_cannot_leave_the_catalog_directory() {
        for path in [
            "../outside.zip",
            "build/../../outside.zip",
            "/etc/passwd",
            "",
        ] {
            let json = local_catalog_json(&local_download(path));
            assert_eq!(
                PluginCatalog::parse(json.as_bytes())
                    .map(|_| ())
                    .unwrap_err()
                    .code(),
                PluginErrorCode::CatalogInvalid,
                "{path} must not be readable by a catalog"
            );
        }
    }

    #[test]
    fn an_entry_that_names_both_a_url_and_a_path_is_refused() {
        let mut both = download();
        both.path = Some("build/pomodoro.zip".to_string());
        let json = local_catalog_json(&both);
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::CatalogInvalid,
            "an entry that could be read either way is read whichever the build prefers"
        );
    }

    #[test]
    fn an_entry_that_names_neither_a_url_nor_a_path_is_refused() {
        let neither = PluginDownload {
            url: None,
            path: None,
            sha256: Some("a".repeat(64)),
            size_bytes: Some(1),
            signature: Some("x".to_string()),
        };
        let json = local_catalog_json(&neither);
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::CatalogInvalid
        );
    }

    #[test]
    fn a_published_download_still_needs_its_signature() {
        let mut unsigned = download();
        unsigned.signature = None;
        let json = local_catalog_json(&unsigned);
        assert_eq!(
            PluginCatalog::parse(json.as_bytes()).unwrap_err().code(),
            PluginErrorCode::SignatureInvalid,
            "a URL is a fetch from the internet, and needs the same proof as the updater"
        );
    }

    #[test]
    fn a_local_entry_reports_no_url_rather_than_an_empty_one() {
        let download = local_download("build/pomodoro.zip");
        assert_eq!(
            download.url().unwrap_err().code(),
            PluginErrorCode::PluginNotPublished,
            "a caller must not be handed an empty string and try to fetch it"
        );
    }
}
