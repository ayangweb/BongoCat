#![allow(clippy::print_stdout)]

#[path = "src/product_icon_contract.rs"]
mod product_icon_contract;

use product_icon_contract::{validate_icns, validate_ico, validate_png};
use std::path::{Path, PathBuf};

const WINDOWS_RESOURCE_FILE: &str = "windows/bongocat-app.rc";

fn main() {
    println!("cargo::rerun-if-changed=src/product_icon_contract.rs");
    link_swift_runtime();

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    );
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        embed_linux_presets(&manifest_dir);
    }
    let resources_dir = manifest_dir.join("../../resources/icons");
    validate_icon(
        &resources_dir.join("logo-macos.icns"),
        "macOS product icon",
        validate_icns,
    );
    validate_icon(
        &resources_dir.join("logo-windows.ico"),
        "Windows product icon",
        validate_ico,
    );
    validate_icon(
        &resources_dir.join("tray-macos.png"),
        "macOS status icon",
        validate_png,
    );
    validate_icon(
        &resources_dir.join("tray-windows.png"),
        "Windows status icon",
        validate_png,
    );

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo::rerun-if-changed={WINDOWS_RESOURCE_FILE}");
        let version = std::env::var("CARGO_PKG_VERSION")
            .expect("Cargo must provide CARGO_PKG_VERSION to the build script");
        let version_major = std::env::var("CARGO_PKG_VERSION_MAJOR")
            .expect("Cargo must provide CARGO_PKG_VERSION_MAJOR to the build script");
        let version_minor = std::env::var("CARGO_PKG_VERSION_MINOR")
            .expect("Cargo must provide CARGO_PKG_VERSION_MINOR to the build script");
        let version_patch = std::env::var("CARGO_PKG_VERSION_PATCH")
            .expect("Cargo must provide CARGO_PKG_VERSION_PATCH to the build script");
        let version_parameters = [
            format!("VERSION=\"{version}\""),
            format!("VERSION_MAJOR={version_major}"),
            format!("VERSION_MINOR={version_minor}"),
            format!("VERSION_PATCH={version_patch}"),
        ];
        embed_resource::compile(WINDOWS_RESOURCE_FILE, version_parameters)
            .manifest_required()
            .unwrap_or_else(|error| panic!("Windows product resource compilation failed: {error}"));
    }
}

/// Puts the Swift runtime on the loader path of this package's binaries.
///
/// The macOS permission flow links a Swift static library, and every binary that links it needs
/// `@rpath/libswift_Concurrency.dylib` and friends at load time. `permission-flow`'s own build
/// script emits the same flag, but a library dependency's `rustc-link-arg` does not reach the
/// binary that finally links, so each package whose binaries link it has to add it — see
/// `tools/tests/test_swift_runtime_rpath.py`. Without it the process aborts before `main`:
///
///   dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
///   Reason: no LC_RPATH's found
fn link_swift_runtime() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo::rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}

fn validate_icon(path: &Path, description: &str, validate: fn(&[u8]) -> Result<(), &'static str>) {
    println!("cargo::rerun-if-changed={}", path.display());
    let bytes = std::fs::read(path).unwrap_or_else(|error| {
        panic!(
            "could not read {description} at {}: {error}",
            path.display()
        )
    });
    validate(&bytes)
        .unwrap_or_else(|error| panic!("invalid {description} at {}: {error}", path.display()));
}

fn embed_linux_presets(manifest_dir: &Path) {
    fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("read bundled models") {
            let entry = entry.expect("read model entry");
            let kind = entry.file_type().expect("model entry type");
            if kind.is_dir() {
                collect(&entry.path(), files);
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    let root = manifest_dir.join("../../resources/models");
    let mut files = Vec::new();
    for model in ["standard", "keyboard", "gamepad"] {
        let directory = root.join(model);
        println!("cargo::rerun-if-changed={}", directory.display());
        collect(&directory, &mut files);
    }
    files.sort();
    let mut generated = String::from("const PRESET_FILES: &[(&str, &[u8])] = &[\n");
    for file in files {
        let relative = file.strip_prefix(&root).expect("bundled model path");
        generated.push_str(&format!(
            "({:?}, include_bytes!({:?})),\n",
            relative.to_str().expect("model path UTF-8"),
            file.to_str().expect("model path UTF-8"),
        ));
    }
    generated.push_str("];\n");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("build output"));
    std::fs::write(output.join("linux_presets.rs"), generated).expect("write embedded model list");
}
