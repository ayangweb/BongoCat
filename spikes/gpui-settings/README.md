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

Successful output includes both `window opened` and `stopped`.

The spike is successful only when the window opens, renders text without
clipping, and closes cleanly on the target machine. It is not evidence that the
production UI or overlay lifecycle is complete.

This probe enables GPUI's public `runtime_shaders` feature because Xcode may
install the Metal compiler as an optional component. Production packaging must
also pass with GPUI's default precompiled shader path and a pinned Xcode/Metal
toolchain.
