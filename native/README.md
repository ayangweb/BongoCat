# BongoCat Native Workspace

This is the formal Rust product workspace. The repository-root Cargo workspace remains the
historical Tauri behavior reference until the release cutover phase.

## Build Environments

Development is the default and is compiled into the artifact:

```text
cargo run --manifest-path native/Cargo.toml -p bongocat-app
```

Production must be selected at build time:

```text
BONGOCAT_BUILD_ENV=production cargo build --manifest-path native/Cargo.toml -p bongocat-app --release
```

The application does not expose a runtime environment switch. Both environments use the same
schema and relative layout under separate `development/` and `production/` roots.

## Verification

```text
cargo fmt --manifest-path native/Cargo.toml --all -- --check
cargo clippy --manifest-path native/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo test --manifest-path native/Cargo.toml --workspace
cargo check --manifest-path native/Cargo.toml --workspace --release
```

The workspace does not download or bundle Cubism artifacts. Local Cubism setup remains a separate,
explicit process governed by `docs/adr/0011-progressive-implementation-release-gates.md`.

Model package parsing is also SDK-independent. `bongocat-model` prepares and validates package
metadata before a typed command transfers ownership to the runtime; Cubism model creation and GPU
upload remain separate commit stages.
