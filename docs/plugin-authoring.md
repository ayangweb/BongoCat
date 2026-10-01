# Plugin authoring

A plugin is a **separate program**. It is not a library the product loads, not a
manifest the host interprets, and not an ABI anything shares. It is an executable
file that the host starts, talks to over two pipes, and stops — and everything it
decides, it decides in its own code, in its own process, with its own dependencies.

`docs/adr/0079-plugins-are-independent-processes.md` is the decision this
implements, and it is the place to look for why a plugin is a process rather than a
shared library.

## What a plugin is

Two things, both inside one directory:

- **A manifest** — `plugin.json`, carrying the identity: id, name, version, author,
  description, an icon, and the name of the executable beside it. `name` and
  `description` are localized — either a plain string or
  `{ "default": …, "by_locale": { "zh-CN": … } }` — because this is what a card shows
  when the plugin is installed but not yet running.
- **An executable** — the plugin's own program, which is where all of its logic,
  state and configuration live.

A plugin may bring any dependency it likes. Two of the plugins this repository ships
depend on a date library and a JSON library respectively, and neither of them is a
dependency of the product: `plugins/Cargo.lock` is its own lockfile, and adding a
plugin never adds a line to the product's `Cargo.lock`.

## The dependency

One crate: `bongocat-plugin-sdk`. It brings the wire protocol and `serde` with it
and nothing else — no renderer, no platform code, no product types. A plugin author
needs to know nothing about the product's internals to write one.

## The conversation

The host writes newline-delimited JSON to the plugin's **standard input** and reads
newline-delimited JSON from its **standard error**. Nothing goes out on standard
output.

```text
host  -- hello ------->  plugin     identity, data directory, language, settings
plugin -- ready ----->  host        the descriptor, and the settings it declares
host  -- tick ------->  plugin     elapsed time, what the host knows, the wall clock
plugin -- panel ----->  host        the panel, as a tree of nodes
plugin -- request --->  host        play a motion, or say something
host  -- answer ----->  plugin      what became of that request
host  -- input ------>  plugin       only if the plugin subscribed to the input feed
host  -- press ------>  plugin       only if the plugin's own button was pressed
host  -- shutdown --->  plugin
```

A plugin answers a **press** by id and nothing else: the host says which of the
plugin's own buttons went down, and the plugin decides what that means. The host
never interprets a panel, and a plugin cannot press a button it did not draw.

A plugin's **settings** are declared as data — a schema of typed fields with labels
and descriptions — and the host renders them with the same controls it uses
everywhere else. The values are the plugin's, the plugin persists them itself, and
the product's configuration file has no knowledge that any of this exists.

## A plugin's own directory

The handshake tells the plugin a **data directory** that belongs to it alone. The
host creates it and never writes inside it. Anything a plugin wants to remember —
a tally, a reminder, a queue — goes there, and replacing the plugin's program
cannot replace what it remembered.

## Subscribing

A plugin declares which feeds it wants, and pays only for those:

| `Subscription`   | What it gets                                              |
| ---------------- | --------------------------------------------------------- |
| `Input`          | Key and mouse events, batched                             |
| `ModelReaction`  | Answers to its own motion and bubble requests             |
| `HostState`      | The host's facts, republished on every tick               |

Mouse *movement* is not in the input feed: it is merged into a latest value by the
runtime, so a plugin that wanted it would get a stale sample and pay for a batch per
mouse movement to receive it.

## The whole loop, with nothing published

A Development build reads its catalog from a directory instead of the network, and
an entry in that catalog may name an archive on the same machine. This repository's
`plugins/` directory *is* such a catalog, and `just plugin <id>` builds the archive
into it:

```sh
# 1. Write the plugin as a crate under plugins/, depending only on
#    bongocat-plugin-sdk, with a plugin.json beside its manifest.

# 2. Pack it. This is the only step that knows about the platform's executable
#    suffix, and it lives in crates/bongocat-packaging.
just plugin pomodoro
#    -> plugins/build/pomodoro.zip

# 3. Run the product, open Settings → Plugins, press Refresh, then Install.
#    The panel is on the model window.
```

