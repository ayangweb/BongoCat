#![allow(clippy::print_stdout)]

#[path = "src/build_environment_contract.rs"]
mod build_environment_contract;

use build_environment_contract::select_build_environment;

fn main() {
    println!("cargo::rerun-if-env-changed=BONGOCAT_BUILD_ENV");
    println!("cargo::rerun-if-changed=src/build_environment_contract.rs");
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
}
