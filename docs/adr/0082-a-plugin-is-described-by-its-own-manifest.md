# 0082 — A plugin is described by its own manifest, and there is no plugin list

**Status:** accepted, and it amends ADR-0079's development loop and its statement that a
catalog entry is metadata the running plugin then contradicts

A plugin's identity and its words were written down twice: once in a Rust module that
spelled out every translation, and once in `plugin.json` for the card the plugin center
draws. There was also a third document, `plugins/plugins.json`, listing the plugins the
repository ships with their names, descriptions and icons — so adding a plugin meant
writing the same sentence three times.

All three are gone. One `plugin.json` per plugin is the whole of what a plugin says about
itself, and the list of plugins is *derived* from the directories beside it rather than
kept anywhere.

## Context

The duplication was not a style problem; it had already produced a bug. The card's text
comes from three places that are written at different times: the catalog entry (plugin not
installed), the manifest inside the archive (installed, not running) and the running
process's own descriptor (running). The last one wins, so when they disagreed the card's
own sentence *changed* when a plugin was started — a sentence that appeared and
disappeared with a lifecycle rather than describing the plugin. Three plugins in this
repository had already drifted, one carrying three different English sentences for one
description.

The same shape appeared in the copy. Each plugin had a `copy.rs` whose every function
returned a `LocalizedText` with its own `.with_locale` chain, and the plugin's *name* and
*description* — the two strings a card shows — lived in `plugin.json` instead. A
translator who wanted to read a plugin's words had to read Rust to find them.

## The decision

### One document, embedded at compile time

`plugin.json` carries the identity (id, version, name, description, author, icon) and a
`copy` table of every other string the plugin uses: its panel words, its settings labels,
its choice options, its error sentences. A plugin reads it through
`bongocat_plugin_sdk::SelfDescription`, which is the only way it learns what it is called.

The manifest is **embedded** at compile time rather than read at runtime:

```rust
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("its own manifest")
});
```

A plugin is a separate process whose working directory the host chooses, and the manifest
is a file inside the archive beside the binary. Reading it at runtime would mean the plugin
guessing where the host unpacked it — exactly the coupling ADR-0079 removed to make a
plugin independent. Embedding it makes the copy part of the binary: it cannot go missing,
it cannot be edited under a running process, and a plugin whose binary and manifest
disagree is a plugin that does not build.

The host still holds the words, and holds none of the translations. A `LocalizedText`
arrives from the plugin carrying every language the plugin has copy for, and the host
picks the one the user reads — because only the host knows the user's language. Drawing is
host work; deciding what a plugin is called is not.

### The list is derived, and a plugin is a directory

A development catalog is no longer a document. It is a directory of plugin sources, and
the host builds the list by looking for directories that hold a `plugin.json`. Adding,
renaming or removing a plugin is a change to one directory, and there is nowhere to forget
a line.

An entry is offered only when the packed archive is beside it, so a card never says
"Install" for something that cannot be installed. A directory that is not a plugin — a
scratch folder, Cargo's own output — is skipped rather than refusing the list, because a
development catalog is the author's own tree and the failure should be about a plugin
rather than about housekeeping. A manifest that will *not parse* is refused with the file
named, because that is a plugin whose author needs to hear about it.

The derived list is put through the same validation a published catalog gets, so building
one in memory cannot ship a document a fetched one would be refused for.

### A plugin speaks every language the product ships

Every plugin carries all seven languages, and a repository-wide test in the packaging
crate reads the locale catalogs **off disk** rather than a list written in the test. A
list in a test is a list that goes stale the day a language is added: the test would keep
passing on the languages it already knew about while the product grew a seventh and every
plugin silently stopped covering it.

The test checks two different things and neither substitutes for the other. An entry per
language is what stops a field from being silently skipped; and a table that is not a
verbatim copy of the default is what stops "translated" from meaning "the same English
string written seven times", which is what a mechanical pass over the keys produces.
Individual languages are allowed to equal the default, because plenty of words are the
same in two languages — a product's own name, and most of Simplified and Traditional
Chinese — and refusing that would make the check wrong often enough that it would be
turned off.

## What this costs

- A plugin's copy is compiled in, so changing a translation means rebuilding the plugin.
  For a plugin that has to be built to be installed anyway, that is no new step, and
  `just plugins` is now run by `just dev`.
- A plugin that shipped only some languages now fails a build rather than shipping a card
  that reads in English on a page the user set to Korean. That is the intended direction
  and it is a real cost for a plugin that is mid-translation.
- The development catalog cannot describe a plugin the author has not unpacked, so the
  loop is "add a directory, build, refresh" rather than "edit a list, refresh". The build
  is the step `just dev` already performs.

## Consequences

- `bongocat-plugin-sdk` gains `SelfDescription` and `Descriptor::at`, and the host
  connection announces the schema `Plugin::settings()` returned rather than a second,
  empty one.
- `bongocat-plugin-protocol` gains the `copy` table on the manifest and makes
  `PluginCatalog::validate` public, because a derived list is a catalog that did not arrive
  as bytes.
- `bongocat-plugin`'s local catalog is built from directories. The network catalog is
  unchanged: a published release asset is still how a *user's* machine learns what exists,
  and it is generated rather than maintained.
- `plugins/plugins.json` is deleted, and ADR-0079's note that development is "a build step
  instead of a file edit" now describes the whole loop rather than half of it.
