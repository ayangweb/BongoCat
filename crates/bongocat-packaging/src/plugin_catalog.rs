//! Signed plugin release assets use the product's existing signing toolchain.
use super::*;
use bongocat_plugin_manager::{Catalog, CatalogEntry, install::inspect_package, sha256_of};
use std::io::Write;

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
