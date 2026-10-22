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
trusted native plugin. A plugin-owned platform monitor returns only source identity,
localized name, and an optional bounded icon. No pressed keys, composition text,
caret position or application identity leaves the platform boundary. There is
no switching API and no accessibility or input-monitoring permission request.

macOS uses `TISCopyCurrentKeyboardInputSource`, including input mode in identity,
and `kTISNotifySelectedKeyboardInputSourceChanged` on the distributed notification
center. Its main thread waits in the default CFRunLoop and queries on source
change notifications, startup and resume; no periodic reconciliation is used. On macOS the system's own icon is preferred, with the TIS
icon URL as the fallback for an input method that ships one; see ADR-0095.
Icons are decoded only when identity changes, using AppKit/CoreGraphics, never
arbitrary plugin paths or network URLs.
The observer is removed before its callback state is freed.

Windows samples the foreground thread's HKL at the existing 200 ms tick cadence.
IMM descriptions and keyboard layout registry names are preferred over a locale
name. This is a keyboard-layout/source indicator: TSF profiles sharing an HKL and
internal ASCII/native conversion toggles are not claimed as distinct input
sources. No hook, injection, COM thread impersonation or blocking SendMessage is
added. Missing native icons use a clearly rendered language badge on either OS.
ADR-0096 replaces the Windows half of this paragraph: the handle decides the
language, the session's active profile decides the input method, that method is
named and drawn from its own registration, and the conversion mode is read through
a bounded message to the focused thread's IME window. Its identity follows a switch
on the next snapshot rather than at process start.

The plugin owns always-on versus switch-only policy, a bounded display duration,
localization, and an injectable monotonic tick timeline. New, reset, or missing-mode
configurations default to always-on; existing saved display modes are preserved.
Settings expose the display mode and, for switch-only, duration without a current
source icon/name preview. Unavailable and paused status messages remain visible.
The source icon/name is published only to the model window. Switch-only starts
hidden, records an initial baseline, and shows only a later source change.
Unavailable snapshots clear stale display and report a localized recoverable
status in settings; recovery re-baselines rather than fabricating a switch.

The SDK adds a generic bounded icon/text display node gated by the declared
`icon_text_v1` capability. The process boundary rejects undeclared nodes; an
older host cannot accept a manifest declaring this extension. Inline pixels are fixed
32x32 straight RGBA8; a short badge is the fallback. The shared HUD paints both
at device scale without adding a framework or exposing native handles, in a slot
whose height is fixed and whose width follows its content: square for artwork, as
wide as its label for a badge.
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

## Plugin ownership audit (2026-10-21)

Input source queries, native observation and icon extraction live under
`plugins/input-source/src/input_source/`, compiled only by the plugin. The plugin
depends on the SDK and narrowly enabled existing system bindings, not the application
platform crate. Unsafe is denied in business code and allowed only in the native
adapter. The obsolete Swift-link workaround and platform public API are removed.
The desktop observer smoke is retained as a plugin example running on the main thread.

Bindings reuse the already selected stable objc2 0.6.5 / objc2 framework 0.3.2
(objc2: MIT; framework bindings: Zlib OR Apache-2.0 OR MIT) and windows 0.62.2 (MIT OR Apache-2.0); `cargo info` verified the runtime
and Windows versions. Their types remain private to the plugin adapter, and
replacing the bindings does not change the SDK or host API. Workspace-wide
`cargo update` also refreshes compatible transitive lock entries as required by
the dependency policy; no direct application dependency was upgraded.

## Notification-driven macOS updates (2026-10-22)

macOS no longer queries TIS on host ticks or performs one-second reconciliation.
Its main thread blocks in the default CFRunLoop; the distributed TIS notification
marks the source dirty and triggers a query and publication immediately. Startup
and resume still establish a baseline. Host ticks only advance the transient
visibility timeline. Windows was attempted the same way and kept its clock; see
the section below.

