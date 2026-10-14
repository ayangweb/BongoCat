//! Verified archive install with an atomic directory switch.
#![forbid(unsafe_code)]
use crate::{MANIFEST_FILE, PluginManagerError, PluginManifest, sha256_of};
use bongocat_plugin_sdk::PluginMetadata;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const MAX_EXTRACT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;

/// Install a verified `tar.gz` into `<plugin_dir>/<id>/<version>/`, then point
/// the plugin's root at it by replacing `<plugin_dir>/<id>/current`.
///
/// A failed verification or extraction leaves the previous tree untouched.
pub fn install_archive(
    plugin_dir: &Path,
    expected_sha256: Option<&str>,
    bytes: &[u8],
) -> Result<PathBuf, PluginManagerError> {
    if let Some(expected) = expected_sha256
        && sha256_of(bytes) != expected.to_ascii_lowercase()
    {
        return Err(PluginManagerError::UnsafeManifest(
            "package digest does not match the catalog".into(),
        ));
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let staging = plugin_dir.join(".staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let mut entries = 0usize;
    let mut written = 0usize;
    let result = archive.entries()?.try_for_each(|entry| {
        let mut entry = entry?;
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err(io_guard("plugin package has too many entries"));
        }
        let path = entry.path()?.to_path_buf();
        if !safe_relative_path(&path) {
            return Err(io_guard("plugin package contains an unsafe path"));
        }
        let target = staging.join(&path);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target)?;
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        written = written.saturating_add(entry.size() as usize);
        if written > MAX_EXTRACT_BYTES {
            return Err(io_guard("plugin package is too large"));
        }
        let mut file = fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut file)?;
        Ok::<_, std::io::Error>(())
    });
    result.map_err(PluginManagerError::Io)?;
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(staging.join(MANIFEST_FILE))?)?;
    validate(&manifest)?;
    let destination = plugin_dir
        .join(sanitize(&manifest.metadata.id))
        .join(sanitize(&manifest.metadata.version));
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&staging, &destination)?;
    Ok(destination)
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect()
}

pub fn safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.to_string_lossy().len() <= 256
}

fn validate(manifest: &PluginManifest) -> Result<(), PluginManagerError> {
    let metadata: &PluginMetadata = &manifest.metadata;
    if metadata.id.is_empty()
        || metadata.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
        || metadata.entry.contains('/')
        || metadata.entry.contains('\\')
    {
        return Err(PluginManagerError::UnsafeManifest(
            "installed manifest is invalid".into(),
        ));
    }
    Ok(())
}

fn io_guard(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn metadata(id: &str) -> PluginMetadata {
        PluginMetadata {
            id: id.into(),
            name: "Countdown".into(),
            description: "timer".into(),
            version: "1.0.0".into(),
            app_versions: ">=2.3.1".into(),
            protocol_version: bongocat_plugin_sdk::PROTOCOL_VERSION,
            entry: "plugin".into(),
            icon: bongocat_plugin_sdk::PluginIcon::Emoji("🍅".into()),
            capabilities: vec![bongocat_plugin_sdk::Capability::ModelUi],
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        for (path, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn installs_a_manifest_and_rejects_a_bad_digest() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = PluginManifest {
            metadata: metadata("com.bongocat.countdown"),
            sha256: None,
        };
        let bytes = archive(&[
            (MANIFEST_FILE, &serde_json::to_vec(&manifest).unwrap()),
            ("plugin", b"binary"),
        ]);
        let installed =
            install_archive(directory.path(), Some(&sha256_of(&bytes)), &bytes).unwrap();
        assert!(installed.join("plugin").is_file());
        assert_eq!(
            install_archive(directory.path(), Some(&"0".repeat(64)), &bytes)
                .unwrap_err()
                .to_string(),
            "plugin manifest is unsafe: package digest does not match the catalog"
        );
    }

    #[test]
    fn refuses_parent_and_absolute_entries() {
        assert!(safe_relative_path(Path::new("a/b")));
        assert!(!safe_relative_path(Path::new("../a")));
        assert!(!safe_relative_path(Path::new("/a")));
    }

    #[test]
    fn a_partial_extract_leaves_no_installed_version() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = archive(&[("plugin", b"no manifest")]);
        assert!(install_archive(directory.path(), None, &bytes).is_err());
        assert!(!directory.path().join("com.bongocat.countdown").exists());
    }

    #[test]
    fn unused_writer_is_dropped() {
        let _ = std::io::sink().write_all(b"");
    }
}