No signature is checked for a local archive, and none is asked for: that is the
whole point of the loop. A catalog that came from the network is a different
document and cannot name a local path at all — see the validation in
`crates/bongocat-plugin-protocol/src/catalog.rs`.

A Development build reads a catalog with no file as an **empty list**, not an error,
so the plugin center is usable before anything is written.

## A card is drawn before the plugin has run

A card's copy comes from one of three documents, depending on the plugin's state:

| State | Where the copy comes from |
| -------------------- | ------------------------- |
| Not installed       | the catalog entry         |
| Installed, stopped  | the archive's `plugin.json` |
| Running             | the process's descriptor  |

The running copy wins, because a plugin may improve the sentence on its card in a
later version. That makes the other two the copy a user reads **before** they have
installed anything, so both carry the plugin's own translations and its icon — a
catalog that could only say its name in English would show an English name on a
non-English page for as long as the plugin stayed uninstalled.

The three are written separately and are checked against each other rather than
trusted: `just plugin <id>` refuses to pack an archive whose `name`, `description`
or `icon` disagrees with the catalog entry that points at it. That check exists
because the drift is visible — a card's sentence changing when a plugin starts is a
sentence describing a lifecycle rather than a plugin. It also means **changing a
plugin's card copy is a change in two files**, `copy.rs` and `plugin.json` plus the
catalog entry, and the packaging test is what says so if one is missed.

A plugin that ships no translations is not an error: a language the plugin has no
copy for shows its `default`.

## The panel

The panel is a tree. A `stack` is a column by default and a row with
`"axis": "horizontal"`; a `stack` is the only container.

| `type`          | What it draws                        |
| --------------- | ------------------------------------ |
| `stack`         | A column or a row of children        |
| `text`          | A string                             |
| `image`         | An image the plugin ships            |
| `spacer`        | Empty space, fixed or growing        |
| `divider`       | A hairline                           |
| `progress_bar`  | A horizontal fill                    |
| `progress_ring` | A circular fill                      |
| `button`        | A pressable, identified by `id`      |

Placement is in fractions of the model window, and a panel is **not** mirrored with
the model: it is interface chrome sitting beside the cat, so a mirrored model does
not flip the panel over it. `anchor` is one of the nine corners and edges, `margin`
is a gap in fractions of the window, and `width_fraction` is the panel's share of
the window's width. `size` is logical pixels — the same relative size at every
display scale.

The window's corner radius is deliberately not applied to a panel. A panel is not
the window, and a panel that inherited the window's rounding would show a second
rounded edge inside the first.

## What a plugin cannot do

- **Touch the product's memory.** There is no shared library, no ABI, and no
  `unsafe` boundary to cross — the plugins workspace forbids `unsafe` outright.
- **Add a window.** A plugin is an extension of the model window, and that is the
  design.
- **Ask the host for a fact the host does not publish.** The host's facts are
  model name, whether the window is visible, the language, the version, and the
  keyboard input source. A plugin that wants something else is a request for a new
  fact, which is a change to the product's surface rather than to a plugin.
- **Read input without subscribing to it**, or be told about presses it did not
  declare.

What a plugin *can* do is everything a normal program can: read and write files in
its own directory, run other programs, open sockets, make network requests. That is
not an oversight. A plugin that fetches the weather needs no permission from the
product, because the product is not the thing standing between the user and the
weather.

## Publishing

A published catalog entry names an HTTPS URL on GitHub, with the archive's SHA-256,
its size, and a Minisign signature over it made with the release key that
`bongocat-update` already carries. The same proxy prefixes the updater uses, in the
same order, are tried for the catalog and the archive. Nothing about the trust model
is plugin-specific: it is the one the updater already gets right.

## The plugins in this repository

Three, each one an issue turned into a program. They are deliberately small, and
none of them depends on another.

| `id`             | What it does                                              |
| --------------- | --------------------------------------------------------- |
| `pomodoro`      | A focus timer with a break                                 |
| `typing-sound`  | A model's own motions and sounds, per keystroke            |
| `keyboard-display` | The keys you are pressing, on the model window          |