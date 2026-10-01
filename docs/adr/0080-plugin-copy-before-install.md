# The plugin center reads a plugin's copy before the plugin has run

Supersedes nothing. Refines ADR-0079's "the catalog entry is metadata, and the
running plugin is the truth" for the case where the running plugin does not exist
yet — which is the case a user spends most of their time in.

## The problem

A card in the plugin center is drawn from three documents, and which one answers
depends on the plugin's state:

| State | Where the card's copy comes from |
| --- | --- |
| Not installed | the catalog entry, projected from the plugin's own `plugin.json` |
| Installed, not running | the archive's `plugin.json` |
| Running | the process's own descriptor |

**Amended by ADR-0082.** The first and second rows are now one document: a
development catalog is *derived* from the plugin directories, projecting each
plugin's own manifest, so a plugin's name and sentence are written once. The
precedence rule below is unchanged.

The precedence was right — a running plugin may improve the sentence on its card in
a later version, so the running copy wins. The problem was that **only the third
document could say anything in more than one language**, and it is the one that does
not exist until the user has installed the thing.

`PluginCatalogEntry.name` and `.description` were plain `String`s, and so was
`PluginManifest.name`. So every uninstalled card on a Chinese page said its name and
its sentence in English, and stayed English until the plugin was installed and had
answered a handshake. Two further facts made the same page wrong in ways that had
nothing to do with language:

- `PluginCatalogEntry` carried an `icon_url` that **nothing ever read**, while
  `manifest_from_catalog` wrote `PluginIcon::default()`. A plugin that ships an icon
  therefore showed a letter — "A AI Watch", "C Cat Skills" — for exactly as long as
  it stayed uninstalled.
- The card's state badge was rendered from `entry.running` alone. A plugin that was
  not installed is not a plugin that has stopped, and a list of plugins the user had
  not asked for showed every one of them reading "Stopped".

## The decision

1. **A catalog entry's copy is localized, and an archive's is too.** Both
   `PluginCatalogEntry` and `PluginManifest` now carry `LocalizedText` for `name`
   and `description` — the type the descriptor already used, with the same rule that
   the *longest* of every language is what the bound is checked against. A catalog
   only has to be right for the state it is drawn in, and an uninstalled card is
   drawn from it.
2. **A catalog entry carries the card's icon.** A new `icon` field, same
   `PluginIcon` the archive uses. `icon_url` stays, validated but unread: a card
   draws an emoji today and a URL is how a picture arrives when an emoji is not
   enough.
3. **A badge describes a process, so only an installed plugin has one.** The state
   badge takes the entry rather than a bool and renders nothing when the plugin is
   not installed.

## Why the copy does not move into the application

The obvious alternative — put plugin names and descriptions in
`crates/bongocat-i18n/locales/` — was considered and rejected, and not on size.

The size argument is weak: seven plugins × two fields × six languages is roughly
6 KB, a few kilobytes compressed, against an application that already embeds 136 KB
of catalogs. Putting them there would not have been a problem *for size*.

It fails on ownership instead. A plugin is a separate artifact and the catalog is a
network document, so **a third-party plugin's name cannot live in this application's
source** — publishing a plugin would require an application release, and a plugin
this repository did not ship could never be named in any language. ADR-0079 already
decided that a plugin's copy is plugin work; the catalog entry is that plugin's copy,
published by whoever publishes the catalog, and it travels in the document the
product already downloads. Nothing about it is bundled: a Production build fetches
the catalog over the network and a Development build reads a directory.

## What this costs, stated plainly

- One fact is now written in three places — `copy.rs`, `plugin.json` and the catalog
  entry — and it **had already drifted**: one plugin carried three different English
  sentences for one description before this change, and three more carried two each.
  Because the running copy wins, that drift was not cosmetic; the card's text changed
  when a plugin started.
- So the three are now checked against each other rather than trusted:
  `a_catalog_entry_and_the_archive_it_points_at_say_the_same_thing` compares the
  archive's copy with the catalog's, and
  `every_catalog_entry_speaks_the_languages_the_product_ships` refuses an entry whose
  Chinese is empty or is the English again. Adding a language to the product does not
  add a plugin translation automatically, and that check is what says so.
- The manifest's `name` was documented as a plain string because "the store compares
  it". The store compares id and version, never the name
  (`PluginDescriptor::agrees_with`), so that reason did not hold and the field is
  localized like the descriptor's.

## Compatibility

Every document already written still reads. `LocalizedText` accepts a bare string on
the wire as well as a table, so an existing catalog and an existing `plugin.json`
parse exactly as they did, and a plugin that ships no translations shows its default
rather than a blank. `icon` and `by_locale` are additive; a catalog entry with
neither is an entry with no icon and no translations, which is the state every
existing one was already in.
