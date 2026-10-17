//! Verified ZIP install with an atomic directory switch.
//!
//! A plugin ships as one ZIP. Installing it unpacks the archive into a staging
//! directory, validates the manifest, moves the tree under its own versioned
//! directory and only then points the package's `current` file at it. A failure
//! at any step leaves the previously selected version selected, so a broken
//! download is never what the product runs next.
#![forbid(unsafe_code)]
use crate::{
    CURRENT_FILE, MANIFEST_FILE, PACKAGES_DIRECTORY, PluginManagerError, PluginManifest, sha256_of,
};
use bongocat_plugin_sdk::PluginMetadata;
use std::{
    collections::BTreeSet,
    fs,
    io::Read as _,
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

/// The most one package may expand to, unpacked.
pub const MAX_EXTRACT_BYTES: usize = 64 * 1024 * 1024;
/// The most files one package may contain.
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The installed version of one plugin: where its files are and what version
/// the `current` pointer selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPackage {
    pub id: String,
    pub version: String,
    pub root: PathBuf,
}

/// Read bounded metadata without executing or extracting package code.
pub fn inspect_package(bytes: &[u8]) -> Result<PluginManifest, PluginManagerError> {
    use std::io::Read;
    if bytes.len() > crate::catalog::MAX_DOWNLOAD_BYTES {
        return Err(PluginManagerError::UnsafeManifest(
            "package exceeds download limit".into(),
        ));
    }
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
    let entry = archive.by_name(MANIFEST_FILE)?;
    let mut bytes = Vec::new();
    entry.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MANIFEST_BYTES as usize {
        return Err(PluginManagerError::UnsafeManifest(
            "manifest exceeds limit".into(),
        ));
    }
    let manifest: PluginManifest = serde_json::from_slice(&bytes)?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Install a verified plugin ZIP under `<root>/packages/<id>/<version>/`, then
/// select it by writing `<root>/packages/<id>/current`.
///
/// A failed digest check, unsafe path, unreadable manifest or oversized archive
/// leaves whatever version was already selected in place.
pub fn install_package(
    root: &Path,
    expected_sha256: Option<&str>,
    bytes: &[u8],
) -> Result<InstalledPackage, PluginManagerError> {
    inspect_package(bytes)?;
    if let Some(expected) = expected_sha256
        && sha256_of(bytes) != expected.to_ascii_lowercase()
    {
        return Err(PluginManagerError::UnsafeManifest(
            "package digest does not match the catalog".into(),
        ));
    }
    let packages = root.join(PACKAGES_DIRECTORY);
    ensure_directory(&packages)?;
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(&packages)?;
    extract_zip(bytes, staging.path())?;
    let manifest = read_manifest(&staging.path().join(MANIFEST_FILE))?;
    let id = manifest.metadata.id;
    let version = manifest.metadata.version;
    if !fs::symlink_metadata(crate::loader::entry_path(
        staging.path(),
        &manifest.metadata.entry,
    ))
    .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin executable is missing".into(),
        ));
    }
    let package_root = packages.join(&id);
    let destination = package_root.join(&version);
    ensure_directory(&package_root)?;
    // Retain the previous complete tree until the pointer has committed. Neither
    // extraction failure nor a failed pointer write may destroy the working copy.
    let backup = tempfile::Builder::new()
        .prefix(".previous-")
        .tempdir_in(&package_root)?;
    let old_tree = backup.path().join("package");
    let replacing = destination.exists();
    if replacing {
        fs::rename(&destination, &old_tree)?;
    }
    let commit = (|| -> Result<(), PluginManagerError> {
        fs::rename(staging.path(), &destination)?;
        write_current(&package_root, &version)
    })();
    if let Err(error) = commit {
        // A second IO failure during rollback must not let TempDir delete the
        // last complete package tree. Keep it available for recovery instead.
        let backup = backup.keep();
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }
        if replacing {
            fs::rename(&old_tree, &destination)?;
        }
        fs::remove_dir_all(backup)?;
        return Err(error);
    }
    Ok(InstalledPackage {
        id,
        version,
        root: destination,
    })
}

/// Remove a plugin's installed package tree.
///
/// Plugin-owned data lives under a separate root and is deliberately kept: an
/// uninstall removes code, not the user's configuration. [`reset_plugin_data`]
/// is the explicit way to drop that too.
///
/// Returns whether anything was removed.
pub fn uninstall_package(root: &Path, id: &str) -> Result<bool, PluginManagerError> {
    validate_id(id)?;
    let package_root = root.join(PACKAGES_DIRECTORY).join(id);
    if !package_root.is_dir() {
        return Ok(false);
    }
    fs::remove_dir_all(&package_root)?;
    Ok(true)
}

