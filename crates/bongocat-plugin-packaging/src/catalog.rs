use crate::{
    Error, PLATFORMS, Result, archive, arg,
    signing::{sign_catalog, verify_catalog},
    workspace::{self, Manifest, PluginSource},
};
use bongocat_plugin_sdk::PluginIcon;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub const STATE_NAME: &str = "release-state.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub localizations: BTreeMap<String, bongocat_plugin_sdk::PluginCopy>,
    pub version: String,
    pub app_versions: String,
    pub protocol_version: u32,
    pub icon: PluginIcon,
    pub icon_url: String,
    pub downloads: BTreeMap<String, String>,
    pub sha256: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub plugins: Vec<CatalogEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseState {
    pub schema: u32,
    pub plugins: Vec<PublishedPlugin>,
    #[serde(default)]
    pub retired: Vec<PublishedPlugin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedPlugin {
    pub id: String,
    pub version: String,
    pub source_sha256: String,
    pub packages: BTreeMap<String, PackageDigest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDigest {
    pub asset: String,
    pub sha256: String,
}

pub fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn command(args: &[String]) -> Result<()> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(Error::Usage(
            "catalog requires assemble, dev, sign, or verify".into(),
        ));
    };
    match action {
        "assemble" => assemble_command(&args[1..]),
        "dev" => dev_command(&args[1..]),
        "sign" => sign_command(&args[1..]),
        "verify" => verify_command(&args[1..]),
        _ => Err(Error::Usage(format!("unknown catalog action {action}"))),
    }
}

