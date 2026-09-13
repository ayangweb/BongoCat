//! Capability contract for the release-archive layouts ADR-0029 requires.
//!
//! `self_update` extracts exactly one path out of a downloaded archive, and which path that is comes
//! from the configuration:
//!
//! * macOS bundle mode uses `bundle_path_in_archive`, so the archive must root at `BongoCat.app/`.
//! * single-binary mode derives the path from `bin_name` plus the platform executable suffix, so a
//!   Windows archive must root at `bongocat-app.exe`.
//!
//! Nothing in the packaging scripts produces those archives yet, and the compiler cannot see the
//! coupling. These tests build both layouts and extract them with the same `self_update::Extract`
//! call the library makes internally, so a layout that `Extract` cannot consume fails here rather
//! than on the first real update.
//!
//! They cover layout only. Signature verification, the network download and the actual install are
//! not exercised — see the ADR's verification section for what remains unverified.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const BUNDLE_NAME: &str = "BongoCat.app";
const BUNDLE_EXECUTABLE: &str = "BongoCat.app/Contents/MacOS/bongocat-app";
const BUNDLE_RESOURCE: &str = "BongoCat.app/Contents/Resources/models/standard/a.moc3";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bongocat-archive-layout-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// Write a stored-only zip so the test does not depend on a compression feature.
fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let file = fs::File::create(path).expect("create archive");
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer.start_file(*name, options).expect("start entry");
        writer
            .write_all(contents.as_bytes())
            .expect("write entry contents");
    }
    writer.finish().expect("finish archive");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read extracted file")
}

#[test]
fn the_documented_macos_layout_extracts_the_whole_bundle() {
    let root = scratch("macos");
    let archive = root.join(format!(
        "BongoCat-{}-aarch64-apple-darwin.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let into = root.join("extracted");
    fs::create_dir_all(&into).expect("extraction directory");

    write_zip(
        &archive,
        &[
            (BUNDLE_EXECUTABLE, "bundle-binary"),
            (BUNDLE_RESOURCE, "bundle-model"),
        ],
    );

    // The same call `self_update` makes in bundle mode: it extracts the whole archive and then
    // takes `bundle_path_in_archive` out of the extraction root.
    self_update::Extract::from_source(&archive)
        .extract_into(&into)
        .expect("extract the archive");

    assert!(
        into.join(BUNDLE_NAME).is_dir(),
        "bundle mode requires {BUNDLE_NAME}/ at the archive root"
    );
    assert_eq!(read(&into.join(BUNDLE_EXECUTABLE)), "bundle-binary");
    assert_eq!(
        read(&into.join(BUNDLE_RESOURCE)),
        "bundle-model",
        "bundle mode must carry resources, which single-binary mode cannot"
    );
}

#[test]
fn the_documented_windows_layout_extracts_the_executable_at_the_archive_root() {
    let root = scratch("windows");
    let archive = root.join(format!(
        "BongoCat-{}-x86_64-pc-windows-msvc.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let into = root.join("extracted");
    fs::create_dir_all(&into).expect("extraction directory");

    // `bin_path_in_archive` for `bin_name("bongocat-app")` on Windows.
    let path_in_archive = format!("bongocat-app{}", std::env::consts::EXE_SUFFIX);
    write_zip(&archive, &[(&path_in_archive, "release-binary")]);

    self_update::Extract::from_source(&archive)
        .extract_file(&into, &path_in_archive)
        .expect("extract the executable");

    assert_eq!(read(&into.join(&path_in_archive)), "release-binary");
}

#[test]
fn an_executable_at_the_archive_root_is_not_reachable_when_nested() {
    let root = scratch("nested");
    let archive = root.join(format!(
        "BongoCat-{}-x86_64-pc-windows-msvc.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let into = root.join("extracted");
    fs::create_dir_all(&into).expect("extraction directory");

    // A wrapper directory is a common packaging slip; it must fail rather than silently install the
    // wrong file, because `Extract` looks for the exact path it was configured with.
    let path_in_archive = format!("bongocat-app{}", std::env::consts::EXE_SUFFIX);
    let nested = format!("BongoCat/{path_in_archive}");
    write_zip(&archive, &[(&nested, "release-binary")]);

    self_update::Extract::from_source(&archive)
        .extract_file(&into, &path_in_archive)
        .expect_err("a nested executable must not satisfy the root-level path");

    // `expect_err` above is the contract: a missing root-level path is a hard failure, never a
    // silent fallback to some other entry. The error variant is backend-specific (`Zip(_)` for zip,
    // `Io(_)` for tar), so only the failure and the absence of extraction are asserted.
    assert!(
        !into.join(&path_in_archive).exists(),
        "nothing may be extracted when the configured path is absent"
    );
    assert_eq!(
        fs::read_dir(&into)
            .expect("read extraction directory")
            .count(),
        0,
        "a failed extraction must leave the destination empty"
    );
}
