# ADR-0090: Independent Rust plugin processes

Status: accepted; implemented, platform acceptance remains open
Date: 2026-10-14

## Research and decision

HashiCorp's [go-plugin](https://github.com/hashicorp/go-plugin) runs plugins in
separate processes, versions its protocol and checks artifacts before execution.
Its Go/gRPC implementation is not a Rust dependency candidate, but the isolation
and handshake design apply directly. [Extism](https://github.com/extism/extism)
provides Wasm isolation, host functions and execution budgets, including our three
targets. A Wasm VM would add a second runtime and substantial package cost before
we need untrusted-code execution; defer it. Rust `libloading` cannot make Rust
trait objects a stable ABI and cannot isolate aborts or hangs. `abi_stable` solves
some ABI problems but not process crashes or main-thread UI ownership. Native
processes, standard-library pipes, existing serde and atomic-write-file are the
smallest viable boundary. Existing cargo-packager-updater transport and Minisign
verification are reused, not a new HTTP or cryptography implementation.

The earlier scaffold's `_plugin_create` exported a thin pointer while its loader
expected a fat trait-object pointer. Do not execute that loader; replace it.

## Candidate requirements (actual GitHub issue bodies)

- #996: countdown reminders. Select first: validates monotonic ticks, actions,
  plugin-owned config validation and model-window presentation without new OS APIs.
- #905: daily keyboard count and mouse travel. Defer until an exact aggregate input
  stream and local-day contract are available; pixels cannot be called kilometres.
- #849: current input source. Implemented as a read-only plugin in ADR-0094;
  Windows TSF/internal conversion-mode and manual platform acceptance remain open.
- #90: meow on typing. Implemented as an independent process in ADR-0091;
  the temporary evaluation audio still needs redistribution license review.
- #74: held-key display. Defer: sensitive input consent, reset/reconcile semantics;
  existing model-authored key images are not a general key display.
- #945: AI skills/agent. Defer: credentials, network, authentication and privacy.
- #927: local AI hook monitoring. Defer: authenticated local IPC, multiple sessions
  and model action admission. No socket port is opened by this implementation.

The acceptance examples are #996 and the independent #90 implementation in
ADR-0091. Earlier input-stats scaffolds are not shipped as working plugins.

## Contract

Each package has a manifest (protocol major, reverse-domain ID, semver app range,
version, entry basename, Emoji/image descriptor and capability list). Discovery
reads metadata without executing code. A plugin is a separately built Rust binary;
no plugin executable or resources enter the application bundle. New plugins need
no host business-code changes. SDK types, not Rust memory layout, cross the wire.
Length-delimited JSON encodes closed typed enums, with a 256 KiB message limit.
Opaque plugin config keys map to typed values; no application `set_value` API is
exposed. Request and response sequence IDs must match. Handshake verifies ID and
protocol before start. Every exchange has a deadline; failure stops just that
plugin. Stdout is protocol-only and stderr is discarded, not a key-sequence log.

Plugins own config defaults, validation, migration, state bytes and both view
surfaces. Views are bounded typed trees (text, action, boolean and integer
controls) with scoped IDs, predefined model-window anchors and settings surfaces.
The host renders a plugin's model view where the model window is shown and its
settings tree in the plugin center. The model view is a dedicated overlay layer,
not part of the Live2D model draw: the platform overlay composites the plugin HUD
into its own texture, quad and clip transform, and the layer's hit rectangles are
routed back to the plugin as click messages on both macOS Metal and Windows D3D11.
Those hit rectangles are in the window's device pixels, because that is the
space a native window reports a pointer position in on both backends; a
rectangle relative to the composed bitmap would sit `origin` away from where the
panel is drawn and no click would ever resolve.
 A panel's palette and metrics come from the gpui-kit theme the settings window
uses — the same `secondary`/`border`/`foreground` tones, `Theme::radius` and the
medium `Button` height and padding. The panel cannot itself be a gpui-kit
`Button`: it is rasterized into one bitmap for the overlay quad and no GPUI
element reaches that path, so the shared tokens are what keep the two surfaces
consistent. Panel text is drawn with a platform face chosen for glyph coverage
of what a plugin may publish, and a character the face lacks is skipped rather
than rasterized as the font's missing-glyph box.
The adapter is shared host code, not a per-plugin rule. Host owns
per-environment/per-plugin storage and atomic writes. Config and state survive
disable/upgrade; uninstall removes executable but preserves data until explicit
reset. Configuration is prepared by the plugin before host commit.
 No GPUI entity, GPU handle or platform pointer crosses the boundary.
Future primitives/capabilities require a protocol minor/version gate, not bespoke
plugin IDs. Native binaries are trusted code, **not an OS sandbox**; capability
checks limit host calls, not their own filesystem/network access. Installation must
explain that trust boundary. Production refuses unsigned catalogs/packages;
Development scans its explicit local package directory and is not Production.

`just dev` and `just dev-smoke` first invoke `just dev-plugins`: the packaging
crate independently builds each local package into a ZIP under `target/plugins`,
exactly the shape a release ships, and stops there. The build tool never writes
the application store — the store has one writer, the product's own install
lifecycle. A Development build discovers those ZIPs after startup and offers
explicit install/update actions through that same lifecycle; discovery does not
reinstall a plugin the user uninstalled (ADR-0091). App bundles still contain no plugins. Runtime directory overrides
are not accepted; Production and Development always use their respective storage
roots. Discovery runs on the background executor and distinguishes loading,
failure/retry and an empty list. The example's display name is Pomodoro and its
source package is `plugins/pomodoro`, with the matching `com.bongocat.pomodoro`
ID and `bongocat-pomodoro` binary. The plugin system is unreleased, so no
persisted identity depends on the earlier `countdown` naming and it needed no
migration path. It is a focus timer, not yet an automatic work/break-cycle
implementation, and its model view is the countdown panel that the plugin keeps
in sync with start, pause and reset. Before the first start the panel shows the
same `MM:SS` shape it will count down from, so the session length is readable
before the timer runs rather than after; a session that has run out stays at
`00:00` instead of reading as a fresh one. That distinction is the plugin's own
state, and state written before the field existed still loads.

Installed packages live under `<plugin_dir>/packages/<id>/<version>/` with a
`current` pointer, and per-plugin data under `<plugin_dir>/data/<id>/`. Install
extracts into staging, validates the manifest, preserves Unix executable bits and
atomically renames into place before switching `current`; uninstall removes the
executable but preserves data until an explicit reset.

`bongocat-plugin-service` owns every plugin for the whole application lifetime.
The settings window is only a client: closing it does not stop plugins, stop
ticks or stop the model-window layer. The service drains commands, advances
plugin ticks on a fixed interval and republishes the composed HUD layer only when
the viewport or the plugin frames actually change.

Install/update prepare a versioned directory, verify signed bytes, then atomically
switch the selected manifest; old running artifacts are stopped before switch.
Failures leave the selected installed version unchanged. GitHub proxy prefixes and
fallback order come from bongocat-update. Shutdown stops plugin admission and asks
for state, then waits within a deadline; forced process termination is a reported
fault containment fallback, not a successful graceful shutdown.

Lifecycle hardening keeps the latest validated output as the registry's only
in-memory configuration/state source. Enable commits only after Start, optional
locale negotiation and session persistence all succeed. A refused configuration
or action consumes its sequence without killing an otherwise healthy session;
transport, framing and advertised-capability failures stop that entry. A failed
configuration write stops the process and retains the previous accepted configuration
for retry. Shutdown attempts every running entry and propagates stop/persistence
failure through the service join; a command-channel shutdown acknowledges only
after successful cleanup, and closes admission and pending replies first.

Install and discovery share bounded manifest validation, application compatibility
checks and directory/manifest identity checks. Discovery isolates bad packages
instead of hiding healthy ones. ZIP extraction rejects symlinks, duplicate or
case-colliding paths and Windows path syntax on every host; version directories
preserve SemVer build metadata. If package rollback itself fails, the previous
tree is retained rather than deleted by temporary-directory cleanup. These checks
do not turn trusted native executables into a sandbox; plugins must keep protocol
stdout private, including when launching descendants. Process-tree containment
for descendants that inherit protocol pipes remains a separate platform gate.

## Gates and limitations

Autotests must cover handshake, compatibility, framing limits, malformed output,
timeout, lifecycle, config rejection, restart, install safety and failed update.
Run the actual separately built pomodoro binary. Both platform adapters need
real UI/interaction/DPI validation; macOS compilation cannot prove Windows behavior.
Production catalog publication/signing is external release work: an unavailable
catalog must be a retryable error, never a fabricated ecosystem or unsigned fallback.
