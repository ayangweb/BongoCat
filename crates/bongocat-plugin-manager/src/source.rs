//! Package discovery never installs or executes the advertised code.
use crate::{Catalog, CatalogEntry, catalog, install};
use bongocat_plugin_sdk::PluginMetadata;
use cargo_packager_updater::semver::{Version, VersionReq};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug)]
pub enum PluginSource {
    InstalledOnly,
    Development {
        catalog: PathBuf,
        packages: PathBuf,
    },
    Production {
        owner: String,
        repository: String,
        public_key: String,
        platform: String,
    },
}

#[derive(Clone, Debug)]
pub struct AvailablePlugin {
    pub metadata: PluginMetadata,
    /// Where the card reads the same PNG the package carries.
    ///
    /// This is catalog projection state rather than package manifest data, so it
    /// stays beside the metadata instead of entering the plugin protocol.
    pub icon_url: String,
    pub digest: String,
    location: PackageLocation,
}
#[derive(Clone, Debug)]
enum PackageLocation {
    Local(PathBuf),
    Https(String),
}

impl AvailablePlugin {
    pub fn read(&self, stop: &AtomicBool) -> Result<Vec<u8>, String> {
        let bytes = match &self.location {
            PackageLocation::Local(path) => {
                install::read_bounded(path, catalog::MAX_DOWNLOAD_BYTES)
                    .map_err(|_| "local plugin package is unavailable".to_owned())?
            }
            PackageLocation::Https(url) => {
                catalog::download_blocking(url, stop).map_err(|error| error.to_string())?
            }
        };
        if catalog::sha256_of(&bytes) != self.digest {
            return Err("plugin package digest changed; refresh the catalog".into());
        }
        let manifest = install::inspect_package(&bytes).map_err(|error| error.to_string())?;
        if manifest.metadata.id != self.metadata.id
            || manifest.metadata.version != self.metadata.version
        {
            return Err("plugin package does not match the catalog".into());
        }
        Ok(bytes)
    }
}

