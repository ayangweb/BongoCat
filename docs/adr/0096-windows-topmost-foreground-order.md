# ADR-0096: Windows Topmost Foreground Order

Status: Accepted; Windows desktop/fullscreen acceptance pending maintainer testing.
Date: 2026-10-24

## Cause and Reference

Issue #1161 includes browser and game coverage despite always-on-top being
enabled. Master already repairs loss of WS_EX_TOPMOST (commit 4f76289d), but
that bit only denotes membership in the topmost band. Two topmost windows
still have an order: a fullscreen foreground browser can move ahead while
the model remains topmost. The existing guard returns early in that case.

The fixed behavior reference Bongo-Cat-Mver at commit
`4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c`,
`BongoCatMver/src/main.cpp`, line 265, reasserts Z order after every display
when topWindow is enabled:
https://github.com/MMmmmoko/Bongo-Cat-Mver/blob/4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c/BongoCatMver/src/main.cpp#L265
It uses HWND_TOP, not a topmost-style test. This establishes that maintaining
actual stacking, rather than merely reading a style bit, matters. We do not
copy its unconditional per-frame raising or activation/show flags.

## Decision

Extend the existing owner-thread frame/show guard, without a worker, timer,
WinEvent hook, new dependency or second policy owner:

- Retain the existing demotion repair, and make the guard itself inert when
  always-on-top is disabled.
- Read GetForegroundWindow. An ordinary foreground window cannot cover a
  topmost model, so skip further work unless it is topmost.
- Use synchronous EnumWindows Z-order enumeration, stopping at the first of
  the foreground or model HWNDs. This avoids the race/infinite-loop risks of
  walking external windows with repeated GetWindow calls.
- Reassert HWND_TOPMOST only when the foreground topmost window is actually
  above the model. Keep SWP_NOACTIVATE, SWP_NOMOVE and SWP_NOSIZE.
- Do not blindly raise above all other topmost windows; menus/tooltips above
  the model stay there while the foreground app is already below it.
- All native calls and the stack callback context stay on the window owner
  thread. No external HWND is retained or dereferenced. A transient missing
  candidate simply produces no reorder; the next existing frame retries.

## Validation and Limits

A Windows regression creates two topmost windows, repeatedly places a simulated
foreground window above the model without clearing either style bit, and checks
that the guard restores actual order without changing bounds or keyboard focus.
An additional popup remains above the model when the foreground window is
already below it. The disabled guard and existing demotion tests remain covered.
Tests deliberately avoid SetForegroundWindow so they do not depend on foreground
activation permission or steal focus from the user running tests.

These native tests can be compiled but not executed on macOS. Windows acceptance
must cover browser fullscreen transitions, repeated page changes, Alt-Tab,
borderless games, hide/show, window-mode recreation, menus, and disabling the
setting. Exclusive fullscreen bypassing DWM and secure desktops are not ordinary
desktop Z-order and cannot be covered by this guard. Other applications actively
fighting for topmost order remain a platform limitation. No local state cleanup
is required; macOS and startup registration are unchanged.
