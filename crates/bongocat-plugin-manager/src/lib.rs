//! Discovery and lifecycle management for independently distributed plugins.
#![forbid(unsafe_code)]
mod catalog;
mod loader;
mod registry;
use bongocat_plugin_sdk::PluginMetadata;
pub use catalog::{
    CATALOG_NAME, Catalog, CatalogEntry, CatalogError, download_verified, fetch_and_select,
    parse_catalog, select_for_host, sha256_of,
};
pub use registry::{PluginRegistry, PluginState, RegistryError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const MANIFEST_FILE: &str = "plugin.json";
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
    pub fn discover_plugins(&mut self) -> Result<usize, PluginManagerError> {
        let mut count = 0;
        for entry in fs::read_dir(&self.plugin_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let root = entry.path();
            let path = root.join(MANIFEST_FILE);
            if !path.is_file() {
                continue;
            }
            let manifest: PluginManifest = serde_json::from_slice(&fs::read(&path)?)?;
            validate_manifest(&manifest)?;
            self.registry.register(manifest.metadata, root)?;
            count += 1;
        }
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

/// Compatibility of an advertised release with this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compatibility {
    /// The plugin may be offered as an install or an update.
    Compatible,
    /// It names a protocol this build does not speak.
    IncompatibleProtocol,
    /// No advertised build matches this host.
    MissingPlatform,
    /// The advertised app range excludes this build.
    IncompatibleAppVersion,
}

impl Compatibility {
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }
}

/// One discoverable plugin as the center shows it.
#[derive(Clone, Debug)]
pub struct PluginCard {
    pub metadata: PluginMetadata,
    pub state: PluginState,
    pub version: Option<String>,
    pub compatibility: Compatibility,
    /// `None` for a locally discovered package, `Some` for a catalog entry.
    pub download_url: Option<String>,
    pub sha256: Option<String>,
}
