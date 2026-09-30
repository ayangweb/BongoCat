// The macOS permission flow links a Swift static library, and every binary this package produces
// needs the Swift runtime on its loader path. `permission-flow`'s own build script emits the same
// flag, but a library dependency's `rustc-link-arg` does not reach the binary that finally links,
// so each package whose binaries link it has to add it. Without it the process aborts before
// `main`:
//
//   dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
//   Reason: no LC_RPATH's found
//
// `tools/tests/test_swift_runtime_rpath.py` fails the build when a package that depends on
// `bongocat-platform` is missing this.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
