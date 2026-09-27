//! The model's tests, split by the module they cover.
//!
//! The fixture harness lives here rather than in each module: it is how a test
//! turns a directory under `shared/fixtures` into a package on disk, and one
//! test reaching the crate root, the module it covers and that harness is the
//! same list eight times over.

use super::*;

use proptest::prelude::*;
use std::fs;
use tempfile::tempdir;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FixtureStage {
    PackageDiscovery,
    JsonParse,
    ReferenceResolution,
    TextureHeader,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FixtureExpectation {
    Accept,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureLimits {
    pub(crate) maximum_texture_dimension: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureEncoding {
    Hex,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureMaterialization {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) encoding: FixtureEncoding,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureCase {
    pub(crate) id: String,
    pub(crate) directory: String,
    pub(crate) stage: FixtureStage,
    pub(crate) entry_source: Option<String>,
    pub(crate) materialized_entry: Option<String>,
    #[serde(default)]
    pub(crate) materialize: Vec<FixtureMaterialization>,
    pub(crate) expected: FixtureExpectation,
    pub(crate) expected_diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureManifest {
    pub(crate) schema_version: u32,
    pub(crate) limits: FixtureLimits,
    pub(crate) cases: Vec<FixtureCase>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_owned()
}

fn fixture(name: &str) -> PathBuf {
    repository_root()
        .join("shared/fixtures/model-fixtures/cases")
        .join(name)
}

fn copy_fixture_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("list fixture source") {
        let entry = entry.expect("read fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read fixture file type");
        if file_type.is_dir() {
            copy_fixture_tree(&source_path, &destination_path);
        } else {
            assert!(
                file_type.is_file(),
                "fixture entries must be files or directories"
            );
            fs::copy(source_path, destination_path).expect("copy fixture file");
        }
    }
}

fn decode_fixture_hex(value: &str) -> Vec<u8> {
    let value = value.trim().as_bytes();
    assert!(
        value.len().is_multiple_of(2),
        "fixture hex must contain pairs"
    );
    value
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex pair"), 16)
                .expect("valid fixture hex")
        })
        .collect()
}

fn fixture_materialization_path(root: &Path, reference: &str) -> PathBuf {
    let normalized = normalize_reference(reference).expect("fixture materialization reference");
    assert_eq!(
        normalized, reference,
        "fixture references must be normalized"
    );
    root.join(path_from_reference(&normalized))
}

fn materialize_fixture_case(case: &FixtureCase) -> tempfile::TempDir {
    let temporary = tempdir().expect("fixture package");
    copy_fixture_tree(&fixture(&case.directory), temporary.path());
    match (&case.entry_source, &case.materialized_entry) {
        (Some(source), Some(target)) => {
            fs::copy(
                fixture_materialization_path(temporary.path(), source),
                fixture_materialization_path(temporary.path(), target),
            )
            .expect("materialize fixture entry");
        }
        (None, None) => {}
        _ => panic!("fixture entry source and target must be paired"),
    }
    for materialization in &case.materialize {
        let source = fixture_materialization_path(temporary.path(), &materialization.source);
        let target = fixture_materialization_path(temporary.path(), &materialization.target);
        fs::create_dir_all(target.parent().expect("materialization parent"))
            .expect("create materialization parent");
        let bytes = match materialization.encoding {
            FixtureEncoding::Hex => decode_fixture_hex(
                &fs::read_to_string(source).expect("read fixture materialization"),
            ),
        };
        fs::write(target, bytes).expect("write fixture materialization");
    }
    temporary
}

fn snapshot_fixture_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("list fixture snapshot") {
            let entry = entry.expect("read fixture snapshot entry");
            let path = entry.path();
            let file_type = entry.file_type().expect("read fixture snapshot type");
            if file_type.is_dir() {
                visit(root, &path, snapshot);
            } else {
                assert!(
                    file_type.is_file(),
                    "fixture snapshot must not contain links"
                );
                let reference = relative_reference(root, &path).expect("fixture reference");
                snapshot.insert(
                    reference,
                    fs::read(path).expect("read fixture snapshot file"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn stage_accepts_diagnostic(stage: FixtureStage, diagnostic: ModelDiagnostic) -> bool {
    match stage {
        FixtureStage::PackageDiscovery => matches!(
            diagnostic,
            ModelDiagnostic::ModelEntryAmbiguous | ModelDiagnostic::ModelEntryMissing
        ),
        FixtureStage::JsonParse => matches!(
            diagnostic,
            ModelDiagnostic::ModelJsonInvalid
                | ModelDiagnostic::ModelJsonTooLarge
                | ModelDiagnostic::ModelUnsupportedVersion
        ),
        FixtureStage::ReferenceResolution => matches!(
            diagnostic,
            ModelDiagnostic::ModelMocMissing
                | ModelDiagnostic::ModelReferenceEscapesRoot
                | ModelDiagnostic::ModelReferenceInvalid
                | ModelDiagnostic::ModelReferenceSymlinkEscape
                | ModelDiagnostic::ModelResourceInvalid
                | ModelDiagnostic::ModelResourceMissing
                | ModelDiagnostic::ModelResourceNotFile
                | ModelDiagnostic::ModelSymlinkDirectoryUnsupported
                | ModelDiagnostic::ModelTextureMissing
        ),
        FixtureStage::TextureHeader => matches!(
            diagnostic,
            ModelDiagnostic::ModelTextureDimensionExceeded
                | ModelDiagnostic::ModelTextureInvalidPng
        ),
    }
}

mod catalog;
mod formats;
mod json;
mod limits;
mod package;
mod reference;
mod snapshot;
mod validate;
