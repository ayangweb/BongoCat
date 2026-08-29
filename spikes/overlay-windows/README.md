# Windows Native Overlay Spike

This Phase 0 crate owns the independent Windows overlay boundary used by the
GPUI coexistence probe. It creates a thread-confined Win32 popup and a separate
D3D11 + DXGI + DirectComposition renderer without accessing GPUI renderer
internals.

## Hypothesis

A Rust-owned `HWND` with a premultiplied-alpha composition swap chain can live
beside a GPUI settings window, clear/present transparently, transition between
visible and hidden states, and release GPU/DirectComposition resources before
destroying the window.

The spike attempts a hardware D3D11 device first and falls back to WARP so CI
can test ownership and composition APIs without claiming physical-GPU
performance. Windows ARM64 is compile-checked only; Cubism R5 still blocks an
ARM64 product release.

## Boundaries

- The crate returns project-local strings and reports; Win32/COM types stay
  private.
- The owner is deliberately `!Send`/`!Sync` and asserts its creation thread.
- `unsafe` is limited to documented Win32, D3D11, DXGI and DirectComposition
  calls.
- Renderer teardown detaches the composition graph and flushes D3D11 before
  the `HWND` owner is dropped.
- This does not render Live2D, validate physical-GPU performance, recover a
  removed device, or prove leak freedom beyond the measured process handle
  contract.

## Validation

Portable and cross-target checks:

```text
cargo fmt --manifest-path spikes/overlay-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/overlay-windows/Cargo.toml --locked --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cargo check --manifest-path spikes/overlay-windows/Cargo.toml --locked --target aarch64-pc-windows-msvc
```

The `windows-latest` CI job runs the GPUI coexistence executable in three
modes:

```text
--auto-quit-ms 1800
--simulate-overlay-init-failure --auto-quit-ms 900
--windows-overlay-cycles 100
```

The normal path must report two transparent presents and ordered GPU/HWND
teardown. The injected failure must leave the GPUI settings window alive with
a degraded status. The cycle probe warms process-global graphics state, then
requires 100 full create/show/present/hide/drop cycles without process handle
growth beyond a fixed four-handle tolerance.
