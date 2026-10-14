# ADR-0090: Independent Rust plugin processes

Status: accepted for implementation; platform acceptance remains open
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
- #849: current input source. Defer: platform IME APIs and session semantics.
- #90: meow on typing. Defer: licensed audio assets, rate policy and audio admission.
  Existing motion sound is not equivalent to a general typing sound.
- #74: held-key display. Defer: sensitive input consent, reset/reconcile semantics;
  existing model-authored key images are not a general key display.
- #945: AI skills/agent. Defer: credentials, network, authentication and privacy.
- #927: local AI hook monitoring. Defer: authenticated local IPC, multiple sessions
  and model action admission. No socket port is opened by this implementation.

Only #996 is an acceptance example. Earlier typing-sounds/input-stats scaffolds
are not shipped or claimed as working plugins.

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
surfaces. Host owns per-environment/per-plugin storage and atomic writes. Config
and state survive disable/upgrade; uninstall removes executable but preserves data
until explicit reset. Configuration is prepared by the plugin before host commit.
Views are bounded typed trees (text, action, boolean and integer controls), scoped
IDs, predefined model-window anchors and settings surfaces. Native adapters render
model content in the existing model window; GPUI renders configuration in the
plugin center. No GPUI entity, GPU handle or platform pointer crosses the boundary.
Future primitives/capabilities require a protocol minor/version gate, not bespoke
plugin IDs. Native binaries are trusted code, **not an OS sandbox**; capability
checks limit host calls, not their own filesystem/network access. Installation must
explain that trust boundary. Production refuses unsigned catalogs/packages;
Development scans its explicit local package directory and is not Production.

Install/update prepare a versioned directory, verify signed bytes, then atomically
switch the selected manifest; old running artifacts are stopped before switch.
Failures leave the selected installed version unchanged. GitHub proxy prefixes and
fallback order come from bongocat-update. Shutdown stops plugin admission and asks
for state, then waits within a deadline; forced process termination is a reported
fault containment fallback, not a successful graceful shutdown.

## Gates and limitations

Autotests must cover handshake, compatibility, framing limits, malformed output,
timeout, lifecycle, config rejection, restart, install safety and failed update.
Run the actual separately built countdown binary. Both platform adapters need
real UI/interaction/DPI validation; macOS compilation cannot prove Windows behavior.
Production catalog publication/signing is external release work: an unavailable
catalog must be a retryable error, never a fabricated ecosystem or unsigned fallback.
