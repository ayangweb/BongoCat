//! Discovery and lifecycle management for independently distributed plugins.
#![forbid(unsafe_code)]
mod catalog;
mod icons;
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
/// A plugin package's manifest: what the package says about itself.
///
/// The digest is not part of it. A package's identity is the digest the signed
/// catalog advertises, so a manifest cannot claim one — a claim the reader would
/// have to ignore, or worse trust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub metadata: PluginMetadata,
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
    /// A running plugin is restarted on replacement, with rollback on failure.
    pub fn install(
        &mut self,
        bytes: &[u8],
        expected_sha256: &str,
    ) -> Result<install::InstalledPackage, PluginManagerError> {
        self.install_verified(bytes, expected_sha256)
    }
    /// Install a package archive after checking it against a catalog digest.
    pub fn install_verified(
        &mut self,
        bytes: &[u8],
        expected_sha256: &str,
    ) -> Result<install::InstalledPackage, PluginManagerError> {
        if sha256_of(bytes) != expected_sha256.to_ascii_lowercase() {
            return Err(PluginManagerError::UnsafeManifest(
                "package digest does not match the catalog".into(),
            ));
        }
        let manifest = install::inspect_package(bytes)?;
        let id = &manifest.metadata.id;
        if !supports_application(&manifest.metadata) {
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
            self.registry.suspend(id)?;
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
        let accept = self.register_installed(id, &installed.root).and_then(|()| {
            if running {
                self.registry.enable(id)?;
            }
            Ok(())
        });
        if let Err(error) = accept {
            // Registration, Start and locale negotiation are all part of the
            // replacement transaction. None may strand the selected pointer.
            self.registry.forget(id);
            if let Some(previous) = previous {
                let package_root = self.plugin_dir.join(PACKAGES_DIRECTORY).join(id);
                install::write_current(&package_root, &previous)?;
                self.register_installed(id, &package_root.join(previous))?;
                if running {
                    self.registry.enable(id)?;
                }
            } else {
                install::uninstall_package(&self.plugin_dir, id)?;
            }
            return Err(error);
        }
        Ok(installed)
    }
    pub fn resume_enabled(&mut self) -> Result<(), PluginManagerError> {
        self.registry.resume_enabled().map_err(Into::into)
    }
    /// Hand every running plugin the device edges counted since the last poll.
    pub fn input_events(
        &mut self,
        events: Vec<bongocat_plugin_sdk::PluginInputEvent>,
        overflow: u64,
    ) {
        self.registry.input_events(events, overflow);
    }
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
    ///
    /// The tag is applied even when one plugin refuses it; that plugin's card
    /// reports the failure and every other process keeps publishing the
    /// language the host is showing.
    pub fn set_locale(&mut self, locale: &str) -> Result<(), PluginManagerError> {
        self.registry.set_locale(locale).map_err(Into::into)
    }
    /// Remove a plugin's executable package. Its data is kept (see [`Self::reset`]).
    pub fn uninstall(&mut self, id: &str) -> Result<bool, PluginManagerError> {
        if self
            .registry
            .list()
            .iter()
            .any(|(metadata, _)| metadata.id == id)
        {
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
            self.registry.suspend(id)?;
        }
        let removed = match install::reset_plugin_data(&self.plugin_dir, id) {
            Ok(removed) => removed,
            Err(error) => {
                if running {
                    self.registry.enable(id)?;
                }
                return Err(error);
            }
        };
        self.registry.forget(id);
        if let Some(root) = install::selected_root(&self.plugin_dir, id) {
            self.register_installed(id, &root)?;
        }
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
        match fs::symlink_metadata(&packages) {
            // A store that has never had a plugin installed is empty, not an error.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.registry.forget_missing(&[]);
                return Ok(0);
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(PluginManagerError::UnsafeManifest(
                    "package store is not a directory".into(),
                ));
            }
        }
        let mut discovered = Vec::new();
        let mut failure = None;
        for entry in fs::read_dir(&packages)? {
            let entry = match entry.and_then(|entry| entry.file_type().map(|kind| (entry, kind))) {
                Ok((entry, kind)) if kind.is_dir() => entry,
                Ok(_) => continue,
                Err(error) => {
                    failure.get_or_insert(PluginManagerError::Io(error));
                    continue;
                }
            };
            let id = entry.file_name().to_string_lossy().into_owned();
            let Some(root) = install::selected_root(&self.plugin_dir, &id) else {
                continue;
            };
            match self.register_installed(&id, &root) {
                Ok(()) => {
                    discovered.push(id);
                    count += 1;
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        self.registry.forget_missing(&discovered);
        failure.map_or(Ok(count), Err)
    }

    fn register_installed(&mut self, id: &str, root: &Path) -> Result<(), PluginManagerError> {
        let manifest = install::read_manifest(&root.join(MANIFEST_FILE))?;
        if manifest.metadata.id != id
            || root.file_name().and_then(|name| name.to_str()) != Some(&manifest.metadata.version)
        {
            return Err(PluginManagerError::UnsafeManifest(
                "selected package identity mismatch".into(),
            ));
        }
        if !supports_application(&manifest.metadata) {
            return Err(PluginManagerError::UnsafeManifest(
                "plugin excludes this application version".into(),
            ));
        }
        self.registry
            .register(manifest.metadata, root.to_path_buf())?;
        Ok(())
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
    pub fn preview_plugin(
        &mut self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), PluginManagerError> {
        self.registry
            .preview(id, key, value)
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
    pub fn collect_updates(&mut self) {
        self.registry.collect_updates();
    }
    pub fn tick(&mut self, elapsed_ms: u64) -> Vec<(String, bongocat_plugin_sdk::PluginOutput)> {
        self.registry.tick(elapsed_ms)
    }
    pub fn plugin_output(&self, id: &str) -> Option<&bongocat_plugin_sdk::PluginOutput> {
        self.registry.output(id)
    }
    pub fn shutdown(&mut self) -> Result<(), PluginManagerError> {
        self.registry.shutdown().map_err(Into::into)
    }
}
fn supports_application(metadata: &PluginMetadata) -> bool {
    use cargo_packager_updater::semver::{Version, VersionReq};
    VersionReq::parse(&metadata.app_versions).is_ok_and(|range| {
        Version::parse(env!("CARGO_PKG_VERSION")).is_ok_and(|version| range.matches(&version))
    })
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
        assert!(install::validate(&PluginManifest { metadata }).is_err());
    }
}

/// A verified plugin package, installed under its own versioned directory.
pub mod install;
