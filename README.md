# BongoCat Native Rewrite

BongoCat is a Rust 2024 desktop application for Windows 10 1903+ and macOS 12+.
GPUI provides the settings interface; native D3D11 and Metal overlays render the model.

The current product workspace is [`native/`](native/). It is the only local build entry.
The Vue/Tauri implementation is retained for historical reference in the remote
[`master`](https://github.com/ayangweb/BongoCat/tree/master) and
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) branches.

## Development

Install Rust `1.97.1` with `clippy` and `rustfmt`. The Native workspace selects its Development
environment through `native/.cargo/config.toml`.

```text
just dev
just check
```

To work in the workspace directly:

```text
cd native
cargo run --locked -p bongocat-app --release
```

See [native/README.md](native/README.md) for product runtime, packaging, and verification details.
The current architecture and release gates are defined in
[Technical Design](docs/BongoCat%20Native%20Rewrite%20Technical%20Design.md) and
[Implementation TODO](docs/BongoCat%20Native%20Rewrite%20Implementation%20TODO.md).

## Status

The Native Rewrite is under active development. Phase 0 evidence, platform validation, and stable
release gates remain tracked in the implementation TODO; this repository does not currently claim a
stable release.
