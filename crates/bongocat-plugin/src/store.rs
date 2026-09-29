//! Where plugins live on disk, and what happens to them.
//!
//! One directory per plugin id, one subdirectory per installed version, and a
//! `current` file naming the version that is live. That shape is what makes an
//! update atomic without a staging area: a new version is unpacked beside the old
//! one, verified, and only then does `current` change. An interrupted update
//! leaves the old version installed and the new one as an orphan directory the
//! next load ignores — which is why a download being cut off cannot leave a
//! plugin half-installed.
//!
//! Every path a plugin's own files can name is checked before it is joined to
//! anything, and every path *this* file builds comes from a validated id. There is
//! no case where a plugin's contents decide where they are written.

use bongocat_plugin_protocol::{
    InstalledPlugin, PLUGIN_MANIFEST_FILE_NAME, PluginError, PluginErrorCode, PluginId,
    PluginManifest, PluginVersion,
};
use bongocat_storage::create_private_dir_all;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The file naming which installed version is live.
pub const CURRENT_VERSION_FILE: &str = "current";

/// The most versions one plugin may keep.
///
/// One live version is what a plugin needs. A small number of old ones are kept
/// because a version that turns out to be broken should be recoverable without
/// another download, and because an update that has to be rolled back should not
/// require the network. Beyond a couple, the directory is just disk.
pub const MAXIMUM_RETAINED_VERSIONS: usize = 3;

/// The plugins this build's storage root holds.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginStore {
    root: PathBuf,
}

impl PluginStore {
    /// A store rooted at `root`, which is the `plugins` directory of a
    /// [`bongocat_config::StorageLayout`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create the store's own directory, and the lock file that guards it.
    pub fn create(&self) -> Result<(), PluginError> {
        create_private_dir_all(&self.root).map_err(store_error)
    }

    fn plugin_root(&self, id: &PluginId) -> PathBuf {
        self.root.join(id.as_str())
    }

    fn version_root(&self, id: &PluginId, version: &PluginVersion) -> PathBuf {
        self.plugin_root(id).join(version.to_string())
    }

    /// The directory an installed version lives in, when it is installed.
    pub fn version_directory(&self, id: &PluginId, version: &PluginVersion) -> Option<PathBuf> {
        let directory = self.version_root(id, version);
        directory
            .join(PLUGIN_MANIFEST_FILE_NAME)
            .is_file()
            .then_some(directory)
    }

    /// The version that is live for a plugin, if any.
    pub fn current_version(&self, id: &PluginId) -> Option<PluginVersion> {
        let recorded = fs::read_to_string(self.plugin_root(id).join(CURRENT_VERSION_FILE)).ok()?;
        let version = PluginVersion::parse(recorded.trim())?;
        self.version_directory(id, &version)?;
        Some(version)
    }