/// Build one host platform and write the same catalog shape Development reads.
///
/// This intentionally does not consume release history, invent a release tag,
/// or sign anything. It is a local inspection surface for the application's
/// `just dev` path, not an alternate release pipeline.
fn dev_command(args: &[String]) -> Result<()> {
    crate::reject_unknown(args, &["--root", "--target", "--output"], &[])?;
    let root = Path::new(arg(args, "--root")?.unwrap_or("."));
    let target = match arg(args, "--target")? {
        Some(target) => crate::Platform::parse(target)?,
        None => crate::Platform::host()?,
    };
    let output = PathBuf::from(
        arg(args, "--output")?
            .ok_or_else(|| Error::Usage("catalog dev requires --output".into()))?,
    );

    workspace::build_target(root, target, &output)?;
    let sources = workspace::plugins(root)?;
    let mut rows = Vec::new();
    for source in &sources {
        let package_path = output.join(archive::package_name(&source.manifest, target));
        let bytes = fs::read(&package_path)?;
        archive::validate_package(&bytes, &source.manifest, target)?;
        let PluginIcon::LocalImage(relative) = &source.manifest.icon else {
            return Err(Error::Invalid("plugin must use a local icon".into()));
        };
        let icon_name = archive::icon_name(&source.manifest);
        let icon_path = output.join(&icon_name);
        crate::output::replace(&icon_path, &fs::read(source.directory.join(relative))?)?;
        rows.push(CatalogEntry {
            id: source.manifest.id.clone(),
            name: source.manifest.name.clone(),
            description: source.manifest.description.clone(),
            localizations: source.manifest.localizations.clone(),
            version: source.manifest.version.clone(),
            app_versions: source.manifest.app_versions.clone(),
            protocol_version: source.manifest.protocol_version,
            icon: source.manifest.icon.clone(),
            icon_url: icon_path.canonicalize()?.to_string_lossy().into_owned(),
            downloads: [(
                target.key().to_owned(),
                archive::package_name(&source.manifest, target),
            )]
            .into(),
            sha256: Some(sha256_of(&bytes)),
            homepage: None,
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    crate::output::replace(
        &output.join("catalog.json"),
        &serde_json::to_vec(&Catalog { plugins: rows })?,
    )?;
    Ok(())
}

fn assemble_command(args: &[String]) -> Result<()> {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                if index + 2 >= args.len() {
                    return Err(Error::Usage(
                        "--platform requires a platform and package directory".into(),
                    ));
                }
                index += 3;
            }
            "--root" | "--tag" | "--previous" | "--output" | "--release-url" => {
                if index + 1 >= args.len() {
                    return Err(Error::Usage(format!("{} requires a value", args[index])));
                }
                index += 2;
            }
            value => return Err(Error::Usage(format!("unknown argument {value}"))),
        }
    }
    let root = Path::new(arg(args, "--root")?.unwrap_or("."));
    let tag = arg(args, "--tag")?.ok_or_else(|| Error::Usage("assemble requires --tag".into()))?;
    let output = PathBuf::from(
        arg(args, "--output")?.ok_or_else(|| Error::Usage("assemble requires --output".into()))?,
    );
    let release_url = arg(args, "--release-url")?.map(release_url);
    let asset_url = |name: &str| match &release_url {
        Some(release_url) => format!("{release_url}/{name}"),
        None => name.to_owned(),
    };
    let previous_dir = PathBuf::from(arg(args, "--previous")?.unwrap_or("."));
    let mut seen = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--platform" {
            let platform = crate::Platform::parse(
                args.get(index + 1)
                    .ok_or_else(|| Error::Usage("--platform requires a value".into()))?,
            )?;
            let packages =
                PathBuf::from(args.get(index + 2).ok_or_else(|| {
                    Error::Usage("--platform requires a package directory".into())
                })?);
            if !seen.insert(platform) {
                return Err(Error::Usage("duplicate platform".into()));
            }
            inputs.push((platform, packages));
            index += 3;
        } else {
            index += 1;
        }
    }
    if inputs.len() != PLATFORMS.len() {
        return Err(Error::Usage(
            "assemble requires exactly one package directory for every platform".into(),
        ));
    }
    let sources = workspace::plugins(root)?;
    let previous_state = if previous_dir.join(STATE_NAME).is_file() {
        Some(read_state(&previous_dir.join(STATE_NAME))?)
    } else {
        None
    };
    if let Some(state) = &previous_state {
        for plugin in &state.plugins {
            if plugin.packages.len() != PLATFORMS.len()
                || PLATFORMS
                    .iter()
                    .any(|platform| !plugin.packages.contains_key(platform.key()))
            {
                return Err(Error::Invalid(format!(
                    "{} has an incomplete historical platform matrix",
                    plugin.id
                )));
            }
        }
        for plugin in &state.retired {
            if plugin.packages.len() != PLATFORMS.len()
                || PLATFORMS
                    .iter()
                    .any(|platform| !plugin.packages.contains_key(platform.key()))
            {
                return Err(Error::Invalid(format!(
                    "{} has an incomplete retired platform matrix",
                    plugin.id
                )));
            }
        }
    }
    let mut assets = BTreeMap::<String, Vec<u8>>::new();
    let mut published = Vec::new();
    let mut retired = Vec::new();
    let mut rows = Vec::new();
    for source in &sources {
        let current_source = source_digest(source)?;
        let previous_plugin = previous_state.as_ref().and_then(|state| {
            state
                .plugins
                .iter()
                .chain(state.retired.iter())
                .find(|plugin| plugin.id == source.manifest.id)
        });
        if let Some(previous) = previous_plugin {
            let current_version = Version::parse(&source.manifest.version)
                .map_err(|_| Error::Invalid("plugin has an invalid version".into()))?;
            let previous_version = Version::parse(&previous.version)
                .map_err(|_| Error::Invalid("historical plugin has an invalid version".into()))?;
            if current_version < previous_version {
                return Err(Error::Invalid(format!(
                    "{} version regressed from {} to {}",
                    source.manifest.id, previous.version, source.manifest.version
                )));
            }
            if current_version == previous_version && previous.source_sha256 != current_source {
                return Err(Error::Invalid(format!(
                    "{} changed without a version bump; publish a newer version",
                    source.manifest.id
                )));
            }
            if current_version > previous_version {
                for platform in PLATFORMS {
                    let package = previous
                        .packages
                        .get(platform.key())
                        .ok_or_else(|| Error::Invalid("historical package is missing".into()))?;
                    let path = previous_dir.join(&package.asset);
                    let bytes = fs::read(&path).map_err(|_| {
                        Error::Invalid(format!(
                            "historical package {} is unavailable",
                            package.asset
                        ))
                    })?;
                    if sha256_of(&bytes) != package.sha256 {
                        return Err(Error::Invalid(format!(
                            "historical package {} digest changed",
                            package.asset
                        )));
                    }
                }
            }
        }
        let mut packages = BTreeMap::new();
        for platform in PLATFORMS {
            let directory = inputs
                .iter()
                .find(|(candidate, _)| *candidate == platform)
                .map(|(_, directory)| directory)
                .ok_or_else(|| Error::Usage("missing platform package directory".into()))?;
            let expected_name = archive::package_name(&source.manifest, platform);
            let package_path = directory.join(&expected_name);
            let unchanged = previous_plugin.is_some_and(|previous| {
                previous.version == source.manifest.version
                    && previous.source_sha256 == current_source
            });
            let (bytes, reused) =
                if let Some(previous_plugin) = previous_plugin.filter(|_| unchanged) {
                    let package = previous_plugin
                        .packages
                        .get(platform.key())
                        .ok_or_else(|| Error::Invalid("historical package is missing".into()))?;
                    let path = previous_dir.join(&package.asset);
                    if !path.is_file() {
                        return Err(Error::Invalid(format!(
                            "historical package {} is unavailable",
                            package.asset
                        )));
                    }
                    let bytes = fs::read(path)?;
                    (bytes, true)
                } else if package_path.is_file() && !unchanged {
                    (fs::read(&package_path)?, false)
                } else {
                    return Err(Error::Invalid(format!(
                        "{} is not reusable or missing for {}",
                        expected_name,
                        platform.key()
                    )));
                };
            let manifest = archive::package_manifest(&bytes)?;
            if manifest.id != source.manifest.id || manifest.version != source.manifest.version {
                return Err(Error::Invalid(
                    "package manifest does not match source".into(),
                ));
            }
            archive::validate_package(&bytes, &manifest, platform)?;
            if reused
                && previous_plugin
                    .and_then(|plugin| plugin.packages.get(platform.key()))
                    .is_none_or(|data| data.sha256 != sha256_of(&bytes))
            {
                return Err(Error::Invalid("historical package digest changed".into()));
            }
            let digest = sha256_of(&bytes);
            let asset_name = archive::package_name(&source.manifest, platform);
            if let Some(existing) = assets.insert(asset_name.clone(), bytes.clone())
                && existing != bytes
            {
                return Err(Error::Invalid("duplicate package bytes".into()));
            }
            packages.insert(
                platform.key().to_owned(),
                PackageDigest {
                    asset: asset_name.clone(),
                    sha256: digest.clone(),
                },
            );
            rows.push(CatalogEntry {
                id: source.manifest.id.clone(),
                name: source.manifest.name.clone(),
                description: source.manifest.description.clone(),
                localizations: source.manifest.localizations.clone(),
                version: source.manifest.version.clone(),
                app_versions: source.manifest.app_versions.clone(),
                protocol_version: source.manifest.protocol_version,
                icon: source.manifest.icon.clone(),
                icon_url: asset_url(&archive::icon_name(&source.manifest)),
                downloads: [(platform.key().to_owned(), asset_url(&asset_name))].into(),
                sha256: Some(digest),
                homepage: None,
            });
        }
        if let PluginIcon::LocalImage(relative) = &source.manifest.icon {
            let icon_name = archive::icon_name(&source.manifest);
            let bytes = fs::read(source.directory.join(relative))?;
            if let Some(existing) = assets.insert(icon_name, bytes.clone())
                && existing != bytes
            {
                return Err(Error::Invalid("plugin icon changed for one version".into()));
            }
        }
        published.push(PublishedPlugin {
            id: source.manifest.id.clone(),
            version: source.manifest.version.clone(),
            source_sha256: current_source,
            packages,
        });
    }
    if let Some(state) = &previous_state {
        let current = published
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect::<BTreeSet<_>>();
        for old in state
            .plugins
            .iter()
            .filter(|plugin| !current.contains(plugin.id.as_str()))
        {
            for package in old.packages.values() {
                let path = previous_dir.join(&package.asset);
                let bytes = fs::read(&path).map_err(|_| {
                    Error::Invalid(format!(
                        "historical package {} is unavailable",
                        package.asset
                    ))
                })?;
                if sha256_of(&bytes) != package.sha256 {
                    return Err(Error::Invalid(format!(
                        "historical package {} digest changed",
                        package.asset
                    )));
                }
                if let Some(existing) = assets.insert(package.asset.clone(), bytes.clone())
                    && existing != bytes
                {
                    return Err(Error::Invalid("duplicate package bytes".into()));
                }
            }
            retired.push(old.clone());
        }
    }
    if let Some(state) = &previous_state {
        for old in &state.retired {
            if !published.iter().any(|plugin| plugin.id == old.id)
                && !retired.iter().any(|plugin| plugin.id == old.id)
            {
                retired.push(old.clone());
            }
        }
    }
    retired.sort_by(|a, b| a.id.cmp(&b.id));
    rows.sort_by(|a, b| (&a.id, &a.version, &a.downloads).cmp(&(&b.id, &b.version, &b.downloads)));
    let catalog = Catalog { plugins: rows };
    let state = ReleaseState {
        schema: 1,
        plugins: published,
        retired,
    };
    fs::create_dir_all(&output)?;
    for (name, bytes) in assets {
        crate::output::new_or_same(&output.join(name), &bytes)?;
    }
    crate::output::new_or_same(&output.join("catalog.json"), &serde_json::to_vec(&catalog)?)?;
    crate::output::new_or_same(
        &output.join(STATE_NAME),
        &serde_json::to_vec_pretty(&state)?,
    )?;
    crate::output::new_or_same(&output.join("RELEASE_TAG"), tag.as_bytes())?;
    Ok(())
}

