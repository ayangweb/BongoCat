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
The per-action placeholder the key-sounds plugin published ("Playing /
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

Plugin copy belongs to the plugin. `plugin.json` keeps its mandatory English
`name` and `description` as the fallback and gains an optional `localizations`
map. The SDK resolves only `zh-CN` (case-insensitive, accepting `_` separators)
and English; every other resolved application language uses English, even if a
manifest declares another translation. Missing, blank or incomplete Chinese
metadata falls back as a whole pair, not field by field. The same metadata map
declares the plugin's available translated catalogs, so its published views
receive English when the Chinese entry is incomplete too. Translated catalogs
must be complete; the shipped plugins use required copy-struct fields rather
than optional per-key lookups, with no independent language setting or persisted
locale. Further languages are deferred until needed; the application's own
language catalogs remain unchanged.

A plugin declares `capability::LocalizedViewsV1` to localize its published views.
The application configuration owner initializes the service with its resolved
language before the overlay or settings window opens, and forwards subsequent
language changes only after configuration commits succeed. Opening, rendering,
closing or replacing the settings window is not a language writer. The registry
normalizes and remembers the tag, deduplicates unchanged effective tags, and
sends each plugin's supported catalog through `HostMessage::Locale`; a plugin
started later (enable, upgrade or startup resume) receives the current tag too.
The capability gates delivery so an older plugin is never sent a frame it cannot
parse; one that advertises the capability but refuses the exchange fails like
any other faulty tick. Framework copy (page title, card chrome and settings panel
chrome) stays in `bongocat-i18n` and follows the application's language.

Both plugins declare a glyph for every button they publish. The key-sounds
preview button reports `loading` from its own playback state instead of
publishing a text node. Runtime failures use the optional typed `errors` field
on `PluginOutput`; the host adds the plugin and operation context, then sends
one global error notification instead of rendering an error text node in the
settings panel.

Installed code, not catalog metadata, publishes action labels and icons. An old
installed package without these declarations therefore keeps its English labels
and neutral glyph even if a rebuilt package is available. `just dev-plugins`
only rebuilds the catalog ZIPs; it does not replace installed code. During this
unreleased PR's fixed 1.0.0 baseline (ADR-0091), testing a rebuilt plugin requires
uninstalling and installing it through the center, which retains its data. The
host must not guess icons or translations from legacy action IDs to hide stale
code, or silently overwrite immutable installed versions.

Each window serializes service requests through publication of their replies,
so a delayed poll cannot overwrite a newer configuration/action snapshot. Failure
publication changes only failure/loading state under the cache lock and never
replaces newer cards or in-flight bookkeeping. Plugin Reset preserves the current
session language rather than treating it as a plugin-owned preference.

## Compatibility

Every protocol addition is optional and defaults to the previous behavior: a
button with no `icon` and no `loading`, a manifest with no `localizations`, a
plugin without `localized_views_v1`. No config, state, session or catalog format
changes, `schema_version` stays 1, and ADR-0091's lifecycle, storage and catalog
decisions are unchanged. Widgets already pinned to `gpui-kit` are reused; no new
dependency is introduced.

## Validation

Unit tests cover supported-tag normalization, unsupported-language fallback,
missing and incomplete translations, optional-field decoding of legacy frames,
the shipped plugins' English fallback for unsupported host languages,
per-control in-flight bookkeeping, grid column selection, the settings entry
opening and closing one plugin's panel, action glyph mapping (including the
neutral fallback), typed plugin error validation, global notification rendering, and that a plugin-reported loading state draws on its own
button. Real-pipe tests check Pomodoro's localized action labels and play, pause
and restart declarations together across timer transitions. Real-process service
tests cover whole-plugin English fallback for incomplete metadata. Application
settings tests verify initialization without windows, all application languages,
stale and failed commits, disable/enable and restart language propagation.
`just check` (fmt, clippy across targets/features, workspace tests, release check)
and `tools/validate-locales.py` pass.
macOS `just dev-smoke` also passes bounded startup, settings window destruction
and reopening with a fresh entity, runtime continuity and normal shutdown.

## Update affordances

The page header carries only the two catalog actions: a borderless text
`Refresh` control and a primary `Update all` control. The subtitle sentence and
its container are removed rather than kept beside the actions. An installed card
whose catalog offers a newer version carries a compact outlined `Tag` beside its
installed version; the update itself remains the existing refresh-icon button.

`Update all` reuses the same per-plugin `InstallAvailable` command, not a second
installation path. Updates run serially because the service owns one catalog
download job at a time. A failed plugin does not stop the remaining updates and
is reported by name in a notification, so the same control can retry the cards
that still offer an update. The batch is reserved on the GPUI thread, so a
second click cannot queue another batch, and all card controls stay disabled
until the last update finishes. `InstallAvailable` acknowledges job submission
rather than byte delivery, so the window waits for the service's catalog job to
finish before retiring the card's loading state or starting the next package.

The footer now wraps as one unit: the switch and the command group may move onto
separate lines, while the command group keeps whole icon buttons on each line
instead of clipping them at the card edge. Uninstall uses the component's danger
outline variant, which gives the trash glyph and border the theme's red semantic
color and the component's hover/active states without a solid red fill.

Plugin cards do not offer a separate reset-data action. Uninstall removes only
the executable package and retains the plugin's data, so a later reinstall
continues from the same configuration and state. The lower-level manager reset
operation remains internal and is not exposed through the Plugin Center.

## Remaining Work

Manual macOS visual acceptance for the grid, settings modal and localized
tooltips, Windows DPI/layout and real-audio checks remain separate platform
gates; the bounded product smoke does not replace them. No acceptance item above
is claimed as verified on a platform that has not run it.

The keyboard/mouse sound plugin's separate preview button is superseded by the
Select hover-preview contract in ADR-0093. Other plugin buttons continue to use
this ADR's loading-state contract.