    /// Every installed plugin, in id order.
    ///
    /// Sorted so the plugin centre's list is stable between runs: a store read
    /// from a directory is not ordered, and a list that reorders itself on every
    /// refresh is a list nobody can find anything in.
    pub fn installed(&self) -> Vec<InstalledPlugin> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut installed = Vec::new();
        for entry in entries.flatten() {
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| PluginId::new(name).ok())
            else {
                // A directory the store did not put there is not a plugin. A
                // partially unpacked archive can leave one, and an interrupted
                // delete another; neither is a reason to fail the whole listing.
                continue;
            };
            let Some(version) = self.current_version(&id) else {
                continue;
            };
            let Some(directory) = self.version_directory(&id, &version) else {
                continue;
            };
            installed.push(InstalledPlugin {
                id,
                version,
                directory,
                enabled: true,
            });
        }
        installed.sort_by(|left, right| left.id.cmp(&right.id));
        installed
    }

    /// Read and validate the manifest of an installed version.
    pub fn manifest(&self, installed: &InstalledPlugin) -> Result<PluginManifest, PluginError> {
        let bytes = fs::read(installed.manifest_path()).map_err(|error| {
            PluginError::with_detail(PluginErrorCode::PluginDirectoryUnreadable, error)
        })?;
        PluginManifest::parse(&bytes)
    }

    /// Unpack a verified archive into a new version's directory.
    ///
    /// The archive's members are checked as they are read rather than after: a
    /// member named `../../something` is refused before its bytes reach the disk,
    /// so a hostile archive cannot write outside the store even if its
    /// declaration is never looked at.
    ///
    /// The unpacked directory is only *visible* once [`Self::set_current`] runs, so
    /// a failure here leaves the previously installed version in place.
    pub fn unpack(
        &self,
        id: &PluginId,
        version: &PluginVersion,
        archive: &[u8],
    ) -> Result<PathBuf, PluginError> {
        self.create()?;
        let directory = self.version_root(id, version);
        if directory.join(PLUGIN_MANIFEST_FILE_NAME).is_file() {
            return Err(PluginError::new(PluginErrorCode::AlreadyInstalled));
        }
        // A leftover directory from an interrupted unpack is removed rather than
        // reused: its contents cannot be trusted, and merging into them is how a
        // previous partial write becomes permanent.
        if directory.exists() {
            fs::remove_dir_all(&directory).map_err(store_error)?;
        }
        create_private_dir_all(&directory).map_err(store_error)?;
        match unpack_archive(archive, &directory) {
            Ok(()) => {}
            Err(error) => {
                // A half-unpacked directory is worse than none: it is a version
                // whose manifest may be absent, which `installed` would skip but
                // a future `version_directory` call would find.
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
        }
        let manifest_path = directory.join(PLUGIN_MANIFEST_FILE_NAME);
        if !manifest_path.is_file() {
            let _ = fs::remove_dir_all(&directory);
            return Err(PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                "the archive has no plugin.json",
            ));
        }
        let manifest = PluginManifest::parse(&fs::read(&manifest_path).map_err(store_error)?)?;
        if manifest.id != *id {
            let _ = fs::remove_dir_all(&directory);
            return Err(PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                "the archive declares a different id",
            ));
        }
        Ok(directory)
    }

    /// Make an unpacked version the live one, and drop the ones beyond the bound.
    pub fn set_current(&self, id: &PluginId, version: &PluginVersion) -> Result<(), PluginError> {
        if self.version_directory(id, version).is_none() {
            return Err(PluginError::new(PluginErrorCode::NotInstalled));
        }
        let plugin_root = self.plugin_root(id);
        create_private_dir_all(&plugin_root).map_err(store_error)?;
        bongocat_storage::write_private_atomic(
            &plugin_root.join(CURRENT_VERSION_FILE),
            version.to_string().as_bytes(),
        )
        .map_err(store_error)?;
        self.prune(id, version);
        Ok(())
    }

    /// Remove a plugin and everything it installed.
    pub fn uninstall(&self, id: &PluginId) -> Result<(), PluginError> {
        let root = self.plugin_root(id);
        if !root.exists() {
            return Err(PluginError::new(PluginErrorCode::NotInstalled));
        }
        fs::remove_dir_all(&root).map_err(store_error)
    }

    /// Keep the live version and the newest few of the rest.
    fn prune(&self, id: &PluginId, current: &PluginVersion) {
        let Ok(entries) = fs::read_dir(self.plugin_root(id)) else {
            return;
        };
        let mut versions: Vec<PluginVersion> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().and_then(PluginVersion::parse))
            .filter(|version| version != current)
            .collect();
        // Newest first, so the bound keeps the most recent old versions.
        versions.sort();
        versions.reverse();
        for stale in versions.into_iter().skip(MAXIMUM_RETAINED_VERSIONS - 1) {
            let _ = fs::remove_dir_all(self.version_root(id, &stale));
        }
    }
}

