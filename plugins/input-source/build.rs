// Platform's macOS permission adapter links Swift; link args do not propagate
// from a library into its consumer binaries (see the existing rpath contract).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
