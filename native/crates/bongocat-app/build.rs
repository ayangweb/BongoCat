#![allow(clippy::print_stdout)]

#[path = "src/build_environment_contract.rs"]
mod build_environment_contract;
#[path = "src/product_icon_contract.rs"]
mod product_icon_contract;

use build_environment_contract::select_build_environment;
use product_icon_contract::{validate_icns, validate_ico};
use std::path::{Path, PathBuf};

const WINDOWS_RESOURCE_FILE: &str = "windows/bongocat-app.rc";

fn main() {
    println!("cargo::rerun-if-env-changed=BONGOCAT_BUILD_ENV");
    println!("cargo::rerun-if-changed=src/build_environment_contract.rs");
    println!("cargo::rerun-if-changed=src/product_icon_contract.rs");
    println!(
        "cargo::rustc-check-cfg=cfg(bongocat_build_environment, values(\"development\", \"production\"))"
    );
    let environment = std::env::var("BONGOCAT_BUILD_ENV").unwrap_or_else(|error| match error {
        std::env::VarError::NotPresent => {
            panic!("BONGOCAT_BUILD_ENV must be explicitly set to 'development' or 'production'")
        }
        std::env::VarError::NotUnicode(_) => {
            panic!("BONGOCAT_BUILD_ENV must be valid UTF-8")
        }
    });
    let environment = select_build_environment(Some(&environment))
        .unwrap_or_else(|error| panic!("{error}"))
        .cfg_value();
    println!("cargo::rustc-cfg=bongocat_build_environment=\"{environment}\"");

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    );
    let resources_dir = manifest_dir.join("../../resources/icons");
    validate_icon(
        &resources_dir.join("BongoCat.icns"),
        "macOS product icon",
        validate_icns,
    );
    validate_icon(
        &resources_dir.join("BongoCat.ico"),
        "Windows product icon",
        validate_ico,
    );

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo::rerun-if-changed={WINDOWS_RESOURCE_FILE}");
        embed_resource::compile(WINDOWS_RESOURCE_FILE, embed_resource::NONE)
            .manifest_required()
            .unwrap_or_else(|error| {
                panic!("Windows product icon resource compilation failed: {error}")
            });
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
