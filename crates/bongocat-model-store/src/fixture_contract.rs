use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use bongocat_model::{ModelId, ModelPackageLimits, normalize_reference, path_from_reference};
use serde::Deserialize;
use tempfile::{TempDir, tempdir};

use crate::{ModelStore, ModelStoreDiagnostic};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u32,
    limits: FixtureLimits,
    cases: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureLimits {
    maximum_texture_dimension: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCase {
    id: String,
    directory: String,
    #[serde(default)]
    entry_source: Option<String>,
    #[serde(default)]
    materialized_entry: Option<String>,
    #[serde(default)]
    materialize: Vec<FixtureMaterialization>,
    expected: FixtureExpectation,
    #[serde(default)]
    expected_diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureMaterialization {
    source: String,
    target: String,
    encoding: FixtureEncoding,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureEncoding {
    Hex,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureExpectation {
    Accept,
    Reject,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_owned()
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("fixture destination");
    for entry in fs::read_dir(source).expect("fixture source") {
        let entry = entry.expect("fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("fixture file type");
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            assert!(file_type.is_file(), "fixture must not contain links");
            fs::copy(source_path, destination_path).expect("copy fixture file");
        }
    }
}

fn decode_hex(value: &str) -> Vec<u8> {
    let value = value.trim().as_bytes();
    assert!(value.len().is_multiple_of(2), "fixture hex pairs");
    value
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                .expect("fixture hex")
        })
        .collect()
}

fn fixture_path(root: &Path, reference: &str) -> PathBuf {
    let normalized = normalize_reference(reference).expect("fixture reference");
    root.join(path_from_reference(&normalized))
}

fn materialize_case(root: &Path, case: &FixtureCase) -> TempDir {
    let package = tempdir().expect("fixture package");
    copy_tree(&root.join(&case.directory), package.path());
    match (&case.entry_source, &case.materialized_entry) {
        (Some(source), Some(target)) => {
            fs::copy(
                fixture_path(package.path(), source),
                fixture_path(package.path(), target),
            )
            .expect("materialize fixture entry");
        }
        (None, None) => {}
        _ => panic!("fixture entry source and target must be paired"),
    }
    for materialization in &case.materialize {
        let bytes = match materialization.encoding {
            FixtureEncoding::Hex => decode_hex(
                &fs::read_to_string(fixture_path(package.path(), &materialization.source))
                    .expect("read fixture hex"),
            ),
        };
        let target = fixture_path(package.path(), &materialization.target);
        fs::create_dir_all(target.parent().expect("fixture target parent"))
            .expect("create fixture target parent");
        fs::write(target, bytes).expect("write fixture materialization");
    }
    package
}

fn snapshot_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("fixture snapshot") {
            let entry = entry.expect("fixture snapshot entry");
            let path = entry.path();
            let file_type = entry.file_type().expect("fixture snapshot type");
            if file_type.is_dir() {
                visit(root, &path, snapshot);
            } else {
                assert!(
                    file_type.is_file(),
                    "fixture snapshot must not contain links"
                );
                let reference = path
                    .strip_prefix(root)
                    .expect("fixture relative path")
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                snapshot.insert(reference, fs::read(path).expect("fixture snapshot bytes"));
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[test]
fn every_registered_fixture_preserves_the_store_contract() {
    let root = repository_root().join("shared/fixtures/model-fixtures");
    let manifest: FixtureManifest =
        serde_json::from_slice(&fs::read(root.join("cases.json")).expect("read fixture manifest"))
            .expect("fixture manifest");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.limits.maximum_texture_dimension,
        ModelPackageLimits::default().maximum_texture_dimension
    );
    assert!(!manifest.cases.is_empty());

    let mut registered = BTreeSet::new();
    for (index, case) in manifest.cases.iter().enumerate() {
        assert!(
            registered.insert(case.directory.clone()),
            "duplicate fixture directory"
        );
        let package = materialize_case(&root.join("cases"), case);
        let source_before = snapshot_tree(package.path());
        let data = tempdir().expect("fixture store root");
        let store = ModelStore::new(
            data.path().join("models"),
            data.path().join("locks/models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("fixture model store");
        let id = ModelId::parse(format!("fixture-{index}")).expect("fixture model id");

        match case.expected {
            FixtureExpectation::Accept => {
                let installed = store
                    .import(id.clone(), package.path())
                    .expect("accepted fixture import");
                assert_eq!(installed.id(), &id);
                assert_eq!(store.list().expect("fixture catalog").entries.len(), 1);
            }
            FixtureExpectation::Reject => {
                let error = store
                    .import(id, package.path())
                    .expect_err("rejected fixture import");
                assert_eq!(error.code, ModelStoreDiagnostic::InvalidPackage);
                for diagnostic in &case.expected_diagnostics {
                    assert!(
                        error.detail.contains(diagnostic),
                        "fixture {} lost diagnostic {diagnostic}: {}",
                        case.id,
                        error.detail
                    );
                }
                assert!(
                    store
                        .list()
                        .expect("empty fixture catalog")
                        .entries
                        .is_empty()
                );
                assert!(
                    fs::read_dir(store.root())
                        .expect("fixture store entries")
                        .next()
                        .is_none()
                );
            }
        }
        assert_eq!(
            snapshot_tree(package.path()),
            source_before,
            "fixture {} source changed",
            case.id
        );
    }
}