fn sign_command(args: &[String]) -> Result<()> {
    crate::reject_unknown(args, &["--catalog", "--output", "--public-key"], &[])?;
    let input = PathBuf::from(
        arg(args, "--catalog")?.ok_or_else(|| Error::Usage("sign requires --catalog".into()))?,
    );
    let output = PathBuf::from(
        arg(args, "--output")?.ok_or_else(|| Error::Usage("sign requires --output".into()))?,
    );
    let private = std::env::var("PLUGIN_SIGNING_PRIVATE_KEY")
        .map_err(|_| Error::Usage("PLUGIN_SIGNING_PRIVATE_KEY is required".into()))?;
    let catalog_bytes = fs::read(&input)?;
    let canonical_catalog: Catalog = serde_json::from_slice(&catalog_bytes)?;
    let canonical = serde_json::to_vec(&canonical_catalog)?;
    #[derive(serde::Serialize)]
    struct Signed<'a> {
        catalog: &'a Catalog,
        signature: String,
    }
    let bytes = serde_json::to_vec(&Signed {
        catalog: &canonical_catalog,
        signature: sign_catalog(&canonical, &private)?,
    })?;
    if let Some(public_path) = arg(args, "--public-key")? {
        let key = fs::read_to_string(public_path)?;
        verify_catalog(&bytes, &key)?;
    }
    crate::output::replace(&output, &bytes)?;
    Ok(())
}

