use std::{fs, path::Path};

const CATALOGS: [&str; 2] = ["locales/en-US.json", "locales/zh-CN.json"];

fn main() {
    let mut fingerprint = 0xcbf29ce484222325_u64;
    for catalog in CATALOGS {
        println!("cargo:rerun-if-changed={catalog}");
        let bytes = fs::read(Path::new(catalog)).expect("read localization catalog");
        for byte in bytes {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(0x100000001b3);
        }
    }
    println!("cargo:rustc-env=BONGOCAT_I18N_CATALOG_REVISION={fingerprint:016x}");
}
