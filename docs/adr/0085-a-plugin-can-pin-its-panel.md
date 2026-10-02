# 0085 — A plugin can pin its panel, and the Key Display records presses instead of holding keys

**Status:** accepted

Two changes that only make sense together, because the first is what the second needs.

## Context

The key display is the one plugin whose panel is read rather than operated. Nobody touches
it; a viewer on a stream reads it. And two of its settings were making it worse than it had
to be.

**The display position.** ADR-0083 made a panel's corner the host's to decide and the
user's to choose, with one plugin per position. The key display's panel is the one panel
whose *place* is most of its value: a viewer either notices it or does not, and a panel that
can sit in any of nine corners is one they have to find. So the corner should not be
movable — but the only way to say that with the vocabulary that existed was to stop
declaring `draws_panel`, and that is a lie: the panel does exist, it does take a corner, and
a panel the host does not know about is one another plugin is free to be allocated on top of.
The exact overlap ADR-0083 exists to prevent.

**What the display showed.** It showed the keys held *down*. A fast keystroke is down for
tens of milliseconds, so on a screencast — the case the plugin exists for — a panel of held
keys is a panel that is almost never there. KeyCastr, which is what this plugin is modelled
on, does not do this: it keeps a *record* of what was pressed, a chord's modifiers on the
key they were held with, and a line that outlives the keys and then goes away on its own.
Adopting that also settles the second setting. "Hide the panel when nothing is held" has no
meaning under the record model — nothing is held a moment after a keystroke — so the answer
was never going to be a choice between showing an empty box and not showing it.

## The decision

### A plugin may pin its panel, and the pin is a claim rather than a preference

`PluginDescriptor` gains `pinned_panel: Option<PluginAnchor>`, additive and absent by default,
and the SDK gains `Descriptor::pins_panel(anchor)`. It implies `draws_panel`, because a pin
is about a panel; a descriptor that pinned one without declaring it is refused rather than
resolved, for the same reason two spellings of a descriptor are refused elsewhere.

The host's placement allocator grows one rule in front of ADR-0083's three:

1. **The panel's own pin.** The corner is reserved before anything else is considered.
2. The user's choice, if it is free.
3. The plugin's own preference, if it is free.
4. The first free position.

A pin therefore outranks a position the user chose *earlier*, which is the only interesting
part: the preference outlives a plugin's restart, so without this, switching a plugin off and
on again would move a panel that cannot be moved. What the pin does not do is take the panel
out of the arrangement — it is still allocated, it still holds a position, and switching it
off still frees the corner for somebody else.

Two answers change, and both are `None`/`empty` rather than a new value to render:

* `Placements::of` reports `pinned`, and the snapshot's `position` is `None` for a pinned
  panel, so the settings form draws no row at all.
* `Placements::available_for` returns nothing for a pinned panel, so no menu is offered that
  the host would ignore.

Dropping `draws_panel` instead would have been smaller. It would also have taken the panel
out of every arrangement — which is precisely what makes it wrong rather than merely blunt:
the panel would still be drawn, in a corner the host no longer knew anything about, and
another plugin placed in that corner by the user would overlap it.

### The Key Display records presses

Four behaviours, all following KeyCastr's `KCDefaultVisualizer` and `KCEventTransformer`:

* **A chord is one keycap.** The modifiers down at the moment a key went down, written as
  `⌃⌥⇧⌘` in that order, then the key: `⌘S`. A modifier pressed on its own is its own cap
  first — `⇧` then `⇧A` — because the `⇧` before the `⇧A` is what tells a viewer the order.
* **A burst is one line.** Half a second of quiet starts a new line, which is KeyCastr's
  `keystrokeDelay`.
* **A command breaks the line.** A chord containing `⌃` or `⌘` starts its own line, so a
  shortcut reads alone rather than at the end of the word being typed. The test is the
  *chord*, not the key: `S` is a letter and `⌘S` is a shortcut.
* **A line outlives the keys and then goes.** Two seconds after its last key — KeyCastr's
  `fadeDelay` — the panel takes itself down. KeyCastr then fades it over a fifth of a
  second; this does not, because the host ticks a plugin at most every 250 ms and a fade
  shorter than that is one frame. The *timing* is the part worth copying and the fade is not.

There is no longer a setting for whether the panel is up when nothing is held. A panel with
nothing on it is a box in the corner of the model window saying nothing, so it is not drawn;
one behaviour is the honest behaviour and a choice between two is a control that lies about
one of them.

Auto-repeat is still ignored, which is a divergence from KeyCastr: the protocol marks a
repeat explicitly, and a panel of `A A A` while one key is held shows the keyboard's timer
rather than the person's hands.

Two consequences worth writing down:

* **A release takes nothing off the display.** It leaves the held set and the cap stays.
  That is the record model: what a viewer needs is the key that was pressed.
* **A reset clears the held set and not the line.** A lock screen does not un-press what was
  pressed; what it means is that the next chord must not be composed from keys this plugin
  believes are down when they are not.

### The plugin's own clock, not the tick's

The line break and the lifetime are measured from the process's monotonic clock rather than
from `Tick::elapsed_ms`. The host sends a tick at most every 250 ms, so a plugin deciding
"was that a pause?" from a tick decides it from a reading up to a quarter of a second old —
which at a typist's speed is enough to break a line in the middle of a word. A tick is still
what drives the expiry, because expiry is the one thing that happens without an event.

## What this costs

- `bongocat-plugin-protocol` gains `pinned_panel`, and `bongocat-plugin`'s allocator gains a
  struct for what a plugin claims and a rule for pins. `Placements::allocate` no longer takes
  a bare `(id, anchor)` pair, which is a breaking change to a `pub` function — the reason it
  is a `Claimed` with both facts rather than two parallel maps.
- A pinned panel cannot be moved, by the user or by a future feature that wanted to. That is
  the point, and it is why the field is a claim rather than a default: a plugin that wants a
  movable panel says nothing.
- The panel's keycap width now follows the labels it is showing, so the layout arithmetic
  packs a row and wraps rather than reserving room for the longest possible chord always.
  `MAXIMUM_PANEL_WIDTH` is derived from the widest cap the plugin can draw and is checked by
  a test, because a cap wider than the panel is a cap drawn off the edge of it.
- The key display no longer offers a position, and the nine positions are still nine.

## Consequences

- `bongocat-plugin-protocol` gains `pinned_panel` and refuses one without `draws_panel`.
- `bongocat-plugin-sdk` gains `Descriptor::pins_panel` and `Descriptor::pinned_anchor`.
- `bongocat-plugin` gains `Claimed`, `Placed::pinned`, and a first pass in the allocator.
- The key display's settings lose the hide-when-idle toggle, and its manifest loses the three
  copy entries that toggle and the idle line needed.