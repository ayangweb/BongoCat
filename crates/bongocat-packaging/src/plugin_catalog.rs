//! Signed plugin release assets use the product's existing signing toolchain.
use super::*;
use bongocat_plugin_manager::{Catalog, CatalogEntry, install::inspect_package, sha256_of};
use std::io::Write;

pub(super) fn publish_release(
    workspace: &Path,
    output: &Path,
    inputs: &[(ReleaseTarget, PathBuf)],
    previous: &Path,
    signing: &SigningConfig,
) -> Result<Vec<PathBuf>> {
    let public_key =
        bongocat_update::RELEASE_SIGNING_KEY.ok_or("release signing key is not configured")?;
    publish_release_with_key(workspace, output, inputs, previous, signing, public_key)
}

fn publish_release_with_key(
    workspace: &Path,
    output: &Path,
    inputs: &[(ReleaseTarget, PathBuf)],
    previous: &Path,
    signing: &SigningConfig,
    public_key: &str,
) -> Result<Vec<PathBuf>> {
    if inputs.len() != ReleaseTarget::ALL.len()
        || ReleaseTarget::ALL.iter().any(|target| {
            inputs
                .iter()
                .filter(|(candidate, _)| candidate == target)
                .count()
                != 1
        })
    {
        return failure(
            "a production plugin catalog requires exactly one package directory for every shipped platform",
        );
    }
    let expected = super::dev_plugins::release_manifests(workspace)?
        .into_iter()
        .map(|(_, manifest, _)| (manifest.metadata.id.clone(), manifest))
        .collect::<BTreeMap<_, _>>();
    let mut seen = std::collections::BTreeSet::new();
    for (target, directory) in inputs {
        if !directory.is_dir() {
            return failure(format!(
                "missing plugin packages for {}",
                target.manifest_platform()
            ));
        }
        let mut count = 0;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "zip")
            {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            let manifest = inspect_package(&bytes)?;
            validate_release_archive(&bytes, &manifest, *target)?;
            let identity = (
                manifest.metadata.id.clone(),
                manifest.metadata.version.clone(),
            );
            if expected.get(&identity.0) != Some(&manifest) {
                return failure(format!(
                    "unexpected plugin package in {}",
                    entry.path().display()
                ));
            }
            if !seen.insert((identity.0, identity.1, target.manifest_platform())) {
                return failure("duplicate plugin package in release matrix");
            }
            count += 1;
        }
        if count != expected.len() {
            return failure(format!(
                "{} contains {count} packages, expected {}",
                directory.display(),
                expected.len()
            ));
        }
    }
    if seen.len() != expected.len() * ReleaseTarget::ALL.len() {
        return failure("release plugin matrix is incomplete");
    }
    // A failed key/history check must not expose a newly written release catalog.
    let staged = tempfile::tempdir()?;
    let catalog_path = staged.path().join("plugins.json");
    let assets = publish(&catalog_path, inputs, signing)?;
    let catalog = bongocat_plugin_manager::parse_catalog(&fs::read(&catalog_path)?, public_key)
        .map_err(|error| format!("generated plugin catalog does not verify: {error}"))?;
    validate_history(&catalog, previous, public_key)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut result = Vec::new();
    for asset in assets.iter().filter(|asset| *asset != &catalog_path) {
        let destination = parent.join(asset.file_name().ok_or("invalid plugin asset name")?);
        if destination.exists() && fs::read(&destination)? != fs::read(asset)? {
            return failure("refusing to overwrite different release plugin bytes");
        }
        fs::copy(asset, &destination)?;
        result.push(destination);
    }
    let catalog_bytes = fs::read(catalog_path)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&catalog_bytes)?;
    file.as_file().sync_all()?;
    file.persist(output)?;
    result.push(output.to_path_buf());
    Ok(result)
}

