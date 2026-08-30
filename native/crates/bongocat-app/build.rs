#![allow(clippy::print_stdout)]

fn main() {
    println!("cargo::rerun-if-env-changed=BONGOCAT_BUILD_ENV");
    println!(
        "cargo::rustc-check-cfg=cfg(bongocat_build_environment, values(\"development\", \"production\"))"
    );
    let environment =
        std::env::var("BONGOCAT_BUILD_ENV").unwrap_or_else(|_| "development".to_owned());
    match environment.as_str() {
        "development" | "production" => {
            println!("cargo::rustc-cfg=bongocat_build_environment=\"{environment}\"");
        }
        value => panic!("BONGOCAT_BUILD_ENV must be 'development' or 'production', got {value:?}"),
    }
}
