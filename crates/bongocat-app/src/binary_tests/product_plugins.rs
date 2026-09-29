//! Where the plugin catalog is read from.

use super::*;

use bongocat_plugin::{PLUGIN_CATALOG_FILE_NAME, local_catalog_directory};

#[test]
fn a_development_run_finds_the_repository_catalog_without_any_setup() {
    // The bug this exists for: the worker read a data directory that nothing ever
    // wrote to, so a developer who cloned the repository and ran the product saw an
    // empty plugin center with no way to tell that a reference plugin was sitting
    // in the tree two directories away.
    let data = repository_plugin_catalog().expect("this binary was built in a repository");
    assert!(
        data.join(PLUGIN_CATALOG_FILE_NAME).is_file(),
        "the repository's own plugins directory must actually hold the catalog \
         this fallback looks for, at {}",
        data.display()
    );
    assert_eq!(
        data,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("plugins")
    );
}

#[test]
fn a_data_root_catalog_is_never_overridden_by_the_repository() {
    // The data root is where a developer's own archive goes, and it has to win: a
    // developer testing their plugin against the shipped example would otherwise
    // never see theirs.
    let mut layout = test_layout();
    let data = local_catalog_directory(&layout.root);
    std::fs::create_dir_all(&data).expect("the data catalog directory");
    std::fs::write(data.join(PLUGIN_CATALOG_FILE_NAME), "{}").expect("a catalog is written");

    let resolved = catalog_directory(&layout);

    assert_eq!(
        resolved, data,
        "a catalog in the data root is the developer's own and outranks the example"
    );
    layout.root = PathBuf::from("/nonexistent");
}

#[test]
fn an_empty_data_root_falls_back_to_the_repository() {
    let layout = test_layout();
    // Deliberately not created: the data root has no catalog, which is what a
    // developer who has never touched a plugin looks like.
    assert_eq!(
        catalog_directory(&layout),
        repository_plugin_catalog().expect("this binary was built in a repository"),
        "with nothing in the data root, the repository's own catalog is the one to read"
    );
}

fn test_layout() -> bongocat_config::StorageLayout {
    let root = std::env::temp_dir().join(format!(
        "bongocat-plugin-catalog-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    bongocat_config::StorageLayout::under_application_root(
        root,
        bongocat_config::BuildEnvironment::Development,
    )
}
