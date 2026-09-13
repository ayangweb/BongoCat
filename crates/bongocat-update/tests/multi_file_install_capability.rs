//! Capability contract for the multi-file install path documented in ADR-0029.
//!
//! `self_update::github::Update::update()` replaces either one executable or one macOS bundle, so a
//! Windows update cannot carry `resources/` with it. ADR-0029 records `self_update::MoveAll` as the
//! resolution: it is the library's own transactional multi-file installer, exported from the crate
//! root, and it either applies every queued `source -> dest` move or rolls all of them back.
//!
//! Production code does not call `MoveAll` yet — the update entry point is still gated off until a
//! release signing key and a real release archive exist. These tests pin the semantics that the
//! documented plan depends on, so a future `self_update` upgrade that quietly drops the rollback
//! guarantee fails here instead of in a half-applied install.

use std::fs;
use std::path::{Path, PathBuf};

const BINARY: &str = "BongoCat.exe";
const RESOURCE: &str = "resources/models/standard/a.moc3";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bongocat-multi-file-install-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent directory")).expect("parent directory");
    fs::write(path, contents).expect("write file");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read file")
}

/// Layout shared by both cases: a staging tree, an installed tree, and the stash directory.
///
/// `MoveAll` renames, so the stash must sit on the same filesystem as the destinations — the
/// `$TMPDIR` scratch root satisfies that here and is what the documented plan requires too.
fn trees(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = scratch(name);
    let staged = root.join("staged");
    let installed = root.join("installed");
    let stash = root.join("stash");
    fs::create_dir_all(&stash).expect("stash directory");
    (staged, installed, stash)
}

#[test]
fn commit_replaces_the_binary_and_the_resources_together() {
    let (staged, installed, stash) = trees("commit");

    write(&staged.join(BINARY), "new-binary");
    write(&staged.join(RESOURCE), "new-model");
    write(&installed.join(BINARY), "old-binary");
    write(&installed.join(RESOURCE), "old-model");

    self_update::MoveAll::from_temp(&stash)
        .add(staged.join(BINARY), installed.join(BINARY))
        .add(staged.join(RESOURCE), installed.join(RESOURCE))
        .commit()
        .expect("commit");

    assert_eq!(read(&installed.join(BINARY)), "new-binary");
    assert_eq!(read(&installed.join(RESOURCE)), "new-model");
}

#[test]
fn a_failed_commit_restores_every_destination() {
    let (staged, installed, stash) = trees("rollback");

    write(&staged.join(BINARY), "new-binary");
    write(&installed.join(BINARY), "old-binary");
    write(&installed.join(RESOURCE), "old-model");

    // The resource source is deliberately absent, so the second move fails after the first applied.
    let error = self_update::MoveAll::from_temp(&stash)
        .add(staged.join(BINARY), installed.join(BINARY))
        .add(staged.join(RESOURCE), installed.join(RESOURCE))
        .commit()
        .expect_err("a missing source must fail the commit");

    assert!(
        matches!(error, self_update::Error::Io(_)),
        "expected an io error, got {error:?}"
    );
    assert_eq!(
        read(&installed.join(BINARY)),
        "old-binary",
        "the already-applied move must be rolled back"
    );
    assert_eq!(
        read(&installed.join(RESOURCE)),
        "old-model",
        "a destination the commit never reached must keep its contents"
    );
}
