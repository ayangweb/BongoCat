# 0078 — A model-window plugin system, and why a plugin is data

**Status:** accepted

## Context

BongoCat's product surface is the model window. Everything a user sees without
opening settings is drawn there: the Live2D model, the pressed-key imagery, the
model's own background. The settings window is configuration, and the model
window is the product.

That makes "add a plugin system" an unusual request, because the feature it is
usually imagined as — an extension that adds its own window, its own process, its
own lifetime — is the wrong shape for this product. A separate window per plugin
would be:

- outside the product's design language, because it is not the model window;
- a new window to place, scale, hide and restore, doubling the placement state
  that `window-state.json` and the overlay session already own;
- a second thing to click through, to raise, and to leave on screen when the
  model window is hidden.

The features that motivate the request are not window-shaped at all. A pomodoro
timer wants a countdown and a progress bar *beside the cat*. A coding-agent
integration wants a status line *under the cat*. Both are information rendered
into the model window, and both want to be readable at a glance from across a
desk, not inside a panel the user has to open and keep open.

So the first question is not "how do plugins load" but "what is a plugin, given
that it must be part of the model window rather than beside it". The answer
determines everything downstream, so it is worth being explicit about the
options.

## Options considered

### A. Native dynamic libraries, host callbacks

The textbook plugin system: a plugin is a `cdylib` the host loads, and the host
exposes a C ABI the plugin calls to draw and to receive events.

This is what VS Code-adjacent systems and most game mod loaders do, and it is
powerful. It is also the wrong answer here, for four reasons that are not
close calls:

1. **It destroys the `unsafe` boundary.** `AGENTS.md` §4.4 requires business
   crates to be `#![forbid(unsafe_code)]` and confines `unsafe` to platform, GPU
   and Cubism edges. Loading a third-party `cdylib` means calling foreign code
   with an ABI the compiler cannot check, from a thread the runtime owns, in a
   process that has a renderer's device handles live. There is no way to make
   that safe; there is only a way to bound it, and every bound (no exceptions
   crossing the frame, no blocking, a watchdog thread) is a piece of machinery
   that exists only because the alternative was rejected.

2. **A crash in a plugin is a crash in the app.** The model window is a
   per-frame surface. A plugin that faults mid-draw takes the renderer with it,
   and there is no recovery short of killing the process — which for a desktop
   pet means the user loses their window position, their model list and their
   timer.

3. **It makes the panel's appearance the plugin author's problem.** With a
   native library, drawing text means choosing a font, laying out a string and
   handling a theme. Every plugin reimplements it, and every plugin's panel
   looks like a different application sitting inside the product. The model
   window is small; a panel that does not match its surroundings reads as a bug
   in the product rather than as a plugin.

4. **It forecloses the actually-requested integrations.** "Connect to Codex and
   Claude Code" means spawning processes, reading their state and reacting to
   events. Under a native ABI that is `CreateProcess` and file I/O handed to
   untrusted code — the largest possible surface, for the least certain gain,
   before any plugin is written.

### B. WebAssembly components (WASI preview 2)

The modern answer, and what Zed does. A plugin is a `.wasm` component compiled
against a WIT world; the host embeds Wasmtime and links a small set of imports.

This is genuinely stronger than native libraries on the safety and portability
axes, and it is the right answer *if* plugins need to compute. It has costs
that do not fit this product's shape:

- **Roughly 30 MB of added release binary and a Cranelift JIT dependency**, for
  plugins whose entire job is to display a countdown.
- **The host still has to render the panel.** A component can compute text but
  cannot lay out or draw it, so the host needs the full scene vocabulary anyway.
  Wasm solves the *computation* problem, and this feature's first generation has
  almost no computation in it — its logic is a timer and a label.
- **A WIT world and a host ABI to design, version and document** before the first
  plugin exists, with the compatibility cost that implies.

Wasm is not rejected. It is deferred to a later `api_version`, and the protocol
is shaped so that adding it does not disturb anything above. What the protocol
must *not* do is pretend a decision has been made about it.

### C. A declarative scene plus a closed set of host-run behaviors — chosen

A plugin is a **manifest and a scene**, both JSON. It declares:

- where its panel sits in the model window (one of nine anchors, a margin, a
  width as a fraction of the window);
- a fixed logical size;
- a tree of nodes — stack, text, spacer, divider, progress bar, progress ring,
  image, button;
- a set of **behaviors** the host runs: `countdown`, `stopwatch`, `local_time`,
  `counter`.

A scene node's text is either a literal or a **binding** — a path such as
`timer.remaining_text` that the host writes each evaluation. A button runs one
of six verbs (`start`, `pause`, `reset`, `toggle`, `increment`, `decrement`)
against a named behavior. That is the entire interface.

The host rasterizes the scene into an RGBA texture and draws it as one
positioned quad above the model and the key overlays, through a new
latest-wins channel that runs beside the frame channel rather than inside it.

## Decision

**A plugin is data, not code.** It declares a panel and the behaviors behind it;
the host owns rendering, layout, timing, input and every bound.

### The properties this buys

- **No `unsafe`, no ABI, no sandbox to get wrong.** There is no permission
  system because there is nothing to grant: a plugin cannot read a file, open a
  socket or start a process, because it has no way to express the intent. The
  threat model reduces to "a plugin is a small, validated, bounded document".
- **A plugin cannot crash the app.** Its worst outcome is a refused load or a
  panel showing a fallback, both of which are ordinary, reportable states.
