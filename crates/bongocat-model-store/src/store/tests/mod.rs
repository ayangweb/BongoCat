//! The store's tests, split by the module they cover.
//!
//! The package builders live here rather than in each module: turning a fixture
//! directory into a package on disk is the same three helpers wherever the
//! assertion is, and repeating them per module would say less than writing them
//! once.

use super::*;

use std::cell::{Cell, RefCell};
use std::fs::TryLockError;
use std::path::Path;
use tempfile::tempdir;

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

fn model_store(base: &Path) -> ModelStore {
    ModelStore::new(
        base.join("models"),
        base.join("locks/models.writer.lock"),
        ModelPackageLimits::default(),
    )
    .expect("model store")
}

// A model source is the folder a user picked. The archive cases this module
// used to carry were removed with the archive source itself (ADR-0036 已撤回);
// what is left exercises the folder path end to end.

const SAMPLE_MODEL_JSON: &[u8] = br#"{
  "Version": 3,
  "FileReferences": {
    "Moc": "model.moc3",
    "Textures": ["textures/texture_00.png"]
  },
  "Groups": [
    {"Target": "Parameter", "Name": "EyeBlink", "Ids": ["ParamEyeLOpen"]}
  ]
}"#;

/// The 24 bytes the package parser inspects: the PNG signature and an IHDR
/// chunk carrying the declared dimensions.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::from(*b"\x89PNG\r\n\x1a\n");
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes
}

fn sample_package_entries() -> Vec<(String, Vec<u8>)> {
    vec![
        ("cat.model3.json".to_owned(), SAMPLE_MODEL_JSON.to_vec()),
        ("model.moc3".to_owned(), b"moc3".to_vec()),
        ("textures/texture_00.png".to_owned(), png_header(1024, 1024)),
    ]
}

fn write_package_directory(root: &Path) {
    for (reference, bytes) in sample_package_entries() {
        let path = root.join(&reference);
        fs::create_dir_all(path.parent().expect("reference parent"))
            .expect("create package directory");
        fs::write(path, bytes).expect("write package file");
    }
}

fn assert_store_holds_no_entries(store: &ModelStore) {
    assert!(
        fs::read_dir(store.root())
            .expect("store entries")
            .next()
            .is_none(),
        "a rejected import must not leave staging or destination entries"
    );
}

fn left_key_name(mode: MverInputMode) -> &'static str {
    match mode {
        MverInputMode::Standard | MverInputMode::Keyboard => "KeyA",
        // The first left-hand gamepad binding of `fixture::all_modes` is
        // XInput button 4, the left shoulder button.
        MverInputMode::Gamepad => "LeftShoulder",
    }
}

/// Read a source folder's key images without importing anything, so a test
/// can compare them with what an import installed. Keys are package-relative
/// references.
fn source_key_images(source: &Path) -> std::collections::BTreeMap<(String, String), Vec<u8>> {
    use std::collections::BTreeMap;

    const DIRECTORIES: [&str; 2] = ["resources/left-keys", "resources/right-keys"];
    let mut images = BTreeMap::new();
    for directory in DIRECTORIES {
        let Ok(entries) = fs::read_dir(source.join(directory)) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("source key image");
            if entry.file_type().expect("source entry type").is_dir() {
                continue;
            }
            images.insert(
                (
                    directory.to_owned(),
                    entry.file_name().into_string().expect("key image name"),
                ),
                fs::read(entry.path()).expect("read source key image"),
            );
        }
    }
    images
}

mod catalog;
mod copy;
mod diagnostic;
mod input_mode;
mod progress;
mod root;
mod staging;
