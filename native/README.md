# BongoCat Native Workspace

This is the formal Rust product workspace. The repository-root Cargo workspace remains the
historical Tauri behavior reference until the release cutover phase.

## Build Environments

The Native workspace explicitly selects Development in `.cargo/config.toml`; the selection is
compiled into the artifact. Run product commands from the Native workspace so Cargo loads that
controlled entry configuration:

```text
cd native
cargo run -p bongocat-app --release
```

This is the current formal visible product entry on macOS and Windows. It loads the selected bundled
preset (`standard` by default), starts the product runtime and platform input producer, and displays
the transparent Metal or D3D11 overlay. Closing the settings window leaves the runtime, input, and
overlay active; reopening the application creates a fresh settings entity from the current runtime
snapshot on macOS. GPUI 0.2.2 cannot safely destroy its Windows window from `WM_CLOSE`, so Windows
hides the native window, retains its sole entity, and refreshes that entity when reopened.
Explicit Windows quit first stops and joins every BongoCat-owned runtime, input, audio, renderer,
GPU, and overlay owner. The platform adapter then terminates the process without dropping the retained
GPUI window, because GPUI 0.2.2 synchronously re-enters its borrowed `AsyncApp` from `WM_DESTROY`.
This final-step workaround must be removed when a fixed GPUI revision is adopted.
The application stays active until an explicit Quit command by default. `--run-seconds <seconds>`
with a positive value is reserved for bounded smoke and diagnostic runs; `0` is the explicit spelling
of the normal unbounded lifetime used by platform startup-item registrations.

The cross-platform product smoke closes or hides the settings window, reopens it, and verifies that
the frame source continued to run and the current snapshot was restored:

```text
cargo run -p bongocat-app --release -- --run-seconds 4 --settings-window-smoke
```

On macOS, grant Input Monitoring permission to the launching terminal for global keyboard and
mouse-button animation. Permission denial is reported as a degraded input state and does not
prevent the model from appearing. On Windows, the product uses a dedicated hidden Raw Input owner
window, periodically reconciles locally pressed candidates with `GetAsyncKeyState`, and resets input
on device, session, power, queue, and service lifecycle changes. Physical PixPin, Win+L, UAC,
administrator-boundary, and long-running input tests remain release evidence tasks.

Production must be selected at build time. Packaging scripts reject a missing, empty, or unknown
selection before invoking Cargo:

```text
BONGOCAT_BUILD_ENV=production cargo build -p bongocat-app --release
```

Native build provenance is written as path-free JSON with the source commit, `Cargo.lock` SHA-256,
Rust toolchain, target, profile, feature set, and build environment. The macOS package includes
`Contents/Resources/build-provenance.json`; CI stores one provenance artifact per native runner.

The application does not expose a runtime environment switch. Both environments use the same
schema and relative layout under separate `development/` and `production/` roots.
The formal startup API always resolves that root from the compiled environment. Process-level tests
that require an isolated temporary layout must explicitly enable `storage-test-injection`; the
feature is absent from the default CLI/API and is rejected at compile time for Production builds.

## Verification

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace --release
```

## Live2D Diagnostic Preview

`bongocat-overlay` remains a diagnostic executable for selecting presets, injecting deterministic
input, and exercising model switching. It is not the product entry. The final positional argument
is the preview duration in seconds. The interactive and fixed-duration preview is currently
available on macOS:

```text
cargo run -p bongocat-overlay --release -- standard 30
cargo run -p bongocat-overlay --release -- keyboard 30
cargo run -p bongocat-overlay --release -- gamepad 30
```

On Windows or macOS, exercise transactional GPU model replacement with all three presets by running
100 measured standard -> keyboard -> gamepad -> standard cycles (300 reported generations):

```text
cargo run -p bongocat-overlay --release -- standard 0 --switch-cycles 100
```

The switch probe first injects one invalid texture preparation and requires the current CPU model,
input bindings, and GPU generation to remain active and drawable. Rejected candidate generations
may create a gap, but committed generations must remain strictly monotonic. Every valid generation
performs a non-transparent frame readback. Before the requested Windows measurement interval, the
probe runs at least 100 equivalent warmup cycles and waits for driver workers to settle. It then
rejects persistent process thread growth above the warmup high-water mark, more than four additional
handles, or DXGI local-memory growth. The macOS probe rejects Metal allocation growth between
warmed-up standard baselines.

By default the preview applies deterministic, model-specific input through the product runtime so
hand, pointer, head, and eye changes exercise per-frame Cubism evaluation and GPU buffer updates.
To use the formal macOS listen-only CGEventTap and GameController producers for keyboard, mouse and
gamepad input instead, grant Input Monitoring permission to the launching terminal and run:

```text
cargo run -p bongocat-overlay --release -- standard 30 --interactive
```

The interactive path uses the same typed runtime input state as the deterministic preview and
stops the platform producer before the runtime and Metal overlay. It seeds the current global
cursor position at startup and then coalesces cursor movement through an independent latest-value
transport; pointer, head, and eye parameters use the active display's logical viewport. The product
entry now owns this runtime/input/render lifecycle on both launch platforms. GPUI settings
coexistence, installed-model selection, physical gamepad validation, and remaining lifecycle evidence are
separate work items.

Cubism model evaluation and Metal GPU ownership are separated by the platform-independent
`bongocat-render` contract. The single runtime worker owns the mutable Cubism model and publishes
immutable resource/frame pairs through its latest-frame transport; the overlay resolves drawables,
masks, and textures with strong resource IDs and never receives the mutable Cubism model.

The same worker now owns typed motion playback. `Application::start_motion` resolves a validated
model3 group/index, applies motion3 linear, Bezier, stepped, or inverse-stepped curves using monotonic
elapsed time, and publishes the resulting immutable drawable frame. Runtime snapshots expose only
the active motion identity, priority, originating command sequence, and optional first stop command
sequence. Explicit stop preserves the first frame, multiplies curve weights by the model3 sine
fade-out, and clears the motion only after the fade completes; duplicate stops cannot restart it.
PartOpacity curves follow the official Framework parameter sink and remain separate from weighted
parameter samples. Model3 Parameter groups drive R5-compatible Model curves: EyeBlink multiplies
matching parameter curves, LipSync adds to them, and both update unmatched group parameters with the
motion fade. Model Opacity travels in the immutable render snapshot and is multiplied only in the
final D3D11/Metal color pass, leaving mask generation unchanged.
Product input is applied after motion curves so an actual pressed key or button remains authoritative
for hand parameters.

Accepted motions now publish an ordered side effect to the independent `bongocat-audio` worker.
The worker uses rodio with only playback and FLAC enabled, owns one voice, and resolves no model
metadata itself. A new motion, explicit stop, disabled audio setting, successful model switch, or
shutdown stops the old voice. Missing/corrupt audio, output failure, or queue pressure is retained as
anonymous runtime diagnostics and never fails motion or rendering. Motion3 UserData crossings are
also evaluated from monotonic elapsed time with loop-safe de-duplication and a bounded batch.

Expression playback uses `Application::set_expression` with the model3 expression name. Every
declared exp3 resource is parsed and cached during model preparation; Add, Multiply, and Overwrite
parameters use the file's sine fade times. Replacing an expression fades the immediately previous
layer out while the new layer fades in, keeping at most two layers; an invalid request leaves the
active expression unchanged. The per-frame order is defaults, motion, expression, typed product
input, then Cubism Core update.

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