fn store_error(error: io::Error) -> PluginError {
    PluginError::with_detail(PluginErrorCode::StoreWriteFailed, error)
}

/// The most members one archive may contain.
const MAXIMUM_ARCHIVE_MEMBERS: usize = 256;

/// The most bytes one archive may expand to.
const MAXIMUM_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;

/// The most bytes one member may be.
const MAXIMUM_MEMBER_BYTES: u64 = 4 * 1024 * 1024;

/// Unpack a zip into `directory`, refusing anything that could escape it.
fn unpack_archive(archive: &[u8], directory: &Path) -> Result<(), PluginError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .map_err(|error| PluginError::with_detail(PluginErrorCode::ArchiveInvalid, error))?;
    if zip.len() > MAXIMUM_ARCHIVE_MEMBERS {
        return Err(PluginError::with_detail(
            PluginErrorCode::ArchiveInvalid,
            "the archive has too many members",
        ));
    }
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut member = zip
            .by_index(index)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::ArchiveInvalid, error))?;
        let Some(name) = member.enclosed_name() else {
            return Err(PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                "a member name is not a plain relative path",
            ));
        };
        // `enclosed_name` already refuses `..` and absolute paths, but the
        // protocol's own rule is stricter — it also refuses a backslash and a
        // colon, which is what stops a Windows path from being read as a plain
        // relative one.
        let relative = name.to_str().ok_or_else(|| {
            PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                "a member name is not UTF-8",
            )
        })?;
        bongocat_plugin_protocol::validate_relative_asset_path(relative)?;
        if member.is_dir() {
            continue;
        }
        let size = member.size();
        if size > MAXIMUM_MEMBER_BYTES {
            return Err(PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                format!("{relative} is too large"),
            ));
        }
        total = total.saturating_add(size);
        if total > MAXIMUM_ARCHIVE_BYTES {
            return Err(PluginError::with_detail(
                PluginErrorCode::ArchiveInvalid,
                "the archive expands to more than the bound",
            ));
        }
        let target = directory.join(relative);
        if let Some(parent) = target.parent() {
            create_private_dir_all(parent).map_err(store_error)?;
        }
        let mut bytes = Vec::with_capacity(size.min(64 * 1024) as usize);
        std::io::Read::read_to_end(&mut member, &mut bytes)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::ArchiveInvalid, error))?;
        bongocat_storage::write_private_atomic(&target, &bytes).map_err(store_error)?;
    }
    Ok(())
}

/// The digest a catalog entry announces, as lower-case hex.
pub fn digest_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Compare a downloaded archive with the digest the catalog announced.
///
/// Constant-time on the comparison, which does not matter for a public digest but
/// does mean the check has no timing shape a future change could accidentally
/// give it.
pub fn digest_matches(expected: &str, bytes: &[u8]) -> bool {
    let actual = digest_hex(bytes);
    if actual.len() != expected.len() {
        return false;
    }
    actual
        .bytes()
        .zip(expected.bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

/// Verify a detached Minisign signature over `bytes`.
///
/// `key` is the compiled-in release public key, the same one the updater checks
/// payloads against. An absent or blank key fails closed: a build with no
/// provisioned key cannot install anything, which is the same rule the update
/// pipeline follows and for the same reason.
pub fn verify_signature(
    key: Option<&str>,
    bytes: &[u8],
    signature: &str,
) -> Result<(), PluginError> {
    let key = key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| PluginError::new(PluginErrorCode::SignatureKeyMissing))?;
    if signature.trim().is_empty() {
        return Err(PluginError::new(PluginErrorCode::SignatureInvalid));
    }
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::SignatureInvalid, error))?;
    let key = minisign_verify::PublicKey::decode(key)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::SignatureInvalid, error))?;
    key.verify(bytes, &signature, false)
        .map_err(|error| PluginError::with_detail(PluginErrorCode::SignatureInvalid, error))
}
