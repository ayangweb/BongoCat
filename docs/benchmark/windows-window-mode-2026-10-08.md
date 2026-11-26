# Windows Window Mode Verification

Date: 2026-10-08
Environment: Windows 11 25H2, x86_64 MSVC, current display scale 150%.
Scope: Issue #1130, ADR-0089; Windows only.

## Product And Contract Checks

- `just check`: passed formatting, all configured clippy combinations, workspace
  tests and release check. Hardware-dependent ignored tests remain ignored.
- `just dev-smoke`: passed settings close/reopen and runtime continuity.
  The system-menu product smoke separately verifies taskbar state and switching
  into/out of window mode through the settings service.
- `cargo test --locked -p bongocat-overlay window_mode -- --nocapture`: passed
  real HWND styles, client bounds, user32 resize aspect constraints, frame
  metrics at 120/144/192 DPI, minimize/resize/restore, hidden-window restoration,
  opaque D3D11 readback and BitBlt capture. Switching among all three presets and
  rejected texture preparation preserve the HWND and usable model.
- `python -B tools/validate-locales.py`: seven locales, 273 keys each.
- `python -B tools/validate-json-schema.py`: 59 configuration fixtures and seven
  window-state fixtures passed, alongside shared input/state fixtures.
- Old v1 documents without the new fields still load with pet mode and the
  default green background. Non-v1 and invalid RGB data remain rejected.
- Follow-up UI verification: the disabled background preview now paints the
  current color with the theme's border/radius. The earlier base `ColorSwatch`
  was unstyled and did not paint its color automatically; this was application
  usage, not an upstream rendering bug. An isolated settings window with a mock
  typed service confirmed the dimmed green preview is visible and inert, and
  enabling window mode restores the working color-picker popover. `just check`
  passed again. Screenshot: ignored `target/window-color-preview-disabled.png`.
- Follow-up movement regression: `WM_EXITSIZEMOVE` also ends caption moves.
  The old handler incorrectly published scale for saved client geometry:
  the real HWND regression failed with an unexpected `120%` resize outcome
  after a move. Scale publication now requires `WM_SIZING` and changed client
  dimensions. The same regression passes for repeated moves; a second HWND
  test verifies one scale publication for border resize, matching geometry
  for its acknowledgement, and no publication for unchanged/cancelled resizing
  or subsequent movement. The window-mode D3D11/BitBlt test also passes.
  `just check` passed again. The subsequent `just dev-smoke` built successfully
  but reported `secondary instance notified primary`; a Development instance
  was running, so this attempt did not exercise product smoke behavior. The
  existing instance was left running. Actual cross-monitor DPI movement remains
  unverified as listed below.
- Follow-up client-size regression: the reported stored geometry was
  `720 x 308`. Restoring it unchanged made the standard model render with
  artificial side margins. The pre-fix HWND and D3D11 tests both failed their
  client-aspect checks. Restore/mode creation now preserves client width and
  derives height from the active canvas before adding the Win32 frame. The
  standard preset restores to a `720 x 417` client area; actual BitBlt capture
  in ignored `target/window-mode-smoke.png` was visually inspected without the
  extra side margins. The HWND test also checks outer frame dimensions and
  switching from saved pet geometry at configured scales 25/100/200%; these
  scale values are not display DPI verification. All four window-mode tests
  pass, including the earlier move/resize regression and model switches.
  `just check` passed on the final code. `just dev-smoke` rebuilt the Development
  executable, but again reported `secondary instance notified primary`; its
  product smoke was not exercised because another Development instance was
  running. Cross-monitor DPI and Windows 10 verification remain outstanding.

- Unified scaling regression: saved client geometry `720 x 308` followed by a
  typed runtime scale command at 125% previously produced a 900-pixel width;
  the corrected width at the test HWND's 96 DPI is 438 pixels. Numeric scale,
  native border resize, pet right-button resize, restore, mode/model switches
  and DPI suggestions now use the same unrounded canvas sizing policy. Native
  caption minimum enforcement previously enlarged width alone; coupled client
  limits prevent that change. Top-right resize also retains the opposite corner.
- `cargo test --locked -p bongocat-overlay windows::tests -- --nocapture`:
  all 23 Windows tests passed. The real runtime/HWND/D3D11 regression covers
  numeric 25/50/100/101/125%, minimized numeric changes, combined mode/scale
  changes and all three preset switches, including a minimized model switch.
  Swap-chain size matches measured client geometry. All eight border anchors,
  direct native resize and synthetic DPI suggestions are covered. Mathematical
  tests cover five canvas ratios, four DPI values and every 25–400% scale
  (7,520 combinations), including rounding and minimum-size plateaus. These
  synthetic cases do not establish actual cross-monitor behavior.
- Final upstream integration: fast-forwarded to `65425b75`, preserving nested
  folder import and overlapping motion playback. Regenerated the combined
  schema with `just schema`; `just check`, locale/schema validation and all 12
  release-changelog contract tests passed on the integrated tree.
- Final product smoke: `just dev-smoke` rebuilt the Development executable and
  returned zero as the primary instance; application logs show startup and
  completed graceful shutdown. The release GUI executable then ran with
  `--run-seconds 15 --system-menu-smoke --settings-window-open-smoke` and returned
  zero after the smoke's explicit Quit, before the time guard. Its Development
  configuration backups confirm window mode changed false → true → false,
  with taskbar/status preferences restored; the smoke also checks the visible
  product surface and rejects mismatched mode or pet preferences. Application
  logs confirm visibility toggles and completed shutdown. Windows GUI stdout
  emitted no status lines in these two final runs, so exit status, application
  logs and persisted transitions were checked together. No existing product
  instance was stopped.

## OBS Capture

Official OBS Studio 32.2.2 x64 ran as a portable instance under ignored `target/`
with an isolated configuration. Its window capture source selected the actual
`BongoCatProductOverlayWindow` HWND. No installed OBS profile was changed.

| Capture | Result |
| --- | --- |
| BitBlt, client area | 350 x 203; 2,350 distinct colors; model and green background visible |
| Windows Graphics Capture, client area | 525 x 304; 4,237 distinct colors; model and green background visible |
| WGC with OBS green chroma key filter | 59,384 transparent pixels and 99,546 opaque pixels; cat and keyboard retained |

The capture images were inspected visually and remain in ignored `target/`:
`obs-window-mode-bitblt.png`, `obs-window-mode-wgc.png` and
`obs-window-mode-chroma.png`. The capture resolution difference is OBS's
BitBlt/WGC handling of the current 150% scale. Model-authored desk/keyboard
artwork remains part of the model, as in the reference implementation.

## Remaining Checks

- Manual settings layout and moving across monitors at 125/150/200% scale have
  not been performed; synthetic frame metrics do not establish those results.
- Windows 10 1903 and additional GPU/OBS versions have not been exercised.
- macOS compilation/smoke and eight-hour soak were not run for this Windows task.
- The existing Python cfg detector self-test assumes POSIX path separators and
  fails on Windows. It also fails using the unchanged `origin/master` source.
  The repository cfg contract scan passes; this unrelated test was not changed.
