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
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::Stdio;
use zip::write::SimpleFileOptions;

/// Where `--prepare-dev-plugins` writes its packages, relative to the workspace.
pub(crate) const PLUGIN_OUTPUT_DIRECTORY: &str = "target/plugins";
const DEV_PLUGIN_CATALOG: &str = "catalog.json";

/// Build every local plugin and package it as a ZIP under `target/plugins`.
///
/// Returns the ZIP paths written, in workspace order.
pub(super) fn prepare(workspace: &Path) -> Result<Vec<PathBuf>> {
    let output = workspace.join(PLUGIN_OUTPUT_DIRECTORY);
    prepare_into(workspace, &output, None)
}

pub(super) fn prepare_release(workspace: &Path, target: ReleaseTarget) -> Result<Vec<PathBuf>> {
    // The input-source plugin links host system APIs. Do not label a host build
    // as another platform, or silently cross-link a host-only SDK dependency.
    if target != ReleaseTarget::host()? {
        return failure("release plugins must be built on a runner of the target architecture");
    }
    let output = workspace
        .join("target/release-plugins")
        .join(target.manifest_platform());
    prepare_into(workspace, &output, Some(target))
}

/// Resolve versions and binary names with Cargo, including inherited fields.
/// Every checked-in plugin directory must be a real workspace package.
pub(super) fn release_manifests(
    workspace: &Path,
) -> Result<Vec<(PathBuf, PluginManifest, Vec<u8>)>> {
    use cargo_packager_updater::semver::{Version, VersionReq};
    let report = Command::new("cargo")
        .current_dir(workspace)
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()?;
    if !report.status.success() {
        return failure("Cargo could not resolve plugin workspace metadata");
    }
    let metadata: serde_json::Value = serde_json::from_slice(&report.stdout)?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("missing Cargo packages")?;
    let app = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let mut sources = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for entry in fs::read_dir(workspace.join("plugins"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let source = entry.path();
        let cargo_path = source.join("Cargo.toml").canonicalize()?;
        let package = packages
            .iter()
            .find(|package| {
                package["manifest_path"].as_str().is_some_and(|path| {
                    Path::new(path)
                        .canonicalize()
                        .is_ok_and(|path| path == cargo_path)
                })
            })
            .ok_or("plugin directory is not a workspace package")?;
        let bytes = fs::read(source.join(MANIFEST_FILE))?;
        let manifest: PluginManifest = serde_json::from_slice(&bytes)?;
        let info = &manifest.metadata;
        Version::parse(&info.version)?;
        if !ids.insert(info.id.clone())
            || package["version"].as_str() != Some(info.version.as_str())
            || !VersionReq::parse(&info.app_versions)?.matches(&app)
            || info.name.trim().is_empty()
            || info.description.trim().is_empty()
            || info.protocol_version != bongocat_plugin_sdk::PROTOCOL_VERSION
            || !bongocat_plugin_manager::install::valid_basename(&info.id)
            || !bongocat_plugin_manager::install::valid_basename(&info.entry)
            || !package["targets"].as_array().is_some_and(|targets| {
                targets.iter().any(|target| {
                    target["name"].as_str() == Some(&info.entry)
                        && target["kind"]
                            .as_array()
                            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
                })
            })
        {
            return failure(format!(
                "invalid release metadata or Cargo version/entry mismatch: {}",
                info.id
            ));
        }
        sources.push((source, manifest, bytes));
    }
    if sources.is_empty() {
        return failure("no release plugins found");
    }
    sources.sort_by(|a, b| a.1.metadata.id.cmp(&b.1.metadata.id));
    Ok(sources)
}

fn prepare_into(
    workspace: &Path,
    output: &Path,
    target: Option<ReleaseTarget>,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(output)?;
    let mut packages = Vec::new();
    // Development and release packages are one shape: both come from the
    // workspace metadata, so a local package cannot be built from a manifest the
    // release path would refuse.
    let sources = release_manifests(workspace)?;
    for (source, manifest, bytes) in &sources {
        let mut command = Command::new("cargo");
        command
            .current_dir(workspace)
            .args(["build", "--locked", "--release", "--manifest-path"])
            .arg(source.join("Cargo.toml"))
            .args(["--bin", &manifest.metadata.entry, "--message-format=json"]);
        if let Some(target) = target {
            command.args(["--target", target.triple()]);
        }
        let report = command.stderr(Stdio::inherit()).output()?;
        if !report.status.success() {
            return failure(format!(
                "failed to build local plugin {}",
                manifest.metadata.id
            ));
        }
        let executable = executable_from_messages(&report.stdout, &manifest.metadata.entry)
            .ok_or("Cargo did not report the plugin executable")?;
        let package = package_into(output, manifest, bytes, source, &executable)?;
        if target.is_some() {
            // A release package that this host cannot install is refused before
            // it is published, so the archive a release uploads is one the
            // product's own installer accepts.
            let store = tempfile::tempdir()?;
            let bytes = fs::read(&package)?;
            bongocat_plugin_manager::install::install_package(
                store.path(),
                &bongocat_plugin_manager::sha256_of(&bytes),
                &bytes,
            )?;
        }
        packages.push(package);
    }
    prune_packages(output, &packages)?;
    if target.is_none() {
        write_dev_catalog(output, &sources, &packages)?;
    }
    Ok(packages)
}

pub(super) fn write_dev_catalog(
    output: &Path,
    sources: &[(PathBuf, PluginManifest, Vec<u8>)],
    packages: &[PathBuf],
) -> Result<()> {
    let mut entries = Vec::new();
    for (source, manifest, _) in sources {
        let icon = match &manifest.metadata.icon {
            bongocat_plugin_sdk::PluginIcon::LocalImage(relative) => {
                let icon = output
                    .join("icons")
                    .join(&manifest.metadata.id)
                    .join("icon.png");
                fs::create_dir_all(icon.parent().ok_or("plugin icon has no parent")?)?;
                fs::copy(source.join(relative), &icon)?;
                icon
            }
            bongocat_plugin_sdk::PluginIcon::Emoji(_)
            | bongocat_plugin_sdk::PluginIcon::RemoteImage(_) => {
                return failure("every bundled plugin must declare a local PNG icon");
            }
        };
        entries.push((manifest.metadata.clone(), icon));
    }
    if entries.len() != packages.len() {
        return failure("local plugin catalog and package set disagree");
    }
    entries.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let catalog = bongocat_plugin_manager::Catalog {
        plugins: entries
            .into_iter()
            .map(|(metadata, icon)| bongocat_plugin_manager::CatalogEntry {
                icon_url: icon.display().to_string(),
                id: metadata.id,
                name: metadata.name,
                description: metadata.description,
                localizations: metadata.localizations,
                version: metadata.version,
                app_versions: metadata.app_versions,
                protocol_version: metadata.protocol_version,
                icon: metadata.icon,
                downloads: BTreeMap::new(),
                sha256: None,
                homepage: None,
            })
            .collect(),
    };
    fs::write(
        output.join(DEV_PLUGIN_CATALOG),
        serde_json::to_vec_pretty(&catalog)?,
    )?;
    Ok(())
}

/// Only current source plugins should be discoverable after a successful build.
/// Prune after every build succeeds so a failed preparation retains its outputs.
fn prune_packages(output: &Path, current: &[PathBuf]) -> Result<()> {
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension().is_some_and(|extension| extension == "zip")
            && !current.contains(&path)
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Write one plugin's ZIP, containing its manifest, executable and declared
/// package-local assets.
#[cfg(test)]
pub(super) fn package_plugin(
    output: &Path,
    source: &Path,
    manifest: &PluginManifest,
    manifest_bytes: &[u8],
    executable: &Path,
) -> Result<PathBuf> {
    let output = output.join(PLUGIN_OUTPUT_DIRECTORY);
    package_into(&output, manifest, manifest_bytes, source, executable)
}

fn package_into(
    output: &Path,
    manifest: &PluginManifest,
    manifest_bytes: &[u8],
    source: &Path,
    executable: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(output)?;
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
    if let bongocat_plugin_sdk::PluginIcon::LocalImage(relative) = &manifest.metadata.icon {
        let relative = Path::new(relative);
        if !bongocat_plugin_manager::install::safe_relative_path(relative) {
            return Err("plugin icon path is unsafe".into());
        }
        let icon = source.join(relative);
        if !icon.is_file() {
            return Err("plugin icon is missing".into());
        }
        archive.start_file(relative.to_string_lossy(), options)?;
        std::io::copy(&mut fs::File::open(icon)?, &mut archive)?;
    }
    // The executable bit has to travel in the archive, because installation is
    // what recreates the file and a download has no mode to inherit. The Unix
    // system is recorded explicitly instead of left to `zip`'s per-host default:
    // on a Windows packaging host that default is DOS, whose external attributes
    // carry no Unix mode, so the mode set here would never be read back and the
    // archive would be refused for holding a non-executable executable.
    archive.start_file(
        entry,
        options.unix_permissions(0o755).system(zip::System::Unix),
    )?;
    archive.write_all(&binary)?;
    let bytes = archive.finish()?.into_inner();

    let staged = tempfile::NamedTempFile::new_in(output)?;
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

    /// Write a stand-in build output where Cargo would put it on this host.
    ///
    /// `metadata.entry` names the plugin, not its file: Windows compiles a
    /// `plugin.exe`, and the installer resolves that same name from the manifest.
    /// A fixture that wrote `plugin` on every host produced an archive whose
    /// executable the installer could not find once it ran on Windows.
    fn write_test_executable(directory: &Path, manifest: &PluginManifest) -> PathBuf {
        let name = if cfg!(windows) {
            format!("{}.exe", manifest.metadata.entry)
        } else {
            manifest.metadata.entry.clone()
        };
        let executable = directory.join(name);
        fs::write(&executable, b"test executable").unwrap();
        executable
    }

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
        let executable = write_test_executable(root.path(), &manifest);

        let package = package_plugin(
            root.path(),
            &workspace_root().unwrap().join("plugins/pomodoro"),
            &manifest,
            bytes,
            &executable,
        )
        .unwrap();
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
        let bytes = fs::read(&package).unwrap();
        manager
            .install(&bytes, &bongocat_plugin_manager::sha256_of(&bytes))
            .unwrap();
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

    #[test]
    fn preparing_plugins_prunes_obsolete_archives_but_not_other_outputs() {
        let output = tempfile::tempdir().unwrap();
        let current = output.path().join("com.bongocat.key-sounds-1.0.0.zip");
        let obsolete = output.path().join("removed-plugin-1.0.0.zip");
        let note = output.path().join("notes.txt");
        let directory = output.path().join("directory.zip");
        for path in [&current, &obsolete, &note] {
            fs::write(path, b"keep or prune").unwrap();
        }
        fs::create_dir(&directory).unwrap();
        prune_packages(output.path(), std::slice::from_ref(&current)).unwrap();
        assert!(current.is_file());
        assert!(!obsolete.exists());
        assert!(note.is_file());
        assert!(directory.is_dir());
        // A removed final plugin must not remain discoverable either.
        prune_packages(output.path(), &[]).unwrap();
        assert!(!current.exists());
    }

    #[test]
    fn key_sounds_identity_survives_discovery_install_update_reset_and_uninstall() {
        use bongocat_plugin_manager::PluginSource;
        use std::sync::atomic::AtomicBool;

        let workspace = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../../plugins/key-sounds/plugin.json");
        let mut manifest: PluginManifest = serde_json::from_slice(bytes).unwrap();
        let id = manifest.metadata.id.clone();
        assert_eq!(id, "com.bongocat.key-sounds");
        let executable = write_test_executable(workspace.path(), &manifest);
        let package = package_plugin(
            workspace.path(),
            &workspace_root().unwrap().join("plugins/key-sounds"),
            &manifest,
            bytes,
            &executable,
        )
        .unwrap();
        assert_eq!(
            package.file_name().unwrap(),
            "com.bongocat.key-sounds-1.0.0.zip"
        );
        let packages = workspace.path().join(PLUGIN_OUTPUT_DIRECTORY);
        let source = workspace_root().unwrap().join("plugins/key-sounds");
        write_dev_catalog(
            &packages,
            &[(source, manifest.clone(), bytes.to_vec())],
            std::slice::from_ref(&package),
        )
        .unwrap();
        let stop = AtomicBool::new(false);
        let source = PluginSource::Development {
            catalog: packages.join(DEV_PLUGIN_CATALOG),
            packages,
        };
        let available = source.discover("2.3.1", &stop).unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].metadata.id, id);
        // Discovery must provide the card artwork before installation.
        let icon = &available[0].icon_url;
        assert!(std::path::Path::new(icon).is_file());
        assert!(std::path::Path::new(icon).ends_with(std::path::Path::new(
            "icons/com.bongocat.key-sounds/icon.png"
        )));
        assert_eq!(available[0].metadata.localized_copy("zh-CN").0, "按键音效");
        assert_eq!(
            available[0].metadata.localized_copy("zh-TW").0,
            "Key Sounds"
        );

        let store = tempfile::tempdir().unwrap();
        let mut manager = PluginManager::new(store.path().into()).unwrap();
        let bytes = available[0].read(&stop).unwrap();
        manager
            .install(&bytes, &bongocat_plugin_manager::sha256_of(&bytes))
            .unwrap();
        assert_eq!(manager.installed_version(&id).as_deref(), Some("1.0.0"));
        let data = manager.data_root(&id, true).unwrap();
        assert!(data.ends_with("data/com.bongocat.key-sounds"));
        fs::write(data.join("marker"), b"keep on update").unwrap();

        manifest.metadata.version = "1.0.1".into();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let package = package_plugin(
            workspace.path(),
            &workspace_root().unwrap().join("plugins/key-sounds"),
            &manifest,
            &bytes,
            &executable,
        )
        .unwrap();
        let bytes = fs::read(package).unwrap();
        manager
            .install(&bytes, &bongocat_plugin_manager::sha256_of(&bytes))
            .unwrap();
        assert_eq!(manager.installed_version(&id).as_deref(), Some("1.0.1"));
        assert!(data.join("marker").is_file());
        assert!(manager.reset(&id).unwrap());
        assert!(!data.join("marker").exists());
        assert_eq!(manager.installed_version(&id).as_deref(), Some("1.0.1"));
        assert!(manager.uninstall(&id).unwrap());
        assert_eq!(manager.installed_version(&id), None);
    }
}
