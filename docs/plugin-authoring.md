# Plugin authoring

A plugin is **data, not code**: a `plugin.json` manifest and nothing else. The host
runs the behaviors it declares, rasterizes the panel it describes, and answers the
presses its buttons name. There is no code to load, no ABI, and nothing a plugin can
do that the host has not already decided it may do.

`docs/adr/0078-model-window-plugin-system.md` is the decision this implements, and
it is the place to look for why a plugin is data and not a shared library.

## What a plugin is

A plugin is one file: `plugin.json`, read from the root of its archive. It carries
three things and nothing else:

- **Identity** — id, name, version, author, description.
- **Behaviors** — the state the host runs on the plugin's behalf: a countdown, a
  stopwatch, the local clock, a counter. A plugin cannot compute anything itself; it
  declares what it wants to know and the host publishes the values.
- **A scene** — the panel, as a tree of nodes with values bound to those behaviors.

The manifest of the reference plugin this repository ships is
[`plugins/pomodoro/plugin.json`](../plugins/pomodoro/plugin.json). It is the file
to copy: a focus timer with a countdown, a progress bar, a progress ring and two
buttons.

## The whole loop, with nothing published

A Development build reads its catalog from a directory instead of the network, and
an entry in that catalog may name an archive on the same machine. This repository's
top-level `plugins/` directory *is* such a catalog: a reference plugin and the
`plugins.json` that offers it. So the loop is:

```sh
# 1. Build the archive. A plugin is one file inside a zip.
cd plugins
zip build/pomodoro.zip pomodoro/plugin.json

# 2. Point the product's data root at this directory, or copy the two files into
#    the catalog directory it reads. `plugins.json` names the archive by a path
#    relative to itself, so a `build/` folder beside it is enough.
#    Development: <data dir>/com.ayangweb.bongo-cat/development/plugin-catalog/
#    Production:  <data dir>/com.ayangweb.bongo-cat/production/plugin-catalog/

# 3. Run the product, open Settings → Plugins, press Refresh, then Install.
#    The panel is on the model window.
```

No signature is checked for a local archive, and none is asked for: that is the
whole point of the loop. A catalog that came from the network is a different
document and cannot name a local path at all — see the validation in
`crates/bongocat-plugin-protocol/src/catalog.rs`.

A Development build reads a catalog with no file as an **empty list**, not an error,
so the plugin center is usable before anything is written.

## Behaviors

Four kinds, and a plugin declares which ones it wants:

| `kind`        | What it is                                                |
| ------------- | --------------------------------------------------------- |
| `countdown`   | Counts down from a duration. A focus timer.                |
| `stopwatch`   | Counts up, wrapping at a declared interval.                 |
| `local_time`  | The user's local wall clock, read on the main thread.      |
| `counter`     | A number the plugin's own buttons change.                   |

Each behavior publishes a fixed set of values, and a scene binds to them by path:

```
<behavior>.remaining_seconds   <behavior>.remaining_text
<behavior>.elapsed_seconds     <behavior>.elapsed_text
<behavior>.progress            <behavior>.running
```

`progress` is a fraction from 0 to 1, which is what a progress bar or ring wants
directly. `remaining_text` is already formatted as a clock face (`25:00`,
`1:05:03`), so a panel does not have to know how to format one.

There is also a `host.` prefix for facts the runtime publishes — the active model,
its display name, whether the model window is visible. The full list is
`HOST_BINDING_PATHS` in the protocol crate; a binding that is not on it is refused
at load time rather than rendering as an empty value.

A binding names what to show and what to show when the value is not available yet:

```json
{"binding": "timer.remaining_text", "fallback": "25:00"}
```

The fallback is what the panel shows before the first evaluation and whenever the
behavior cannot produce a value. It is not an error state, so it should look like
the panel, not like a message.

## Actions

A button runs exactly one of six actions against one behavior:

`toggle` · `start` · `pause` · `reset` · `increment` · `decrement`

The host decides what each one means, which is why a plugin cannot use a button to
do something the behaviors did not already declare. A button with no `target` draws
and does nothing, which is a legitimate thing to want.

## The panel

The scene is a tree. A `stack` is a column by default and a row with
`"axis": "horizontal"`; a `stack` is the only container.

| `type`          | What it draws                                       |
| --------------- | ---------------------------------------------------- |
| `stack`         | A column or a row of children                       |
| `text`          | A string, bound or literal                          |
| `spacer`        | Empty space that grows                              |
| `divider`       | A hairline                                          |
| `progress_bar`  | A horizontal fill                                   |
| `progress_ring` | A circular fill                                     |
| `image`         | A PNG beside the manifest                          |
| `button`        | A pressable that runs one action                    |

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

- **Run code.** There is no `unsafe` boundary to cross and nothing to compile.
- **Reach the filesystem** except through the image assets it ships.
- **Read input, the window state, or the configuration.** The `host.` bindings are
  the whole of what a panel can be told, and they are all read-only facts.
- **Add its own window.** A plugin is an extension of the model window, and that is
  the design.

## Publishing

A published catalog entry names an HTTPS URL on GitHub, with the archive's SHA-256,
its size, and a Minisign signature over it made with the release key that
`bongocat-update` already carries. The same proxy prefixes the updater uses, in the
same order, are tried for the catalog and the archive. Nothing about the trust model
is plugin-specific: it is the one the updater already gets right.
