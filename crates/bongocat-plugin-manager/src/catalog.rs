//! The production catalog: a signed, protocol- and app-version-gated list.
//!
//! Transport, HTTPS, proxy order and Minisign verification are the libraries the
//! updater already uses; this module owns only the catalog shape and policy.
use bongocat_plugin_sdk::{PROTOCOL_VERSION, PluginIcon};
use bongocat_update::GITHUB_PROXY_PREFIXES;
use cargo_packager_updater::{reqwest, semver};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use thiserror::Error;

pub const CATALOG_NAME: &str = "plugins.json";
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;
pub const MAX_DOWNLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("catalog transport failed")]
    Transport,
    #[error("catalog is not signed correctly")]
    Signature,
    #[error("catalog is invalid")]
    Invalid,
    #[error("no plugin release matches this build")]
    NoMatch,
    #[error("plugin download failed")]
    Download,
}

/// One advertised plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub app_versions: String,
    pub protocol_version: u32,
    pub icon: PluginIcon,
    /// Per-platform download paths relative to the catalog URL.
    #[serde(default)]
    pub downloads: BTreeMap<String, String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub plugins: Vec<CatalogEntry>,
}

/// What the release publishes: the catalog plus a Minisign signature over the
/// catalog's own canonical JSON bytes.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedCatalog {
    catalog: Catalog,
    signature: String,
}

pub fn parse_catalog(bytes: &[u8], public_key: &str) -> Result<Catalog, CatalogError> {
    let signed: SignedCatalog = serde_json::from_slice(bytes).map_err(|_| CatalogError::Invalid)?;
    let key =
        minisign_verify::PublicKey::from_base64(public_key).map_err(|_| CatalogError::Signature)?;
    let signature = minisign_verify::Signature::decode(&signed.signature)
        .map_err(|_| CatalogError::Signature)?;
    let canonical = serde_json::to_vec(&signed.catalog).map_err(|_| CatalogError::Invalid)?;
    key.verify(&canonical, &signature, false)
        .map_err(|_| CatalogError::Signature)?;
    Ok(signed.catalog)
}

/// The newest matching release for one platform. Entries are filtered by protocol
/// major, the app range and a matching `<os>-<arch>` download.
pub fn select_for_host(
    catalog: &Catalog,
    app_version: &str,
    platform: &str,
) -> Result<CatalogEntry, CatalogError> {
    let version = semver::Version::parse(app_version.trim_start_matches('v'))
        .map_err(|_| CatalogError::Invalid)?;
    let mut matches = catalog
        .plugins
        .iter()
        .filter(|entry| {
            entry.protocol_version == PROTOCOL_VERSION
                && entry.downloads.contains_key(platform)
                && semver::VersionReq::parse(&entry.app_versions)
                    .is_ok_and(|requirement| requirement.matches(&version))
        })
        .cloned()
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        semver::Version::parse(left.version.trim_start_matches('v'))
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0))
            .cmp(
                &semver::Version::parse(right.version.trim_start_matches('v'))
                    .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
            )
    });
    matches.pop().ok_or(CatalogError::NoMatch)
}

/// Fetch, verify and select in one step, through the update proxies in order.
pub async fn fetch_and_select(
    repository_owner: &str,
    repository_name: &str,
    public_key: &str,
    app_version: &str,
    platform: &str,
) -> Result<(CatalogEntry, String), CatalogError> {
    let official = format!(
        "https://github.com/{repository_owner}/{repository_name}/releases/latest/download/{CATALOG_NAME}"
    );
    let sources = catalog_sources(&official);
    let client = reqwest::Client::new();
    let mut last = CatalogError::Transport;
    for (proxy, source) in sources {
        let body = match client
            .get(&source)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => match response.bytes().await {
                Ok(bytes) if bytes.len() <= MAX_CATALOG_BYTES => bytes,
                _ => {
                    last = CatalogError::Invalid;
                    continue;
                }
            },
            Err(_) => continue,
        };
        let catalog = match parse_catalog(&body, public_key) {
            Ok(catalog) => catalog,
            Err(error) => {
                last = error;
                continue;
            }
        };
        let entry = resolve_downloads(
            select_for_host(&catalog, app_version, platform)?,
            proxy,
            &official,
        );
        return Ok((entry, official.clone()));
    }
    Err(last)
}

