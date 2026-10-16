//! Local plugin packaging for Development, kept out of product bundles.
//!
//! A plugin is a ZIP, exactly as it is when published, so preparing a local
//! plugin has to produce the same artifact a release does rather than a second,
//! looser shape. This tool builds each checked-in plugin and writes its package to
//! `target/plugins/<id>-<version>.zip`.
//!
//! It stops there on purpose. Installing is the product's own lifecycle — it owns
//! the store, it validates the package and it is the only thing that decides what
//! runs — so a build tool writing into the application's data directory would be a
//! second writer of a store that must have one. A Development build installs these
//! artifacts through the same path a downloaded package takes, and the only
//! difference between the two environments is where the bytes came from.
use super::*;
use bongocat_plugin_manager::{MANIFEST_FILE, PluginManifest};
use std::io::{Read, Write};
use std::process::Stdio;
use zip::write::SimpleFileOptions;

/// Where `--prepare-dev-plugins` writes its packages, relative to the workspace.
pub(crate) const PLUGIN_OUTPUT_DIRECTORY: &str = "target/plugins";

/// Build every local plugin and package it as a ZIP under `target/plugins`.
///
/// Returns the ZIP paths written, in workspace order.
pub(super) fn prepare(workspace: &Path) -> Result<Vec<PathBuf>> {
    let output = workspace.join(PLUGIN_OUTPUT_DIRECTORY);
    fs::create_dir_all(&output)?;
    let mut packages = Vec::new();
    for entry in fs::read_dir(workspace.join("plugins"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let source = entry.path();
        let manifest = source.join(MANIFEST_FILE);
        if !manifest.is_file() {
            continue;
        }
        let bytes = fs::read(&manifest)?;
        let manifest: PluginManifest = serde_json::from_slice(&bytes)?;
        for component in [&manifest.metadata.id, &manifest.metadata.entry] {
            if !bongocat_plugin_manager::install::valid_basename(component) {
                return failure("local plugin id and entry must be safe basenames");
            }
        }
        let output = Command::new("cargo")
            .current_dir(workspace)
            .args(["build", "--locked", "--release", "--manifest-path"])
            .arg(source.join("Cargo.toml"))
            .args(["--bin", &manifest.metadata.entry, "--message-format=json"])
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            return failure(format!(
                "failed to build local plugin {}",
                manifest.metadata.id
            ));
        }
        let executable = executable_from_messages(&output.stdout, &manifest.metadata.entry)
            .ok_or("Cargo did not report the plugin executable")?;
        packages.push(package_plugin(workspace, &manifest, &bytes, &executable)?);
    }
    Ok(packages)
}

/// Write one plugin's ZIP, containing its manifest and its executable.
fn package_plugin(
    workspace: &Path,
    manifest: &PluginManifest,
    manifest_bytes: &[u8],
    executable: &Path,
) -> Result<PathBuf> {
    let output = workspace.join(PLUGIN_OUTPUT_DIRECTORY);
    fs::create_dir_all(&output)?;
    let name = format!("{}-{}.zip", manifest.metadata.id, manifest.metadata.version);
    let mut binary = Vec::new();
    fs::File::open(executable)?.read_to_end(&mut binary)?;
    let entry = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("plugin executable has no filename")?;

    // Build the archive in memory and only then publish it, so a half-written
    // ZIP is never what an install reads.
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file(MANIFEST_FILE, options)?;
    archive.write_all(manifest_bytes)?;
    // The executable bit has to travel in the archive, because installation is
    // what recreates the file and a download has no mode to inherit.
    archive.start_file(entry, options.unix_permissions(0o755))?;
    archive.write_all(&binary)?;
    let bytes = archive.finish()?.into_inner();

    let staged = tempfile::NamedTempFile::new_in(&output)?;
    staged.as_file().write_all(&bytes)?;
    staged.as_file().sync_all()?;
    let destination = output.join(&name);
    staged.persist(&destination)?;
    Ok(destination)
}

fn executable_from_messages(bytes: &[u8], entry: &str) -> Option<PathBuf> {
    bytes.split(|byte| *byte == b'\n').find_map(|line| {
        let value: serde_json::Value = serde_json::from_slice(line).ok()?;
        if value["reason"] != "compiler-artifact" || value["target"]["name"] != entry {
            return None;
        }
        value["executable"].as_str().map(PathBuf::from)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_manager::PluginManager;

    #[test]
    fn preparation_is_a_separate_mode_not_a_production_build_option() {
        assert!(matches!(
            Invocation::parse(vec!["--prepare-dev-plugins".into()]).unwrap(),
            Invocation::PrepareDevPlugins
        ));
        for option in [
            "--environment",
            "--target",
            "--formats",
            "--merge-manifests",
            "--generate-signing-key",
        ] {
            assert!(
                Invocation::parse(vec![
                    "--prepare-dev-plugins".into(),
                    option.into(),
                    "production".into()
                ])
                .is_err()
            );
        }
    }

    #[test]
    fn a_local_plugin_packages_as_a_zip_the_manager_can_install() {
        let root = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../../plugins/pomodoro/plugin.json");
        let manifest: PluginManifest = serde_json::from_slice(bytes).unwrap();
        let executable = root.path().join(&manifest.metadata.entry);
        fs::write(&executable, b"test executable").unwrap();

        let package = package_plugin(root.path(), &manifest, bytes, &executable).unwrap();
        assert_eq!(
            package.file_name().unwrap().to_string_lossy(),
            "com.bongocat.pomodoro-1.0.0.zip"
        );
        // The artifact is a build output under `target/`, and the tool's job ends
        // there: installing it is the product's own lifecycle.
        assert_eq!(
            package.parent().unwrap(),
            root.path().join(PLUGIN_OUTPUT_DIRECTORY)
        );

        // The package is one the manager accepts, through the same install call a
        // download takes.
        let store = tempfile::tempdir().unwrap();
        let mut manager = PluginManager::new(store.path().to_path_buf()).unwrap();
        manager.install(&fs::read(&package).unwrap()).unwrap();
        assert_eq!(manager.list_plugins().len(), 1);
        let metadata = &manager.list_plugins()[0].0;
        assert_eq!(metadata.name, "Pomodoro");
        // The package carries the plugin's own localized copy, so the center can
        // show the name in the language the window is already using.
        assert_eq!(metadata.localized_copy("zh-CN").0, "番茄钟");
        assert_eq!(metadata.localized_copy("zh-TW").0, "Pomodoro");
        assert_eq!(metadata.localized_copy("fr-FR").0, "Pomodoro");
        assert_eq!(
            manager
                .installed_version("com.bongocat.pomodoro")
                .as_deref(),
            Some("1.0.0")
        );
        // The install writes the selected package and nothing else at the store
        // root, so a build artifact is never what the product runs in place.
        assert!(
            store
                .path()
                .join("packages/com.bongocat.pomodoro/1.0.0")
                .is_dir()
        );
    }

    #[test]
    fn cargo_report_resolves_custom_target_directories() {
        let report = "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"plugin\"},\"executable\":\"/custom/release/plugin\"}\n{\"reason\":\"build-finished\",\"success\":true}";
        assert_eq!(
            executable_from_messages(report.as_bytes(), "plugin"),
            Some(PathBuf::from("/custom/release/plugin"))
        );
        assert_eq!(executable_from_messages(report.as_bytes(), "other"), None);
        for value in ["", ".", "..", "../plugin", "C:\\plugin", "/plugin"] {
            assert!(!bongocat_plugin_manager::install::valid_basename(value));
        }
    }
}
