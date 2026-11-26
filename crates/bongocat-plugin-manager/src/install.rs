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
use bongocat_plugin_sdk::{PluginIcon, PluginMetadata};
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
    expected_sha256: &str,
    bytes: &[u8],
) -> Result<InstalledPackage, PluginManagerError> {
    inspect_package(bytes)?;
    if sha256_of(bytes) != expected_sha256.to_ascii_lowercase() {
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
    // Both are what the product runs or draws, so a package that lacks either is
    // refused here rather than discovered as a broken card after install.
    let nonempty_file = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    };
    if let PluginIcon::LocalImage(relative) = &manifest.metadata.icon
        && !nonempty_file(&staging.path().join(relative))
    {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin icon is missing".into(),
        ));
    }
    if !nonempty_file(&crate::loader::entry_path(
        staging.path(),
        &manifest.metadata.entry,
    )) {
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
pub fn selected_root(root: &Path, id: &str) -> Option<PathBuf> {
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
        // The checks run on the name the archive declares, not on a native path
        // rebuilt from it: Windows renders `resources/icon.png` as
        // `resources\icon.png`, so a rebuilt path reads the host's own
        // separator as the escape the archive asked for.
        let Some(path) = entry
            .name()
            .ok()
            .and_then(|name| entry_relative_path(&name))
        else {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin package contains an unsafe path".into(),
            ));
        };
        if !paths.insert(deduplication_key(&path)) || entry.is_symlink() {
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

fn valid_key(value: &str) -> bool {
    value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Whether a string is copy a card can show: present, printable and bounded.
///
/// The center draws a plugin's name and description verbatim, so a manifest that
/// carries control characters or an unbounded blurb is refused before it can
/// become a card.
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_COPY_LENGTH
        && !value.chars().any(char::is_control)
}

/// The longest name or description one locale may publish, in bytes.
const MAX_COPY_LENGTH: usize = 1024;

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

/// The relative path an archive entry names, validated before it becomes native.
///
/// A ZIP stores entry names with `/` separators, and that spelling is the only
/// portable one. Validating a path the host already rendered would reject every
/// nested entry on Windows — where `resources/icon.png` reads back as
/// `resources\icon.png` — while that same separator is exactly what makes a name
/// an escape attempt on Unix. So the check runs on the declared name and the
/// native path is built from the segments only after it passes.
fn entry_relative_path(name: &str) -> Option<PathBuf> {
    // A trailing `/` marks a directory entry; every other empty segment is a
    // name the filesystem would have to interpret.
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty()
        || name.len() > 256
        || name.contains(['\\', ':', '\0'])
        || name.starts_with('/')
    {
        return None;
    }
    let mut path = PathBuf::new();
    for segment in name.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") {
            return None;
        }
        path.push(segment);
    }
    Some(path)
}

/// How one entry name is compared against another for duplication.
///
/// Case-insensitive because the package has to unpack on a case-insensitive
/// filesystem too, and joined with `/` so two entries that only differ by the
/// host's separator still collide.
fn deduplication_key(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .to_ascii_lowercase()
}

