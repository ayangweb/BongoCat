use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../vendor/cubism/5-r.5/Core/include/Live2DCubismCore.h");

    let target = env::var("TARGET").expect("Cargo must provide TARGET");
    let vendor = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../../vendor/cubism/5-r.5/Core");

    match target.as_str() {
        "aarch64-apple-darwin" => link_static_core(vendor.join("lib/macos/arm64")),
        "x86_64-apple-darwin" => link_static_core(vendor.join("lib/macos/x86_64")),
        "x86_64-pc-windows-msvc" => {
            println!(
                "cargo:rustc-link-search=native={}",
                vendor.join("lib/windows/x86_64/143").display()
            );
            println!("cargo:rustc-link-lib=static=Live2DCubismCore_MD");
        }
        _ => {}
    }
}

fn link_static_core(directory: PathBuf) {
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=Live2DCubismCore");
}
