# 0081 — A plugin declares the controls it wants the host to draw

**Status:** accepted

**Related:** ADR-0079 (a plugin is a process), ADR-0080 (plugin copy before
install), ADR-0078 (the model window plugin system it superseded).

## Context

ADR-0079 made a plugin a program that owns its own logic, and gave it five
things to ask the host for: a panel to draw, a clock to read, a press to hear
about, a set of settings to be configured, and — when it wants the model to
react — a motion or a bubble.

Five is enough for everything those six plugins *do*. It is not enough for
everything a user does with them.

The complaint that forced this was one sentence long: after installing the
pomodoro there was no way to start the countdown. Everything was correct and
the product was unusable, because the only Start button was drawn inside a
260-pixel panel on the model window, and the pomodoro is *configured* in the
settings window. So the two halves of one feature were in two places, and the
half you needed after configuring it was the half you had to go and find.

The same report contained two more facts that turned out to be one root cause:

* The settings button **disappeared** whenever the switch was off. A plugin's
  config schema arrives with its handshake, so a stopped plugin has no fields,
  and the button was drawn only when there were fields. A card for a
  just-installed plugin therefore had a delete button and one mystery switch.
* The switch was labelled *"Show on the model window"* — which describes the
  consequence of the switch rather than what the switch is, and is not an
  answer at all to someone who does not yet know the product has a model
  window.

Neither of those is about the protocol. They are about a card whose controls
depend on a process that may not be running, and about a label that describes
the wrong thing.

### What the host could have done instead

The obvious move is to read the buttons out of the panel the host already has
and draw those on the card. No protocol change, no plugin change, and the
labels are already live.

It is wrong for three reasons, and the first is the one that decides it:

1. **A panel's layout is not an inventory of what a plugin can do.** Key Stats
   draws a row of keys. Turning those into application controls means
   offering forty-odd buttons nobody asked for. An action is something a
   plugin *chooses* to offer, and a plugin may offer one whether or not it
   ever draws a panel.
2. **A plugin with no panel gets nothing.** A plugin that reacts to the model
   without putting anything on screen has no panel to read buttons from, which
   is exactly backwards.
3. **A card is built from a snapshot; a panel is not in the snapshot.** Deriving
   an action from a panel would make the settings window read the renderer's
   state — the coupling this system removes twice already, since the host never
   evaluates what a panel shows and the window never sees a scene.

The host-side alternative — knowing that `pomodoro` has a start button — was
not seriously on the table. It makes the host know a specific plugin's
business meaning, which is precisely what ADR-0079 moved out of the host, and
it gives third-party plugins nothing.

## Decision

**A plugin may declare *actions*: controls it wants the host to draw on its own
card in the settings window.**

The division is the one the whole plugin system already uses. The plugin says
what a control *means* and what it is *called*; the host says what it *looks*
like. So an action carries an id, the plugin's own localized label, and a glyph
from a closed set — never a colour, a size, a font or a position.

Three decisions inside that are load-bearing:

**An action's id is a panel button's id.** A press of either arrives as
`HostMessage::Press` with the same string. Adding the first therefore costs a
plugin *no second handler*: pomodoro's `on_press` already answers `"toggle"`,
and the card's button and the panel's button are one control to the plugin and
one control to the user. This is what makes the mechanism cheap enough to be
universal rather than a per-plugin special case.

**Actions are a message, not part of the handshake.** An action carries a
label, and a label that goes stale lies: a timer whose card still reads
"Start" while its round is counting says the opposite of what the press will
do. So the plugin re-sends the whole list whenever any of its meaning changed,
and the host *replaces* rather than merges — which is what lets a plugin
withdraw a control, and what makes a press of a withdrawn id ignored rather
than delivered. A plugin that re-sends an unchanged list writes nothing, so
calling this every tick is free.

**The two UI defects are fixed separately, and neither is a protocol change.**
The settings button is now drawn unconditionally for an installed plugin:
pressing it on a stopped plugin turns the plugin on, because that is the only
thing that can make a form exist, and the form opens when the handshake
arrives rather than being refused. The switch is labelled "Enabled", which
names the switch instead of describing its consequence.

`MAXIMUM_ACTIONS` is 4. These are controls on a card about 260 pixels wide,
beside a switch and two buttons every installed plugin has. A plugin wanting
more room than that wants a panel, which is the surface built for showing many
controls at once.

## Consequences

- **A third-party plugin can now offer a control from the settings window**,
  with its own words and its own icon, and change what that control means while
  the user is looking at it.
- **Adding one costs a plugin no new handler.** The press vocabulary is shared
  with panel buttons.
- **A plugin that offers nothing is unaffected.** Empty is the default, and
  `offer_actions` sends nothing until the list is non-empty — so the six
  shipped plugins keep behaving exactly as they did.
- **The card's settings button no longer disappears**, which is the difference
  between a plugin being configurable and not being.
- **A stopped plugin's settings still cannot be *written* before it starts**,
  because the plugin owns its own file and only a running process reads and
  writes it. Turning it on is the honest answer and it is what the button does.
- **The host gains a small amount of protocol surface** — one message, one type,
  one command, one field per entry — and no knowledge of any plugin's meaning.
- **The label burden moved to where the copy already lives.** An action's label
  is a plugin's own `LocalizedText`, exactly like its name and its settings
  labels; the application does not learn a third-party plugin's words.