/// Validate cross-platform archives without relying on the publisher's host OS.
fn validate_release_archive(
    bytes: &[u8],
    manifest: &bongocat_plugin_manager::PluginManifest,
    target: ReleaseTarget,
) -> Result<()> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let entry = match target {
        ReleaseTarget::WindowsX86_64 => format!("{}.exe", manifest.metadata.entry),
        _ => manifest.metadata.entry.clone(),
    };
    if archive.len() != 2 {
        return failure("release plugin archive must contain only its manifest and executable");
    }
    let mut executable = archive.by_name(&entry)?;
    if executable.is_dir()
        || executable.is_symlink()
        || executable.size() == 0
        || executable.size() > bongocat_plugin_manager::install::MAX_EXTRACT_BYTES as u64
        || target.is_apple() && executable.unix_mode().is_none_or(|mode| mode & 0o111 == 0)
    {
        return failure("release plugin executable is invalid or not executable");
    }
    // Fully consume the entry to check decompression and CRC before signing it.
    let mut sink = std::io::sink();
    std::io::copy(
        &mut executable
            .by_ref()
            .take(bongocat_plugin_manager::install::MAX_EXTRACT_BYTES as u64 + 1),
        &mut sink,
    )?;
    Ok(())
}

/// History is downloaded from every published release, not just `latest`:
/// removal and reintroduction must not allow a version to be rewritten.
fn validate_history(current: &Catalog, previous: &Path, public_key: &str) -> Result<()> {
    use cargo_packager_updater::semver::Version;
    for entry in fs::read_dir(previous)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            return failure("release history must contain only signed catalog JSON files");
        }
        let old = bongocat_plugin_manager::parse_catalog(&fs::read(entry.path())?, public_key)?;
        for candidate in &current.plugins {
            for published in old.plugins.iter().filter(|old| old.id == candidate.id) {
                let version = Version::parse(&candidate.version)?;
                let previous_version = Version::parse(&published.version)?;
                if version < previous_version {
                    return failure(format!("plugin version regression: {}", candidate.id));
                }
                if version == previous_version
                    && candidate
                        .downloads
                        .keys()
                        .any(|platform| published.downloads.contains_key(platform))
                    && candidate != published
                {
                    return failure(format!(
                        "published plugin version changed; bump {}",
                        candidate.id
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn publish(
    output: &Path,
    inputs: &[(ReleaseTarget, PathBuf)],
    signing: &SigningConfig,
) -> Result<Vec<PathBuf>> {
    if signing.private_key.trim().is_empty() {
        return failure("plugin catalogs require SIGNING_PRIVATE_KEY");
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    let mut catalog = Catalog {
        plugins: Vec::new(),
    };
    let mut names = std::collections::BTreeSet::new();
    for (target, directory) in inputs {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "zip")
            {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            let manifest = inspect_package(&bytes)?;
            let metadata = manifest.metadata;
            let name = format!(
                "{}-{}-{}.zip",
                metadata.id,
                metadata.version,
                target.manifest_platform()
            );
            if !names.insert(name.clone()) {
                return failure("duplicate plugin release asset");
            }
            fs::write(staging.path().join(&name), &bytes)?;
            catalog.plugins.push(CatalogEntry {
                id: metadata.id,
                name: metadata.name,
                description: metadata.description,
                localizations: metadata.localizations,
                version: metadata.version,
                app_versions: metadata.app_versions,
                protocol_version: metadata.protocol_version,
                icon: metadata.icon,
                downloads: [(target.manifest_platform().to_owned(), name)].into(),
                sha256: Some(sha256_of(&bytes)),
                homepage: None,
            });
        }
    }
    catalog
        .plugins
        .sort_by(|a, b| (&a.id, &a.version, &a.downloads).cmp(&(&b.id, &b.version, &b.downloads)));
    let canonical = staging.path().join("catalog.json");
    fs::write(&canonical, serde_json::to_vec(&catalog)?)?;
    let signature = fs::read_to_string(cargo_packager::sign::sign_file(signing, &canonical)?)?;
    let signed = serde_json::json!({ "catalog": catalog, "signature": signature });
    let mut assets = Vec::new();
    for name in names {
        let destination = parent.join(&name);
        // Versioned assets are immutable, just as installed package versions are.
        if destination.exists() && fs::read(&destination)? != fs::read(staging.path().join(&name))?
        {
            return failure("refusing to overwrite different bytes for a published plugin version");
        }
        fs::copy(staging.path().join(name), &destination)?;
        assets.push(destination);
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(&signed)?)?;
    file.as_file().sync_all()?;
    file.persist(output)?;
    assets.push(output.to_path_buf());
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_requires_the_complete_target_matrix() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            publish_release(
                root.path(),
                &root.path().join("plugins.json"),
                &[],
                root.path(),
                &SigningConfig::new()
            )
            .unwrap_err()
            .to_string()
            .contains("every shipped platform")
        );
        let inputs = vec![(ReleaseTarget::MacosAarch64, root.path().to_owned()); 3];
        assert!(
            publish_release(
                root.path(),
                &root.path().join("plugins.json"),
                &inputs,
                root.path(),
                &SigningConfig::new()
            )
            .is_err()
        );
    }

    #[test]
    fn production_catalog_round_trip_matrix_history_and_wrong_key() {
        let root = tempfile::tempdir().unwrap();
        let workspace = workspace_root().unwrap();
        let sources = super::super::dev_plugins::release_manifests(&workspace).unwrap();
        let mut inputs = Vec::new();
        for target in ReleaseTarget::ALL {
            let source = root.path().join(target.manifest_platform());
            for (_, manifest, bytes) in &sources {
                fs::create_dir_all(&source).unwrap();
                let executable = source.join(if target == ReleaseTarget::WindowsX86_64 {
                    format!("{}.exe", manifest.metadata.entry)
                } else {
                    manifest.metadata.entry.clone()
                });
                fs::write(&executable, b"test executable").unwrap();
                super::super::dev_plugins::package_plugin(&source, manifest, bytes, &executable)
                    .unwrap();
            }
            inputs.push((
                target,
                source.join(super::super::dev_plugins::PLUGIN_OUTPUT_DIRECTORY),
            ));
        }
        let history = root.path().join("history");
        fs::create_dir(&history).unwrap();
        let keys = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        let signing = SigningConfig::new().private_key(keys.sk).password("");
        let output = root.path().join("release/plugins.json");
        let assets =
            publish_release_with_key(&workspace, &output, &inputs, &history, &signing, &keys.pk)
                .unwrap();
        assert_eq!(assets.len(), sources.len() * 3 + 1);
        let catalog =
            bongocat_plugin_manager::parse_catalog(&fs::read(&output).unwrap(), &keys.pk).unwrap();
        for entry in &catalog.plugins {
            let asset = entry.downloads.values().next().unwrap();
            assert_eq!(
                entry.sha256,
                Some(sha256_of(
                    &fs::read(output.parent().unwrap().join(asset)).unwrap()
                ))
            );
        }
        fs::copy(&output, history.join("old.json")).unwrap();
        publish_release_with_key(&workspace, &output, &inputs, &history, &signing, &keys.pk)
            .unwrap();
        let mut changed = catalog.clone();
        changed.plugins[0].sha256 = Some("0".repeat(64));
        assert!(validate_history(&changed, &history, &keys.pk).is_err());
        let mut downgraded = catalog;
        downgraded.plugins[0].version = "0.9.0".into();
        assert!(validate_history(&downgraded, &history, &keys.pk).is_err());
        let other = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        let absent = root.path().join("wrong-key/plugins.json");
        assert!(
            publish_release_with_key(&workspace, &absent, &inputs, &history, &signing, &other.pk)
                .is_err()
        );
        assert!(!absent.exists());
        fs::write(history.join("old.json"), b"{}").unwrap();
        assert!(validate_history(&changed, &history, &keys.pk).is_err());
        let first = fs::read_dir(&inputs[0].1)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(first).unwrap();
        assert!(
            publish_release_with_key(&workspace, &output, &inputs, &history, &signing, &keys.pk)
                .is_err()
        );
    }

    #[test]
    fn release_archive_must_contain_the_target_executable() {
        let root = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../../plugins/pomodoro/plugin.json");
        let manifest: bongocat_plugin_manager::PluginManifest =
            serde_json::from_slice(bytes).unwrap();
        let executable = root.path().join(&manifest.metadata.entry);
        fs::write(&executable, b"executable").unwrap();
        let package =
            super::super::dev_plugins::package_plugin(root.path(), &manifest, bytes, &executable)
                .unwrap();
        let archive = fs::read(package).unwrap();
        validate_release_archive(&archive, &manifest, ReleaseTarget::MacosAarch64).unwrap();
        assert!(
            validate_release_archive(&archive, &manifest, ReleaseTarget::WindowsX86_64).is_err()
        );
    }
    #[test]
    fn signing_is_required_and_the_mode_cannot_be_mixed_with_build_options() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            publish(
                &directory.path().join("plugins.json"),
                &[],
                &SigningConfig::new()
            )
            .is_err()
        );
        assert!(matches!(
            Invocation::parse(vec![
                "--plugin-catalog".into(),
                "out/plugins.json".into(),
                "macos-aarch64".into(),
                "target/plugins".into()
            ])
            .unwrap(),
            Invocation::PluginCatalog { .. }
        ));
        for args in [
            vec!["--plugin-catalog", "out"],
            vec!["--plugin-catalog", "out", "linux-x64", "target"],
            vec!["--plugin-catalog", "out", "--environment", "development"],
        ] {
            assert!(Invocation::parse(args.into_iter().map(String::from).collect()).is_err());
        }
    }
    #[test]
    fn published_catalog_verifies_with_the_updaters_key_format() {
        let directory = tempfile::tempdir().unwrap();
        let keys = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        let output = directory.path().join("plugins.json");
        let signing = SigningConfig::new().private_key(keys.sk).password("");
        publish(&output, &[], &signing).unwrap();
        assert!(
            bongocat_plugin_manager::parse_catalog(&fs::read(output).unwrap(), &keys.pk)
                .unwrap()
                .plugins
                .is_empty()
        );
    }

    #[test]
    fn key_sounds_catalog_and_asset_names_use_the_manifest_identity() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../../plugins/key-sounds/plugin.json");
        let manifest: bongocat_plugin_manager::PluginManifest =
            serde_json::from_slice(bytes).unwrap();
        let executable = workspace.path().join(&manifest.metadata.entry);
        fs::write(&executable, b"test executable").unwrap();
        super::super::dev_plugins::package_plugin(workspace.path(), &manifest, bytes, &executable)
            .unwrap();
        let keys = cargo_packager::sign::generate_key(Some(String::new())).unwrap();
        let signing = SigningConfig::new().private_key(keys.sk).password("");
        let output = directory.path().join("plugins.json");
        let target = ReleaseTarget::host().unwrap();
        publish(
            &output,
            &[(
                target,
                workspace
                    .path()
                    .join(super::super::dev_plugins::PLUGIN_OUTPUT_DIRECTORY),
            )],
            &signing,
        )
        .unwrap();
        let catalog =
            bongocat_plugin_manager::parse_catalog(&fs::read(output).unwrap(), &keys.pk).unwrap();
        assert_eq!(catalog.plugins.len(), 1);
        let entry = &catalog.plugins[0];
        assert_eq!(entry.id, "com.bongocat.key-sounds");
        assert_eq!(entry.name, "Key Sounds");
        assert_eq!(entry.localizations["zh-CN"].name, "按键音效");
        let name = format!(
            "com.bongocat.key-sounds-1.0.0-{}.zip",
            target.manifest_platform()
        );
        assert_eq!(entry.downloads[target.manifest_platform()], name);
        assert!(directory.path().join(name).is_file());
    }
}