/// Drop a plugin's persisted data. The executable tree is untouched.
pub fn reset_plugin_data(root: &Path, id: &str) -> Result<bool, PluginManagerError> {
    validate_id(id)?;
    let data_root = root.join(crate::DATA_DIRECTORY).join(id);
    if !data_root.is_dir() {
        return Ok(false);
    }
    fs::remove_dir_all(&data_root)?;
    Ok(true)
}

/// Where a plugin's persisted data is rooted, creating it when asked.
pub(crate) fn plugin_data_root(
    root: &Path,
    id: &str,
    create: bool,
) -> Result<PathBuf, PluginManagerError> {
    validate_id(id)?;
    let data_root = root.join(crate::DATA_DIRECTORY).join(id);
    if create {
        fs::create_dir_all(&data_root)?;
    }
    Ok(data_root)
}

/// The directory of the currently selected version of a plugin, if the pointer
/// and its directory are both present.
pub(crate) fn selected_root(root: &Path, id: &str) -> Option<PathBuf> {
    validate_id(id).ok()?;
    let package_root = root.join(PACKAGES_DIRECTORY).join(id);
    if !plain_directory(&root.join(PACKAGES_DIRECTORY)) || !plain_directory(&package_root) {
        return None;
    }
    let version =
        String::from_utf8(read_bounded(&package_root.join(CURRENT_FILE), 256).ok()?).ok()?;
    let version = version.trim();
    if !valid_version(version) {
        return None;
    }
    let directory = package_root.join(version);
    plain_directory(&directory).then_some(directory)
}

pub(crate) fn write_current(package_root: &Path, version: &str) -> Result<(), PluginManagerError> {
    use std::io::Write as _;
    let mut file =
        atomic_write_file::AtomicWriteFile::options().open(package_root.join(CURRENT_FILE))?;
    file.write_all(version.as_bytes())?;
    file.commit()?;
    Ok(())
}

pub(crate) fn read_manifest(path: &Path) -> Result<PluginManifest, PluginManagerError> {
    let bytes = read_bounded(path, MAX_MANIFEST_BYTES as usize)?;
    let manifest = serde_json::from_slice(&bytes)?;
    validate(&manifest)?;
    Ok(manifest)
}

pub(crate) fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(std::io::Error::other("plugin file is not a regular file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("plugin file exceeds limit"));
    }
    Ok(bytes)
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<(), PluginManagerError> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin package has too many entries".into(),
        ));
    }
    let mut written = 0usize;
    let mut paths = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        // `enclosed_name` already rejects absolute paths and `..`; the stricter
        // check below rejects any non-normal component and a name long enough to
        // be a path-length attack on its own.
        let Some(path) = entry.enclosed_name() else {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package contains an unsafe path".into(),
            ));
        };
        let normalized: PathBuf = path.components().collect();
        if !safe_relative_path(&path)
            || !paths.insert(normalized.to_string_lossy().to_ascii_lowercase())
            || entry.is_symlink()
        {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package contains an unsafe path".into(),
            ));
        }
        let target = destination.join(&path);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if entry.size() > (MAX_EXTRACT_BYTES - written) as u64 {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package is too large".into(),
            ));
        }
        let mut file = fs::File::create(&target)?;
        let remaining = (MAX_EXTRACT_BYTES - written) as u64;
        let copied = std::io::copy(&mut entry.by_ref().take(remaining + 1), &mut file)?;
        if copied > remaining {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package is too large".into(),
            ));
        }
        written += copied as usize;
        // A plugin package is an executable plus its manifest, and the archive
        // is what carries the executable bit: ZIP records a Unix mode, and a
        // package that only worked because the local filesystem already had the
        // bit set would fail the first time it was installed from a download.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), PluginManagerError> {
    if !valid_basename(id) {
        return Err(PluginManagerError::UnsafeManifest(
            "invalid plugin id".into(),
        ));
    }
    Ok(())
}

pub fn valid_basename(value: &str) -> bool {
    bongocat_plugin_sdk::valid_key(value) && !matches!(value, "." | "..")
}

fn valid_version(value: &str) -> bool {
    value.len() <= 128 && cargo_packager_updater::semver::Version::parse(value).is_ok()
}

fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn ensure_directory(path: &Path) -> Result<(), PluginManagerError> {
    fs::create_dir_all(path)?;
    if !plain_directory(path) {
        return Err(PluginManagerError::UnsafeManifest(
            "package directory is not a directory".into(),
        ));
    }
    Ok(())
}

/// Whether a package entry is a plain relative path of bounded length.
pub fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.to_string_lossy().contains(['\\', ':'])
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.to_string_lossy().len() <= 256
}

