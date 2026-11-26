use super::*;

fn discoverable_package(root: &Path) {
    write_package_directory(root);
    let keys = root.join("resources/left-keys");
    fs::create_dir_all(&keys).expect("keys");
    fs::write(keys.join("KeyA.png"), png_header(4, 4)).expect("key");
}

#[test]
fn nested_discovery_finds_valid_models_in_path_order_and_imports_only_their_roots() {
    let data = tempdir().unwrap();
    let store = model_store(data.path());
    let sources = tempdir().unwrap();
    let a = sources.path().join("a/deeper/同名");
    let b = sources.path().join("b/同名");
    discoverable_package(&b);
    discoverable_package(&a);
    write_package_directory(&sources.path().join("no-key-art"));
    let invalid = sources.path().join("broken");
    fs::create_dir(&invalid).unwrap();
    fs::write(invalid.join("cat.model3.json"), b"broken").unwrap();
    let nested_valid = invalid.join("valid-child");
    discoverable_package(&nested_valid);
    fs::write(sources.path().join("README.txt"), b"collection notes").unwrap();
    let candidates = store.discover_sources(sources.path()).unwrap();
    assert_eq!(candidates.len(), 3);
    assert_eq!(candidates[0].relative_path, Path::new("a/deeper/同名"));
    assert_eq!(candidates[1].relative_path, Path::new("b/同名"));
    assert_eq!(candidates[2].relative_path, Path::new("broken/valid-child"));
    assert_store_holds_no_entries(&store);
    for (index, candidate) in candidates.iter().enumerate() {
        assert_eq!(candidate.content, ModelSourceContent::Package);
        let installed = store
            .import(
                ModelId::parse(format!("nested-{index}")).unwrap(),
                &candidate.source_root,
            )
            .unwrap();
        assert!(installed.root().join("cat.model3.json").is_file());
        assert!(!installed.root().join("README.txt").exists());
        assert!(candidate.source_root.join("cat.model3.json").is_file());
    }
}

#[test]
fn a_single_deep_model_is_one_candidate_and_a_direct_package_stays_one_root() {
    let data = tempdir().unwrap();
    let store = model_store(data.path());
    let sources = tempdir().unwrap();
    let source = sources.path().join("one/two/three/猫");
    discoverable_package(&source);
    let found = store.discover_sources(sources.path()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source_root, source.canonicalize().unwrap());
    // A package's internal directories never become separate sources.
    discoverable_package(&source.join("assets/another-model"));
    let direct = store.discover_sources(&source).unwrap();
    assert_eq!(direct.len(), 1);
    assert!(direct[0].relative_path.as_os_str().is_empty());
}

#[test]
fn a_nested_mver_source_stays_one_candidate_with_all_its_modes() {
    let data = tempdir().unwrap();
    let store = model_store(data.path());
    let sources = tempdir().unwrap();
    let legacy = sources.path().join("collection/legacy");
    crate::mver::fixture::legacy_source(&legacy, &crate::mver::fixture::all_modes(), true);
    discoverable_package(&sources.path().join("ordinary"));
    let found = store.discover_sources(sources.path()).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].relative_path, Path::new("collection/legacy"));
    assert_eq!(
        found[0].content,
        ModelSourceContent::Mver {
            modes: MverInputMode::ALL.to_vec()
        }
    );
    assert_store_holds_no_entries(&store);
}

#[test]
fn discovery_bounds_depth_and_counts_empty_directories_against_the_entry_budget() {
    let data = tempdir().unwrap();
    let mut store = model_store(data.path());
    let sources = tempdir().unwrap();
    fs::create_dir_all(sources.path().join("a/b/c")).unwrap();
    store.limits.maximum_directory_depth = 2;
    assert_eq!(
        store.discover_sources(sources.path()).unwrap_err().code,
        ModelStoreDiagnostic::InvalidPackage
    );
    store.limits.maximum_directory_depth = 32;
    store.limits.maximum_file_count = 2;
    assert_eq!(
        store.discover_sources(sources.path()).unwrap_err().code,
        ModelStoreDiagnostic::InvalidPackage
    );
    assert_store_holds_no_entries(&store);
}

#[test]
fn discovery_rejects_a_source_containing_the_store_and_keeps_empty_sources_empty() {
    let data = tempdir().unwrap();
    let store = model_store(data.path());
    assert_eq!(
        store.discover_sources(data.path()).unwrap_err().code,
        ModelStoreDiagnostic::SourceContainsStore
    );
    let empty = tempdir().unwrap();
    assert!(store.discover_sources(empty.path()).unwrap().is_empty());
    assert_store_holds_no_entries(&store);
}

#[test]
fn discovery_bounds_resources_before_mver_detection_and_package_validation() {
    let data = tempdir().unwrap();
    let mut store = model_store(data.path());
    let sources = tempdir().unwrap();
    crate::mver::fixture::legacy_source(
        &sources.path().join("legacy"),
        &crate::mver::fixture::all_modes(),
        true,
    );
    store.limits.maximum_file_count = 5;
    assert_eq!(
        store.discover_sources(sources.path()).unwrap_err().code,
        ModelStoreDiagnostic::InvalidPackage
    );
    assert_store_holds_no_entries(&store);
    let package = tempdir().unwrap();
    discoverable_package(package.path());
    fs::create_dir_all(package.path().join("assets/a/b/c")).unwrap();
    store.limits.maximum_file_count = 4096;
    store.limits.maximum_directory_depth = 3;
    assert_eq!(
        store.discover_sources(package.path()).unwrap_err().code,
        ModelStoreDiagnostic::InvalidPackage
    );
}

#[cfg(target_os = "macos")]
#[test]
fn discovery_rejects_symlinked_children_even_when_they_point_inside_the_folder() {
    let data = tempdir().unwrap();
    let store = model_store(data.path());
    let sources = tempdir().unwrap();
    discoverable_package(&sources.path().join("model"));
    std::os::unix::fs::symlink(sources.path().join("model"), sources.path().join("alias")).unwrap();
    assert_eq!(
        store.discover_sources(sources.path()).unwrap_err().code,
        ModelStoreDiagnostic::SourceSymlinkUnsupported
    );
    assert_store_holds_no_entries(&store);
}
