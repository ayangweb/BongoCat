# ADR-0091: Automatic plugin catalogs and anonymous input activity

Status: accepted; implementation and platform acceptance tracked below
Date: 2026-10-15

## Decision

Discovery is not installation. Development discovers ZIP manifests in the fixed
workspace `target/plugins` directory; Production discovers the signed GitHub
release catalog. Both expose available and installed versions in the same center,
with explicit install/update/uninstall controls. Startup and retry refresh the
catalog without reinstalling removed packages. Catalog IO runs separately from
the plugin tick/action owner. Production requires a signed catalog and a package
digest bound to that catalog; there is no unsigned or local-file fallback.

Catalog selection is per plugin ID, not the single newest release across all IDs.
An upgrade preserves plugin config/state, replaces registry metadata and restarts
an enabled process. A refused package must retain the previous selected version.

> Amended by ADR-0093: the single keyboard-down counter below is superseded by
> four per-device counters (`input_activity_v1`), the temporary MP3 evaluation
> asset is removed, and the plugin now embeds its own sound sets. The privacy
> argument, the coalescing rule and the independent-process boundary are
> unchanged.

Issue #90 is implemented by an independent button-sounds process. The generic
`keyboard_activity_v1` capability gates a new typed message carrying only a
monotonic timestamp: no key identity, text, modifiers or release sequence crosses
the plugin boundary. Activity is coalesced over the service cadence and never
shares or delays the reliable edge queue. The runtime counts only newly captured
keyboard downs; repeats, mouse/gamepad input and reconciliation do not count.
The plugin owns volume, cooldown, playback, device idle release and its audio.
It reuses the workspace's pinned rodio, enabling its existing MP3 decoder feature
only for the plugin. No audio dependency or button-sounds rule enters the host.

The supplied `/Users/ayang/Downloads/0.mp3` is a temporary evaluation asset, copied
into the plugin's own resources, not referenced by an absolute runtime path.
Public redistribution remains gated on confirming its license and attribution.

## Dependencies and publication

The catalog reuses cargo-packager's complete base64-encoded Minisign key/signature
boxes, including prehashed signatures, not the naked public-key line. The
`base64` crate is pinned to 0.23.1, the latest stable reported by `cargo info` on
2026-10-15; it is MIT/Apache-2.0, actively maintained, portable safe encoding code,
and confined to catalog serialization. No third-party type enters a public API.
The existing rodio 0.23.0 is also confirmed current; its optional Symphonia MP3
codec is activated only by the independent plugin.

`just plugin-catalog <output/plugins.json> <platform> <package-directory> ...`
assembles distinct per-platform ZIP asset names and signs canonical catalog bytes
with the existing `SIGNING_PRIVATE_KEY` / `SIGNING_PRIVATE_KEY_PASSWORD` inputs.
It refuses unsigned output and changed bytes under an existing release asset
name, and never uploads anything. Upload the complete output set to the product's
GitHub release only after the asset license/release gates pass.

Published package versions are immutable; changes intended as an update after
release must bump the plugin manifest and package version before rebuilding.
During the current unreleased plugin-system PR, both plugins stay at 1.0.0.
Both declare `app_versions: ">=2.3.1"`, with no upper application-version bound.
Config/state and enabled status use a bounded, atomic v1 session envelope outside
the version directory. Missing optional session fields default safely; disabled
and uninstalled plugins do not restart automatically. Graceful shutdown preserves
the enabled flag while stopping and joining the process.

Replacement stops preserve that flag even if extraction or the new process's
Start/locale exchange fails. Uninstall clears it for failed as well as running
entries before retaining their data, so reinstall cannot resurrect a prior enable
request. Reset restores a previously running process when data deletion fails;
it reloads only its own package, not unrelated damaged packages. Package downloads
serialize conflicting install/uninstall/reset/enable requests for their target.
Service loop drains have finite budgets so a continuously refilled command or
interaction queue cannot starve ticks and shutdown. Newly enabled, replaced,
rolled-back or reset counter-based processes receive a fresh input baseline;
existing processes first receive pending activity and keep their own baselines.
If a damaged installed session prevents registration but the verified catalog
still identifies the package, the card retains its installed version and failed
state so reset/uninstall remain reachable rather than offering a same-version
install that immutable-version checks must reject.

## Model controls

The macOS event location is window-local, not screen-local. Convert via the
content view to backing pixels, then flip Y using content height. Consume a
plugin's mouse down so the panel's movable background cannot also begin a drag.
Windows uses client coordinates for client messages and converts screen points
for non-client messages, preserving window dragging outside controls.

GPUI surfaces use gpui-kit's actual `Button`. The independent Metal/D3D11 model
overlay cannot embed GPUI elements without violating ADR-0090 and the renderer
boundary; its existing shared HUD adapter remains, rather than adding a second
UI framework or private GPUI renderer integration. Actual component reuse inside
the model overlay is not claimed. Consulted gpui-kit repository, component Button
documentation and docs.rs 0.7.3 before UI changes.

## Plugin Center cards

> Amended by ADR-0092: the single-card-list layout, the shared generic action
> glyph and the "Playing / 正在播放" placeholder line below are superseded. The
> center is now a responsive grid shared with the model library, each button
> carries the glyph its meaning calls for (declared by the plugin through the
> protocol, never inferred by the host), in-flight work is drawn as a loading
> button, and each plugin has its own settings modal. Lifecycle, storage and
> catalog decisions in this ADR are unchanged.

The center uses the existing GPUI Kit `Button`, `Switch` and theme tokens, not
custom-painted controls. Each card groups its icon, name and version at the top,
then description/status/configuration, then a shared footer: enable/disable on
the left and icon-only commands on the right. The command group wraps within
narrow cards. Install/update, data reset and uninstall have standard host icons;
plugin-authored buttons from either view join the same footer and use a generic
action icon with the original label as the tooltip. The current protocol has no
action-icon metadata: do not infer semantics from IDs or translated labels, or
add per-plugin rules to the host. Model-window rendering is unchanged.

Loading disables both lifecycle and plugin-authored controls. This presentation
change does not alter the protocol, config/state formats or lifecycle ownership.
Regression tests cover footer alignment, enabled/disabled/available cards and
loading-time command gating. The repository, component Button/Switch/Tooltip
documentation and docs.rs 0.7.3 were consulted before implementation.

## Acceptance

- Automated catalog/lifecycle, previous-data, activity/privacy and cooldown
  regression tests must pass, along with workspace quality checks.
- macOS real UI/audio smoke evidence is recorded with the implementation report.
- Windows interaction/DPI and real audio remain a separate platform gate.
- Signed Production catalog publication and temporary audio licensing remain
  external release gates; unavailable catalogs remain retryable failures.
