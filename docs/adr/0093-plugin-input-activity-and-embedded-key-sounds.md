# ADR-0093: Per-device input activity, plugin-published selects and embedded key sounds

Status: accepted; implementation complete, platform acceptance tracked below
Date: 2026-10-15

## Context

ADR-0091 gave the key-sounds plugin one anonymous number: how many keyboard
presses the runtime had captured. That is enough for "play a sound while typing"
and nothing more. The plugin cannot tell a press from a release, cannot tell the
keyboard from the mouse, and cannot let a user choose which sound set each
device uses. The user-facing request is separate keyboard and mouse handling, a
switch for mouse sounds, and one sound-set choice per device. Per-edge choices
are deliberately deferred until custom sound effects are expanded.

The reference implementation is Thock (`kamillobinski/thock`), which resolves the
same four edges and plays a `down` or `up` recording out of a soundpack, and
which treats a soundpack that has no release recording as silent on release
rather than reusing its press sound.

Two things in the host had to change for that, and both are protocol surface: a
plugin needs a way to be told about mouse edges and releases, and a plugin needs
a way to publish a list of choices rather than a single number.

## Decision

### One name covers keyboard keys and mouse buttons

The plugin is named **Key Sounds** in English and **按键音效** in Simplified
Chinese. The name matches the product's key-sound vocabulary without suggesting
UI button feedback. Because "key" alone can suggest keyboard-only behavior, the
description explicitly identifies keyboard keys, mouse buttons and their
press/release edges, without implying gamepad, pointer movement or scroll sounds.

The directory is `plugins/key-sounds`, the Cargo package and executable are
`bongocat-key-sounds`, the plugin ID is `com.bongocat.key-sounds`, and the Rust owner
is `KeySounds`. This is a pre-release identity change across source, package
discovery, catalog entries and storage namespaces, not just display copy. No old
ID aliases, directory fallback or data migration are provided. Configuration
keys still describe their devices and actions; their vocabulary and the v1 data
format do not change. The package version remains 1.0.0.
Previously installed Development packages and their data are left untouched;
testing the new identity requires installing its newly built package.

The complete Chinese name/description pair and plugin controls follow `zh-CN`;
all other application languages use English, as required by the existing plugin
locale contract.

Local package preparation removes obsolete ZIP outputs only after all source
plugins build successfully. Renaming or removing a source plugin must not leave
a stale package discoverable in the Development catalog. This cleanup is generic,
does not recognize historical plugin IDs, and never touches installed data.

### Activity is four device totals plus special-key counters

`Capability::InputActivityV1` gates `HostMessage::InputActivity`, which carries
four monotonic device totals (`keyboard_down`, `keyboard_up`, `mouse_down`,
`mouse_up`) plus down/up subsets for a bounded set of physical special keys:
Space, main/keypad Enter, Backspace, Tab, left/right Shift and left Control. The
plugin compares them with the values it saw last, which is what a coalesced,
best-effort signal has to be read as: an idle desktop costs one comparison per
poll and no message, and an edge that arrived between two polls is not lost.

The special-key subset follows Thock's `KeyMapper` and soundpack lookup. Thock
maps Space to `space`, both Return and keypad Enter to `enter`, key code 51
(Backspace/Delete) to `del`, Tab to `tab`, the two Shift keys to
`shiftLeft`/`shiftRight`, and left Control to `ctrlLeft`. It does not map right
Control, and it never selects a `backspace` group. The runtime therefore counts
the physical usages that can reach those groups and the plugin uses the same
names. A key group missing from a pack falls back to `default`; if the group
exists but its selected direction has no recordings, playback is silent rather
than falling back a second time.

The counters still do not expose letters, symbols, text, pointer position or
mouse-button identity, so the privacy argument is unchanged.
`Capability::KeyboardActivityV1` and `HostMessage::KeyboardActivity` remain in
the protocol: a plugin built against the older message keeps receiving it and
keeps working. The registry picks the message by capability, never by protocol
version, and a plugin that declared the newer capability is not sent the older
message as well — it did not ask for two signals, and the duplicate would double
every sound it plays.

### The runtime counts four edges, per device

`InputDiagnostics` gains `keyboard_release`, `mouse_activity`, `mouse_release`
and the special-key subsets beside the existing `keyboard_activity`. A captured
release is a physical edge whatever the pressed set believed about it, so it is
counted even when the press it belongs to is already gone; a reconciled release
and the release a reset invents are the runtime's own correction and are counted
only by the reconciliation counters. A repeat is the platform re-sending a
press, not a second one, so it moves no counter.
`RuntimeClient::input_diagnostics` hands the counter block to the plugin owner
without copying the model or the pressed set.

### A plugin may publish a list of choices

