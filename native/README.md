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

Model imports are copied into a unique staging directory under the current build environment's
`models/` root. The importer rejects symbolic links and unsupported filesystem entries, reapplies
package limits while copying, flushes every file, validates the staged package again, and commits it
with a same-root directory rename. An existing model ID is never overwritten, and a failed import
removes only the staging directory owned by that operation.

The user-model catalog is rebuilt deterministically from the environment's installed directories;
no separate database can drift from disk. A writer lock under `locks/` serializes import, catalog,
load, delete, and startup recovery. Corrupt packages remain visible as per-model diagnostics, while
well-formed abandoned import/delete directories are removed on the next start. Product code can
activate only an opaque `InstalledModel` issued after the store commits or reloads a package, and it
must replace the active model before deleting it.
