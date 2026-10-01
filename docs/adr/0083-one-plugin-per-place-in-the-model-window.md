# 0083 — One plugin per place in the model window, and the host decides where

**Status:** accepted

A plugin's panel used to sit wherever the plugin's own code asked it to. Two panels that
asked for the same corner overlapped, and an overlap is not something a user can resolve —
they can only turn something off. Placement is now the host's, the user chooses it, and one
position holds one plugin.

## Context

The model window has nine places a panel can go: four corners, four edges and the middle.
Nine is not a lot, which is the whole reason this is a design decision rather than a
detail. It is also why the previous answer — "the plugin picks, and the author can see the
result" — did not survive contact with more than one plugin. A plugin author chose a
corner in the abstract, in a build they ran alone, and had no way to learn that somebody
else's plugin had already claimed it.

The old bound on how many plugins could run at once was four, which is a separate symptom
of the same gap: a number somebody typed, described as being about panels being too small
to see, which was never what it was for. It refused the fifth plugin for a reason that had
nothing to do with the window, and it meant two of the plugins you had installed could not
run at the same time.

## The decision

### A plugin says it has a place, not where the place is

`PluginDescriptor` gains `draws_panel`. It is additive and defaults off, so a plugin built
against a host that did not have it still starts: its panel still draws, in the corner it
asked for. The flag is what makes the position *editable*, not what makes the panel appear,
and that is why a plugin can be silent about it safely.

A plugin with nothing visual — a sound, a tally — does not set it, and is offered no
position row at all. There is nothing on the model window to move, so a menu of nine
corners for a plugin that draws nothing would be a control that changes nothing. The
shipped typing-sound plugin is the worked example: it puts up a chip for a moment after a
keystroke, and that chip is a notification rather than something a user would place.

### One function decides, and everything reads it

A single allocation produces every answer, so a card, a settings form and the published
layer cannot disagree about where a plugin is. In order:

1. **The user's choice, if it is free.** A plugin the user placed keeps that place even if
   a plugin it preferred arrives later — otherwise enabling a second plugin would silently
   move a panel somebody had just arranged.
2. **The plugin's own corner, if it is free.** So a plugin nobody has moved sits where its
   author put it, which is the answer for a fresh install.
3. **The first free position.** A tie, and the only tie. Which of two plugins that both
   default to the same corner keeps it is arbitrary, so the first by id keeps it.

The allocation is a pure function of the sessions and the preferences, so it is computed on
demand rather than cached. There is no map to forget to update when a plugin starts, stops,
draws its first panel or changes its mind, and the cost is a handful of comparisons per
published snapshot against a bug class — a card, a form and a layer disagreeing — that no
test of the allocator alone would find.

The published layer takes its *anchor* from the host and everything else from the plugin: a
panel's size and opacity are the plugin's business, and a corner is the model's.

### A position another plugin holds is not on the list

The settings form offers a plugin its own position and the free ones, and omits the rest
rather than greying them out. A menu that offers a corner and then refuses it is a control
that lies; a position that is not on the list is the truth.

The preference is a preference, not a reservation. A position named for a plugin that is
not drawing anything is not held, so switching a plugin off frees its corner without the
file being edited — and the position itself is *kept*, so a plugin switched off and back on
comes back where it was. A hand-edited file that names the same position twice, or one a
newer build wrote, is resolved rather than refused: a duplicate is a file a person edited,
not a contradiction the product can resolve at load, and refusing it would drop a panel over
a line the user can see and fix in the form.

### The bound is the number of positions

Nine, read from the protocol rather than written down, because a plugin costs a position
and a position is all there is. It is still a bound and still needed — an enabled plugin is
a process the worker is responsible for — but the number now means something.

### Where it is kept

`config.json`, under `plugins.positions`, as a map of plugin id to a position name in the
protocol's own spelling. It is the user's preference about *this product's window*, not a
plugin's own state, so it belongs beside `plugins.enabled` and `plugins.disabled` rather
than in a plugin's data directory.

The configuration crate checks the value is a name rather than a sentence; the protocol
owns the vocabulary, and a name a newer build wrote is read as "the plugin's own corner"
rather than refused, because refusing it would drop a panel over a spelling this build has
not heard of. Two entries may name the same position, for the reason above.

A form written before this field existed reads with every position free, which is a
fixture, and the position names are the product's own copy in all seven languages because
placement is the host's and the user's rather than a plugin's.

## What this costs

- A plugin can no longer choose its own corner, which is a real loss for a plugin whose
  layout depends on where it is. The author still chooses the *default*, and that default
  is honoured whenever it is free — so a plugin nobody has moved looks exactly as its
  author intended.
- A ninth panel on a window with nine places means overlapping, and the product does not
  prevent it; it allocates the tenth and eleventh anyway rather than refusing a plugin the
  user switched on. The bound is on what the *settings window* will admit, and a
  hand-edited file can still exceed it.
- Placement is now product behaviour, so a change to it is a change to the model window
  rather than to a plugin.

## Consequences

- `bongocat-plugin-protocol` gains `draws_panel` and orders `PluginAnchor`.
- `bongocat-plugin` gains `placement`, a `Placements` allocator, and overrides the anchor
  of a published layer. It also answers with what a plugin *actually* got, so the form can
  show the position one plugin ended up at rather than the one that was asked for.
- `bongocat-config` gains `plugins.positions`, and its generated JSON Schema, two fixtures
  and a compatibility test change with it.
- The settings protocol gains a position on a card and two commands — set and clear — kept
  separate from `SetPluginConfig` because the position is not the plugin's and is never
  sent to it.
- A plugin that draws a panel and declares no settings now has a form with one row on it.
  The flag that decided this counted only the plugin's own fields, so such a plugin had a
  settings button that opened nothing.
