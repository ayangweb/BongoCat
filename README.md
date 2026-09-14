# BongoCat Native Rewrite

BongoCat is a Rust 2024 desktop application for Windows 10 1903+ and macOS 12+.
GPUI provides the settings interface; native D3D11 and Metal overlays render the model.

The repository root is the only local product build entry.
The Vue/Tauri implementation is retained for historical reference in the remote
[`master`](https://github.com/ayangweb/BongoCat/tree/master) and
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) branches.

## Development

Install Rust `1.97.1` with `clippy` and `rustfmt`. The formal `just` entrypoints build the
Development environment by default.

```text
just dev
just check
```

Direct Cargo commands from the repository root use the Development environment by default, so
`cargo check --workspace` works without additional setup. Production builds enable the
`bongocat-app/production` Cargo feature explicitly; the packaging entry point does this
automatically.

See [docs/product-runtime.md](docs/product-runtime.md) for product runtime, packaging, and verification details.
The current architecture and release gates are defined in
[Technical Design](docs/BongoCat%20Native%20Rewrite%20Technical%20Design.md) and
[Implementation TODO](docs/BongoCat%20Native%20Rewrite%20Implementation%20TODO.md).

## Build and package

`just build` is the single build entry point. It compiles the Production product and packages the
release artifacts for the host platform: a `.app` and a `.dmg` on macOS, an NSIS installer on
Windows. `cargo-packager` owns the bundle and installer layout, and the project configuration lives
in [`crates/bongocat-packaging`](crates/bongocat-packaging/src/main.rs).

```text
just build                                          # host target, all of its release artifacts
just build --target x86_64-apple-darwin             # explicit target
just version                                        # the single product version source
```

Prerequisites beyond the Rust toolchain: `just` and a Python 3 interpreter. The interpreter is used
by `tools/record-native-provenance.py`, which writes the build provenance record that every packaged
artifact carries. `cargo-packager` fetches the NSIS toolchain on Windows, and the macOS disk-image
tooling is part of the operating system, so no other tool needs a global install.

## Status

The Native Rewrite is under active development. Phase 0 evidence, platform validation, and stable
release gates remain tracked in the implementation TODO; this repository does not currently claim a
stable release.
