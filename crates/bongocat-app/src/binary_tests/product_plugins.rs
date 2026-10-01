//! Where the plugin catalog is read from.

use super::*;

use bongocat_plugin::{LOCAL_BUILD_DIRECTORY, local_catalog_directory, local_plugin_directories};

/// A plugin directory in a catalog directory, with a manifest this host accepts.
fn written_plugin(catalog: &Path, id: &str) {
    let directory = catalog.join(id);
    std::fs::create_dir_all(&directory).expect("a plugin directory");
    std::fs::write(
        directory.join("plugin.json"),
        format!(
            r#"{{"schema_version":1,"api_version":1,"id":"{id}","name":"{id}",
                "version":"1.0.0","executable":"{id}","icon":{{"emoji":"🧩"}}}}"#
        ),
    )
    .expect("a manifest is written");
    std::fs::create_dir_all(catalog.join(LOCAL_BUILD_DIRECTORY)).expect("a build directory");
    std::fs::write(
        catalog
            .join(LOCAL_BUILD_DIRECTORY)
            .join(format!("{id}.zip")),
        b"pk",
    )
    .expect("a packed archive is written");
}

#[test]
fn a_development_run_finds_the_repository_plugins_without_any_setup() {
    // The bug this exists for: the worker read a data directory that nothing ever
    // wrote to, so a developer who cloned the repository and ran the product saw an
    // empty plugin center with no way to tell that reference plugins were sitting
    // in the tree two directories away.
    let data = repository_plugin_catalog().expect("this binary was built in a repository");
    assert_eq!(
        data,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("plugins")
    );
    let plugins = local_plugin_directories(&data);
    assert!(
        !plugins.is_empty(),
        "the repository's own plugins directory must actually hold the plugins this fallback \\
         looks for, at {}",
        data.display()
    );
    for plugin in &plugins {
        assert!(
            plugin.join("plugin.json").is_file(),
            "and a plugin is a directory with a manifest in it, not a convention: {}",
            plugin.display()
        );
    }
}

#[test]
fn a_data_root_plugin_is_never_overridden_by_the_repository() {
    // The data root is where a developer's own plugin goes, and it has to win: a
    // developer testing their plugin against the shipped ones would otherwise
    // never see theirs.
    let mut layout = test_layout("data-root-wins");
    let data = local_catalog_directory(&layout.root);
    written_plugin(&data, "my-own-plugin");

    let resolved = catalog_directory(&layout);

    assert_eq!(
        resolved, data,
        "a plugin in the data root is the developer's own and outranks the shipped ones"
    );
    // Left in place rather than cleaned up: the layout is this test's own directory,
    // and removing it here would only be undone by the next run's own cleanup.
    layout.root = PathBuf::from("/nonexistent");
}

#[test]
fn a_data_root_with_no_plugin_falls_back_to_the_repository() {
    let layout = test_layout("repository-fallback");
    // Deliberately not created: the data root has no plugin, which is what a
    // developer who has never touched a plugin looks like.
    assert_eq!(
        catalog_directory(&layout),
        repository_plugin_catalog().expect("this binary was built in a repository"),
        "with nothing in the data root, the repository's own plugins are the ones to read"
    );
}

#[test]
fn a_data_root_holding_a_directory_that_is_not_a_plugin_still_falls_back() {
    // The data root is a directory the product owns, so it will hold other things over
    // time. A folder that is not a plugin must not be read as one, or the fallback would
    // stop working the first time the product wrote something there.
    let mut layout = test_layout("not-a-plugin");
    let data = local_catalog_directory(&layout.root);
    std::fs::create_dir_all(data.join("something-else")).expect("an unrelated directory");
    assert_eq!(
        catalog_directory(&layout),
        repository_plugin_catalog().expect("this binary was built in a repository"),
        "because a plugin is a directory with a manifest in it"
    );
    layout.root = PathBuf::from("/nonexistent");
}

/// A storage layout rooted at a directory this test alone owns.
///
/// Named per test rather than shared, because the tests above deliberately put
/// *different* things in the data root — one writes a plugin and two assert there is
/// none — and a shared root would have them overwrite each other's answer. Cargo runs
/// a binary's tests on parallel threads, so that is a real flake and not a theoretical
/// one.
fn test_layout(name: &str) -> bongocat_config::StorageLayout {
    let root = std::env::temp_dir().join(format!(
        "bongocat-plugin-catalog-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    bongocat_config::StorageLayout::under_application_root(
        root,
        bongocat_config::BuildEnvironment::Development,
    )
}
