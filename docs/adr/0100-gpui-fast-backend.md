# ADR-0100: GPUI Fast backend and the Rust 1.99 toolchain

## Status

Accepted (2026-10-27). The backend switch is macOS-verified; Windows and
real-device smoke evidence is still pending (see Verification).

## Context

`gpui-kit` is the only direct GPUI dependency (ADR-0020). Upgrading it to the
latest non-yanked stable version, `0.8.1`, brings two changes at once:

- `gpui-kit 0.8.1` depends unconditionally on the `gpui-pre 0.3.10` family. That
  release uses the `result_option_map_or_default` standard-library method, which
  is unstable before Rust 1.98.0. The workspace toolchain was `1.97.1`, so the
  dependency could not compile until the toolchain moved.
- `0.8.1` also carries the optional `gpui-fast` feature that swaps the whole GPUI
  implementation from `gpui-pre` (Zed's GPUI) to `gpui-fast`, a wgpu-backed
  implementation. Enabling the feature does **not** drop `gpui-pre` from the
  graph — that dependency stays unconditional — so the toolchain bump is required
  either way.

## Decision

- Move the workspace toolchain to Rust `1.99.1` (current stable) and set
  `[workspace.package].rust-version = "1.99"`. The dependency only needs the
  method stabilized in 1.98.0; the pinned channel continues to track current
  stable.
- Upgrade `gpui-kit` to `=0.8.1` and enable its `gpui-fast` feature at the
  workspace dependency, so the settings window and the app binary resolve one
  backend. `gpui-pre-reqwest-client` moves to `=0.3.10` to stay on the same
  `gpui-pre` line.
- Keep the ADR-0020 replacement boundary: `gpui-kit` remains the single entry
  point and selects the facade internally (`::gpui::*` vs `::gpui_fast::*`,
  `gpui_kit::platform` vs `gpui_fast_platform`).
- Adapt the two places the change is visible in source:
  - GPUI derive macros expand to `gpui::` paths; `bongocat-ui` aliases the facade
    with `extern crate gpui_kit as gpui;` so those paths resolve.
  - `gpui-base 0.8` leaves Enter/Space inside popover content to the focused
    control. `PopConfirm` moves the decision handler from the wrapper's
    `on_action` to the button's own `on_click`, so a focused `Button` turns the
    activation keys into a click.
- Adopt the new clippy lints that ship with 1.98/1.99 across the workspace
  (`chunks_exact_to_as_chunks`, and the `Atomic::fetch_update` rename to
  `try_update`).

## Consequences

- The toolchain and MSRV rise to 1.99. The CI stable canary keeps checking a
  second, newer stable toolchain.
- The settings and update windows render through wgpu instead of Zed's GPUI. The
  product's overlay/live2d renderer is independent native code and does not use
  GPUI, so it is unaffected.
- `gpui-pre 0.3.10` stays in the dependency graph even with `gpui-fast`, so the
  toolchain requirement is not removable by dropping the backend feature.

## Verification

- `just check` passes on macOS `aarch64`: fmt, clippy (all targets and features,
  including the `bongocat-app` feature groups), the full workspace test suite
  (including the GPUI UI tests), and the release check.
- `tools/check-dependencies.sh` passes: the release dependency tree stays clear
  of forbidden UI stacks on all three targets, and `cargo deny` reports licenses
  and sources ok.
- Pending: real-device Windows and macOS settings-window smoke, IME, HiDPI
  scaling, window rebuild and shutdown coverage for the wgpu backend. Compiling
  and headless UI tests are not a substitute for that evidence.
