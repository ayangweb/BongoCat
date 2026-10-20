# ADR-0092: Plugin center card presentation, per-action icons and plugin-owned localization

Status: accepted; implemented, platform smoke remains open
Date: 2026-10-15
Amended: 2026-10-20

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

Plugin copy belongs to the plugin. `plugin.json` carries a `localizations` map
keyed by locale tag. The SDK resolves only `zh-CN` and English; every other
resolved application language uses English, even if a manifest declares another
translation. The map is the only copy: English is an entry in it like every other
language, and an entry missing a name or a description is an invalid manifest
rather than a pair the host completes. The same map declares the plugin's
available translated catalogs, so its published views receive English when no
Chinese entry exists. The shipped plugins use required copy-struct fields rather
than optional per-key lookups, with no independent language setting or persisted
locale. Further languages are deferred until needed; the application's own
language catalogs remain unchanged.

A `description` states the plugin's core function in one sentence and stops
there. It does not enumerate settings, list configuration options or explain how
to use a feature: the card's settings surface and the plugin's own published
controls already carry that, and a description that repeats them dates the
manifest every time a control changes. The name and the icon carry identity, so
the description only has to say what the plugin is for. The shipped copy later
gained one clause naming the value the user gets from the plugin; see the
amendment below.

A plugin declares `capability::LocalizedViewsV1` to localize its published views.
The application configuration owner initializes the service with its resolved
language before the overlay or settings window opens, and forwards subsequent
language changes only after configuration commits succeed. Opening, rendering,
closing or replacing the settings window is not a language writer. The registry
normalizes and remembers the tag, deduplicates unchanged effective tags, and
sends each plugin's supported catalog through `HostMessage::Locale`; a plugin
started later (enable, upgrade or startup resume) receives the current tag as part
of that start, so there is exactly one delivery path. The capability gates
delivery so a plugin that does not localize its views is never sent a frame it
cannot parse; one that advertises the capability but refuses the exchange fails
like any other faulty tick. Framework copy (page title, card chrome and settings panel
chrome) stays in `bongocat-i18n` and follows the application's language.

Every button declares a glyph, and every action the host draws maps to artwork
the bundled `gpui-kit` assets ship, so no button depends on a font glyph for its
meaning and there is no neutral placeholder. The key-sounds preview button
reports `loading` from its own playback state instead of publishing a text node.
Runtime failures use the typed `errors` field on `PluginOutput`; the host adds
the plugin and operation context, then sends one global error notification
instead of rendering an error text node in the settings panel.

Installed code, not catalog metadata, publishes action labels and icons, so an
installed package always describes its own controls. `just dev-plugins` only
rebuilds the catalog ZIPs; it does not replace installed code. During this
unreleased PR's fixed 1.0.0 baseline (ADR-0091), testing a rebuilt plugin requires
uninstalling and installing it through the center, which retains its data. The
host must not guess icons or translations from action IDs, or silently overwrite
immutable installed versions.

Each window serializes service requests through publication of their replies,
so a delayed poll cannot overwrite a newer configuration/action snapshot. Failure
publication changes only failure/loading state under the cache lock and never
replaces newer cards or in-flight bookkeeping. Plugin Reset preserves the current
session language rather than treating it as a plugin-owned preference.

## Plugin identity artwork

Every bundled plugin uses one artwork entry: its manifest names
`resources/icon.png` as a safe package-relative `local_image`. The image is a
square RGBA PNG exported at its native source resolution. Its rounded silhouette
reaches every edge of the canvas, so no transparent padding is baked around the
artwork. No second icon directory or plugin-specific asset path is supported.
The manager resolves that path against the selected installed version and
requires the file to exist during install.

