# ADR-0014: Formal Settings Accessibility Bridge

状态：Accepted（2026-08-31）

## Context

The settings window needs project-owned accessibility semantics before the startup-item UI can
close its accessibility gate. GPUI 0.2.2 does not provide a stable public element-level semantic
API, while platform accessibility adapters must not become a second business-state owner.

## Decision

- `bongocat-ui` builds an `AccessibilityTree` from its existing page, snapshot, pending and focus
  state. Node IDs, roles, values, toggles and supported actions are project-owned types.
- `bongocat-platform` owns the AccessKit macOS/Windows adapter and receives only a copied raw
  window handle plus the project tree. It does not read settings, runtime or config state.
- AccessKit Click/Focus actions cross a bounded capacity-32 channel and are applied by the GPUI
  entity. Clicks reuse the existing typed settings commands; focus is applied through existing
  `FocusHandle`s on the next render.
- Disabled/loading/unsupported controls do not expose mutation actions. Adapter diagnostics count
  forwarded and rejected actions without recording labels, key identities or user content.
- Product smoke may inspect the native AX/UIA role, value, enabled/focusable state and advertised
  action pattern. It must not toggle the startup item; platform lifecycle smoke remains responsible
  for mutation and restoration.
- The bridge is attached after the GPUI native handle exists but while `WindowOptions::show` is
  still false, then the window is activated. It is retained by `SettingsView` and dropped before
  the corresponding GPUI window is destroyed; attachment failure leaves the window hidden and is
  returned to the product coordinator.

## Consequences

The startup switch now has a formal switch role, value, toggle state and action mapping on both
platform adapters. This does not claim VoiceOver/Narrator or physical assistive-technology
verification; those remain release evidence gates. If GPUI later exposes an equivalent stable API,
the platform adapter can be removed without changing runtime or settings command contracts.