pub(crate) fn validate(manifest: &PluginManifest) -> Result<(), PluginManagerError> {
    let metadata: &PluginMetadata = &manifest.metadata;
    validate_id(&metadata.id)?;
    if !valid_version(&metadata.version)
        || cargo_packager_updater::semver::VersionReq::parse(&metadata.app_versions).is_err()
        || metadata.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
        || !valid_basename(&metadata.entry)
    {
        return Err(PluginManagerError::UnsafeManifest(
            "installed manifest is invalid".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PluginManager, PluginState};
    use std::io::Write;

    fn metadata(id: &str, version: &str) -> PluginMetadata {
        PluginMetadata {
            id: id.into(),
            name: "Pomodoro".into(),
            description: "timer".into(),
            version: version.into(),
            app_versions: ">=2.3.1".into(),
            protocol_version: bongocat_plugin_sdk::PROTOCOL_VERSION,
            entry: "plugin".into(),
            icon: bongocat_plugin_sdk::PluginIcon::Emoji("🍅".into()),
            capabilities: vec![bongocat_plugin_sdk::Capability::ModelUi],
            localizations: Default::default(),
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (path, bytes) in entries {
            let options = if *path == MANIFEST_FILE {
                options
            } else {
                options.unix_permissions(0o755)
            };
            writer.start_file(*path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn package(id: &str, version: &str) -> Vec<u8> {
        let manifest = PluginManifest {
            metadata: metadata(id, version),
            sha256: None,
        };
        archive(&[
            (MANIFEST_FILE, &serde_json::to_vec(&manifest).unwrap()),
            ("plugin", b"binary"),
        ])
    }

    #[test]
    fn installs_a_zip_and_rejects_a_bad_digest() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = package("com.bongocat.pomodoro", "1.0.0");
        let installed =
            install_package(directory.path(), Some(&sha256_of(&bytes)), &bytes).unwrap();
        assert_eq!(installed.version, "1.0.0");
        assert!(installed.root.join("plugin").is_file());
        assert!(installed.root.join(MANIFEST_FILE).is_file());
        assert_eq!(
            install_package(directory.path(), Some(&"0".repeat(64)), &bytes)
                .unwrap_err()
                .to_string(),
            "plugin manifest is unsafe: package digest does not match the catalog"
        );
    }

    #[test]
    fn installing_a_newer_version_switches_the_pointer_and_keeps_the_old_one() {
        let directory = tempfile::tempdir().unwrap();
        install_package(
            directory.path(),
            None,
            &package("com.bongocat.pomodoro", "1.0.0"),
        )
        .unwrap();
        install_package(
            directory.path(),
            None,
            &package("com.bongocat.pomodoro", "1.1.0"),
        )
        .unwrap();
        assert_eq!(
            selected_root(directory.path(), "com.bongocat.pomodoro")
                .unwrap()
                .file_name()
                .unwrap(),
            "1.1.0"
        );
        // The replaced version is still on disk, so a rollback needs no download.
        assert!(
            directory
                .path()
                .join(PACKAGES_DIRECTORY)
                .join("com.bongocat.pomodoro/1.0.0/plugin")
                .is_file()
        );
    }

    #[test]
    fn uninstall_removes_the_package_but_keeps_plugin_data() {
        let directory = tempfile::tempdir().unwrap();
        install_package(
            directory.path(),
            None,
            &package("com.bongocat.pomodoro", "1.0.0"),
        )
        .unwrap();
        let data = plugin_data_root(directory.path(), "com.bongocat.pomodoro", true).unwrap();
        fs::write(data.join("state.json"), b"{}").unwrap();

        assert!(uninstall_package(directory.path(), "com.bongocat.pomodoro").unwrap());
        assert!(selected_root(directory.path(), "com.bongocat.pomodoro").is_none());
        assert!(data.join("state.json").is_file());

        assert!(reset_plugin_data(directory.path(), "com.bongocat.pomodoro").unwrap());
        assert!(!data.exists());
        // Removing something absent is not an error.
        assert!(!uninstall_package(directory.path(), "com.bongocat.pomodoro").unwrap());
    }

    #[test]
    fn a_partial_or_unsafe_archive_leaves_no_selected_version() {
        let directory = tempfile::tempdir().unwrap();
        let no_manifest = archive(&[("plugin", b"no manifest")]);
        assert!(install_package(directory.path(), None, &no_manifest).is_err());
        assert!(selected_root(directory.path(), "com.bongocat.pomodoro").is_none());

        let manifest = PluginManifest {
            metadata: metadata("com.bongocat.pomodoro", "1.0.0"),
            sha256: None,
        };
        let escaping = archive(&[
            (MANIFEST_FILE, &serde_json::to_vec(&manifest).unwrap()),
            ("../escape", b"x"),
        ]);
        assert!(install_package(directory.path(), None, &escaping).is_err());
    }

    #[test]
    fn refuses_parent_and_absolute_entries() {
        assert!(safe_relative_path(Path::new("a/b")));
        assert!(!safe_relative_path(Path::new("../a")));
        assert!(!safe_relative_path(Path::new("/a")));
        assert!(!safe_relative_path(Path::new("")));
        assert!(!safe_relative_path(Path::new("a\\..\\outside")));
        assert!(!safe_relative_path(Path::new("file:stream")));
    }

    #[test]
    fn build_metadata_is_preserved_in_the_selected_version() {
        let directory = tempfile::tempdir().unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        manager
            .install(&package("com.bongocat.pomodoro", "1.0.0+build.1"))
            .unwrap();
        assert_eq!(
            manager
                .installed_version("com.bongocat.pomodoro")
                .as_deref(),
            Some("1.0.0+build.1")
        );
        assert_eq!(manager.discover_plugins().unwrap(), 1);
    }

    #[test]
    fn a_corrupt_store_is_not_reported_as_an_empty_catalog() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join(PACKAGES_DIRECTORY),
            b"not a directory",
        )
        .unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        assert!(manager.discover_plugins().is_err());
    }

    #[test]
    fn invalid_replacements_leave_the_previous_package_selected() {
        let directory = tempfile::tempdir().unwrap();
        let id = "com.bongocat.pomodoro";
        install_package(directory.path(), None, &package(id, "1.0.0")).unwrap();
        let manifest = serde_json::to_vec(&PluginManifest {
            metadata: metadata(id, "1.1.0"),
            sha256: None,
        })
        .unwrap();
        for bytes in [
            archive(&[(MANIFEST_FILE, &manifest)]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                ("plugin", b"binary"),
                ("PLUGIN", b"other"),
            ]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                ("plugin", b"binary"),
                ("..\\outside", b"other"),
            ]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                ("plugin", b"binary"),
                ("assets/file", b"one"),
                ("assets//file", b"two"),
            ]),
        ] {
            assert!(install_package(directory.path(), None, &bytes).is_err());
            assert!(
                selected_root(directory.path(), id)
                    .unwrap()
                    .ends_with("1.0.0")
            );
        }
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file(MANIFEST_FILE, options).unwrap();
        writer.write_all(&manifest).unwrap();
        writer.add_symlink("plugin", "outside", options).unwrap();
        assert!(
            install_package(
                directory.path(),
                None,
                &writer.finish().unwrap().into_inner()
            )
            .is_err()
        );
        assert!(
            selected_root(directory.path(), id)
                .unwrap()
                .ends_with("1.0.0")
        );
    }

    #[test]
    fn corrupt_or_mismatched_packages_do_not_hide_healthy_plugins() {
        let directory = tempfile::tempdir().unwrap();
        let good = "com.bongocat.healthy";
        let bad = "com.bongocat.broken";
        install_package(directory.path(), None, &package(good, "1.0.0")).unwrap();
        let broken = install_package(directory.path(), None, &package(bad, "1.0.0")).unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        for content in [
            b"not JSON".to_vec(),
            serde_json::to_vec(&PluginManifest {
                metadata: metadata(good, "1.0.0"),
                sha256: None,
            })
            .unwrap(),
            serde_json::to_vec(&PluginManifest {
                metadata: metadata(bad, "2.0.0"),
                sha256: None,
            })
            .unwrap(),
        ] {
            fs::write(broken.root.join(MANIFEST_FILE), content).unwrap();
            assert!(manager.discover_plugins().is_err());
            assert_eq!(manager.list_plugins().len(), 1);
            assert_eq!(manager.list_plugins()[0].0.id, good);
        }
    }

    #[test]
    fn an_installed_package_is_discoverable_and_uninstallable_through_the_manager() {
        let directory = tempfile::tempdir().unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        assert_eq!(manager.discover_plugins().unwrap(), 0);

        manager
            .install(&package("com.bongocat.pomodoro", "1.0.0"))
            .unwrap();
        assert_eq!(manager.discover_plugins().unwrap(), 1);
        assert_eq!(manager.list_plugins()[0].1, PluginState::Discovered);
        assert_eq!(
            manager
                .installed_version("com.bongocat.pomodoro")
                .as_deref(),
            Some("1.0.0")
        );

        assert!(manager.uninstall("com.bongocat.pomodoro").unwrap());
        assert_eq!(manager.discover_plugins().unwrap(), 0);
        assert!(manager.list_plugins().is_empty());
    }
}
