use crate::{Error, Platform, Result};
use bongocat_plugin_sdk::{PROTOCOL_VERSION, PluginIcon};
use semver::{Version, VersionReq};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const MANIFEST_FILE: &str = "plugin.json";
pub const MAX_PACKAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct PluginSource {
    pub directory: PathBuf,
    pub manifest_bytes: Vec<u8>,
    pub manifest: Manifest,
}

pub type Manifest = bongocat_plugin_sdk::PluginMetadata;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    metadata: Manifest,
}

pub fn plugins(root: &Path) -> Result<Vec<PluginSource>> {
    let plugins_dir = root.join("plugins");
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for entry in fs::read_dir(&plugins_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let manifest_path = directory.join(MANIFEST_FILE);
        if !manifest_path.is_file() {
            continue;
        }
        let manifest_bytes = fs::read(&manifest_path)?;
        if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(Error::Invalid(format!(
                "{} is too large",
                manifest_path.display()
            )));
        }
        let ManifestFile { metadata } = serde_json::from_slice(&manifest_bytes)?;
        validate_manifest(&metadata, &directory)?;
        if !ids.insert(metadata.id.clone()) {
            return Err(Error::Invalid(format!(
                "duplicate plugin id {}",
                metadata.id
            )));
        }
        result.push(PluginSource {
            directory,
            manifest_bytes,
            manifest: metadata,
        });
    }
    result.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    if result.is_empty() {
        return Err(Error::Invalid("no plugin manifests found".into()));
    }
    Ok(result)
}

fn validate_manifest(manifest: &Manifest, directory: &Path) -> Result<()> {
    Version::parse(&manifest.version)
        .map_err(|_| Error::Invalid(format!("{} has an invalid version", manifest.id)))?;
    VersionReq::parse(&manifest.app_versions).map_err(|_| {
        Error::Invalid(format!("{} has an invalid app_versions range", manifest.id))
    })?;
    if !valid_id(&manifest.id)
        || !valid_name(&manifest.entry)
        || manifest.protocol_version != PROTOCOL_VERSION
        || manifest.name.trim().is_empty()
        || manifest.description.trim().is_empty()
    {
        return Err(Error::Invalid(format!(
            "{} has invalid release metadata",
            directory.display()
        )));
    }
    let PluginIcon::LocalImage(relative) = &manifest.icon else {
        return Err(Error::Invalid(format!(
            "{} must declare a local PNG icon",
            manifest.id
        )));
    };
    let icon = Path::new(relative);
    if !safe_relative_path(icon) || !directory.join(icon).is_file() {
        return Err(Error::Invalid(format!(
            "{} has a missing or unsafe icon",
            manifest.id
        )));
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !matches!(value, "." | "..")
}

pub fn safe_relative_path(path: &Path) -> bool {
    use std::path::Component;
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.to_string_lossy().contains(['\\', ':'])
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.to_string_lossy().len() <= 256
}

pub fn build_target(root: &Path, target: Platform, output: &Path) -> Result<Vec<PathBuf>> {
    if (target.is_apple() && !cfg!(target_os = "macos"))
        || (target == Platform::WindowsX86_64 && !cfg!(target_os = "windows"))
    {
        return Err(Error::Invalid(format!(
            "{} must be built by a runner of the target operating system",
            target.key()
        )));
    }
    fs::create_dir_all(output)?;
    let mut packages = Vec::new();
    for source in plugins(root)? {
        let build = Command::new("cargo")
            .current_dir(root)
            .args([
                "build",
                "--locked",
                "--release",
                "--target",
                target.triple(),
                "--manifest-path",
            ])
            .arg(source.directory.join("Cargo.toml"))
            .args(["--bin", &source.manifest.entry, "--message-format=json"])
            .output()?;
        if !build.status.success() {
            return Err(Error::Invalid(format!(
                "failed to build {} for {}",
                source.manifest.id,
                target.key()
            )));
        }
        let executable = executable_from_messages(&build.stdout, &source.manifest.entry)
            .ok_or_else(|| Error::Invalid("Cargo did not report a plugin executable".into()))?;
        let package = crate::archive::write_package(&source, target, &executable, output)?;
        crate::archive::validate_package(&fs::read(&package)?, &source.manifest, target)?;
        packages.push(package);
    }
    Ok(packages)
}

fn executable_from_messages(bytes: &[u8], entry: &str) -> Option<PathBuf> {
    bytes.split(|byte| *byte == b'\n').find_map(|line| {
        let value: serde_json::Value = serde_json::from_slice(line).ok()?;
        (value["reason"] == "compiler-artifact"
            && value["target"]["name"] == entry
            && value["executable"].is_string())
        .then(|| PathBuf::from(value["executable"].as_str().unwrap()))
    })
}
