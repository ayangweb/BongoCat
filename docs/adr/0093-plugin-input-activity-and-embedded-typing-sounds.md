# ADR-0093: Per-device input activity, plugin-published selects and embedded typing sounds

Status: accepted; implementation complete, platform acceptance tracked below
Date: 2026-10-15

## Context

ADR-0091 gave the typing-sound plugin one anonymous number: how many keyboard
presses the runtime had captured. That is enough for "play a sound while typing"
and nothing more. The plugin cannot tell a press from a release, cannot tell the
keyboard from the mouse, and cannot let a user choose which recording each of
those four edges plays. The user-facing request is exactly that: separate
keyboard and mouse handling, a switch for mouse sounds, and an independent sound
choice for each of the four edges.

The reference implementation is Thock (`kamillobinski/thock`), which resolves the
same four edges and plays a `down` or `up` recording out of a soundpack, and
which treats a soundpack that has no release recording as silent on release
rather than reusing its press sound.

Two things in the host had to change for that, and both are protocol surface: a
plugin needs a way to be told about mouse edges and releases, and a plugin needs
a way to publish a list of choices rather than a single number.

## Decision

### Activity is four counters, not one

`Capability::InputActivityV1` gates `HostMessage::InputActivity`, which carries
`InputActivity { keyboard_down, keyboard_up, mouse_down, mouse_up }` — four
monotonic totals counted since the process started. The plugin compares them with
the values it saw last, which is what a coalesced, best-effort signal has to be
read as: an idle desktop costs one comparison per poll and no message, and an
edge that arrived between two polls is not lost.

The counters are anonymous in exactly the way ADR-0091 requires. Nothing about
which key, button, modifier or pointer position produced an edge crosses the
boundary; the four numbers are the whole message, and the privacy argument is
unchanged. `Capability::KeyboardActivityV1` and `HostMessage::KeyboardActivity`
remain in the protocol: a plugin built against the older message keeps receiving
it and keeps working. The registry picks the message by capability, never by
protocol version, and a plugin that declared the newer capability is not sent the
older message as well — it did not ask for two signals, and the duplicate would
double every sound it plays.

### The runtime counts four edges, per device

`InputDiagnostics` gains `keyboard_release`, `mouse_activity` and `mouse_release`
beside the existing `keyboard_activity`. A captured release is a physical edge
whatever the pressed set believed about it, so it is counted even when the press
it belongs to is already gone; a reconciled release and the release a reset
invents are the runtime's own correction and are counted only by the
reconciliation counters. A repeat is the platform re-sending a press, not a
second one, so it moves no counter. `RuntimeClient::input_diagnostics` hands the
counter block to the plugin owner without copying the model or the pressed set.

### A plugin may publish a list of choices

`UiNode::Select { key, label, value, options }` carries its own
`Vec<SelectOption { value, label }>`. The list travels with the node because the
options are the plugin's own vocabulary — which typing sounds it carries, which
of them a user may pick — and the host has no way to invent them. Validation
requires `value` to be one of `options`, every value to be a valid key, and the
list to be non-empty and bounded by `SELECT_OPTION_LIMIT` (64), so a plugin can
never publish a selection the list cannot show.

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

### The typing-sound plugin owns four sound choices

The plugin carries 25 sound sets copied out of Thock's public soundpack registry
and compiled into its own executable with `include_bytes!`, so nothing it needs
is fetched at runtime and no audio dependency or typing-sound rule enters the
host. Of each set it keeps only the groups it can address — the keyboard
`default` group and the mouse `left`/`right` groups, split into their `down` and
`up` recordings. Per-key recordings are dropped: the host reports anonymous
activity and no key identity, so no code path could ever address them.

Configuration is a closed vocabulary: `volume`, `cooldown_ms`, `mouse_enabled`,
`keyboard_pack`, `mouse_pack`, and one `*_pack` key per action. A per-action value
is either the reserved `auto` — follow the device's own set — or a set id. A
stored id this build no longer carries resolves to the device's default set and
the resolved id is what the plugin publishes, so a value that names nothing
repairs itself on the first frame instead of failing the exchange. A set with no
release recording is silent on release rather than reusing a press sound, and the
panel says so beside the option. Mouse sounds default to off, matching Thock.

## Dependencies and licensing

No dependency is added. `rodio` stays the workspace's pinned 0.23.0 and the
plugin enables only its `wav` decoder feature, because every embedded recording
is a WAV; the MP3 decoder ADR-0091 enabled for the temporary evaluation asset is
no longer needed, and that asset is removed. The `zip` dependency the first draft
of this plugin used to unpack downloaded soundpacks is removed as well: the
recordings are now committed as files and embedded directly, which is what makes
the plugin offline-capable and what keeps `cargo build -p bongocat-typing-sound`
from depending on a zlib backend.

Only soundpacks whose license permits redistribution are carried: 22 MIT sets,
one CC0 set, and two Pixabay Content License sets, whose terms permit use of the
content as part of a larger work (which embedding in the plugin executable is)
and forbid only redistribution on a standalone basis. Thock's `Mamba Elite` set
is licensed for the Thock application alone and is refused rather than reviewed
case by case. `resources/index.json` records the author, license URL, source URL
and per-file SHA-256 of every recording;
`tools/tests/test_typing_sound_assets.py` checks that the index, the committed
files and the generated Rust table agree, and that no proprietary set is carried.

## Compatibility

Every protocol addition is optional and defaults to the previous behavior: a
plugin that declared only `keyboard_activity_v1` keeps receiving the older
message, a view with no `Select` node is unaffected, and the typing-sound
plugin's earlier keys (`volume`, `cooldown_ms`) keep parsing, so a session saved
by the previous build still starts. No config, state, session or catalog format
changes and `schema_version` stays 1. The plugin's package version remains 1.0.0
throughout this unreleased PR; ADR-0091 requires a new version only for an update
after the first release. Both plugins accept application versions `>=2.3.1`
without an upper bound.

## Validation

Unit tests cover the select node's validation (a value outside its own list, an
empty list and an invalid option value are all refused), the activity message's
round trip, the runtime's per-device counters (a repeat, a mouse edge, a
reconciliation and a reset each move exactly what they should), and the plugin's
own policy: closed and bounded configuration, a lost set id repairing itself,
per-action pinning, a set with no release recording staying silent, one sound per
edge with an unchanged counter playing nothing, the mouse switch gating both
mouse edges, the cooldown being per action, the locale relabelling every control,
and the published panel validating inside its node budget. Every embedded set is
decoded by a test, so a recording this build cannot read fails at test time
rather than at a user's first keystroke.

`just check` (fmt, clippy across targets and features, workspace tests, release
check), `python3 -m unittest discover -s tools/tests` and
`tools/validate-locales.py` are the gates.

## Remaining Work

macOS real-audio smoke (the four edges, the mouse switch and the preview) is an
opt-in test and is not recorded with evidence here; Windows real-audio and DPI
checks remain a separate platform gate. Publishing the plugin to the signed
catalog remains a release gate, as does the final attribution and
redistribution-list review before any public release.
