# ADR-0092: Plugin center card presentation, per-action icons and plugin-owned localization

Status: accepted; implemented, platform smoke remains open
Date: 2026-10-15

## Decision

The Plugin Center reuses the model library's grid rather than growing a second
catalog layout. `window::models` owns the rule once — usable width divided by a
cell floor, clamped to a two-column minimum and the shared five-column cap — and
`grid_with_id` owns the scroll container, row template and row splitting. The
plugin page passes its own scroll id (`plugin-catalog`) and its own cell floor
(300px, higher than the model grid's 240px because a plugin card carries a title,
a version line, a description and a footer of controls rather than a cover).
Two catalogs in one window therefore never disagree about how many cells a row
holds, and neither page keeps a private copy of the arithmetic.

Action buttons are identified by meaning, never by string. Package lifecycle
operations are host-owned and map to fixed host glyphs (install → download,
update → refresh, reset → eraser, uninstall → trash, enable/disable remains a
`Switch`, which cannot spin). Icons a plugin asks for travel in the protocol as
the closed `bongocat-plugin-sdk::ActionIcon` enum (`play`, `pause`, `restart`,
`volume`), added to `UiNode::Button` as `Option<ActionIcon>` with
`#[serde(default)]`. The host keeps one translation table from that enum to
`IconName`; a button that declares no icon keeps the neutral action glyph. The
host still never derives an icon or tooltip from a plugin id, an action id or a
translated label.

In-flight work is drawn on the control that started it. Two sources cover the
whole life of a command: the window marks a command before it hands it to the
background executor and retires the mark when the reply lands (typed
`PluginInFlight` keys, counted so two overlapping sends stay apart), and a plugin
reports its own long-running state through `UiNode::Button::loading` (a preview
that is still audible). A busy control renders `Button::loading` and is disabled.
The per-action placeholder the typing-sound plugin published ("Playing /
正在播放", a second text node beside the preview control) is removed rather than
kept next to the button: the button the user pressed is the thing that shows the
action is still running. The page's own first-load state stays a page state — the
refresh control spins and the empty catalog says it is discovering — because that
is what ADR-0054's real loading state means, not an action in flight. The default
hub state is now `loading()`, so a window that has read nothing yet draws as
discovering instead of as an empty catalog.

Every installed, enabled plugin has the same settings entry: a gear in the card
header opening a modal (`plugin_settings`) whose content is exactly the plugin's
published settings tree, in the plugin's order, with typed writes back through
the same command path the card uses. A modal rather than a drawer because a
plugin declares a handful of fields — there is nothing to scroll and nothing to
switch between — and because the panel is about one plugin, so it must not become
a second column that has to say whose settings it shows. A plugin with no
settings says so. The panel closes when the plugin is uninstalled, disabled or
fails, because controls nothing would answer are not a configuration surface. A
dialog dismissed by the overlay or by Escape never reaches a callback, so the
draft is reconciled on the next frame instead of leaking an open panel.

Plugin copy belongs to the plugin. `plugin.json` keeps its mandatory `name` and
`description` as the fallback and gains an optional `localizations` map keyed by
catalog tag; the SDK (not each plugin) owns tag resolution, matching the whole
tag first and then the primary subtag, with Chinese resolved by script subtag
because Simplified and Traditional are different written forms rather than
regional variants (ADR-0012). A plugin that declares only one Chinese script does
not serve the other. The plugin declares
`capability::LocalizedViewsV1` to say it can localize the views it publishes, and
the host sends its resolved tag once per change through `HostMessage::Locale`.
The capability is the gate rather than the protocol version, so an installed
plugin built before the message existed is never sent a frame it cannot parse and
keeps publishing its fallback copy; a plugin that declared the capability and
then refuses the exchange fails like any other faulty tick. The registry owns the
current tag so a plugin started later — enable, upgrade, resume at startup —
receives it too. Framework copy (page title, card chrome, tooltips, the panel
title and its empty state) stays in `bongocat-i18n`, which now carries
`plugins.settings.{open,title,empty}` in all eight catalogs.

Both shipped plugins (pomodoro, typing-sound) carry English, Simplified and
Traditional copy and declare a glyph for every button they publish. The typing
sound preview button reports `loading` from its own playback state instead of
publishing a text node, and the failure it can report is an error code the plugin
renders in the language on screen rather than a sentence frozen in English.

## Compatibility

Every protocol addition is optional and defaults to the previous behavior: a
button with no `icon` and no `loading`, a manifest with no `localizations`, a
plugin without `localized_views_v1`. No config, state, session or catalog format
changes, `schema_version` stays 1, and ADR-0091's lifecycle, storage and catalog
decisions are unchanged. Widgets already pinned to `gpui-kit` are reused; no new
dependency is introduced.

## Validation

Unit tests cover tag resolution (whole tag, primary subtag, both Chinese scripts,
single-script plugins, unknown tags), optional-field decoding of legacy frames,
per-control in-flight bookkeeping, grid column selection, the settings entry
opening and closing one plugin's panel, action glyph mapping (including the
neutral fallback), and that a plugin-reported loading state draws on its own
button. `just check` (fmt, clippy across targets/features, workspace tests,
release check) and `tools/validate-locales.py` pass.

## Remaining Work

macOS product smoke for the new grid, settings modal and localized copy is not
yet recorded with evidence; Windows DPI/layout and real-audio checks remain a
separate platform gate. No acceptance item above is claimed as verified on a
platform that has not run it.
