# BongoCat Native Workspace

This is the formal Rust product workspace. The repository-root Cargo workspace remains the
historical Tauri behavior reference until the release cutover phase.

## Build Environments

Development is the default and is compiled into the artifact:

```text
cargo run --manifest-path native/Cargo.toml -p bongocat-app --release -- --run-seconds 0
```

On macOS this is the current formal visible product entry. It loads the selected bundled preset
(`standard` by default), starts the product runtime and listen-only input producer, and displays the
transparent Metal overlay. `--run-seconds 0` keeps it visible until the overlay is closed; omit the
argument for a bounded 30-second run. Grant Input Monitoring permission to the launching terminal
for global keyboard and mouse-button animation. Permission denial is reported as a degraded input
state and does not prevent the model from appearing.

The assembled Windows entry currently starts and stops the platform-independent application
services but does not yet draw the preset model. The D3D11 preset-model renderer remains an active
TODO item.

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

## macOS Live2D Diagnostic Preview

`bongocat-overlay` remains a diagnostic executable for selecting presets, injecting deterministic
input, and exercising model switching. It is not the product entry. The final positional argument
is the preview duration in seconds:

```text
cargo run --manifest-path native/Cargo.toml -p bongocat-overlay --release -- standard 30
cargo run --manifest-path native/Cargo.toml -p bongocat-overlay --release -- keyboard 30
cargo run --manifest-path native/Cargo.toml -p bongocat-overlay --release -- gamepad 30
```

Exercise transactional GPU model replacement with all three presets by running 100 complete
standard -> keyboard -> gamepad -> standard cycles (300 committed generations):

```text
cargo run --manifest-path native/Cargo.toml -p bongocat-overlay --release -- standard 0 --switch-cycles 100
```

The switch probe first injects one invalid texture preparation and requires the current GPU model
to remain drawable. Every valid generation performs a non-transparent frame readback, generations
cannot skip, and the command fails if Metal's current allocated size grows between the warmed-up
standard baselines.

By default the preview applies deterministic, model-specific input through the product runtime so
hand, pointer, head, and eye changes exercise per-frame Cubism evaluation and GPU buffer updates.
Formal gamepad button/axis integration remains a separate work item. To use the formal macOS
listen-only CGEventTap producer for keyboard and mouse button edges instead, grant Input Monitoring
permission to the launching terminal and run:

```text
cargo run --manifest-path native/Cargo.toml -p bongocat-overlay --release -- standard 30 --interactive
```

The interactive path uses the same typed runtime input state as the deterministic preview and
stops the platform producer before the runtime and Metal overlay. It seeds the current global
cursor position at startup and then coalesces cursor movement through an independent latest-value
transport; pointer, head, and eye parameters use the active display's logical viewport. The product
entry now owns this runtime/input/render lifecycle. GPUI settings coexistence, lifecycle
notifications, coordinated runtime/GPU model-switch acknowledgement, installed-model selection,
and the Windows preset-model renderer remain separate work items.

Cubism model evaluation and Metal GPU ownership are separated by the platform-independent
`bongocat-render` contract. The single runtime worker owns the mutable Cubism model and publishes
immutable resource/frame pairs through its latest-frame transport; the overlay resolves drawables,
masks, and textures with strong resource IDs and never receives the mutable Cubism model.

The fixed-version Cubism Core, header, generated bindings, and preset model development baseline are
committed under `vendor/cubism/5-r.5` and `resources/models`. Builds do not download SDK artifacts.
Their provenance and release gates are documented in
`docs/adr/0011-progressive-implementation-release-gates.md` and the Phase 0 Cubism records.

Model package parsing is also SDK-independent. `bongocat-model` prepares and validates package
metadata before a typed command transfers an opaque committed model to the runtime. The runtime
creates and evaluates Cubism state; GPU resource upload remains a separate renderer stage.

Model imports are copied into a unique staging directory under the current build environment's
`models/` root. The importer rejects symbolic links and unsupported filesystem entries, reapplies
package limits while copying, flushes every file, validates the staged package again, and commits it
with a same-root directory rename. An existing model ID is never overwritten, and a failed import
removes only the staging directory owned by that operation.

The user-model catalog is rebuilt deterministically from the environment's installed directories;
no separate database can drift from disk. A writer lock under `locks/` serializes import, catalog,
load, delete, and startup recovery. Corrupt packages remain visible as per-model diagnostics, while
well-formed abandoned import/delete directories are removed on the next start. Product code can
activate only an opaque `CommittedModel` issued by either the environment store after commit/load
or the bundled read-only preset catalog, and it must replace the active model before deleting it.
