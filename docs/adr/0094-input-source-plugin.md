# ADR-0094: Read-only input source indicator plugin

Status: accepted; platform acceptance recorded below
Date: 2026-10-16

## Research

Issue #849 requests a legible current input source near the model on macOS.
The following actual implementations were inspected before implementation:

- [im-select](https://github.com/daipeihust/im-select/tree/9cd5278b185a9d6daa12ba35471ec2cc1a2e3012)
  (MIT): `macOS/im-select/im-select/main.m` copies the current TIS keyboard
  input source and reads its ID. Its small synchronous query fits a plugin.
- [InputSourcePro](https://github.com/runjuu/InputSourcePro/tree/11e86641146b557c9725c88812fe6aa8bea2bb31)
  (GPL-3.0): `InputSourceVM.swift` observes the distributed TIS change notification
  and reconciles once a second. `InputSource.swift` includes the input mode in
  identity, lazily loads icon URLs, and supports always-on and transient views.
  Only the documented OS API approach is referenced; no code, icons or assets
  from this project are incorporated.
- [language-indicator](https://github.com/crencirs/language-indicator/tree/b9cfa01f487d409b4113c440e5ae292a18e48f96)
  (MIT): `input_language_probe.cpp` queries the foreground thread's HKL, never
  thread zero. Its separate IME probe uses timeout-bounded cross-process calls;
  this plugin does not send messages to another application's IME window.

`objc2-carbon` 0.3.2 was inspected (`cargo info`): its bindings are empty, so it
cannot replace the small TIS FFI boundary. Existing objc2/CoreFoundation/AppKit,
CoreGraphics and windows bindings are reused. No new external dependency is needed.
GPUI Kit repository, component documentation and docs.rs were consulted; existing
Select, Integer and Toggle nodes remain the configuration primitives.

## Decision

Ship `plugins/input-source`, ID `com.bongocat.input-source`, as an independent
trusted native plugin. A platform-owned monitor returns only source identity,
localized name, and an optional bounded icon. No pressed keys, composition text,
caret position or application identity leaves the platform boundary. There is
no switching API and no accessibility or input-monitoring permission request.

macOS uses `TISCopyCurrentKeyboardInputSource`, including input mode in identity,
and `kTISNotifySelectedKeyboardInputSourceChanged` on the distributed notification
center. The plugin's main thread services the default CFRunLoop without waiting
on each existing host tick; notifications only mark the snapshot dirty. A one
second reconciliation handles missed notifications and application-specific
source restoration. Icons are decoded only when identity changes, using the TIS
icon URL and AppKit/CoreGraphics, never arbitrary plugin paths or network URLs.
The observer is removed before its callback state is freed.

Windows samples the foreground thread's HKL at the existing 200 ms tick cadence.
IMM descriptions and keyboard layout registry names are preferred over a locale
name. This is a keyboard-layout/source indicator: TSF profiles sharing an HKL and
internal ASCII/native conversion toggles are not claimed as distinct input
sources. No hook, injection, COM thread impersonation or blocking SendMessage is
added. Missing native icons use a clearly rendered language badge on either OS.

The plugin owns always-on versus switch-only policy, a bounded display duration,
localization, and an injectable monotonic tick timeline. Switch-only starts
hidden, records an initial baseline, and shows only a later source change.
Unavailable snapshots clear stale display and report a localized recoverable
status in settings; recovery re-baselines rather than fabricating a switch.

The SDK adds a generic bounded icon/text display node gated by the declared
`icon_text_v1` capability. The process boundary rejects undeclared nodes; an
older host cannot accept a manifest declaring this extension. Inline pixels are fixed
32x32 straight RGBA8; a short badge is the fallback. The shared HUD paints both
at device scale without adding a framework or exposing native handles.
Single neutral-color glyphs on transparency are treated as templates and tinted
with each surface's foreground color; multi-tone and colored icons keep their
artwork. Settings reuse GPUI's content-addressed image cache so repainting does
not create a fresh sprite-atlas entry. Existing
plugins and stored v1 formats are unchanged. The new node is only published by
the new plugin.

## Acceptance

- Automated mode, baseline, rapid switch, expiry, unavailable/recovery,
  configuration, locale, protocol bound and HUD rasterization contracts.
- macOS native monitor and packaged-process smoke, plus workspace quality checks.
- Windows native behavior, third-party TSF coverage, Retina/DPI and manual
  switching remain platform acceptance gates, not inferred from compilation.
- No signed catalog publication or production installation in this task.
