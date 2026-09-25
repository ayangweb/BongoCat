# BongoCat Product Runtime

The repository root is the formal Rust product workspace and the only product build entry. Historical
Vue/Tauri code is retained only in the remote `master` and `pre-refactor-tauri` branches.

## Build Environments

Direct Cargo commands from the repository root use the Development environment by default; the
selection is compiled into the artifact. The formal `just dev`, `just test`, and `just check`
recipes also use Development for the workspace. Run product commands from the repository root:

```text
cargo run -p bongocat-app --release
```

Enable the `production` feature for a standalone Production build:

```text
cargo run -p bongocat-app --release --features production
```

The environment is a mutually exclusive compile-time feature, not a runtime environment variable;
the legacy `BONGOCAT_BUILD_ENV` name is ignored.

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

Production must be selected at build time. The packaging entry point rejects an unknown
`--environment` value before invoking Cargo and translates the selection into the app's Cargo
feature:

```text
just build
```

`just build` forwards its arguments to `crates/bongocat-packaging`, which is the only place that
decides how the product is compiled and packaged. It compiles the release binary with the immutable
`production` feature where selected, writes build provenance, and hands the executable to
`cargo-packager`, which owns the macOS `.app` layout, `Info.plist` generation and the Windows NSIS
installer. Both local developers and CI run this same entry point, so there is one code path and one
configuration:

```text
just build                                            # Production, host target, all artifacts
just build --target x86_64-apple-darwin               # explicit target (macOS can cross-build both)
just build --environment development --formats app     # Development .app only, for smoke tests
just version                                          # the resolved product version
```

The product version has one source of truth: `[workspace.package].version` in the root `Cargo.toml`.
All Native workspace crates inherit it, including `bongocat-packaging`, so Cargo itself resolves the
value before the packager runs; the settings window, system menu, and update runtime read the
compiled `CARGO_PKG_VERSION`. `Cargo.lock` records the resolved workspace versions as generated
metadata; a release changes the root manifest once and lets Cargo refresh the lockfile. The Windows
executable resource derives its file, product, and display versions from that value, and
`cargo-packager` writes `CFBundleShortVersionString` into the bundle from the same configured
version. The release workflow refuses to publish a tag that disagrees with `just version`.

On macOS `just build` produces `target/package/BongoCat.app` and
`target/package/BongoCat-<version>-<arch>.dmg`, and prints both absolute paths. The disk image
contains the signed bundle plus an `/Applications` drop link. On Windows it produces the x64 NSIS
current-user installer at `target/package/BongoCat_<version>_x64.exe`.

Native build provenance is written as path-free JSON with the source commit, `Cargo.lock` SHA-256,
Rust toolchain, target, profile, feature set, and build environment. The macOS package includes
`Contents/Resources/build-provenance.json`; the Windows installer payload does too.

Windows x64 packaging is a current-user NSIS install. `cargo-packager` obtains its own NSIS
toolchain, so no release workstation state is required, and the installer requests no administrator
privileges and writes only HKCU uninstall metadata plus its own product directory. The installer is
signed only when a signing identity or signing command is configured; without one the release
workflow reports the unsigned installer as unsuitable for a stable release. Windows install,
upgrade, uninstall, and rollback smoke remain release gates.

Preset models are product resources, not user data. macOS loads them from
`BongoCat.app/Contents/Resources/models`; Windows packages must place them under
`resources/models` beside `bongocat-app.exe`. An unpackaged development binary falls back to the
repository resource tree only when that product-relative location is absent.

The application does not expose a runtime environment switch. Both environments use the same
schema and relative layout under separate `development/` and `production/` roots.
The formal startup API always resolves that root from the compiled environment. Process-level tests
that require an isolated temporary layout must explicitly enable `storage-test-injection`; the
feature is absent from the default CLI/API and is rejected at compile time for Production builds.

## Verification

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings
cargo clippy -p bongocat-app --all-targets --features storage-test-injection -- -D warnings
cargo clippy -p bongocat-app --all-targets --features production -- -D warnings
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

On macOS, the completion line includes `frame_timing=Some(...)`. Its `draw_*_us`
values are nearest-rank percentiles over the interval surrounding each Metal
`draw` call, including the initial draw. `missed_deadlines` is the number of
full preview-loop iterations that reached the next 60 FPS deadline before the
loop could sleep. The collector retains at most 4,096 samples and reports
additional samples as `samples_dropped`; it intentionally excludes input,
runtime handoff, and sleep. Windows' switch-only preview reports
`frame_timing=None` until it has an equivalent paced diagnostic path.

On Windows or macOS, exercise transactional GPU model replacement with all three presets by running
100 measured standard -> keyboard -> gamepad -> standard cycles (300 reported generations):

```text
cargo run -p bongocat-overlay --release -- standard 0 --switch-cycles 100
```