Catalog discovery projects a separate `icon_url` for the card. In Development,
`just dev-plugins` copies each package's artwork to
`target/plugins/icons/<plugin-id>/icon.png` and writes that absolute path into
the generated catalog. Release packaging extracts the same PNG from each plugin
ZIP, publishes it as the immutable `<plugin-id>-<version>.png` asset, and writes
that asset name into the signed catalog. The runtime resolves it to the catalog
URL or the configured GitHub proxy, exactly as it resolves package downloads.
An installed card continues to use the catalog URL, so installing, updating or
uninstalling a package never changes which artwork the card reads.

There is no font or Emoji fallback. Every bundled plugin must declare a local
PNG, and every catalog offer must resolve to one. A URL that cannot be loaded
leaves the fixed square empty; it is not replaced by a glyph, a generic icon or
the installed package's private file. This keeps Development and Production on
the same presentation path instead of making Development depend on an installed
tree.

A card displays a local image in a fixed 48px square frame with
`ObjectFit::Contain`, which preserves aspect ratio without stretching or
cropping. The image supplies its own rounded silhouette; the frame adds no
surface, padding, clipping or second radius around it. Packaging includes
exactly the declared icon alongside the manifest and executable, and a
repository contract test requires the same file name, square canvas, RGBA mode,
antialiased corners and opaque edge midpoints for every bundled plugin.

## Format

The plugin system is unreleased, so this ADR defines one current shape instead of
tolerating earlier ones. A button always carries `icon` and `loading`, a select
always carries `preview` and a group on every option, a manifest always carries
`localizations`, and `PluginOutput::errors` is always present; unknown fields
remain rejected. The manifest has no icon fallback and no digest field — a
package's identity is the digest of the signed catalog entry that offers it.
Every release and Development catalog writes `icon_url`, and every bundled
manifest names the same `resources/icon.png`. There is no format path for a
manifest or catalog that omits card artwork; both are build inputs and must be
corrected at the source. No config, state or session format changes,
`schema_version` stays 1, and ADR-0091's lifecycle, storage and catalog decisions
are unchanged. Widgets already pinned to `gpui-kit` are reused. The desktop
application adds the matching `gpui-pre-reqwest-client` transport so GPUI's image
element can load release PNGs; it is not a second UI dependency.

## Validation

Unit tests cover supported-tag normalization, unsupported-language fallback,
incomplete translations, rejection of a frame or manifest that omits a required
field, the shipped plugins' English fallback for unsupported host languages,
per-control in-flight bookkeeping, grid column selection, the settings entry
opening and closing one plugin's panel, action glyph mapping to bundled artwork,
typed plugin error validation, global notification rendering, and that a
plugin-reported loading state draws on its own button. Real-pipe tests check Pomodoro's localized action labels and play, pause
and restart declarations together across timer transitions. Real-process service
tests cover whole-plugin English fallback for incomplete metadata. Application
settings tests verify initialization without windows, all application languages,
stale and failed commits, disable/enable and restart language propagation.
Repository tests also pin the single artwork entry and verify every shipped icon
is a square RGBA PNG with partial-alpha rounded edges.
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
`Update all` is hidden, not disabled, while no installed plugin is behind the
catalog; loading, refreshing and a running package operation keep the disabled
control on screen because those reasons are temporary and readable.

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

## Amendment: a description names the value the plugin gives

The original rule stopped a description at the plugin's core function. The
shipped copy now also names what the user gets, because a card is read by people
deciding whether to install the plugin, and a bare function phrase repeated the
name back at them:

- Input Source: "Shows the current input source in real time, so you always know
  the input state." / "实时显示当前使用的输入法，方便随时了解输入状态。"
- Key Sounds: "Adds lively sound feedback to keyboard and mouse actions, making
  every interaction more fun." / "为键盘和鼠标操作添加生动的声音反馈，让每次交互更有趣。"
- Pomodoro: "A focus timer that helps you stay focused and plan your time." /
  "通过专注计时帮助你保持专注、合理安排时间。"

The rule keeps its other halves: one sentence, no settings enumeration, no usage
instructions, and no feature the plugin does not have. Pomodoro still does not
announce a work/break cycle (ADR-0090 records it as a focus timer only), so its
description names focus and time planning without claiming rest intervals.