The generic `push_output_v1` capability reserves response sequence zero for
validated unsolicited output. Ordinary replies keep their request sequence.
A bounded reader queue wakes the plugin-service owner, which collects updates
and composes the HUD without waiting for the next 200 ms tick. Other plugins
retain their existing request/reply behavior. The native callback only marks an
atomic flag; queries and stdout writes remain on the plugin's main thread.
Protocol input signals a separate run-loop source from the stdin reader using
CoreFoundation's documented thread-safe Signal/WakeUp operations.

Tests cover notification capability admission, request sequence preservation,
and macOS ticks not querying the system. Actual switch-to-presentation latency,
application-specific restoration notifications and sleep/session recovery still
require interactive macOS acceptance. No polling fallback masks missing system
notifications; transient hiding retains the existing host clock cadence.

## Windows keeps reading on the host clock (2026-10-22)

Windows was converted to the same notification-driven shape as macOS and then
reverted, because the notifications it needs do not reach a background process.
This is what was measured on Windows 11 26200 before deciding.

What was tried: the plugin's main thread blocked in `GetMessageW` on its own
queue, the stdin reader waking it with `PostThreadMessageW`, a
`SetWinEventHook(EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT)` foreground hook,
and an `ITfInputProcessorProfileActivationSink` advised through `ITfThreadMgr` and
`ITfSource`. A change was published as a sequence-zero update instead of being
answered on the next tick.

- The foreground hook works, and it is not the problem. It delivers to a
  **windowless** background thread's `GetMessageW`, wakes it, and the callback has
  already run by the time the wait returns; `DispatchMessageW` is not what invokes
  it. End to end against the real plugin binary, a foreground change produced a
  sequence-zero update within measurement noise of the OS delivering the event to
  a hook in the driving process (98.1 ms versus 98.2 ms).
- The activation sink does not work for this process. It fires only on a thread
  that owns the focused window: advising it on a windowless background thread and
  then switching the session's input method produced **no** callback, while a
  fresh `GetActiveProfile` on that same thread did report the new source. A switch
  made from another thread was equally invisible. The plugin never owns the
  focused window, so this sink can never be its trigger.
- There is nothing to listen to instead. A hidden top-level window receives
  neither `WM_SETTINGCHANGE` nor `WM_INPUTLANGCHANGE` when the session's input
  method or layout changes, so no broadcast exists either.

Together that leaves exactly one case uncovered: the user switches input method
inside the window they are already in. The foreground hook cannot see it (the
window did not change), and the sink cannot see it (no focused window of ours).
That is the common case for this plugin, and a live desktop test confirmed the
display simply stayed on the previous source. Covering it with an event would
need a hook into another process, which this plugin does not do.

Two smaller findings survive the revert and are recorded because they cost time:

- The apartment must be opened before a TSF object is created. With
  `CoInitializeEx` running afterwards, `CoCreateInstance(CLSID_TF_ThreadMgr)`
  fails with "CoInitialize has not been called".
- Holding the thread manager that carries an activation sink does **not**
  reintroduce the freeze this ADR recorded; that belongs to
  `ITfInputProcessorProfileMgr`, which is still created and dropped per read.

Windows therefore keeps the pre-existing behavior: `HostMessage::Tick` reads the
source while the plugin is running, so a switch is reflected within one host tick,
and `Start`/`Resume` still establish the baseline. The window from a switch to the
display is the host's 200 ms cadence, which is what this plugin shipped with and
what a live desktop test confirms works. macOS keeps its notification loop, where
the distributed TIS notification does reach a background process.

Rejected alternative: keeping the foreground hook as an accelerator on top of the
clock. It would shorten app-switch latency from at most one tick to nothing, at
the cost of a second write path, a custom message loop and a system-wide hook, for
a delay the user cannot perceive. The plugin publishes nothing it cannot also
report on the tick, so the simpler path is the one that is kept.

Tests: Windows ticks are covered by a test that releases the monitor and asserts
the tick re-creates it, i.e. that a tick observes rather than only advancing time;
the packaged-process test still tolerates interleaved sequence-zero frames so the
macOS path stays covered.
