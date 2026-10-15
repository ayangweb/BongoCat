# ADR-0091: Automatic plugin catalogs and anonymous typing activity

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

Issue #90 is implemented by an independent typing-sound process. The generic
`keyboard_activity_v1` capability gates a new typed message carrying only a
monotonic timestamp: no key identity, text, modifiers or release sequence crosses
the plugin boundary. Activity is coalesced over the service cadence and never
shares or delays the reliable edge queue. The runtime counts only newly captured
keyboard downs; repeats, mouse/gamepad input and reconciliation do not count.
The plugin owns volume, cooldown, playback, device idle release and its audio.
It reuses the workspace's pinned rodio, enabling its existing MP3 decoder feature
only for the plugin. No audio dependency or typing-sound rule enters the host.

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

Installed package versions are immutable; local plugin code changes intended as
an update must bump the plugin manifest and package version before rebuilding.
Config/state and enabled status use a bounded, atomic v1 session envelope outside
the version directory. Missing optional session fields default safely; disabled
and uninstalled plugins do not restart automatically. Graceful shutdown preserves
the enabled flag while stopping and joining the process.

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

## Acceptance

- Automated catalog/lifecycle, previous-data, activity/privacy and cooldown
  regression tests must pass, along with workspace quality checks.
- macOS real UI/audio smoke evidence is recorded with the implementation report.
- Windows interaction/DPI and real audio remain a separate platform gate.
- Signed Production catalog publication and temporary audio licensing remain
  external release gates; unavailable catalogs remain retryable failures.
