//! Local plugin preparation stays outside product bundles and Production data.
use super::*;
use bongocat_config::{BuildEnvironment, platform_layout};
use bongocat_plugin_manager::{MANIFEST_FILE, PluginManifest};
use std::process::Stdio;

pub(super) fn prepare(workspace: &Path) -> Result<Vec<PathBuf>> {
    let destination = platform_layout(BuildEnvironment::Development)?
        .root
        .join("plugins");
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
            if !safe_basename(component) {
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
        packages.push(stage(&destination, &manifest, &bytes, &executable)?);
    }
    Ok(packages)
}

fn safe_basename(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
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

fn stage(
    destination: &Path,
    manifest: &PluginManifest,
    bytes: &[u8],
    executable: &Path,
) -> Result<PathBuf> {
    let package = destination.join(&manifest.metadata.id);
    fs::create_dir_all(&package)?;
    let name = executable
        .file_name()
        .ok_or("plugin executable has no filename")?;
    let staged_binary = tempfile::NamedTempFile::new_in(&package)?;
    fs::copy(executable, staged_binary.path())?;
    staged_binary.as_file().sync_all()?;
    staged_binary.persist(package.join(name))?;
    // Publish metadata only after the executable is present, so discovery cannot
    // advertise a newly prepared package that has no runnable entry.
    use std::io::Write as _;
    let mut staged_manifest = tempfile::NamedTempFile::new_in(&package)?;
    staged_manifest.write_all(bytes)?;
    staged_manifest.as_file().sync_all()?;
    staged_manifest.persist(package.join(MANIFEST_FILE))?;
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn preparation_produces_a_discoverable_package_without_touching_production() {
        let root = tempfile::tempdir().unwrap();
        let development = root.path().join("development/plugins");
        let production = root.path().join("production/plugins");
        fs::create_dir_all(&production).unwrap();
        fs::write(production.join("sentinel"), b"unchanged").unwrap();
        let bytes = include_bytes!("../../../plugins/pomodoro/plugin.json");
        let manifest: PluginManifest = serde_json::from_slice(bytes).unwrap();
        let executable = root.path().join(&manifest.metadata.entry);
        fs::write(&executable, b"test executable").unwrap();
        let package = stage(&development, &manifest, bytes, &executable).unwrap();
        let mut manager = bongocat_plugin_manager::PluginManager::new(development).unwrap();
        assert_eq!(manager.discover_plugins().unwrap(), 1);
        assert_eq!(manager.list_plugins()[0].0.name, "Pomodoro / 番茄钟");
        assert!(package.join(&manifest.metadata.entry).is_file());
        assert_eq!(fs::read(production.join("sentinel")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(production).unwrap().count(), 1);
    }

    #[test]
    fn cargo_report_resolves_custom_target_directories() {
        let bytes = br#"{"reason":"compiler-artifact","target":{"name":"plugin"},"executable":"/custom/release/plugin"}
{"reason":"build-finished","success":true}"#;
        assert_eq!(
            executable_from_messages(bytes, "plugin"),
            Some(PathBuf::from("/custom/release/plugin"))
        );
        assert_eq!(executable_from_messages(bytes, "other"), None);
        for value in ["", ".", "..", "../plugin", "C:\\plugin", "/plugin"] {
            assert!(!safe_basename(value));
        }
    }
}
