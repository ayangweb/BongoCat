//! Development-only side-loading of the plugins the workspace just built.
//!
//! A plugin is installed by the product, never by a build tool: the store has one
//! writer, and that writer is the same one a downloaded package goes through. So
//! `just dev-plugins` builds ZIPs under `target/plugins` and stops, and this is
//! what puts them where the product reads them.
//!
//! Only a Development build takes this path, and it is decided at compile time.
//! A Production build has no repository to read and scans only its own private
//! store.

use bongocat_config::BuildEnvironment;
use bongocat_plugin_service::PluginServiceClient;
use std::path::{Path, PathBuf};

/// The directory the packaging tool writes local plugin packages to.
const PACKAGES_DIRECTORY: &str = "target/plugins";

/// The workspace a Development build was compiled from.
///
/// Resolved from this crate's own manifest directory rather than the working
/// directory, so `cargo run` finds it however it was launched.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
}

/// Install every locally built package, returning what was installed.
///
/// A missing directory is the ordinary state of a checkout that has not run
/// `just dev-plugins` yet, not an error: it installs nothing and says so.
pub(crate) fn side_load_development_packages(client: &PluginServiceClient) -> Vec<PathBuf> {
    if !matches!(
        bongocat_app::BUILD_ENVIRONMENT,
        BuildEnvironment::Development
    ) {
        return Vec::new();
    }
    let Some(directory) = workspace_root().map(|root| root.join(PACKAGES_DIRECTORY)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut installed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("zip") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if client.install_blocking(bytes).is_ok() {
            installed.push(path);
        }
    }
    installed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_load_directory_is_the_workspace_build_output() {
        let root = workspace_root().expect("a Development build has a workspace");
        assert!(root.join("Cargo.toml").is_file());
        assert_eq!(
            root.join(PACKAGES_DIRECTORY).file_name().unwrap(),
            "plugins"
        );
        assert_eq!(
            root.join(PACKAGES_DIRECTORY)
                .parent()
                .unwrap()
                .file_name()
                .unwrap(),
            "target"
        );
    }
}
