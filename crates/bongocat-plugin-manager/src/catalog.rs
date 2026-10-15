//! The production catalog: a signed, protocol- and app-version-gated list.
//!
//! Transport, HTTPS, proxy order and Minisign verification are the libraries the
//! updater already uses; this module owns only the catalog shape and policy.
use bongocat_plugin_sdk::PluginIcon;
use bongocat_update::GITHUB_PROXY_PREFIXES;
use cargo_packager_updater::reqwest;
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
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::Invalid);
    }
    let signed: SignedCatalog = serde_json::from_slice(bytes).map_err(|_| CatalogError::Invalid)?;
    use base64::Engine;
    let decode = |value: &str| -> Result<String, CatalogError> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| CatalogError::Signature)?;
        String::from_utf8(bytes).map_err(|_| CatalogError::Signature)
    };
    // Match cargo-packager's public-key and .sig encoding, exactly as the
    // application updater does (base64 of the complete Minisign text boxes).
    let key = minisign_verify::PublicKey::decode(&decode(public_key)?)
        .map_err(|_| CatalogError::Signature)?;
    let signature = minisign_verify::Signature::decode(&decode(&signed.signature)?)
        .map_err(|_| CatalogError::Signature)?;
    let canonical = serde_json::to_vec(&signed.catalog).map_err(|_| CatalogError::Invalid)?;
    key.verify(&canonical, &signature, true)
        .map_err(|_| CatalogError::Signature)?;
    Ok(signed.catalog)
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

pub fn sha256_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Blocking transport for the catalog IO worker, not the runtime/UI executor.
pub(crate) fn fetch_catalog_blocking(
    owner: &str,
    repository: &str,
    key: &str,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Catalog, CatalogError> {
    let official =
        format!("https://github.com/{owner}/{repository}/releases/latest/download/{CATALOG_NAME}");
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .build()
        .map_err(|_| CatalogError::Transport)?;
    let mut last = CatalogError::Transport;
    for (proxy, source) in catalog_sources(&official) {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let result = (|| {
            let response = client
                .get(source)
                .timeout(Duration::from_secs(3))
                .send()
                .and_then(|response| response.error_for_status())
                .map_err(|_| CatalogError::Transport)?;
            let bytes = bounded_body(response, MAX_CATALOG_BYTES)?;
            let mut catalog = parse_catalog(&bytes, key)?;
            catalog.plugins = catalog
                .plugins
                .into_iter()
                .map(|entry| resolve_downloads(entry, proxy, &official))
                .collect();
            Ok(catalog)
        })();
        match result {
            Ok(catalog) => return Ok(catalog),
            Err(error) => last = error,
        }
    }
    Err(last)
}

fn bounded_body(
    response: reqwest::blocking::Response,
    limit: usize,
) -> Result<Vec<u8>, CatalogError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CatalogError::Download)?;
    if bytes.len() > limit {
        return Err(CatalogError::Download);
    }
    Ok(bytes)
}

pub(crate) fn download_blocking(
    url: &str,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .build()
        .map_err(|_| CatalogError::Download)?;
    let official = GITHUB_PROXY_PREFIXES
        .iter()
        .find_map(|prefix| url.strip_prefix(&format!("{}/", prefix.trim_end_matches('/'))))
        .unwrap_or(url);
    let mut sources = vec![url.to_owned()];
    if official.starts_with("https://github.com/") {
        for (_, source) in catalog_sources(official) {
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
    }
    for source in sources {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let result = client
            .get(source)
            .timeout(Duration::from_secs(3))
            .send()
            .and_then(|response| response.error_for_status());
        if let Ok(response) = result
            && let Ok(bytes) = bounded_body(response, MAX_DOWNLOAD_BYTES)
        {
            return Ok(bytes);
        }
    }
    Err(CatalogError::Download)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsigned_or_unknown_catalog_is_rejected() {
        assert!(matches!(
            parse_catalog(b"{}", "not-a-key"),
            Err(CatalogError::Invalid) | Err(CatalogError::Signature)
        ));
    }
}

#[cfg(test)]
mod signed_tests {
    use super::*;
    #[test]
    fn accepts_updater_key_and_signature_boxes_and_rejects_tampering() {
        let directory = tempfile::tempdir().unwrap();
        let keys = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        let catalog = Catalog {
            plugins: Vec::new(),
        };
        let path = directory.path().join("catalog.json");
        std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        let signature_path = cargo_packager::sign::sign_file(
            &cargo_packager::sign::SigningConfig::new()
                .private_key(keys.sk)
                .password(""),
            &path,
        )
        .unwrap();
        let signed = SignedCatalog {
            catalog,
            signature: std::fs::read_to_string(signature_path).unwrap(),
        };
        let mut bytes = serde_json::to_vec(&signed).unwrap();
        assert_eq!(parse_catalog(&bytes, &keys.pk).unwrap().plugins.len(), 0);
        let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        tampered["catalog"]["plugins"] = serde_json::json!([{
            "id": "org.test.a", "name": "A", "description": "", "version": "1.0.0", "app_versions": ">=2", "protocol_version": 1,
            "icon": { "type": "emoji", "value": "A" }, "downloads": {}
        }]);
        bytes = serde_json::to_vec(&tampered).unwrap();
        assert_eq!(
            parse_catalog(&bytes, &keys.pk),
            Err(CatalogError::Signature)
        );
        let other_keys = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        assert_eq!(
            parse_catalog(&serde_json::to_vec(&signed).unwrap(), &other_keys.pk),
            Err(CatalogError::Signature)
        );
    }
    #[test]
    fn compiled_release_key_uses_the_same_box_format() {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(bongocat_update::RELEASE_SIGNING_KEY.unwrap())
            .unwrap();
        assert!(minisign_verify::PublicKey::decode(std::str::from_utf8(&bytes).unwrap()).is_ok());
    }
}
