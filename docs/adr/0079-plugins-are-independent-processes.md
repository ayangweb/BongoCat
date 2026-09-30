# 0079 — A plugin is a process, and the host is a sandbox it does not know about

**Status:** accepted

**Supersedes:** the "a plugin is data" decision in ADR-0078. The catalog, the
store, the trust model, the layer channel and the plugin center survive. The
behavior engine, the binding vocabulary and the declarative panel do not.

## Context

ADR-0078 chose data over code, and it was right about the properties it was
buying: no `unsafe`, no ABI, a plugin cannot crash the app, and every bound is
checkable before load. What it also did was decide that a plugin's logic is not
plugin's business — that the interesting part of a feature belongs to the host
and the plugin is the part that draws it.

That is backwards for this product, and the issues asking for it are what made it
visible. `#996` wants a pomodoro timer, `#905` wants daily keypress and mouse
distance statistics, `#849` wants the current input method on the cat's paw,
`#90` wants a sound on every keystroke, `#74` wants a key display in the corner,
`#927` and `#945` want to watch a coding agent over a socket and react to it.
Every one of those is state plus arithmetic plus a reaction. None of them is a
countdown, a stopwatch, a clock or a counter, so a plugin vocabulary of four
behaviors expresses none of them, and each one would have to be added to the
host as a fifth behavior — which is exactly the coupling the request was about.

The properties ADR-0078 bought are worth keeping. They are properties of *how*
a plugin is isolated, not of whether a plugin has code. So the question is which
isolation mechanism buys the same properties while letting the plugin hold its
own logic.

### A. A native dynamic library, loaded in-process

Rejected, for the reasons ADR-0078 gave and which still hold. Calling foreign
code with an ABI the compiler cannot check, from a thread the runtime owns, in a
process with live device handles, needs `unsafe` in the host — which
`AGENTS.md` §6 forbids in a business crate — and a plugin fault takes the
renderer with it. There is no way to bound that into safety.

### B. A WebAssembly component

Also rejected, and the cost is worse than ADR-0078 recorded, because the request
adds a constraint it did not have: *adding a plugin must not grow the
application*. A Wasm tier means Wasmtime and roughly 30 MB in every user's
installer, forever, to run a timer. That is the opposite of the requirement.

It remains the right answer for a plugin that must ship a large dependency
without a subprocess, and nothing here forecloses it: the descriptor is the
contract, and a future `api_version` can add an `engine` field naming how a
plugin is executed. Today's answer is process.

### C. A plugin is a process — chosen

A plugin is **its own executable**, in its own directory, with its own `Cargo.toml`,
its own dependencies and its own data directory. The host starts it, speaks a
versioned line-delimited JSON protocol over its stdin and stdout, and never loads
its code into its own address space.

Everything ADR-0078 wanted follows, and none of its costs are paid:

- **No `unsafe` and no ABI.** The host uses `std::process` and `std::io`. A
  plugin is another program with a pipe, not a function pointer with a calling
  convention.
- **A plugin cannot crash the app.** It is a separate address space with its own
  exit code. The worst outcomes are "the process exited" and "the panel stopped
  updating", both ordinary reportable states, and the host restarts a plugin that
  dies without being asked to.
- **The host owns the panel's appearance.** A plugin sends a *scene* — the same
  node vocabulary `bongocat-plugin-render` already lays out and rasterizes — not
  pixels and not a widget tree. The plugin computes; the host draws. One font,
  one theme, one opacity model, consistent with the product by construction.
- **Every bound is still checkable before anything is drawn**, because a scene
  arrives as a document and is validated against the same constants as before.
- **The application does not grow.** A plugin is not linked into
  `bongocat-app`; it is a separate artifact the user installs from the catalog.
  Adding a plugin adds nothing to the installer.
- **A plugin may use the whole machine**, which is what `#927` needs: it opens a
  loopback socket, it reads files, it spawns nothing the user did not ask for.
  There is no permission system because a separate process is already the
  boundary — and, stated honestly, a plugin is *not* untrusted code. It is code
  the user installed, running with the user's privileges. The trust model is the
  same one an editor extension has.

## The shape of the boundary

