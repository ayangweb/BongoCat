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
    fs,
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

/// The most one package may expand to, unpacked.
pub const MAX_EXTRACT_BYTES: usize = 64 * 1024 * 1024;
/// The most files one package may contain.
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;

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
    entry.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
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
    if let Some(expected) = expected_sha256
        && sha256_of(bytes) != expected.to_ascii_lowercase()
    {
        return Err(PluginManagerError::UnsafeManifest(
            "package digest does not match the catalog".into(),
        ));
    }
    let packages = root.join(PACKAGES_DIRECTORY);
    fs::create_dir_all(&packages)?;
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(&packages)?;
    extract_zip(bytes, staging.path())?;
    let manifest: PluginManifest =
        serde_json::from_slice(&fs::read(staging.path().join(MANIFEST_FILE))?)?;
    validate(&manifest)?;
    let id = sanitize(&manifest.metadata.id);
    let version = sanitize(&manifest.metadata.version);
    if id.is_empty() || version.is_empty() {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin id or version is not a usable directory name".into(),
        ));
    }
    let package_root = packages.join(&id);
    let destination = package_root.join(&version);
    fs::create_dir_all(&package_root)?;
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
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }
        if replacing {
            fs::rename(&old_tree, &destination)?;
        }
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
    let version = fs::read_to_string(package_root.join(CURRENT_FILE)).ok()?;
    let version = version.trim();
    if version.is_empty()
        || !safe_relative_path(Path::new(version))
        || version.contains(['/', '\\'])
    {
        return None;
    }
    let directory = package_root.join(version);
    directory.is_dir().then_some(directory)
}

pub(crate) fn write_current(package_root: &Path, version: &str) -> Result<(), PluginManagerError> {
    use std::io::Write as _;
    let mut file =
        atomic_write_file::AtomicWriteFile::options().open(package_root.join(CURRENT_FILE))?;
    file.write_all(version.as_bytes())?;
    file.commit()?;
    Ok(())
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<(), PluginManagerError> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin package has too many entries".into(),
        ));
    }
    let mut written = 0usize;
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
        if !safe_relative_path(&path) {
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
        written = written.saturating_add(entry.size() as usize);
        if written > MAX_EXTRACT_BYTES {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package is too large".into(),
            ));
        }
        let mut file = fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut file)?;
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
    if id.is_empty() || id == "." || id == ".." || id.len() > 128 || sanitize(id) != id {
        return Err(PluginManagerError::UnsafeManifest(
            "invalid plugin id".into(),
        ));
    }
    Ok(())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect()
}

/// Whether a package entry is a plain relative path of bounded length.
pub fn safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.to_string_lossy().len() <= 256
}

fn validate(manifest: &PluginManifest) -> Result<(), PluginManagerError> {
    let metadata: &PluginMetadata = &manifest.metadata;
    validate_id(&metadata.id)?;
    if metadata.id.is_empty()
        || metadata.id == "."
        || metadata.id == ".."
        || sanitize(&metadata.id) != metadata.id
        || cargo_packager_updater::semver::Version::parse(&metadata.version).is_err()
        || cargo_packager_updater::semver::VersionReq::parse(&metadata.app_versions).is_err()
        || metadata.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
        || metadata.entry.is_empty()
        || metadata.entry == "."
        || metadata.entry == ".."
        || sanitize(&metadata.entry) != metadata.entry
        || metadata.entry.contains('/')
        || metadata.entry.contains('\\')
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