/// Proxy prefixes first, the official endpoint last, each with a resolver that
/// converts a repository-relative asset path into that source's URL.
fn catalog_sources(official: &str) -> Vec<(Option<&'static str>, String)> {
    let mut sources = GITHUB_PROXY_PREFIXES
        .iter()
        .map(|prefix| {
            (
                Some(*prefix),
                format!("{}/{official}", prefix.trim_end_matches('/')),
            )
        })
        .collect::<Vec<_>>();
    sources.push((None, official.to_owned()));
    sources
}

fn resolve_downloads(
    mut entry: CatalogEntry,
    proxy: Option<&'static str>,
    official: &str,
) -> CatalogEntry {
    let base = official
        .rsplit_once('/')
        .map(|(base, _)| base)
        .unwrap_or(official);
    for value in entry.downloads.values_mut() {
        if value.starts_with("https://") {
            continue;
        }
        let relative = value.trim_start_matches('/');
        *value = format!("{base}/{relative}");
    }
    if let Some(prefix) = proxy {
        let prefix = format!("{}/", prefix.trim_end_matches('/'));
        for value in entry.downloads.values_mut() {
            if value.starts_with("https://") {
                *value = format!("{prefix}{value}");
            }
        }
    }
    entry
}

pub async fn download_verified(
    url: &str,
    sha256: Option<&str>,
    public_key: Option<&str>,
) -> Result<Vec<u8>, CatalogError> {
    if !url.starts_with("https://") {
        return Err(CatalogError::Download);
    }
    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| CatalogError::Download)?;
    if !response.status().is_success() {
        return Err(CatalogError::Download);
    }
    let bytes = response.bytes().await.map_err(|_| CatalogError::Download)?;
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err(CatalogError::Download);
    }
    if let Some(expected) = sha256
        && sha256_of(&bytes) != expected.to_ascii_lowercase()
    {
        return Err(CatalogError::Signature);
    }
    if let Some(key) = public_key
        && minisign_verify::PublicKey::from_base64(key).is_err()
    {
        return Err(CatalogError::Signature);
    }
    Ok(bytes.to_vec())
}

pub fn sha256_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            plugins: vec![CatalogEntry {
                id: "com.bongocat.countdown".into(),
                name: "Countdown".into(),
                description: "A timer".into(),
                version: "1.0.0".into(),
                app_versions: ">=2.3.0, <3".into(),
                protocol_version: PROTOCOL_VERSION,
                icon: PluginIcon::Emoji("🍅".into()),
                downloads: BTreeMap::from([(
                    "macos".to_owned(),
                    "countdown-1.0.0-macos.zip".to_owned(),
                )]),
                sha256: None,
                homepage: None,
            }],
        }
    }

    #[test]
    fn select_requires_protocol_app_window_and_platform() {
        let catalog = catalog();
        assert!(select_for_host(&catalog, "2.3.1", "macos").is_ok());
        assert_eq!(
            select_for_host(&catalog, "2.3.1", "windows"),
            Err(CatalogError::NoMatch)
        );
        assert_eq!(
            select_for_host(&catalog, "3.0.0", "macos"),
            Err(CatalogError::NoMatch)
        );
    }

    #[test]
    fn an_unsigned_or_unknown_catalog_is_rejected() {
        assert!(matches!(
            parse_catalog(b"{}", "not-a-key"),
            Err(CatalogError::Invalid) | Err(CatalogError::Signature)
        ));
    }
}