`UiNode::Select { key, label, value, options }` carries its own
`Vec<SelectOption { value, label, group }>`. The optional `group` is plugin-owned
display metadata: options with the same non-empty group are shown under one
stock `gpui-kit` section header, while an omitted group keeps the legacy flat
list. The list travels with the node because the
options are the plugin's own vocabulary — which key sounds it carries, which
of them a user may pick — and the host has no way to invent them. Validation
requires `value` to be one of `options`, every value to be a valid key, and the
list to be non-empty and bounded by `SELECT_OPTION_LIMIT` (64), so a plugin can
never publish a selection the list cannot show.

The host does not infer categories from plugin ids or labels. It reuses the
component's grouped delegate for filtering, row indexing, selection restoration
and muted section headers. A list that mixes grouped and ungrouped options falls
back to the flat delegate, which keeps older or partially upgraded plugins
readable without inventing a host-local category. The key-sounds plugin groups
its embedded sets by the source soundpack brand; brand names are provenance
metadata and therefore do not need translation. In particular, Thock's
`Unknown` brand stays `Unknown` rather than being renamed. Within a brand the
catalog follows Thock's author-then-name option order.
Option labels show only the soundpack name, without repeating the brand or
author; a set without a release recording appends the localized warning.
Source attribution remains in the resource index. Group headings and the
missing-release warning use the same English/Chinese locale selection and
English fallback as the rest of the panel; changing language never changes
option ids or persisted selections. Menu width includes section titles as well
as option labels.

The settings panel draws one `gpui-kit` `Select` per node, created once for the
life of the open panel and only refreshed afterwards: recreating it on every
frame would close a menu the user has open. Its items are rebuilt from what the
plugin published last, and a confirmed choice is written back through the same
typed command every other control uses, so the plugin stays the one that decides
whether a value is acceptable. The menu is measured from the option labels at
paint time, because `Select` sizes its menu from its trigger and a plugin's
longest option is the one a user scanning the list is looking for. The model
window draws a select as `label: selected option`, exactly as it draws a number;
picking another option is a panel action.

Integer nodes use one persistent `gpui-kit` `NumberInput` per config key. Users
can type a value, submit it with Enter or blur, and step it with Up/Down or the
component's buttons. Every accepted path is clamped to the `min..=max` range the
plugin published and saved through the same typed configuration command as the
other controls; an empty or incomplete draft does not issue a command and is
restored to the plugin's last published value on blur.

Select nodes may opt into `preview: true`. The settings window keeps the existing
Select component, but gives each opted-in option row a hover listener that sends
a typed `Preview { key, value }` message without committing configuration. The
key-sounds plugin stops the previous preview before starting the new one, so
rapid movement between options never leaves overlapping preview voices. The
window keeps only the latest pending hover request; leaving a row cancels its
unsent request, dismissing the menu cancels its pending request, and closing the
panel invalidates its pending work. An in-flight
one-shot may finish naturally. The registry validates that the running plugin
published the preview-enabled select and option before sending the message.
The plugin reuses the output device, preserves the fixed input gap, suppresses
input audio while the preview is audible (without replaying those input edges),
and respects pause and mute. A missing release recording remains silent. The
separate preview button and its localized labels are removed. Plugins
and selects that do not opt in retain ordinary confirm-only behavior, preserving
compatibility with older installed plugins.

Only non-loading buttons and toggles are model-window hit targets. A button routes
an action, while a toggle writes the inverse of its latest published Boolean;
display-only numbers/selects and stale/loading controls never route actions.
Control IDs are unique within each view, and option values within each select,
so neither surface can ambiguously resolve a control or choice.

### The key-sounds plugin owns one sound choice per device

The plugin carries the 25 sound sets in Thock's public soundpack registry whose
licenses permit redistribution and compiles them into its own executable with
`include_bytes!`, so nothing it needs is fetched at runtime and no audio
dependency or key-sounds rule enters the host. Of each set it keeps only the
groups it can address: the keyboard
`default`, `space`, `enter`, `del`, `tab`, `shiftLeft`, `shiftRight` and
`ctrlLeft` groups, plus the mouse `left`/`right` groups, each split into `down`
and `up` recordings. Letter and symbol groups are dropped because no product
surface can address them. `backspace` and `ctrlRight` are also omitted because
Thock's mapper never selects those groups.

When a keyboard batch contains exactly one addressable special key and no
ordinary keys, the plugin selects that key's group. A batch with several keys,
or with a special key mixed with an ordinary key, uses `default` and plays once;
the counters cannot identify the individual keys in that batch, and guessing
would either play the wrong recording or play several sounds for one coalesced
edge.