pub(crate) fn validate(manifest: &PluginManifest) -> Result<(), PluginManagerError> {
    let metadata: &PluginMetadata = &manifest.metadata;
    validate_id(&metadata.id)?;
    if !valid_version(&metadata.version)
        || cargo_packager_updater::semver::VersionReq::parse(&metadata.app_versions).is_err()
        || metadata.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
        || !valid_basename(&metadata.entry)
        || !valid_text(&metadata.name)
        || !valid_text(&metadata.description)
        || !metadata.localizations.iter().all(|(tag, copy)| {
            valid_key(tag)
                && valid_text(&copy.name)
                && valid_text(&copy.description)
                && copy.name.len() <= MAX_COPY_LENGTH
                && copy.description.len() <= MAX_COPY_LENGTH
        })
    {
        return Err(PluginManagerError::UnsafeManifest(
            "installed manifest is invalid".into(),
        ));
    }
    if let PluginIcon::LocalImage(path) = &metadata.icon
        && !safe_relative_path(Path::new(path))
    {
        return Err(PluginManagerError::UnsafeManifest(
            "plugin icon path is unsafe".into(),
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

    /// A catalog digest is what makes a package the one the catalog offered, so
    /// every install names the digest of the bytes it installs.
    fn digest(bytes: &[u8]) -> String {
        sha256_of(bytes)
    }

    /// The archive name of the executable, spelled the way the installer looks
    /// for it on this host.
    ///
    /// `metadata.entry` names the plugin, not its file: the loader resolves
    /// `plugin.exe` on Windows and `plugin` elsewhere. A fixture that wrote the
    /// bare entry produced a package whose executable the installer could not
    /// find as soon as the suite ran on Windows.
    fn entry_name() -> String {
        crate::loader::entry_path(Path::new(""), "plugin")
            .to_string_lossy()
            .into_owned()
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
        };
        archive(&[
            (MANIFEST_FILE, &serde_json::to_vec(&manifest).unwrap()),
            (entry_name().as_str(), b"binary"),
        ])
    }

    #[test]
    fn installs_a_zip_and_rejects_a_bad_digest() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = package("com.bongocat.pomodoro", "1.0.0");
        let installed = install_package(directory.path(), &digest(&bytes), &bytes).unwrap();
        assert_eq!(installed.version, "1.0.0");
        assert!(installed.root.join(entry_name()).is_file());
        assert!(installed.root.join(MANIFEST_FILE).is_file());
        assert_eq!(
            install_package(directory.path(), &"0".repeat(64), &bytes)
                .unwrap_err()
                .to_string(),
            "plugin manifest is unsafe: package digest does not match the catalog"
        );
    }

    #[test]
    fn installing_a_newer_version_switches_the_pointer_and_keeps_the_old_one() {
        let directory = tempfile::tempdir().unwrap();
        let previous = package("com.bongocat.pomodoro", "1.0.0");
        install_package(directory.path(), &digest(&previous), &previous).unwrap();
        let next = package("com.bongocat.pomodoro", "1.1.0");
        install_package(directory.path(), &digest(&next), &next).unwrap();
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
                .join("com.bongocat.pomodoro/1.0.0")
                .join(entry_name())
                .is_file()
        );
    }

    #[test]
    fn uninstall_removes_the_package_but_keeps_plugin_data() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = package("com.bongocat.pomodoro", "1.0.0");
        install_package(directory.path(), &digest(&bytes), &bytes).unwrap();
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
        assert!(install_package(directory.path(), &digest(&no_manifest), &no_manifest).is_err());
        assert!(selected_root(directory.path(), "com.bongocat.pomodoro").is_none());

        let manifest = PluginManifest {
            metadata: metadata("com.bongocat.pomodoro", "1.0.0"),
        };
        let escaping = archive(&[
            (MANIFEST_FILE, &serde_json::to_vec(&manifest).unwrap()),
            ("../escape", b"x"),
        ]);
        assert!(install_package(directory.path(), &digest(&escaping), &escaping).is_err());
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
    fn archive_entry_names_are_read_in_the_archives_own_spelling() {
        // A ZIP entry name is always `/`-separated. Reading it back through the
        // host path type would turn `resources/icon.png` into
        // `resources\icon.png` on Windows and then reject it as an escape.
        assert_eq!(
            entry_relative_path("resources/icon.png"),
            Some(PathBuf::from("resources").join("icon.png"))
        );
        assert_eq!(entry_relative_path("plugin"), Some(PathBuf::from("plugin")));
        // A directory entry carries its trailing separator; it names the same
        // place as the file entries nested under it, so both spellings have to
        // resolve identically for the duplicate check to catch the pair.
        assert_eq!(
            entry_relative_path("resources/"),
            Some(PathBuf::from("resources"))
        );
        assert_eq!(
            deduplication_key(&entry_relative_path("resources/icon.png").unwrap()),
            deduplication_key(&entry_relative_path("resources/Icon.PNG/").unwrap()),
        );
        for unsafe_name in [
            "",
            "/",
            "../escape",
            "resources/../escape",
            "resources//icon.png",
            "resources\\icon.png",
            "file:stream",
            "C:/icon.png",
            "\\icon.png",
        ] {
            assert_eq!(
                entry_relative_path(unsafe_name),
                None,
                "{unsafe_name:?} should not be a safe entry name"
            );
        }
    }

    #[test]
    fn nested_entries_install_on_every_host() {
        // The archive below is what a published package looks like: a manifest,
        // an executable and a nested icon. It has to install on the host running
        // this test, whichever separator that host spells paths with.
        let directory = tempfile::tempdir().unwrap();
        let mut local = metadata("com.bongocat.pomodoro", "1.0.0");
        local.icon = PluginIcon::LocalImage("resources/icon.png".into());
        let manifest = serde_json::to_vec(&PluginManifest { metadata: local }).unwrap();
        let bytes = archive(&[
            (MANIFEST_FILE, &manifest),
            (entry_name().as_str(), b"binary"),
            ("resources/icon.png", b"png"),
        ]);
        let installed = install_package(directory.path(), &digest(&bytes), &bytes).unwrap();
        assert!(installed.root.join("resources").join("icon.png").is_file());
    }

    #[test]
    fn local_icon_paths_are_validated_against_the_package() {
        let directory = tempfile::tempdir().unwrap();
        let mut local = metadata("com.bongocat.pomodoro", "1.0.0");
        local.icon = PluginIcon::LocalImage("resources/icon.png".into());
        let manifest = serde_json::to_vec(&PluginManifest {
            metadata: local.clone(),
        })
        .unwrap();
        let bytes = archive(&[
            (MANIFEST_FILE, &manifest),
            (entry_name().as_str(), b"binary"),
            ("resources/icon.png", b"png"),
        ]);
        install_package(directory.path(), &digest(&bytes), &bytes).unwrap();

        for unsafe_path in ["../icon.png", "/icon.png", "resources\\icon.png"] {
            let mut invalid = local.clone();
            invalid.icon = PluginIcon::LocalImage(unsafe_path.into());
            assert!(validate(&PluginManifest { metadata: invalid }).is_err());
        }
    }

    #[test]
    fn a_manifest_with_a_removed_icon_fallback_is_rejected() {
        let mut manifest = serde_json::to_value(PluginManifest {
            metadata: metadata("com.bongocat.pomodoro", "1.0.0"),
        })
        .unwrap();
        manifest["metadata"]["icon_fallback"] =
            serde_json::json!({ "type": "emoji", "value": "🍅" });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let archive = archive(&[(MANIFEST_FILE, &bytes), (entry_name().as_str(), b"binary")]);
        assert!(matches!(
            inspect_package(&archive),
            Err(PluginManagerError::Manifest(_))
        ));
    }

    #[test]
    fn build_metadata_is_preserved_in_the_selected_version() {
        let directory = tempfile::tempdir().unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        let bytes = package("com.bongocat.pomodoro", "1.0.0+build.1");
        manager.install(&bytes, &digest(&bytes)).unwrap();
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
        let first = package(id, "1.0.0");
        install_package(directory.path(), &digest(&first), &first).unwrap();
        let manifest = serde_json::to_vec(&PluginManifest {
            metadata: metadata(id, "1.1.0"),
        })
        .unwrap();
        for bytes in [
            archive(&[(MANIFEST_FILE, &manifest)]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                (entry_name().as_str(), b"binary"),
                // The same entry in another case: the installer compares names
                // case-insensitively, so this is a duplicate, not a second file.
                (entry_name().to_uppercase().as_str(), b"other"),
            ]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                (entry_name().as_str(), b"binary"),
                ("..\\outside", b"other"),
            ]),
            archive(&[
                (MANIFEST_FILE, &manifest),
                (entry_name().as_str(), b"binary"),
                ("assets/file", b"one"),
                ("assets//file", b"two"),
            ]),
        ] {
            assert!(install_package(directory.path(), &digest(&bytes), &bytes).is_err());
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
        let symbolic_link = writer.finish().unwrap().into_inner();
        assert!(
            install_package(directory.path(), &digest(&symbolic_link), &symbolic_link).is_err()
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
        let healthy = package(good, "1.0.0");
        install_package(directory.path(), &digest(&healthy), &healthy).unwrap();
        let broken = package(bad, "1.0.0");
        let broken = install_package(directory.path(), &digest(&broken), &broken).unwrap();
        let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
        for content in [
            b"not JSON".to_vec(),
            serde_json::to_vec(&PluginManifest {
                metadata: metadata(good, "1.0.0"),
            })
            .unwrap(),
            serde_json::to_vec(&PluginManifest {
                metadata: metadata(bad, "2.0.0"),
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

        let bytes = package("com.bongocat.pomodoro", "1.0.0");
        manager.install(&bytes, &digest(&bytes)).unwrap();
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
