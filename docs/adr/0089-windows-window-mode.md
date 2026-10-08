# ADR-0089: Windows window mode for capture

Date: 2026-10-08
Status: Accepted; Windows implementation and OBS capture verified

## Context

Issue #1130 needs an ordinary Windows model window that OBS can select and
capture, with a solid background for chroma keying. The existing popup uses a
transparent DirectComposition surface and is not an ordinary capture target.

Mver baseline `4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c`,
`BongoCatMver/include/catmain.h`, provides the behavior evidence:
`setWindow()` restores the original caption and thick frame when
`decoration.desktop_pet` is false; `clearCatWindow()` clears to opaque
`decoration.rgb`; desktop-pet movement/resizing and click-through are pet-only.
`src/main.cpp` maintains the model aspect ratio on ordinary window resize.

## Decision

- Add optional v1 `overlay.window_mode` (default false) and
  `overlay.window_background_color` (RGB bytes, default green `[0, 255, 0]`).
  Missing fields keep existing data readable. Shared typed configuration,
  commands and snapshots carry these values; only Windows applies them.
- Windows settings expose a switch and the GPUI Kit color picker. No dependency
  or second UI/rendering framework is added. macOS behavior stays unchanged.
- Window mode uses a caption, resize frame, minimize and close buttons, an
  activatable taskbar window and a D3D11 HWND swap chain with an opaque RGB clear.
  Preserve model-authored backgrounds and draw order. Users can choose a key
  color that differs from their model and its background.
- Suspend click-through, hover/idle hide, presentation opacity and rounded
  clipping while in window mode, without modifying the user's pet preferences.
  Always-on-top and explicit visibility remain independently controllable.
- Keep client dimensions separate from non-client frame dimensions. DPI and
  resize use current window metrics; the renderer sizes to the client area.
  Both Windows presentation modes use one client-size policy, retaining the
  unrounded canvas width/height. Numeric and gesture percentages use the current
  DPI and the 350-logical-pixel width at 100%; never multiply saved width/height
  by a relative ratio. Restore and mode/model changes retain client width and
  derive height from the current canvas before adding the frame. Apply minimum
  and maximum size limits uniformly, and retain the 25% gesture endpoint when
  several percentages map to the same minimum size.
  Native position changes (including DPI suggestions) enforce this policy;
  the independent system caption minimum must not resize one axis afterwards.
  The swap chain follows measured client bounds, including minimized placement.
  Publish scale only after a native sizing gesture changes client dimensions.
  Caption moves (including DPI changes), unchanged and cancelled resizing must
  not reinterpret saved geometry as a new configured scale.
- Closing requests runtime visibility off instead of destroying a live renderer's
  HWND. Minimizing must not overwrite saved geometry or break restoration.
- Model changes prepare/validate before commit and retain the capture HWND in
  window mode. Mode changes may replace presentation resources.

## Verification Gates

- [x] Old v1 data, defaults, RGB bounds, fixture/schema and typed round-trip.
- [x] Windows style, client geometry, close/minimize/restore and DPI contracts.
- [x] Opaque background and actual model pixels from D3D11 readback/capture.
- [x] Model switch and rejected model preserve capture/window state.
- [x] Workspace checks and Development product smoke: `just check`,
  `just dev-smoke` and system-menu smoke passed; the latter verifies
  window-mode toggle and restoration.
- [x] OBS window capture and chroma key inspection on Windows.
- [ ] Manual settings layout and cross-monitor movement at 125/150/200% DPI.
  Automated frame metrics at 120/144/192 DPI passed; actual OBS capture was
  inspected at the current 150% display scale.

The corresponding Phase 4 task is recorded in `docs/TODO.md`. Evidence and
remaining Windows platform checks are recorded in
`docs/benchmark/windows-window-mode-2026-10-08.md`.