Configuration is a closed vocabulary: `volume`, `mouse_enabled`,
`keyboard_pack` and `mouse_pack`. Both edges of a device play from that device's
chosen set, so a press and its release cannot drift into different sound packs.
The plugin has no user-facing limiter: it always ignores a second sound of the
same action within 10 ms, the window Thock uses for its rapid-key filter. That
window is per action, so a release is never suppressed by the press before it.
The retired `cooldown_ms` and four per-action keys
(`keyboard_down_pack`, `keyboard_up_pack`, `mouse_down_pack`, `mouse_up_pack`)
from this plugin's unreleased development builds remain accepted and ignored, so
an existing session still starts; the current values are what the plugin
publishes. A stored id this build no longer carries resolves to the device's
default set, so a value that names nothing repairs itself on the first frame
instead of failing the exchange. A set with no release recording is silent on
release rather than reusing a press sound, and the panel says so beside the
option. Mouse sounds default to on for new or reset configurations and when the
field is absent; an explicitly stored `false` stays off.

## Dependencies and licensing

No dependency is added. `rodio` stays the workspace's pinned 0.23.0 and the
plugin enables only its `wav` decoder feature, because every embedded recording
is a WAV; the MP3 decoder ADR-0091 enabled for the temporary evaluation asset is
no longer needed, and that asset is removed. The `zip` dependency the first draft
of this plugin used to unpack downloaded soundpacks is removed as well: the
recordings are now committed as files and embedded directly, which is what makes
the plugin offline-capable and what keeps `cargo build -p bongocat-key-sounds`
from depending on a zlib backend.

Only soundpacks whose license permits redistribution carry audio: 22 MIT sets,
one CC0 set, and two Pixabay Content License sets, whose terms permit use of the
content as part of a larger work (which embedding in the plugin executable is)
and forbid only redistribution on a standalone basis. Thock's `Mamba Elite` set
is licensed for the Thock application alone; it is refused rather than reviewed
case by case, and none of its bytes are downloaded, written or embedded.
`resources/index.json` records the author, license URL, source URL and per-file
SHA-256 of every recording; the frozen `registry-manifest.json` is the catalog
oracle. `tools/tests/test_key_sounds_assets.py` checks that the catalog matches
the redistributable subset of that registry snapshot, that the index, committed
files and generated Rust table agree, and that no proprietary set is carried.

## Compatibility

Every protocol addition is optional and defaults to the previous behavior: a
plugin that declared only `keyboard_activity_v1` keeps receiving the older
message and a view with no `Select` node is unaffected. The key-sounds
configuration vocabulary still accepts the same keys, so the exchanged session
payload is unchanged. The ID rename itself is deliberately not migrated: no ID
alias or session re-keying is present while the plugin is unreleased. No config,
state or catalog format changes and `schema_version` stays 1. The plugin's
package version remains 1.0.0 throughout this unreleased PR; ADR-0091 requires
a new version only for an update after the first release. Both plugins accept
application versions `>=2.3.1` without an upper bound.

## Validation

Unit tests cover the select node's validation (a value outside its own list, an
empty list and an invalid option value are all refused), the activity message's
round trip, the runtime's per-device counters (a repeat, a mouse edge, a
reconciliation and a reset each move exactly what they should), and the plugin's
own policy: closed and bounded configuration, retired per-action keys still
loading, a lost set id repairing itself, both edges sharing the device set, a set
with no release recording staying silent, one sound per edge with an unchanged
counter playing nothing, the mouse switch gating both mouse edges, the fixed
10 ms gap being per action, the locale relabelling every control, and the published panel
validating inside its node budget. Every embedded set is decoded by a test, so a
recording this build cannot read fails at test time rather than at a user's first
keystroke.

`just check` (fmt, clippy across targets and features, workspace tests, release
check), `python3 -m unittest discover -s tools/tests` and
`tools/validate-locales.py` are the gates.

### Hover-preview verification

Headless GPUI tests open the stock Select, move the pointer between real rows,
check preview callbacks without selection changes, then confirm a choice with a
click. Mailbox tests cover latest-only delivery, stale row-leave cancellation and
panel lifetime invalidation. Plugin tests cover every published option, device
following, direction/category validation, missing release recordings, mute,
pause, reset, stop and input suppression. Real-pipe tests exercise the preview
message without persisting a selection. An opt-in audio-device unit test asserts
one preview voice, output-device reuse during rapid switching and normal input
recovery after completion; it remains separate from hardware-free CI tests.

On 2026-10-16, both opt-in plugin tests passed on macOS with the real output
device: rapid preview replacement/device reuse/input recovery, and
preview/activity over real pipes.
These are automated audio-output checks, not a manual listening or physical-input
acceptance record. The current invocation is
`cargo test --locked -p bongocat-key-sounds -- --ignored`.

## Remaining Work

Manual listening and physical-input acceptance for all four edges and the mouse
switch remain open. Windows real-audio and DPI checks remain a separate platform
gate. Publishing the plugin to the signed
catalog remains a release gate, as does the final attribution and
redistribution-list review before any public release.