impl PluginSource {
    pub fn discover(&self, app: &str, stop: &AtomicBool) -> Result<Vec<AvailablePlugin>, String> {
        let mut candidates = Vec::new();
        match self {
            Self::InstalledOnly => {}
            Self::Development { catalog, packages } => {
                let catalog = match fs::read(catalog) {
                    Ok(bytes) => serde_json::from_slice::<Catalog>(&bytes)
                        .map_err(|error| error.to_string())?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => return Err(error.to_string()),
                };
                for entry in catalog.plugins {
                    if stop.load(Ordering::Acquire) {
                        return Err("catalog refresh cancelled".into());
                    }
                    let package = packages.join(format!("{}-{}.zip", entry.id, entry.version));
                    let bytes = install::read_bounded(&package, catalog::MAX_DOWNLOAD_BYTES)
                        .map_err(|error| error.to_string())?;
                    let manifest =
                        install::inspect_package(&bytes).map_err(|error| error.to_string())?;
                    if manifest.metadata.id != entry.id
                        || manifest.metadata.version != entry.version
                    {
                        return Err("local catalog and package disagree".into());
                    }
                    candidates.push(AvailablePlugin {
                        metadata: from_catalog(&entry, String::new(), catalog::sha256_of(&bytes))
                            .metadata,
                        icon_url: entry.icon_url,
                        digest: catalog::sha256_of(&bytes),
                        location: PackageLocation::Local(package),
                    });
                }
            }
            Self::Production {
                owner,
                repository,
                public_key,
                platform,
            } => {
                let catalog = catalog::fetch_catalog_blocking(owner, repository, public_key, stop)
                    .map_err(|error| error.to_string())?;
                for entry in catalog.plugins {
                    if entry.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION {
                        continue;
                    }
                    let Some(url) = entry.downloads.get(platform) else {
                        continue;
                    };
                    // A package is only ever the one the signed catalog
                    // describes, so an entry without a usable digest is a
                    // catalog that cannot be acted on rather than one that is
                    // installed unverified.
                    let digest = entry
                        .sha256
                        .clone()
                        .filter(|digest| {
                            digest.len() == 64
                                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                        .ok_or("signed catalog lacks a package digest")?;
                    candidates.push(from_catalog(
                        &entry,
                        url.clone(),
                        digest.to_ascii_lowercase(),
                    ));
                }
            }
        }
        select_candidates(candidates, app)
    }
}

fn select_candidates(
    candidates: Vec<AvailablePlugin>,
    app: &str,
) -> Result<Vec<AvailablePlugin>, String> {
    let app = Version::parse(app).map_err(|error| error.to_string())?;
    let mut newest: BTreeMap<String, (Version, AvailablePlugin)> = BTreeMap::new();
    for candidate in candidates {
        if !VersionReq::parse(&candidate.metadata.app_versions).is_ok_and(|req| req.matches(&app)) {
            continue;
        }
        let version =
            Version::parse(&candidate.metadata.version).map_err(|error| error.to_string())?;
        if newest
            .get(&candidate.metadata.id)
            .is_none_or(|(previous, _)| version > *previous)
        {
            newest.insert(candidate.metadata.id.clone(), (version, candidate));
        }
    }
    Ok(newest
        .into_values()
        .map(|(_, candidate)| candidate)
        .collect())
}

fn from_catalog(entry: &CatalogEntry, url: String, digest: String) -> AvailablePlugin {
    AvailablePlugin {
        metadata: PluginMetadata {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            version: entry.version.clone(),
            app_versions: entry.app_versions.clone(),
            protocol_version: entry.protocol_version,
            entry: String::new(),
            icon: entry.icon.clone(),
            capabilities: Vec::new(),
            // The catalog carries the plugin's own localized copy, so an
            // available card reads the same words an installed one does.
            localizations: entry.localizations.clone(),
        },
        icon_url: entry.icon_url.clone(),
        digest,
        location: PackageLocation::Https(url),
    }
}

pub fn is_newer(available: &str, installed: &str) -> bool {
    match (Version::parse(available), Version::parse(installed)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn bundled_plugin_copy_is_simplified_chinese_or_english() {
        for bytes in [
            include_bytes!("../../../plugins/pomodoro/plugin.json").as_slice(),
            include_bytes!("../../../plugins/key-sounds/plugin.json").as_slice(),
            include_bytes!("../../../plugins/input-source/plugin.json").as_slice(),
        ] {
            let manifest: crate::PluginManifest = serde_json::from_slice(bytes).unwrap();
            let metadata = manifest.metadata;
            assert_eq!(metadata.localizations.len(), 1, "{}", metadata.id);
            let chinese = metadata.localizations.get("zh-CN").unwrap();
            for locale in ["zh-CN", "zh_CN", "ZH-cn"] {
                assert_eq!(metadata.view_locale(locale), "zh-CN");
                assert_eq!(
                    metadata.localized_copy(locale),
                    (chinese.name.as_str(), chinese.description.as_str()),
                );
            }
            for locale in [
                "en-US", "en-GB", "zh-TW", "zh-HK", "zh-MO", "zh-Hant", "ar-SA", "vi-VN", "pt-BR",
                "ko-KR", "fr-FR", "de-DE", "",
            ] {
                assert_eq!(metadata.view_locale(locale), "en-US");
                assert_eq!(
                    metadata.localized_copy(locale),
                    (metadata.name.as_str(), metadata.description.as_str()),
                    "{} fallback for {locale}",
                    metadata.id,
                );
            }
        }
    }

    #[test]
    fn key_sounds_plugin_identity_and_copy_cover_keyboard_and_mouse() {
        let manifest: crate::PluginManifest =
            serde_json::from_slice(include_bytes!("../../../plugins/key-sounds/plugin.json"))
                .unwrap();
        let metadata = manifest.metadata;
        assert_eq!(metadata.id, "com.bongocat.key-sounds");
        assert_eq!(metadata.entry, "bongocat-key-sounds");
        assert_eq!(
            metadata.localized_copy("en-US"),
            ("Key Sounds", "Keyboard and mouse sound effects"),
        );
        assert_eq!(
            metadata.localized_copy("zh-CN"),
            ("按键音效", "键盘和鼠标音效"),
        );
    }

    /// Which plugins the installed application may offer is the catalog's own
    /// `app_versions` range, so a bundled manifest declares a lower bound and no
    /// upper one: a plugin written against this app keeps working in the next
    /// release instead of disappearing from the center.
    #[test]
    fn bundled_manifests_accept_supported_app_versions_without_an_upper_bound() {
        for bytes in [
            include_bytes!("../../../plugins/pomodoro/plugin.json").as_slice(),
            include_bytes!("../../../plugins/key-sounds/plugin.json").as_slice(),
            include_bytes!("../../../plugins/input-source/plugin.json").as_slice(),
        ] {
            let manifest: crate::PluginManifest = serde_json::from_slice(bytes).unwrap();
            let requirement = VersionReq::parse(&manifest.metadata.app_versions).unwrap();
            for (app, supported) in [
                ("2.3.0", false),
                ("2.3.1", true),
                ("2.4.0", true),
                ("3.0.0", true),
                ("99.0.0", true),
            ] {
                assert_eq!(
                    requirement.matches(&Version::parse(app).unwrap()),
                    supported,
                    "{} application compatibility for {app}",
                    manifest.metadata.id,
                );
            }
        }
    }

    fn package(id: &str, version: &str, app_versions: &str) -> Vec<u8> {
        let mut manifest: crate::PluginManifest =
            serde_json::from_slice(include_bytes!("../../../plugins/pomodoro/plugin.json"))
                .unwrap();
        manifest.metadata.id = id.into();
        manifest.metadata.version = version.into();
        manifest.metadata.app_versions = app_versions.into();
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file(
                crate::MANIFEST_FILE,
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        archive.finish().unwrap().into_inner()
    }
    #[test]
    fn discovers_latest_per_id_without_installing_or_executing() {
        let directory = tempfile::tempdir().unwrap();
        let mut entries = Vec::new();
        for (filename, id, version, app) in [
            ("org.test.a-1.0.0.zip", "org.test.a", "1.0.0", ">=2"),
            ("org.test.a-1.1.0.zip", "org.test.a", "1.1.0", ">=2"),
            ("org.test.b-1.0.0.zip", "org.test.b", "1.0.0", ">=2"),
            ("org.test.a-9.0.0.zip", "org.test.a", "9.0.0", ">=9"),
        ] {
            let bytes = package(id, version, app);
            fs::write(directory.path().join(filename), &bytes).unwrap();
            if filename != "org.test.a-9.0.0.zip" {
                let manifest = install::inspect_package(&bytes).unwrap();
                entries.push(CatalogEntry {
                    id: manifest.metadata.id,
                    name: manifest.metadata.name,
                    description: manifest.metadata.description,
                    localizations: manifest.metadata.localizations,
                    version: manifest.metadata.version,
                    app_versions: manifest.metadata.app_versions,
                    protocol_version: manifest.metadata.protocol_version,
                    icon: manifest.metadata.icon,
                    icon_url: "https://example.com/a.png".into(),
                    downloads: Default::default(),
                    sha256: Some(catalog::sha256_of(&bytes)),
                    homepage: None,
                });
            }
        }
        fs::write(
            directory.path().join("catalog.json"),
            serde_json::to_vec(&Catalog { plugins: entries }).unwrap(),
        )
        .unwrap();
        let source = PluginSource::Development {
            catalog: directory.path().join("catalog.json"),
            packages: directory.path().to_owned(),
        };
        let offers = source.discover("2.3.1", &AtomicBool::new(false)).unwrap();
        assert_eq!(offers.len(), 2);
        assert_eq!(offers[0].metadata.version, "1.1.0");
        assert_eq!(offers[1].metadata.id, "org.test.b");
        assert!(!directory.path().join(crate::PACKAGES_DIRECTORY).exists());
        assert!(offers[0].read(&AtomicBool::new(false)).is_ok());
        let mut replaced = package("org.test.a", "1.1.0", ">=2");
        // ZIP readers accept trailing data, so this changes the digest while
        // leaving the manifest identity the catalog validates.
        replaced.extend_from_slice(b"replaced");
        fs::write(directory.path().join("org.test.a-1.1.0.zip"), replaced).unwrap();
        assert!(
            offers[0].read(&AtomicBool::new(false)).is_err(),
            "a replaced local package needs a fresh catalog/digest"
        );
    }
    #[test]
    fn missing_directory_empty_catalog_and_version_ordering() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            PluginSource::Development {
                catalog: root.path().join("not-built/catalog.json"),
                packages: root.path().join("not-built"),
            }
            .discover("2.3.1", &AtomicBool::new(false))
            .unwrap()
            .is_empty()
        );
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "2.0.0"));
    }
}

