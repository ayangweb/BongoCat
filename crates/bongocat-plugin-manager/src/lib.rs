//! Discovery and lifecycle management for independently distributed plugins.
#![forbid(unsafe_code)]
mod catalog;
mod loader;
mod registry;
mod source;
use bongocat_plugin_sdk::PluginMetadata;
pub use catalog::{CATALOG_NAME, Catalog, CatalogEntry, CatalogError, parse_catalog, sha256_of};
pub use registry::{PluginRegistry, PluginState, RegistryError};
use serde::{Deserialize, Serialize};
pub use source::{AvailablePlugin, PluginSource, is_newer};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const MANIFEST_FILE: &str = "plugin.json";
/// The directory under a plugin store root that holds installed package trees.
pub const PACKAGES_DIRECTORY: &str = "packages";
/// The directory under a plugin store root that holds plugin-owned data.
pub const DATA_DIRECTORY: &str = "data";
/// The file inside `<root>/packages/<id>/` naming the selected version.
pub const CURRENT_FILE: &str = "current";
#[derive(Debug, Error)]
pub enum PluginManagerError {
    #[error("plugin manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("plugin IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("plugin registry error: {0}")]
    Registry(#[from] RegistryError),
    #[error("plugin manifest is unsafe: {0}")]
    UnsafeManifest(String),
    #[error("plugin package is not a readable ZIP archive")]
    Archive(#[from] zip::result::ZipError),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub metadata: PluginMetadata,
    pub sha256: Option<String>,
}

pub struct PluginManager {
    registry: PluginRegistry,
    plugin_dir: PathBuf,
}
impl PluginManager {
    pub fn new(plugin_dir: PathBuf) -> Result<Self, PluginManagerError> {
        fs::create_dir_all(&plugin_dir)?;
        Ok(Self {
            registry: PluginRegistry::new(),
            plugin_dir,
        })
    }
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }
    /// Install a verified package archive, then make it discoverable.
    ///
    /// Discovery is re-run so a newly installed plugin appears without a restart;
    /// installing a version over a running one does not by itself restart it.
    pub fn install(
        &mut self,
        bytes: &[u8],
    ) -> Result<install::InstalledPackage, PluginManagerError> {
        self.install_verified(bytes, None)
    }
    /// Install a package archive after checking it against a catalog digest.
    pub fn install_verified(
        &mut self,
        bytes: &[u8],
        expected_sha256: Option<&str>,
    ) -> Result<install::InstalledPackage, PluginManagerError> {
        if expected_sha256.is_some_and(|digest| sha256_of(bytes) != digest.to_ascii_lowercase()) {
            return Err(PluginManagerError::UnsafeManifest(
                "package digest does not match the catalog".into(),
            ));
        }
        let manifest = install::inspect_package(bytes)?;
        let id = &manifest.metadata.id;
        if !cargo_packager_updater::semver::VersionReq::parse(&manifest.metadata.app_versions)
            .is_ok_and(|range| {
                cargo_packager_updater::semver::Version::parse(env!("CARGO_PKG_VERSION"))
                    .is_ok_and(|version| range.matches(&version))
            })
        {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin excludes this application version".into(),
            ));
        }
        if self.installed_version(id).as_deref() == Some(&manifest.metadata.version) {
            return Err(PluginManagerError::UnsafeManifest(
                "installed versions are immutable; publish a newer version".into(),
            ));
        }
        let previous = self.installed_version(id);
        let running = self.registry.is_running(id);
        if running {
            self.registry.disable(id)?;
        }
        let result = install::install_package(&self.plugin_dir, expected_sha256, bytes);
        let installed = match result {
            Ok(installed) => installed,
            Err(error) => {
                if running {
                    let _ = self.registry.enable(id);
                }
                return Err(error);
            }
        };
        self.discover_plugins()?;
        // Validate the real process before accepting a replacement. The old
        // version remains on disk so a refused startup can restore the pointer.
        if running && let Err(error) = self.registry.enable(id) {
            if let Some(previous) = previous {
                install::write_current(
                    &self.plugin_dir.join(PACKAGES_DIRECTORY).join(id),
                    &previous,
                )?;
                self.discover_plugins()?;
                let _ = self.registry.enable(id);
            }
            return Err(error.into());
        }
        Ok(installed)
    }
    pub fn resume_enabled(&mut self) -> Result<(), PluginManagerError> {
        self.registry.resume_enabled().map_err(Into::into)
    }
    /// Hand every running plugin the device edges counted since the last poll.
    pub fn input_activity(
        &mut self,
        elapsed_ms: u64,
        activity: bongocat_plugin_sdk::InputActivity,
    ) {
        self.registry.input_activity(elapsed_ms, activity);
    }
    /// Establish the cumulative input baseline for a plugin that just started.
    pub fn initialize_input_activity(
        &mut self,
        id: &str,
        elapsed_ms: u64,
        activity: bongocat_plugin_sdk::InputActivity,
    ) {
        self.registry
            .initialize_input_activity(id, elapsed_ms, activity);
    }
    /// Tell every running plugin that localizes its own views which catalog tag
    /// the host is showing.
    pub fn set_locale(&mut self, locale: &str) {
        self.registry.set_locale(locale);
    }
    /// Remove a plugin's executable package. Its data is kept (see [`Self::reset`]).
    pub fn uninstall(&mut self, id: &str) -> Result<bool, PluginManagerError> {
        if self.registry.is_running(id) {
            self.registry.disable(id)?;
        }
        let removed = install::uninstall_package(&self.plugin_dir, id)?;
        self.registry.forget(id);
        Ok(removed)
    }
    /// Drop a plugin's persisted data. The installed package is untouched.
    pub fn reset(&mut self, id: &str) -> Result<bool, PluginManagerError> {
        let running = self.registry.is_running(id);
        if running {
            self.registry.disable(id)?;
        }
        let removed = install::reset_plugin_data(&self.plugin_dir, id)?;
        self.registry.forget(id);
        self.discover_plugins()?;
        if running {
            self.registry.enable(id)?;
        }
        Ok(removed)
    }
    /// Where a plugin's persisted data lives, creating it when `create` is set.
    ///
    /// Data is keyed by plugin id and kept outside the versioned package tree, so
    /// upgrading or reinstalling a plugin never writes into the data it owns.
    pub fn data_root(&self, id: &str, create: bool) -> Result<PathBuf, PluginManagerError> {
        install::plugin_data_root(&self.plugin_dir, id, create)
    }
    /// The version currently selected for a plugin, if it is installed.
    pub fn installed_version(&self, id: &str) -> Option<String> {
        install::selected_root(&self.plugin_dir, id).and_then(|root| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }
    pub fn discover_plugins(&mut self) -> Result<usize, PluginManagerError> {
        let mut count = 0;
        let packages = self.plugin_dir.join(PACKAGES_DIRECTORY);
        // A store that has never had a plugin installed is empty, not an error.
        if !packages.is_dir() {
            self.registry.forget_missing(&[]);
            return Ok(0);
        }
        let mut discovered = Vec::new();
        for entry in fs::read_dir(&packages)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            // A package with no `current` pointer, or one naming a directory that
            // is not there, is treated as absent rather than as a corrupt plugin.
            let Some(root) = install::selected_root(&self.plugin_dir, &id) else {
                continue;
            };
            let path = root.join(MANIFEST_FILE);
            if !path.is_file() {
                continue;
            }
            let manifest: PluginManifest = serde_json::from_slice(&fs::read(&path)?)?;
            validate_manifest(&manifest)?;
            discovered.push(manifest.metadata.id.clone());
            self.registry.register(manifest.metadata, root)?;
            count += 1;
        }
        self.registry.forget_missing(&discovered);
        Ok(count)
    }
    pub fn list_plugins(&self) -> Vec<(PluginMetadata, PluginState)> {
        self.registry.list()
    }
    pub fn enable_plugin(&mut self, id: &str) -> Result<(), PluginManagerError> {
        self.registry.enable(id).map(|_| ()).map_err(Into::into)
    }
    pub fn disable_plugin(&mut self, id: &str) -> Result<(), PluginManagerError> {
        self.registry.disable(id).map_err(Into::into)
    }
    pub fn action_plugin(&mut self, id: &str, action: &str) -> Result<(), PluginManagerError> {
        self.registry
            .action(id, action)
            .map(|_| ())
            .map_err(Into::into)
    }
    pub fn configure_plugin(
        &mut self,
        id: &str,
        config: bongocat_plugin_sdk::Configuration,
    ) -> Result<(), PluginManagerError> {
        self.registry
            .configure(id, config)
            .map(|_| ())
            .map_err(Into::into)
    }
    pub fn tick(&mut self, elapsed_ms: u64) -> Vec<(String, bongocat_plugin_sdk::PluginOutput)> {
        self.registry.tick(elapsed_ms)
    }
    pub fn plugin_output(&self, id: &str) -> Option<&bongocat_plugin_sdk::PluginOutput> {
        self.registry.output(id)
    }
    pub fn shutdown(&mut self) {
        self.registry.shutdown();
    }
}
fn validate_manifest(manifest: &PluginManifest) -> Result<(), PluginManagerError> {
    let m = &manifest.metadata;
    if m.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
        || m.id.is_empty()
        || m.entry.contains('/')
        || m.entry.contains('\\')
        || Path::new(&m.entry).is_absolute()
    {
        return Err(PluginManagerError::UnsafeManifest(
            "protocol, id or entry is invalid".into(),
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_manifest_path_escape() {
        let metadata = PluginMetadata {
            id: "x".into(),
            name: "x".into(),
            description: "x".into(),
            version: "1.0.0".into(),
            app_versions: ">=2.3.1".into(),
            protocol_version: 1,
            entry: "../x".into(),
            icon: bongocat_plugin_sdk::PluginIcon::Emoji("x".into()),
            capabilities: vec![],
            localizations: Default::default(),
        };
        assert!(
            validate_manifest(&PluginManifest {
                metadata,
                sha256: None
            })
            .is_err()
        );
    }
}

/// A verified plugin package, installed under its own versioned directory.
pub mod install;