fn verify_command(args: &[String]) -> Result<()> {
    crate::reject_unknown(args, &["--catalog", "--public-key"], &[])?;
    let catalog_path = PathBuf::from(
        arg(args, "--catalog")?.ok_or_else(|| Error::Usage("verify requires --catalog".into()))?,
    );
    let key = fs::read_to_string(
        arg(args, "--public-key")?
            .ok_or_else(|| Error::Usage("verify requires --public-key".into()))?,
    )?;
    let catalog = verify_catalog(&fs::read(catalog_path)?, &key)?;
    if catalog.plugins.is_empty() {
        return Err(Error::Invalid("signed catalog has no plugins array".into()));
    }
    validate_catalog_matrix(&catalog, true)?;
    Ok(())
}

fn validate_catalog_matrix(catalog: &Catalog, require_https: bool) -> Result<()> {
    for entry in &catalog.plugins {
        if entry.downloads.len() != 1 {
            return Err(Error::Invalid(format!(
                "{} {} must have exactly one platform download per row",
                entry.id, entry.version
            )));
        }
        let platform = crate::Platform::parse(
            entry
                .downloads
                .keys()
                .next()
                .ok_or_else(|| Error::Invalid("catalog row has no platform".into()))?,
        )?;
        let expected = archive::package_name(
            &Manifest {
                id: entry.id.clone(),
                name: entry.name.clone(),
                description: entry.description.clone(),
                version: entry.version.clone(),
                app_versions: entry.app_versions.clone(),
                protocol_version: entry.protocol_version,
                entry: String::new(),
                icon: entry.icon.clone(),
                capabilities: Vec::new(),
                localizations: entry.localizations.clone(),
            },
            platform,
        );
        let download = entry
            .downloads
            .values()
            .next()
            .ok_or_else(|| Error::Invalid("catalog row has no download".into()))?;
        if catalog_asset_name(download) != expected
            || (require_https && !download.starts_with("https://"))
        {
            return Err(Error::Invalid(format!(
                "{} {} names an unexpected package for {}",
                entry.id,
                entry.version,
                platform.key()
            )));
        }
        if require_https && !entry.icon_url.starts_with("https://") {
            return Err(Error::Invalid(format!(
                "{} {} has a non-HTTPS icon URL",
                entry.id, entry.version
            )));
        }
        if entry.sha256.as_ref().is_none_or(|digest| {
            digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(Error::Invalid(format!(
                "{} {} has an invalid package digest",
                entry.id, entry.version
            )));
        }
    }
    Ok(())
}

fn release_url(value: &str) -> String {
    value.trim_end_matches('/').to_owned()
}

pub fn catalog_asset_name(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

pub fn read_state(path: &Path) -> Result<ReleaseState> {
    let state: ReleaseState = serde_json::from_slice(&fs::read(path)?)?;
    if state.schema != 1 {
        return Err(Error::Invalid("unsupported release state schema".into()));
    }
    Ok(state)
}

fn source_digest(source: &PluginSource) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"bongocat-plugin-source-v2\0");
    hasher.update(&source.manifest_bytes);
    let root = source
        .directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::Invalid("plugin directory has no workspace root".into()))?;
    let mut sdk_files = Vec::new();
    collect(
        &root.join("crates/bongocat-plugin-sdk"),
        &root.join("crates/bongocat-plugin-sdk"),
        &mut sdk_files,
    )?;
    sdk_files.sort();
    for path in sdk_files {
        let relative = path
            .strip_prefix(root.join("crates/bongocat-plugin-sdk"))
            .map_err(|_| Error::Invalid("SDK source path escaped its directory".into()))?;
        hasher.update(b"sdk/");
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(fs::read(&path)?);
    }
    let PluginIcon::LocalImage(relative) = &source.manifest.icon else {
        return Err(Error::Invalid("plugin must use a local icon".into()));
    };
    hasher.update(&fs::read(source.directory.join(relative))?);
    let mut files = Vec::new();
    collect(&source.directory, &source.directory, &mut files)?;
    files.sort();
    for path in files {
        let relative = path
            .strip_prefix(&source.directory)
            .map_err(|_| Error::Invalid("plugin source path escaped its directory".into()))?;
        let name = relative.to_string_lossy();
        if name == "plugin.json" {
            continue;
        }
        hasher.update(name.as_bytes());
        hasher.update(fs::read(&path)?);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(root, &path, files)?;
        } else if entry.file_type()?.is_file()
            && !path.starts_with(root.join("target"))
            && path.file_name().is_some_and(|name| name != "plugin.json")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod development_tests {
    use super::*;

    #[test]
    fn development_catalog_names_local_packages_and_icons() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let Ok(target) = crate::Platform::host() else {
            return;
        };
        let output = tempfile::tempdir().unwrap();
        let args = [
            "--root".into(),
            root.display().to_string(),
            "--target".into(),
            target.triple().into(),
            "--output".into(),
            output.path().display().to_string(),
        ];
        dev_command(&args).unwrap();
        // Development builds are routinely repeated without cleaning target/.
        dev_command(&args).unwrap();
        let catalog: Catalog =
            serde_json::from_slice(&fs::read(output.path().join("catalog.json")).unwrap()).unwrap();
        assert_eq!(catalog.plugins.len(), 3);
        for entry in catalog.plugins {
            let package = output
                .path()
                .join(entry.downloads.get(target.key()).unwrap());
            assert!(package.is_file(), "{}", package.display());
            assert_eq!(
                entry.sha256.as_deref(),
                Some(sha256_of(&fs::read(package).unwrap()).as_str())
            );
            assert!(Path::new(&entry.icon_url).is_file());
        }
    }
}