#[cfg(test)]
mod production_tests {
    use super::*;
    #[test]
    fn production_candidates_choose_latest_per_plugin_not_single_global_release() {
        let entry = |id: &str, version: &str, app: &str| CatalogEntry {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            localizations: Default::default(),
            version: version.into(),
            app_versions: app.into(),
            protocol_version: 1,
            icon: bongocat_plugin_sdk::PluginIcon::Emoji("A".into()),
            icon_url: "https://example.com/a.png".into(),
            downloads: Default::default(),
            sha256: None,
            homepage: None,
        };
        let candidates = [
            ("org.test.a", "1.0.0", ">=2"),
            ("org.test.a", "1.1.0", ">=2"),
            ("org.test.b", "1.0.0", ">=2"),
            ("org.test.a", "9.0.0", ">=9"),
        ]
        .into_iter()
        .map(|(id, version, app)| {
            from_catalog(
                &entry(id, version, app),
                "https://example.com/package.zip".into(),
                "0".repeat(64),
            )
        })
        .collect();
        let offers = select_candidates(candidates, "2.3.1").unwrap();
        assert_eq!(offers.len(), 2);
        assert_eq!(offers[0].metadata.version, "1.1.0");
        assert_eq!(offers[1].metadata.id, "org.test.b");
    }
}