The switch probe first injects one invalid texture preparation and requires the current CPU model,
input bindings, and GPU generation to remain active and drawable. Rejected candidate generations
may create a gap, but committed generations must remain strictly monotonic. Every valid generation
performs a non-transparent frame readback. Before the requested Windows measurement interval, the
probe runs at least 100 equivalent warmup cycles and waits for driver workers to settle. The warmup
high-water mark is a snapshot, not a hard ceiling: process-global D3D11, DXGI, and thread-pool
workers can be created after the settle window and then persist for the life of the process, so the
probe accepts a settled count within two threads of the mark while still rejecting the per-switch
growth that scales with the measured switch count. It also rejects more than four additional
handles or DXGI local-memory growth, and the preview reports the warmup mark and the settled count
so the remaining allowance stays visible. The macOS probe rejects Metal allocation growth between
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
elapsed time, and publishes the resulting immutable drawable frame. A product motion advances for one
cycle and then remains the current motion layer at the clip's fully evaluated terminal sample; later
frames restore Core defaults and reapply that final motion contribution instead of snapping back to
idle. The held sample retains the resource's natural fade weights. Completion is derived from elapsed
monotonic time even when no frame was delivered, so a hidden or sleeping overlay still releases the
completed layer's priority before the next command. The completed layer no longer reserves priority,
so the next motion request may replace or restart it.
Runtime snapshots expose only the active motion identity, priority, originating command sequence,
and optional first stop command sequence. Explicit stop also matches a completed held motion,
preserves the first frame, multiplies curve weights by the model3 sine fade-out, and clears the motion
only after the fade completes; duplicate stops cannot restart it. A different old motion ID cannot
stop a newer run, while a same-ID replay is the current run targeted by a later same-ID stop.
PartOpacity curves follow the official Framework part-opacity sink and remain separate from weighted
parameter samples. Each frame restores the fresh-model part opacity values before applying the active
motion, preventing stop or replacement from leaving stale part visibility. Model3 Parameter groups
drive R5-compatible Model curves: EyeBlink multiplies matching parameter curves, LipSync adds to
them, and both update unmatched group parameters with the motion fade. Model Opacity travels in the
immutable render snapshot and is multiplied only in the final D3D11/Metal color pass, leaving mask
generation unchanged.
Product input is applied after motion curves so an actual pressed key or button remains authoritative
for hand parameters.

Accepted motions now publish an ordered side effect to the independent `bongocat-audio` worker.
The worker uses rodio with only playback and FLAC enabled, owns one voice, and resolves no model
metadata itself. A new motion, explicit stop, disabled audio setting, successful model switch, or
shutdown stops the old voice. Missing/corrupt audio, output failure, or queue pressure is retained as
anonymous runtime diagnostics and never fails motion or rendering. Motion3 UserData crossings use
the effective runtime playback mode, so a one-shot run of a looping asset emits its start timestamp
only once; crossings remain monotonic, de-duplicated, and bounded.

Expression playback uses `Application::set_expression` with the model3 expression name. Every
declared exp3 resource is parsed and cached during model preparation; Add, Multiply, and Overwrite
parameters use the file's sine fade times. Replacing an expression fades the immediately previous
layer out while the new layer fades in, keeping at most two layers. Once its fade-in completes, the
newest expression's full weight is pinned until another valid expression replaces it, a model commit
succeeds, or shutdown begins; a test-clock rollback cannot restart that completed fade-in, and
expressions do not auto-clear to idle. An invalid request leaves the active expression unchanged. The
per-frame order is parameter/part-opacity defaults, motion, expression, typed product input, then
Cubism Core update.

`model.random_behavior_enabled` and `model.random_behavior_interval_seconds` form one typed runtime
setting. When enabled, the worker waits one complete interval on its injected monotonic clock and
uniformly selects from the active model's combined list of declared motions and expressions (each
item has equal weight). A successful model
commit, a settings change, or a re-enable re-anchors the schedule; a long pause does not emit a
catch-up burst. Automatic motion uses `Idle` priority and cannot replace a live `Normal`/`Force`
product motion, while expressions follow the normal latest-expression replacement rule. The
scheduler is disabled during a pending model commit and shutdown, and an empty behavior list is a
no-op. Automatic behavior and shutdown requests share an admission gate: the request records
shutdown immediately and starts its bounded wait, while an already-admitted action finishes before
the worker drains shutdown state and later actions are skipped. Runtime event sequences for automatic
playback are separate from the `bongocat-audio` command sequence allocator; overflow recovery retains
model-preparation commands so accepted audio preparation cannot leave a model commit pending. A
hidden overlay's non-zero motion fade is considered settled from the injected clock even when no
frame was delivered. The GPUI Model behavior page exposes the switch and whole-second interval
through the revision-checked settings service; the interval row is disabled while the switch is
off but keeps its saved value.

`bongocat-live2d-playback` owns the SDK-independent motion3/exp3 byte parser and numeric
curve/blend evaluation. `bongocat-live2d-render` prepares model-package `RenderResources` and
owns the shared key-image inventory/overlay resolver; `bongocat-live2d` writes the resulting
motion/expression values into Core. Runtime remains the sole owner of active playback identity,
priority, stop/crossfade timing, and model-generation cleanup.

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
