# GPUI Settings Spike

This is an isolated Phase 0 probe for `gpui = 0.2.2`. It intentionally does not
belong to the legacy Tauri workspace and does not implement product state,
Live2D, input capture, or the native overlay.

Run it from this directory:

```text
cargo run
```

For a repeatable lifecycle smoke test:

```text
BONGOCAT_SPIKE_AUTO_QUIT_MS=1500 cargo run --locked
```

Successful output includes `window opened`, `runtime snapshot revision=1`,
`runtime stopped`, and `stopped`.

The synthetic runtime bridge uses a bounded typed command channel. The UI reads
revisioned snapshots through a GPUI task, while application shutdown waits for
the runtime acknowledgement. Run its contract test with:

```text
cargo test --locked
```

Build an ad-hoc-signed macOS application bundle and launch it through
LaunchServices:

```text
./scripts/package-macos.sh
open -W "target/package/BongoCat GPUI Spike.app" --args --auto-quit-ms 1500
```

The ad-hoc signature only validates local bundle integrity. It is not a
Developer ID signature or notarization result.

The spike is successful only when the window opens, renders text without
clipping, and closes cleanly on the target machine. It is not evidence that the
production runtime, UI, or overlay lifecycle is complete.

This probe uses GPUI's default precompiled shader path. On macOS, install the
optional Metal Toolchain before building:

```text
xcodebuild -downloadComponent MetalToolchain
```

The validated component version is recorded in
`docs/phase-0/gpui-settings-spike.md`.
