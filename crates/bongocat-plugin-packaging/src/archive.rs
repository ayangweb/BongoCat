use crate::{
    Error, Platform, Result,
    workspace::{
        MAX_ARCHIVE_ENTRIES, MAX_EXTRACT_BYTES, MAX_PACKAGE_BYTES, Manifest, PluginSource,
    },
};
use bongocat_plugin_sdk::PluginIcon;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use zip::{CompressionMethod, System, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub fn asset_stem(manifest: &Manifest) -> &str {
    manifest
        .id
        .rsplit('.')
        .find(|component| !component.is_empty())
        .unwrap_or(&manifest.id)
}

pub fn package_name(manifest: &Manifest, target: Platform) -> String {
    format!(
        "{}-{}-{}.zip",
        asset_stem(manifest),
        manifest.version,
        target.key()
    )
}

pub fn icon_name(manifest: &Manifest) -> String {
    format!("{}-{}.png", asset_stem(manifest), manifest.version)
}

pub fn write_package(
    source: &PluginSource,
    target: Platform,
    executable: &Path,
    output: &Path,
) -> Result<std::path::PathBuf> {
    let binary = fs::read(executable)?;
    if binary.is_empty() || binary.len() as u64 > MAX_EXTRACT_BYTES {
        return Err(Error::Invalid(
            "plugin executable is empty or too large".into(),
        ));
    }
    let entry = if target == Platform::WindowsX86_64 {
        format!("{}.exe", source.manifest.entry)
    } else {
        source.manifest.entry.clone()
    };
    let mut archive = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    archive.start_file(crate::workspace::MANIFEST_FILE, options)?;
    archive.write_all(&source.manifest_bytes)?;
    let PluginIcon::LocalImage(relative) = &source.manifest.icon else {
        unreachable!("validated manifests always use a local icon");
    };
    archive.start_file(relative, options)?;
    archive.write_all(&fs::read(source.directory.join(relative))?)?;
    archive.start_file(entry, options.unix_permissions(0o755).system(System::Unix))?;
    archive.write_all(&binary)?;
    let bytes = archive.finish()?.into_inner();
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(Error::Invalid(
            "plugin package exceeds the download limit".into(),
        ));
    }
    let path = output.join(package_name(&source.manifest, target));
    crate::output::replace(&path, &bytes)?;
    Ok(path)
}

pub fn validate_package(bytes: &[u8], manifest: &Manifest, target: Platform) -> Result<()> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(Error::Invalid("plugin package exceeds 16 MiB".into()));
    }
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::Invalid("plugin package has too many entries".into()));
    }
    let entry = if target == Platform::WindowsX86_64 {
        format!("{}.exe", manifest.entry)
    } else {
        manifest.entry.clone()
    };
    let PluginIcon::LocalImage(relative) = &manifest.icon else {
        return Err(Error::Invalid("plugin must use a local PNG icon".into()));
    };
    let expected = [crate::workspace::MANIFEST_FILE, relative, entry.as_str()];
    let names = archive
        .file_names()
        .map(|name| name.map(|name| name.into_owned()))
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()?;
    if names.len() != archive.len() {
        return Err(Error::Invalid(
            "plugin package contains duplicate names".into(),
        ));
    }
    let mut total = 0_u64;
    for name in &names {
        if name.contains(['\\', ':', '\0']) || name.starts_with('/') {
            return Err(Error::Invalid("unsafe zip entry".into()));
        }
        if !expected.iter().any(|entry| *entry == name) {
            return Err(Error::Invalid(format!(
                "unexpected zip entry {name}; expected {:?}",
                expected
            )));
        }
        let mut item = archive.by_name(name)?;
        if item.is_dir() || item.is_symlink() || item.size() > MAX_EXTRACT_BYTES {
            return Err(Error::Invalid("invalid zip entry type".into()));
        }
        total = total
            .checked_add(item.size())
            .filter(|total| *total <= MAX_EXTRACT_BYTES)
            .ok_or_else(|| Error::Invalid("plugin package expands too large".into()))?;
        if *name == entry
            && (item.size() == 0
                || (target.is_apple() && item.unix_mode().is_none_or(|mode| mode & 0o111 == 0)))
        {
            return Err(Error::Invalid("plugin executable is not executable".into()));
        }
        let mut sink = std::io::sink();
        let copied = std::io::copy(&mut item, &mut sink)?;
        if copied > MAX_EXTRACT_BYTES {
            return Err(Error::Invalid("plugin entry exceeds extract limit".into()));
        }
    }
    if names.len() != expected.len() {
        return Err(Error::Invalid(
            "plugin archive is missing declared entries".into(),
        ));
    }
    Ok(())
}

pub fn package_manifest(bytes: &[u8]) -> Result<Manifest> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut index = archive.by_name(crate::workspace::MANIFEST_FILE)?;
    let mut manifest = Vec::new();
    index
        .by_ref()
        .take(crate::workspace::MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest)?;
    if manifest.len() as u64 > crate::workspace::MAX_MANIFEST_BYTES {
        return Err(Error::Invalid("plugin manifest exceeds limit".into()));
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        metadata: Manifest,
    }
    Ok(serde_json::from_slice::<File>(&manifest)?.metadata)
}