The host provides infrastructure and nothing else. Five things, all generic:

1. **Lifecycle.** Spawn, handshake, restart on exit, stop in the product's own
   shutdown order between the frame source stopping and the renderer releasing.
2. **A panel channel.** A plugin sends a scene; the host validates it, rasterizes
   it, and draws it as one positioned quad on the existing latest-wins layer
   channel. Presses come back as the button id the scene named.
3. **A clock and host facts.** A monotonic tick and a small read-only set of
   facts about the product — the active model's name, whether the window is
   visible, the current locale. The wall clock is read on the main thread and
   handed over, for the reason ADR-0078 already gave.
4. **An input feed.** A plugin may subscribe to key edges, mouse buttons and
   mouse movement. The feed is a bounded, counted, latest-value channel of
   already-validated `InputEvent`s; the host invents nothing and the runtime stays
   the single owner of pressed state.
5. **A model reaction channel.** A plugin may ask for a motion or an expression
   *by name*. The host maps a name onto the active model's own ids and issues the
   runtime command it already issues for a shortcut. A plugin cannot reach a
   parameter, a texture or a device handle, because there is nothing in the
   protocol that names one.

Everything else is the plugin's: its state, its arithmetic, its persistence, its
copy, its dependencies, its lifecycle beyond start and stop.

## Configuration is the plugin's, and the panel is the host's

A plugin declares a **configuration schema** — a closed set of typed fields:
toggle, integer, decimal, text, choice — each with a default, a range where the
kind has one, and a label. The host renders that schema with the same
`gpui-kit` controls the settings window already uses, and relays a typed
`ConfigValue` back. The host never interprets a value: it checks that the value
fits the field the plugin declared, and that is all.

The plugin owns the file. It reads it at start, writes it atomically through
`bongocat-storage`, and decides what a value means, what range is sensible, and
what to do with a value the user typed. So `config.json` gains no plugin section,
the schema never enters the product's own JSON Schema, and a plugin's settings
survive a plugin upgrade because the plugin wrote them.

This is the same reason the descriptor carries the plugin's *icon*: the host
draws it, and drawing is host work. What the icon *is* is plugin work.

## The catalog entry is metadata, and the running plugin is the truth

A published plugin is an archive holding an executable, a `plugin.json` and its
assets. The `plugin.json` exists so the center can list a plugin that is *not*
installed — a name, a description, an author, an icon and a version. When the
plugin runs it sends its own descriptor over the protocol, and the host checks
that the two agree on id and version before showing it. A descriptor that
disagrees is refused rather than displayed, because a card that says one version
and runs another is worse than a plugin that will not start.

## What this costs, stated plainly

- A plugin is a program, so shipping one means shipping a binary per platform,
  and the trust model is "the user installed it", not "it is a validated
  document". A signed catalog still authenticates the archive; it no longer
  makes the contents inert.
- Crossing a process boundary costs something a library call does not: a tick is
  a JSON line, and a panel update is a scene document. At a panel's cadence —
  once a second, not sixty times — that is not measurable, and the host coalesces
  updates per evaluation rather than forwarding every one.
- Development is a build step instead of a file edit. `just plugin <id>` builds
  the plugin and packs the archive the development catalog names.

## Consequences

- `bongocat-plugin-protocol` loses `behavior`, `bindings` and `SceneValue`, and
  gains the descriptor, the configuration schema, the input feed, the model
  reaction request and the message vocabulary.
- `bongocat-plugin-sdk` is new and is the only crate a plugin depends on. It
  depends on `serde` and the protocol and nothing else — no GPUI, no platform
  code, no renderer — so a plugin author needs the product's internals to know
  nothing about.
- `bongocat-plugin` gains a session per plugin: a child process, a reader thread,
  a writer, and a bounded queue in each direction. It loses the engine.
- `bongocat-plugin-render` keeps its canvas, font book, layout and raster; it
  loses the binding table, because a scene now carries concrete values.
- The plugin center becomes a card grid with an icon per card, and gains a
  configuration dialog driven entirely by the schema a plugin sent.
- `plugins/` becomes its own Cargo workspace. A plugin is built and versioned on
  its own, and nothing in the application's lockfile changes when one is added.