- **The host owns the panel's appearance.** One font, one theme, one corner
  radius, one opacity model, consistent with the product by construction, and it
  follows the user's theme and scale settings without the plugin knowing they
  exist.
- **Every bound is checkable before load.** Node count, tree depth, panel size,
  behavior count, binding count, behavior ranges, asset paths, duplicate ids.
  All of it is a constant in `bongocat-plugin-protocol` and a check in
  `PluginManifest::validate`, so an over-large or over-deep panel is refused
  rather than half-drawn.
- **A plugin is portable.** One JSON file per platform-agnostic behavior, plus
  PNGs. The same plugin works on both platforms and every OS version BongoCat
  supports, because it uses no platform capability.

### What a plugin cannot do, stated plainly

This is the real cost of the decision, and it belongs in the ADR rather than in a
code comment:

- It cannot run arbitrary logic. A plugin whose behavior is not one of the four
  kinds is not expressible, and adding one is a change to this design.
- It cannot fetch anything. A plugin cannot show a live agent status by polling
  an API.
- It cannot open files, spawn processes, or receive the raw input stream.

The agent-integration case is therefore **not served by this design**, and the
product must not claim it is. It is served by either:

1. a later `api_version` adding a Wasm tier (the deferred option B), which
   brings its own capabilities and its own dependency; or
2. the agent integration living in the host as a first-class feature, with a
   plugin surface that *displays* the state the host already has. The second is
   the better fit: BongoCat already knows about models, input and rendering, and
   an agent status is a display concern wearing a plugin's clothes.

Either way, the panel, the lifecycle, the center and the transport are reusable,
and neither choice has to be made now to build them.

### The trust model for distribution

A plugin archive is fetched from the same place the update manifest is fetched
from, through the same proxy prefixes, with the same fallback order, and
authenticated with the **same Minisign release key** compiled into
`bongocat-update`. A catalog entry additionally carries a SHA-256 and a size,
so a truncated or substituted archive is caught before it is unpacked. The
archive is unpacked through a path check that refuses `..`, absolute paths,
Windows drive letters, UNC prefixes and NUL bytes, with a bounded entry count,
a bounded total size and a per-file size cap.

Development builds read the catalog from a local directory instead of the
network, and an entry in such a catalog may name an **archive on the same
machine** by a path relative to the catalog's own directory. That is what makes
the whole loop — author, install, see it on the model window — work with no
publish step and no signature: the archive never goes near a network, so there
is nothing to sign.

A local entry is refused three ways, and all three matter:

- It must be **relative** and must not contain `..`. A development path is still
  a path, and a catalog that could name `../../..` would be a catalog that could
  read anywhere the user can.
- A network catalog **cannot carry one at all**. The parse-time validation is
  shared by both sources, and it is the *shape* of the entry that decides: an
  entry that names both a URL and a path is refused, because which one was read
  would otherwise depend on the build rather than on the document.
- A local entry's digest is **optional and its signature is absent**. An author
  iterating on a plugin has not built a release artifact yet, so requiring a
  fingerprint would mean the loop could not start. A URL entry — a fetch from
  the internet — still needs both, exactly as the updater does.

The archive a local entry names is read through the same size cap a network
download is held to, so a local path is not a way to make the worker allocate
without bound.

### Why the layer does not go through the frame channel

The frame channel is the real-time path, budgeted per tick by
`runtime_tick_work_budget`, and `AGENTS.md` §4.2 makes the runtime the single
owner of model state. A plugin panel does not belong there:

- It changes on its own cadence — once a second for a countdown, never for a
  counter — not at up to 240 Hz.
- Its cost is dominated by rasterization, which is milliseconds, not the
  sub-millisecond budget of a frame.
- It is produced by a worker thread that may be doing a download, and a frame
  channel that blocked would stall the frame.

So layers travel on their own `overlay_layer_channel`, latest-wins like the frame
channel, with the same reasoning: a layer that arrives late is a layer nobody
looks at. The raster carries a content hash, so a panel whose bound values did
not change is not re-uploaded — the difference between one upload a second and
sixty a second.

### Layer placement is in device space, not model space

A layer's rectangle is built directly in normalized device coordinates rather
than in the model's own space. A panel is interface chrome: a mirrored model
must not mirror the panel beside it, and a layer that scaled with the model's
units would drift as the user changed the model-window scale. Fractions of the
window box make one placement valid at every window size, so a resize needs no
re-derivation and the placement arithmetic exists once, testable, in
`bongocat-render`.

## Consequences

- `bongocat-render` gains a topmost layer vocabulary: an anchor, a placement in
  fractions, a raster, a quad in NDC and a pointer-to-layer mapping. It learns
  nothing about plugins — who produces a layer is decided above it.
- Both native backends gain one more draw loop after the key overlays, plus the
  ability to create and update a texture from memory rather than from a file.
- `bongocat-plugin` owns the catalog, the store, the engine and the lifecycle. It
  has a worker thread; it is joined in the existing shutdown order, between the
  frame source stopping and the renderer being released.
- `bongocat-plugin-render` owns rasterization, including the one piece of text
  rendering the product did not have: a font is chosen from a fixed per-platform
  candidate list, glyphs are rasterized once per unique string, and the result
  is cached by content.
- The plugin center is an ordinary settings page with its own command family,
  snapshot section and localized strings in all six catalogs.
- Agent and coding-tool integrations are not delivered by this ADR. If they are
  wanted, they are host features or a later `api_version`, and either way the
  panel surface is already there to display them.
