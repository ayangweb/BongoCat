//! The plugin center: a grid of cards, one per plugin, and the settings the
//! plugin that is open declared.
//!
//! A plugin is a program with its own logic, its own state and its own settings,
//! and this page is where the product meets that. Four decisions shape
//! everything here, and each one exists because the alternative was tried:
//!
//! * **A card, not a row.** A plugin is a thing a user *installs*, and an install
//!   list of one-line rows reads as a list of switches. A card carries an icon, a
//!   name, a version, a sentence, whether its process is alive, and its own
//!   controls.
//! * **A grid of cards, not a section each.** `gpui-kit` renders every *titled*
//!   group of a page that has more than one group as a second-level entry in the
//!   sidebar, so a titled group per plugin turned this destination into a menu of
//!   plugin names: the cards were all reachable, but as a submenu rather than as
//!   the page. The page therefore owns exactly one untitled group and draws its
//!   cards itself — the same shape the model library uses, and the reason a plugin
//!   is a cell in a grid rather than a section in a list.
//! * **A card that is not a model card.** A model is a picture with a name, so a
//!   model card leads with its cover. A plugin is a program with a sentence, so
//!   its card leads with its icon and its name, keeps the sentence, and puts its
//!   state control and its actions underneath — no artwork, and none of the
//!   heights a cover forces on every cell in the row.
//! * **An icon the plugin declares.** An emoji today and a PNG beside it
//!   tomorrow, drawn by the host either way. The plugin says *what the icon is*;
//!   the host draws it, which is the same division the panel follows.
//!
//! One plugin's own settings open as a panel under the grid rather than inside
//! its card: a field row is a `SettingItem`, and `gpui-kit` only renders those
//! inside a `SettingGroup`, which a card cannot host. Every control in
//! [`super::plugin_settings`] is still chosen by a field's `kind`, not by the
//! plugin's name — adding a plugin that wants a switch, a number, a line of text
//! and a menu adds no code to the window.
//!
//! Four states are rendered distinctly and none of them is an empty list: a host
//! that is not running, a catalog that has not been read yet, a catalog with
//! nothing in it, and a catalog with cards in it.

use super::*;
use gpui_kit::AnyElement;
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{Sizable as _, Size};

/// Every user-visible string on the plugin page and in its settings panel.
///
/// The page's own copy contract: a key it renders but does not declare here is
/// one the catalogs can lose without anything noticing, and a declared key with
/// no copy in some language renders as the key itself.
#[cfg(test)]
pub(super) const PLUGINS_LOCALIZED_KEYS: [&str; 25] = [
    "settings.plugins.catalog.refresh",
    "settings.plugins.catalog.loading",
    "settings.plugins.catalog.empty",
    "settings.plugins.enable",
    "settings.plugins.configure_needs_running",
    "settings.plugins.action.install",
    "settings.plugins.action.update",
    "settings.plugins.action.uninstall",
    "settings.plugins.action.configure",
    "settings.plugins.state.running",
    "settings.plugins.state.stopped",
    "settings.plugins.refusal.not_published",
    "settings.plugins.error.unavailable",
    "settings.plugins.error.not_published",
    "settings.plugins.error.already_installed",
    "settings.plugins.error.catalog",
    "settings.plugins.error.network",
    "settings.plugins.error.checksum",
    "settings.plugins.error.signature",
    "settings.plugins.error.store",
    "settings.plugins.error.too_many",
    "settings.plugins.error.manifest",
    "settings.plugins.error.render",
    "settings.plugins.error.plugin",
    "settings.plugins.error.other",
];

/// The narrowest column the plugin grid allows. The number of columns is the
/// width divided by this floor, up to [`PLUGIN_GRID_MAX_COLUMNS`].
///
/// Narrower than a model card's floor on purpose: a plugin card is a tile of
/// icon, name and controls rather than a picture with a caption, so two or three
/// of them fit a settings window where one model cover and a half would.
pub(super) const PLUGIN_CARD_MIN_WIDTH: f32 = 260.0;
/// The grid never spreads past this many columns, even on a very wide window.
///
/// Three is the count that keeps a card's sentence readable — a fourth column on
/// a wide display would break every description into four words a line — and it
/// is the cap that keeps the second column from becoming a very wide card with
/// one sentence on it.
pub(super) const PLUGIN_GRID_MAX_COLUMNS: usize = 3;
/// Horizontal chrome between the settings window and the plugin grid: the
/// resizable sidebar and the page and group padding the grid sits inside.
/// Column selection only needs the grid's approximate width; the row grid still
/// stretches exactly to the space it is given.
const PLUGIN_GRID_WINDOW_CHROME: f32 = 284.0;
/// The edge of the tile that carries the icon the plugin declared.
const PLUGIN_CARD_ICON_SIZE: f32 = 32.0;
/// The size every control on a card is built from.
///
/// One value rather than the page's own `RenderOptions`: a card is drawn here,
/// outside the settings component's row layout, and a card whose controls changed
/// size with the window's density setting would be the only place in the window
/// where that happened.
const PLUGIN_CARD_SIZE: Size = Size::Medium;

/// How many columns the plugin grid uses at a usable width.
///
/// Two at the narrowest desktop window, then one more per
/// [`PLUGIN_CARD_MIN_WIDTH`] of room until the cap. The count is derived from the
/// same width the column floor came from, so the columns stay square-necked: they
/// neither balloon on a wide window nor fall below a card that fits its sentence.
pub(super) fn plugin_grid_columns(width: Pixels) -> usize {
    let fit = (f32::from(width) / PLUGIN_CARD_MIN_WIDTH).floor().max(2.0);
    (fit as usize).clamp(2, PLUGIN_GRID_MAX_COLUMNS)
}

/// Select the column count from the settings window's available width.
pub(super) fn plugin_grid_columns_for_window(width: Pixels) -> usize {
    plugin_grid_columns(px((f32::from(width) - PLUGIN_GRID_WINDOW_CHROME).max(0.0)))
}

/// What one card offers, decided from the card's own state.
///
/// Two variants and three facts rather than four variants: the facts are
/// independent — installed, updatable, pressable — and a variant per combination
/// would be eight arms of which the compiler can prove four unreachable. An
/// earlier shape had one variant for "installed" and a separate one for "installed
/// but at the panel bound", which could not say "installed, updatable *and* at the
/// bound" and therefore dropped the update button on exactly the card that needed
/// it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CardAction {
    /// Nothing is installed, so the card offers the one action that changes that.
    ///
    /// The panel bound does not apply here, and deliberately: installing a plugin
    /// does not show its panel until the switch is turned on, so refusing the
    /// install would leave a user at the bound with no way to acquire a plugin at
    /// all.
    Install,
    Show {
        enabled: bool,
        /// Whether the catalog offers something newer.
        update: bool,
        /// Whether the switch accepts a press.
        ///
        /// False at the panel bound. The switch stays on screen and is marked
        /// unpressable rather than being replaced by an install button, because
        /// the card describes a plugin that *is* installed and an install button
        /// on it would describe a different action.
        live: bool,
    },
}

impl CardAction {
    fn for_entry(entry: &SettingsPluginEntry, switch_live: bool) -> Self {
        if !entry.installed {
            return Self::Install;
        }
        Self::Show {
            enabled: entry.enabled,
            update: entry.update_available,
            live: switch_live,
        }
    }

    /// Whether the card shows the "show on the model window" switch.
    const fn shows_switch(self) -> bool {
        matches!(self, Self::Show { .. })
    }

    /// Whether the card offers the update button.
    const fn shows_update(self) -> bool {
        matches!(self, Self::Show { update: true, .. })
    }

    /// Whether the switch is pressable.
    const fn switch_is_live(self) -> bool {
        matches!(self, Self::Show { live: true, .. })
    }

    /// Whether the card's switch is switched on.
    ///
    /// The card reads the switch's own state out of the view rather than from
    /// this, because the host is the only side that knows it — a refused press
    /// moves the host's answer and the switch follows on the next render. This is
    /// what a *test* reads, which is why it exists rather than being a field.
    #[allow(
        dead_code,
        reason = "read by the card's own tests, which pin what the switch says"
    )]
    const fn enabled(self) -> bool {
        matches!(self, Self::Show { enabled: true, .. })
    }
}

/// Whether this host still has room for one more panel.
fn switch_is_live(plugins: &SettingsPlugins) -> bool {
    plugins.active < plugins.maximum_active
}

/// The sentence for one plugin failure, as a catalog key.
///
/// Exhaustive over the protocol's codes with no catch-all, so a code the host can
/// report and this page cannot name is a compile error here rather than a blank
/// line in a user's settings window.
fn error_key(code: SettingsPluginErrorCode) -> &'static str {
    match code {
        SettingsPluginErrorCode::HostUnavailable => "settings.plugins.error.unavailable",
        SettingsPluginErrorCode::NotPublished => "settings.plugins.error.not_published",
        SettingsPluginErrorCode::AlreadyInstalled => "settings.plugins.error.already_installed",
        SettingsPluginErrorCode::CatalogUnavailable => "settings.plugins.error.catalog",
        SettingsPluginErrorCode::NetworkUnavailable => "settings.plugins.error.network",
        SettingsPluginErrorCode::ChecksumMismatch => "settings.plugins.error.checksum",
        SettingsPluginErrorCode::SignatureInvalid => "settings.plugins.error.signature",
        SettingsPluginErrorCode::StoreWriteFailed => "settings.plugins.error.store",
        SettingsPluginErrorCode::TooManyEnabled => "settings.plugins.error.too_many",
        SettingsPluginErrorCode::InvalidManifest => "settings.plugins.error.manifest",
        SettingsPluginErrorCode::RenderFailed => "settings.plugins.error.render",
        SettingsPluginErrorCode::PluginFailed => "settings.plugins.error.plugin",
        SettingsPluginErrorCode::Other => "settings.plugins.error.other",
    }
}

/// Why one plugin cannot be installed here, in the window's language.
///
/// A refusal on a card is always about the *platform* in practice — the catalog
/// publishes per-target archives and this host is not one of them — so the whole
/// set maps onto the one sentence that is true. A refusal is not an error, so it
/// gets its own line rather than the page's error line.
fn refusal_text(_refusal: &SettingsPluginRefusal, language: SettingsLanguage) -> SharedString {
    bongocat_i18n::text(
        language.catalog_locale(),
        "settings.plugins.refusal.not_published",
    )
    .into()
}

/// A card's name: the plugin's own, or its id when it declared none.
///
/// The icon no longer rides in this string. It is a tile beside the name rather
/// than a character in front of it, which is what lets a grid of cards line their
/// names up — a row of titles each starting with its own glyph starts them at as
/// many different offsets as there are glyphs. The settings panel reads the same
/// name, so a form names its plugin the way its card does.
pub(super) fn card_heading(entry: &SettingsPluginEntry) -> SharedString {
    if entry.name.is_empty() {
        entry.id.clone().into()
    } else {
        entry.name.clone().into()
    }
}

/// The tile that carries the icon the plugin declared.
///
/// Emoji today and a PNG beside it tomorrow, and the tile is where both land: it
/// is a fixed square, so a picture replacing the glyph changes nothing about the
/// card's shape. A plugin that declared neither gets a letter, because a grid of
/// cards with a blank where the icon goes is a grid of blanks.
///
/// The same tile heads the settings panel, so a form names its plugin with the
/// mark the rest of the page uses for it rather than with text alone.
pub(super) fn card_icon(entry: &SettingsPluginEntry, tokens: Tokens) -> Div {
    let glyph = match &entry.icon.emoji {
        Some(emoji) => div().text_lg().child(emoji.clone()),
        None => div()
            .text_sm()
            .font_semibold()
            .text_color(tokens.muted)
            .child(entry.initial()),
    };
    div()
        .flex_none()
        .size(px(PLUGIN_CARD_ICON_SIZE))
        .rounded_md()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.overlay)
        .flex()
        .items_center()
        .justify_center()
        .child(glyph)
}

/// The quiet line under a card's name: who made it and which version is here.
///
/// Not a sentence and not a state — the identity of the thing, for a card whose
/// plugin did not describe itself. A card with neither says nothing at all rather
/// than saying a bullet on its own.
fn card_meta(entry: &SettingsPluginEntry) -> Option<SharedString> {
    let mut parts: Vec<&str> = Vec::new();
    if !entry.author.is_empty() {
        parts.push(entry.author.as_str());
    }
    if let Some(version) = &entry.installed_version {
        parts.push(version.as_str());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" · ").into())
}

/// The sentence under a card's meta line.
///
/// The failure first, then the refusal, then the description. The failure leads
/// because a card that looks fine and is not running is the state a user cannot
/// work out from a sentence about its author, and it is drawn in the text colour
/// rather than the muted one so the card says so without a badge of its own.
fn card_summary(
    entry: &SettingsPluginEntry,
    language: SettingsLanguage,
) -> Option<(SharedString, bool)> {
    let locale = language.catalog_locale();
    if let Some(failure) = &entry.failure {
        return Some((
            bongocat_i18n::text(locale, error_key(failure.code)).into(),
            true,
        ));
    }
    if let Some(refusal) = &entry.refusal {
        return Some((refusal_text(refusal, language), false));
    }
    if !entry.description.is_empty() {
        return Some((entry.description.clone().into(), false));
    }
    None
}

/// The two facts a card's state badge is decided from.
///
/// A copy rather than a reference into the entry, because the badge is rebuilt on
/// every render and a rendered element cannot be built once and kept. Two bools is
/// the whole of it, and it is deliberately not the whole entry: anything else a card
/// shows is read where the card is built, where the view is not already borrowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CardState {
    installed: bool,
    running: bool,
}

/// The badge that says whether a plugin's process is alive, or that there is none.
///
/// Separate from the switch because the two answer different questions: the switch
/// is what the user asked for and the badge is what happened. A card whose plugin is
/// switched on and not running is the state that needs saying out loud, because it is
/// what a crash looks like from outside.
///
/// **Nothing at all** for a plugin that is not installed, and that is the whole reason
/// this takes the entry rather than a bool. There is no process to have stopped: the
/// badge was answering a question about something that does not exist yet, and every
/// uninstalled card in a list said "Stopped" — a row of plugins the user had not
/// asked for, all reporting a state none of them was in.
fn state_badge(state: CardState, locale: &'static str) -> Option<AnyElement> {
    if !state.installed {
        return None;
    }
    let key = if state.running {
        "settings.plugins.state.running"
    } else {
        "settings.plugins.state.stopped"
    };
    let style = if state.running {
        Tag::secondary()
    } else {
        Tag::new().outline()
    };
    Some(
        Badge::new()
            .child(
                style
                    .small()
                    .rounded_full()
                    .child(bongocat_i18n::text(locale, key)),
            )
            .into_any_element(),
    )
}

/// The page's own line: the catalog's state, or nothing at all.
///
/// A plugin operation that failed leaves its reason here rather than in a
/// notification: the failure is a property of the catalog the page is showing, so a
/// notification would vanish and leave the page looking healthy. A catalog that is
/// simply fine says nothing — the cards below are the answer, and a settled page
/// with a paragraph above its list is a page that reads as unfinished.
fn catalog_description(
    plugins: &SettingsPlugins,
    language: SettingsLanguage,
) -> Option<SharedString> {
    let locale = language.catalog_locale();
    if let Some(error) = &plugins.last_error {
        return Some(bongocat_i18n::text(locale, error_key(error.code)).into());
    }
    if !plugins.available {
        return Some(bongocat_i18n::text(locale, "settings.plugins.error.unavailable").into());
    }
    if plugins.is_pending() {
        return Some(bongocat_i18n::text(locale, "settings.plugins.catalog.loading").into());
    }
    if plugins.entries.is_empty() {
        return Some(bongocat_i18n::text(locale, "settings.plugins.catalog.empty").into());
    }
    None
}

/// The row above the grid: what the catalog last said, and the one control that
/// re-reads it.
///
/// One sentence and one button, and the sentence is absent whenever the catalog has
/// nothing to report — a refresh in flight is a disabled button over the cards the
/// user is already reading, not a blanked page.
fn catalog_toolbar(
    plugins: &SettingsPlugins,
    language: SettingsLanguage,
    view: Entity<SettingsView>,
    tokens: Tokens,
) -> impl IntoElement {
    let locale = language.catalog_locale();
    let description = catalog_description(plugins, language);
    let refresh_view = view.clone();
    div()
        .id("plugin-catalog-toolbar")
        .w_full()
        .flex()
        .items_start()
        .justify_between()
        .gap_3()
        .child(description.map_or_else(
            || div().flex_1(),
            |description| {
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(tokens.muted)
                    .child(description)
            },
        ))
        .child(
            Button::new("plugin-catalog-refresh")
                .label(bongocat_i18n::text(
                    locale,
                    "settings.plugins.catalog.refresh",
                ))
                .icon(IconName::RefreshCw)
                .with_size(PLUGIN_CARD_SIZE)
                .with_variant(ButtonVariant::Secondary)
                .disabled(plugins.busy)
                .on_click(move |_, _, app| {
                    refresh_view.update(app, |view, cx| view.refresh_plugin_catalog(cx));
                }),
        )
}

/// The switch, and what it is for.
///
/// Beside an unlabelled switch rather than inside one, because a card is narrow and
/// the rest of this window already reads as label on the left and control on the right.
///
/// The sentence is the one the window can actually vouch for. The earlier label — "Show
/// on the model window" — described the *consequence* of the switch rather than what the
/// switch is, and it was the wrong shape of sentence twice over: a user who does not
/// know the product has a model window cannot act on it, and a user who has just
/// installed a plugin and turned this on saw nothing happen and had nothing on the card
/// that told them where to look. So the card names the switch, and the card's own
/// summary — which the plugin writes and which therefore *is* about the model window —
/// carries the rest.
///
/// The switch reads its own state out of the view at build time, so a refused press —
/// the panel bound, a host that is not running — puts the switch back where the host
/// actually is without the page keeping a copy of the truth.
fn card_switch(
    id: &str,
    enabled: bool,
    live: bool,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> impl IntoElement {
    let switch_view = view.clone();
    let switch_id = id.to_string();
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .text_sm()
                .text_color(tokens.muted)
                .truncate()
                .child(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.plugins.enable",
                )),
        )
        .child(
            Switch::new(SharedString::from(format!("plugin-show-{switch_id}")))
                .checked(enabled)
                .with_size(PLUGIN_CARD_SIZE)
                .disabled(!live)
                .on_click(move |_checked, _window, app| {
                    let plugin = switch_id.to_string();
                    switch_view.update(app, |view, cx| view.toggle_plugin_enabled(plugin, cx));
                }),
        )
}

/// The controls a plugin asked the host to draw, as buttons.
///
/// These are the plugin's own words and its own meaning, and the host draws them with
/// the product's buttons — which is the whole of the division an action exists to keep.
/// A timer that has offered a "Start" control gets a Start button on its card, in the
/// user's language, that becomes "Pause" the moment the round is counting: the label is
/// what the user reads to decide what a press will do, so it travels with the press
/// rather than being fixed at the handshake.
///
/// Labelled rather than icon-only, and that is the one place this card uses a word on
/// its own: the icon beside the label is chosen by the plugin, so there may not be one
/// (a control with no glyph draws no icon), and a row of unlabelled icons beside a
/// switch is the row that produced the original complaint — controls a user cannot name
/// and therefore cannot decide between.
fn card_offered_actions(
    entry: &SettingsPluginEntry,
    view: Entity<SettingsView>,
) -> Vec<AnyElement> {
    entry
        .actions
        .iter()
        .map(|action| {
            let press_view = view.clone();
            let plugin = entry.id.clone();
            let control = action.id.clone();
            let mut button = Button::new(SharedString::from(format!(
                "plugin-action-{plugin}-{control}"
            )))
            .label(action.label.clone())
            .with_size(PLUGIN_CARD_SIZE)
            .disabled(action.disabled)
            .on_click(move |_, _, app| {
                press_view.update(app, |view, cx| {
                    view.press_plugin_action(plugin.clone(), control.clone(), cx)
                });
            });
            button = match action.glyph {
                SettingsActionGlyph::None => button,
                SettingsActionGlyph::Play => button.icon(IconName::Play),
                SettingsActionGlyph::Pause => button.icon(IconName::Pause),
                SettingsActionGlyph::Reset => button.icon(IconName::RotateCcw),
            };
            button.into_any_element()
        })
        .collect()
}

/// The card's own buttons: one labelled action when there is nothing installed, and
/// the icon actions of an installed plugin otherwise.
///
/// Uninstall is the one destructive control here and it asks for nothing first, on
/// purpose: a plugin the catalog offers is one press away from being installed
/// again, so the cost of a mispress is a download rather than a loss.
#[allow(clippy::too_many_arguments)]
fn card_actions(
    entry: &SettingsPluginEntry,
    action: CardAction,
    settings_open: bool,
    settings_pending: bool,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> AnyElement {
    let locale = language.catalog_locale();
    let id = entry.id.clone();
    if !action.shows_switch() {
        let install_view = view.clone();
        let install_id = id.clone();
        return Button::new(SharedString::from(format!("plugin-install-{install_id}")))
            .label(bongocat_i18n::text(
                locale,
                "settings.plugins.action.install",
            ))
            .icon(IconName::Download)
            .with_size(PLUGIN_CARD_SIZE)
            .with_variant(ButtonVariant::Primary)
            .w_full()
            .on_click(move |_, _, app| {
                let plugin = install_id.to_string();
                install_view.update(app, |view, cx| view.install_plugin(plugin, cx));
            })
            .into_any_element();
    }

    // The plugin's own settings. **Always** drawn for an installed plugin, and that is
    // the change that fixes the dead end this card used to have: the form comes from the
    // running process's handshake, so a plugin that is switched off has no fields yet —
    // and a button that appears only once a plugin is running is a button that is
    // missing at exactly the moment a user who has just installed something looks for
    // it. So the control is always there, and what pressing it does depends on whether
    // the form can be shown yet: open it, or turn the plugin on and say so. A control
    // that is present and explains itself beats a control that is absent.
    let configure_view = view.clone();
    let configure_id = id.clone();
    // The tooltip says what pressing this will actually do, which is not the same thing
    // for a running plugin and a stopped one. A stopped plugin's form is opened by
    // turning it on, and a tooltip that said "Settings" on a card whose only other
    // control is a switch would be describing a control that does nothing — the tooltip
    // is the one place a user can find out before pressing.
    //
    // Marked unpressable while the plugin is starting, for the reason the tooltip exists
    // plus one: a second press would queue a second enable behind the first, and the
    // user would be told they had pressed it twice for no visible change.
    let configure_tooltip = if entry.enabled {
        bongocat_i18n::text(locale, "settings.plugins.action.configure")
    } else {
        bongocat_i18n::text(locale, "settings.plugins.configure_needs_running")
    };
    let settings_button = Button::new(SharedString::from(format!(
        "plugin-configure-{configure_id}"
    )))
    .icon(IconName::SlidersHorizontal)
    .tooltip(configure_tooltip)
    .toggled(settings_open)
    .disabled(settings_pending)
    .with_size(PLUGIN_CARD_SIZE)
    .with_variant(ButtonVariant::Ghost)
    .on_click(move |_, _, app| {
        let plugin = configure_id.to_string();
        configure_view.update(app, |view, cx| view.toggle_plugin_settings(plugin, cx));
    })
    .into_any_element();

    let mut row = div().flex().items_center().gap_1();
    for action in card_offered_actions(entry, view.clone()) {
        row = row.child(action);
    }
    let mut row = row.child(settings_button);
    if action.shows_update() {
        let update_view = view.clone();
        let update_id = id.clone();
        row = row.child(
            Button::new(SharedString::from(format!("plugin-update-{update_id}")))
                .icon(IconName::RefreshCw)
                .tooltip(bongocat_i18n::text(
                    locale,
                    "settings.plugins.action.update",
                ))
                .with_size(PLUGIN_CARD_SIZE)
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _, app| {
                    let plugin = update_id.to_string();
                    update_view.update(app, |view, cx| view.install_plugin(plugin, cx));
                }),
        );
    }
    let uninstall_view = view;
    let uninstall_id = id;
    row.child(
        Button::new(SharedString::from(format!(
            "plugin-uninstall-{uninstall_id}"
        )))
        .icon(IconName::Trash)
        .tooltip(bongocat_i18n::text(
            locale,
            "settings.plugins.action.uninstall",
        ))
        .with_size(PLUGIN_CARD_SIZE)
        .with_variant(ButtonVariant::Ghost)
        .on_click(move |_, _, app| {
            let plugin = uninstall_id.to_string();
            uninstall_view.update(app, |view, cx| view.uninstall_plugin(plugin, cx));
        }),
    )
    .into_any_element()
}

/// One plugin, as one cell of the grid.
///
/// Three blocks top to bottom: the mark, the name and the sentence; then whatever
/// the card controls, pushed to the bottom edge so that every card in a row lines
/// its controls up whatever its own sentence turned out to be worth.
#[allow(clippy::too_many_arguments)]
fn plugin_card(
    entry: &SettingsPluginEntry,
    action: CardAction,
    enabled: bool,
    settings_open: bool,
    settings_pending: bool,
    index: usize,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> impl IntoElement {
    let locale = language.catalog_locale();
    let state = CardState {
        installed: entry.installed,
        running: entry.running,
    };
    let heading = card_heading(entry);
    let meta = card_meta(entry);
    let summary = card_summary(entry, language);
    // The badge, or nothing at all in its place: a card whose plugin is not
    // installed has no process to report on, and an empty box there would be a
    // hole in the middle of every card in the catalog the user has not installed.
    let badge = state_badge(state, locale).unwrap_or_else(|| div().flex_none().into_any_element());
    let identity = div()
        .min_w_0()
        .flex_1()
        .child(div().text_sm().font_semibold().truncate().child(heading));
    let identity = match meta {
        Some(meta) => identity.child(
            div()
                .min_w_0()
                .text_sm()
                .text_color(tokens.muted)
                .truncate()
                .child(meta),
        ),
        None => identity,
    };
    // One row, switch on the left and everything else on the right, and the reason is
    // the same one the rest of this window uses: a card is 260 pixels wide, and two
    // stacked rows of controls left the switch's label in a row of its own with the
    // buttons underneath it, which is a column that reads as a form rather than as the
    // controls for one thing. Beside each other, the switch and the buttons are one
    // answer to one question — is this running, and what can I do to it — and the row
    // fits in the width the card already has.
    let mut controls = div().w_full().flex().flex_col().gap_2().mt_auto();
    if action.shows_switch() {
        controls = controls.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(card_switch(
                    &entry.id,
                    enabled,
                    action.switch_is_live(),
                    view.clone(),
                    language,
                    tokens,
                ))
                .child(card_actions(
                    entry,
                    action,
                    settings_open,
                    settings_pending,
                    view,
                    language,
                )),
        );
    } else {
        controls = controls.child(card_actions(
            entry,
            action,
            settings_open,
            settings_pending,
            view,
            language,
        ));
    }

    div()
        .id(("plugin-card", index))
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas)
        // Observed so a test can read the cell the card really occupies: that cards
        // share a row, and that a row is as tall as its tallest card, are the two
        // claims the grid's whole shape rests on.
        .test_support()
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .child(card_icon(entry, tokens))
                .child(identity)
                .child(badge),
        )
        .child(summary.map_or_else(
            || div().flex_1(),
            |(text, failed)| {
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(if failed { tokens.text } else { tokens.muted })
                    .line_clamp(3)
                    .child(text)
            },
        ))
        .child(controls)
}

/// The grid itself: rows of cards, each row as wide as the page.
///
/// Every row keeps the full column template, so a short final row leaves its unused
/// columns empty instead of stretching one card across the whole width. The grid
/// does not scroll itself — the page's own list does — so a plugin's settings can
/// open under it without two scroll regions fighting over the same wheel.
fn plugin_grid(columns: usize, children: impl IntoIterator<Item: IntoElement>) -> Div {
    let columns = columns.clamp(2, PLUGIN_GRID_MAX_COLUMNS);
    let mut children = children.into_iter();
    let mut rows = Vec::new();
    loop {
        let row = children.by_ref().take(columns).collect::<Vec<_>>();
        if row.is_empty() {
            break;
        }
        rows.push(
            div()
                .w_full()
                .grid()
                .grid_cols(u16::try_from(columns).expect("the column cap fits in u16"))
                .gap_3()
                .children(row),
        );
    }

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .children(rows)
}

/// The page's whole body: the catalog's own toolbar, then a card per plugin.
pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::English, |snapshot| {
        snapshot.resolved_language
    });
    let plugins = snapshot.map_or_else(SettingsPlugins::default, |snapshot| {
        snapshot.plugins.clone()
    });
    let switch_live = switch_is_live(&plugins);
    let cards_view = cx.entity();
    // Read here, before any card exists: a card is built while this function
    // already holds the view, so the two facts a card needs from it — what its
    // switch says and whether its settings are open — have to be decided now
    // rather than asked for from inside the card.
    let cards = plugins
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let action = CardAction::for_entry(entry, switch_live);
            let enabled = view.plugin_is_enabled(entry.id.as_str());
            let settings_open = view.plugin_settings_are_open(entry.id.as_str());
            let settings_pending = view.plugin_awaiting_settings(entry.id.as_str());
            plugin_card(
                entry,
                action,
                enabled,
                settings_open,
                settings_pending,
                index,
                cards_view.clone(),
                language,
                tokens,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

    div()
        .id("plugins-content")
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .text_color(tokens.text)
        .child(catalog_toolbar(&plugins, language, cards_view, tokens))
        .child(
            div()
                .id("plugin-grid")
                // Observed so a test can read the width the grid was laid out at: the
                // column count is the window's decision, and asserting it from the
                // rendered width is what keeps the two in step.
                .test_support()
                .child(plugin_grid(
                    plugin_grid_columns_for_window(window.viewport_size().width),
                    cards,
                )),
        )
}

/// The page's one group: the cards, and the open plugin's settings under them.
///
/// **One** group, and that is the whole of the page's shape: `gpui-kit` turns every
/// titled group of a multi-group page into a second-level sidebar entry, so a group
/// per plugin is what made this destination a menu of plugin names. The group's
/// surface is dropped for the same reason the model library drops it — the cards
/// draw their own borders, and a card grid inside a card is a card in a card.
///
/// The settings panel is a sibling of the grid rather than a section of it, because
/// `gpui-kit` renders a `SettingItem` only inside a group: a card cannot host a
/// field row. It is built here rather than inside the grid for the same reason the
/// grid itself is — the body is drawn while the view is already borrowed, so what
/// the page needs from the view is decided before any element exists and handed in.
pub(super) fn group(
    view: Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    keywords: Vec<SharedString>,
    expanded: Option<String>,
) -> SettingGroup {
    let plugins = snapshot.map_or_else(SettingsPlugins::default, |snapshot| {
        snapshot.plugins.clone()
    });
    let open_entry = expanded
        .as_deref()
        .and_then(|id| plugins.entries.iter().find(|entry| entry.id == id))
        .filter(|entry| !entry.fields.is_empty());
    // The cards are one item, so searching the page matches the plugin names and
    // the page's own words rather than each card separately — and when a panel is
    // open its field labels join them, so a search for one of a plugin's settings
    // still finds the page it lives on.
    let mut search = keywords;
    for entry in &plugins.entries {
        search.push(entry.name.clone().into());
        search.push(entry.id.clone().into());
    }
    if let Some(entry) = open_entry {
        search.extend(entry.fields.iter().map(|field| field.label.clone().into()));
    }

    let body_view = view.clone();
    let body = SettingItem::render(
        move |_: &RenderOptions, window: &mut Window, app: &mut App| {
            let snapshot = body_view.read(app).snapshot.clone();
            let tokens = Tokens::from_theme(app);
            body_view
                .update(app, move |view, cx| {
                    content(view, window, cx, snapshot.as_ref(), tokens)
                })
                .into_any_element()
        },
    )
    .keywords(search);

    let mut items = vec![body];
    if let Some(entry) = open_entry {
        items.extend(plugin_settings::panel(entry, view, language));
    }
    SettingGroup::new()
        .variant(GroupBoxVariant::Normal)
        .items(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_ui_protocol::{SettingsPluginError, SettingsPluginIcon};

    fn entry(installed: bool, enabled: bool, update: bool) -> SettingsPluginEntry {
        SettingsPluginEntry {
            id: "pomodoro".to_string(),
            name: "Pomodoro".to_string(),
            description: String::new(),
            author: String::new(),
            icon: SettingsPluginIcon::default(),
            installed_version: installed.then(|| "1.0.0".to_string()),
            available_version: Some("1.0.0".to_string()),
            installed,
            enabled,
            running: installed && enabled,
            update_available: update,
            fields: Vec::new(),
            // A plugin that declares no settings has none to show, so `enabled` alone
            // decides the flag here — exactly as the projection computes it for a
            // running plugin with an empty schema.
            settings_available: installed && enabled,
            actions: Vec::new(),
            values: Default::default(),
            log: Vec::new(),
            refusal: None,
            failure: None,
        }
    }

    /// An entry whose running plugin declared one setting.
    ///
    /// `settings_available` rather than a field, because the card's behaviour depends on
    /// whether a form *can* be shown and a test that only set `fields` would be asserting
    /// against a state the projection cannot produce.
    fn with_fields(mut entry: SettingsPluginEntry) -> SettingsPluginEntry {
        entry.fields = vec![SettingsPluginField {
            key: "minutes".to_string(),
            label: "Minutes".to_string(),
            description: None,
            kind: SettingsFieldKind::Integer,
            default: SettingsFieldValue::Integer(25),
            minimum: Some(1.0),
            maximum: Some(120.0),
            step: Some(5.0),
            unit: None,
            placeholder: None,
            multiline: false,
            options: Vec::new(),
        }];
        entry.settings_available = entry.enabled;
        entry
    }

    /// An entry whose running plugin offered one control.
    fn with_actions(
        mut entry: SettingsPluginEntry,
        label: &str,
        glyph: SettingsActionGlyph,
    ) -> SettingsPluginEntry {
        entry.actions = vec![bongocat_ui_protocol::SettingsPluginAction {
            id: "toggle".to_string(),
            label: label.to_string(),
            glyph,
            disabled: false,
        }];
        entry
    }

    #[test]
    fn a_plugin_that_is_not_installed_offers_install_and_no_switch() {
        let action = CardAction::for_entry(&entry(false, false, false), true);
        assert_eq!(action, CardAction::Install);
        assert!(!action.shows_switch());
        assert!(!action.shows_update());
    }

    #[test]
    fn an_installed_plugin_offers_a_switch() {
        let action = CardAction::for_entry(&entry(true, true, false), true);
        assert_eq!(
            action,
            CardAction::Show {
                enabled: true,
                update: false,
                live: true
            }
        );
        assert!(action.shows_switch());
        assert!(action.switch_is_live());
        assert!(action.enabled());
    }

    #[test]
    fn an_updatable_plugin_offers_the_switch_and_the_update() {
        let action = CardAction::for_entry(&entry(true, true, true), true);
        assert_eq!(
            action,
            CardAction::Show {
                enabled: true,
                update: true,
                live: true
            }
        );
        assert!(action.shows_update());
    }

    #[test]
    fn the_panel_bound_greys_the_switch_instead_of_offering_an_install() {
        // A card whose plugin *is* installed must never present an Install button at
        // the bound: it would describe the wrong action, and pressing it would send
        // `InstallPlugin` for something the store already holds.
        for (installed, update) in [(true, false), (true, true)] {
            let plugins = SettingsPlugins {
                active: 4,
                maximum_active: 4,
                ..SettingsPlugins::default()
            };
            assert!(!switch_is_live(&plugins));
            let action =
                CardAction::for_entry(&entry(installed, false, update), switch_is_live(&plugins));
            assert!(
                !matches!(action, CardAction::Install),
                "an installed card must not offer Install at the bound, got {action:?}"
            );
            assert_eq!(
                action,
                CardAction::Show {
                    enabled: false,
                    update,
                    live: false
                },
                "and it keeps a switch that is on screen but refuses the press"
            );
            assert!(!action.switch_is_live());
        }
    }

    #[test]
    fn a_card_at_the_bound_keeps_its_update_and_uninstall() {
        // Only the switch is affected by the bound. Update and uninstall do not add a
        // panel, so they stay live — otherwise a user at the bound could not free a
        // slot by removing a plugin.
        let plugins = SettingsPlugins {
            active: 4,
            maximum_active: 4,
            ..SettingsPlugins::default()
        };
        let action = CardAction::for_entry(&entry(true, false, true), switch_is_live(&plugins));
        assert_eq!(
            action,
            CardAction::Show {
                enabled: false,
                update: true,
                live: false
            }
        );
        assert!(
            action.shows_update(),
            "and the update button is still there, because it does not add a panel — this is the \
             combination the old variant shape could not express, and it dropped the button"
        );
        assert!(!action.switch_is_live());
    }

    #[test]
    fn a_not_installed_card_still_offers_install_at_the_bound() {
        // The bound governs panels that are *showing*, and installing a plugin does not
        // show its panel until the switch is turned on. Refusing the install here would
        // leave a user with no way to acquire a plugin at all.
        let plugins = SettingsPlugins {
            active: 4,
            maximum_active: 4,
            ..SettingsPlugins::default()
        };
        assert_eq!(
            CardAction::for_entry(&entry(false, false, false), switch_is_live(&plugins)),
            CardAction::Install,
        );
    }

    #[test]
    fn one_panel_short_of_the_bound_still_offers_the_switch() {
        let plugins = SettingsPlugins {
            active: 3,
            maximum_active: 4,
            ..SettingsPlugins::default()
        };
        assert!(switch_is_live(&plugins));
        assert_eq!(
            CardAction::for_entry(&entry(true, false, false), switch_is_live(&plugins)),
            CardAction::Show {
                enabled: false,
                update: false,
                live: true
            }
        );
    }

    #[test]
    fn the_grid_is_two_columns_at_the_narrowest_window_and_never_wider_than_three() {
        // The window's own floor: the sidebar and the page padding leave about 516px
        // for the grid at 800px wide, which is two cards and not one, and which is
        // also not three.
        let narrow = plugin_grid_columns_for_window(px(800.0));
        assert_eq!(narrow, 2);

        // Three cards from the width three floors need, and never four: a fourth
        // column would break every description into four words a line.
        let three = PLUGIN_CARD_MIN_WIDTH * 3.0;
        assert_eq!(
            plugin_grid_columns_for_window(px(three + PLUGIN_GRID_WINDOW_CHROME)),
            3
        );
        assert_eq!(
            plugin_grid_columns_for_window(px(3840.0)),
            PLUGIN_GRID_MAX_COLUMNS
        );

        // And the floor itself, so a card is never narrower than its own sentence.
        assert_eq!(plugin_grid_columns(px(PLUGIN_CARD_MIN_WIDTH * 2.0)), 2);
        assert_eq!(plugin_grid_columns(px(0.0)), 2);
    }

    #[test]
    fn a_degraded_host_says_so_rather_than_showing_an_empty_catalog() {
        let plugins = SettingsPlugins::default();
        assert!(plugins.entries.is_empty());
        assert!(
            !plugins.is_pending(),
            "no host is a failure to report, not a catalog to wait for"
        );
        let text = catalog_description(&plugins, SettingsLanguage::English);
        assert!(text.is_some(), "no host is a failure, not an empty catalog");
    }

    #[test]
    fn an_unread_catalog_is_a_loading_state_and_a_read_empty_one_is_empty() {
        let pending = SettingsPlugins {
            available: true,
            ..SettingsPlugins::default()
        };
        assert!(pending.is_pending());
        assert!(catalog_description(&pending, SettingsLanguage::English).is_some());

        // The one this page used to get wrong: a catalog that has been read and has
        // nothing in it is an answer, and answering it "still reading" forever is what
        // a stuck page looks like.
        let read_and_empty = SettingsPlugins {
            available: true,
            catalog_read: true,
            ..SettingsPlugins::default()
        };
        assert!(
            !read_and_empty.is_pending(),
            "a read empty catalog is settled, not loading"
        );
        assert!(
            catalog_description(&read_and_empty, SettingsLanguage::English).is_some(),
            "and it says so in the empty state's own words"
        );
        assert_ne!(
            bongocat_i18n::text(
                SettingsLanguage::English.catalog_locale(),
                "settings.plugins.catalog.loading"
            ),
            bongocat_i18n::text(
                SettingsLanguage::English.catalog_locale(),
                "settings.plugins.catalog.empty"
            ),
            "loading and empty are different sentences, not the same one twice"
        );

        // A refresh in flight is deliberately *not* pending: the page already has an
        // answer, and blanking a list the user is reading to show a spinner is worse
        // than a disabled Refresh button over a list that is a few seconds old.
        let reading = SettingsPlugins {
            available: true,
            busy: true,
            catalog_read: true,
            entries: vec![entry(true, true, false)],
            ..SettingsPlugins::default()
        };
        assert!(
            !reading.is_pending(),
            "a refresh keeps the previous list on screen rather than blanking it"
        );
        assert!(
            catalog_description(&reading, SettingsLanguage::English).is_none(),
            "and does not claim to be loading when it is refreshing"
        );

        let settled = SettingsPlugins {
            available: true,
            catalog_read: true,
            entries: vec![entry(true, true, false)],
            ..SettingsPlugins::default()
        };
        assert!(
            catalog_description(&settled, SettingsLanguage::English).is_none(),
            "a catalog with cards in it needs no explanation above the list"
        );
    }

    #[test]
    fn a_cards_summary_leads_with_its_failure_then_its_description() {
        let mut failing = entry(true, true, false);
        failing.failure = Some(SettingsPluginError {
            code: SettingsPluginErrorCode::PluginFailed,
            detail: None,
        });
        failing.description = "A focus timer.".to_string();
        let (text, failed) = card_summary(&failing, SettingsLanguage::English).expect("a failure");
        assert_eq!(
            text.as_ref(),
            bongocat_i18n::text(
                SettingsLanguage::English.catalog_locale(),
                "settings.plugins.error.plugin"
            ),
            "because a card that looks fine and is not running is the state a user cannot work \
             out from a sentence about its author"
        );
        assert!(
            failed,
            "and it is drawn in the text colour rather than the muted one, because that sentence \
             is the card's own news"
        );

        let described = entry(true, true, false);
        let mut described = described;
        described.description = "A focus timer.".to_string();
        assert_eq!(
            card_summary(&described, SettingsLanguage::English),
            Some(("A focus timer.".to_string().into(), false)),
        );

        // With nothing wrong and nothing to say, the version is the meta line that
        // still tells the user what they have — so a settled card is not silent, and
        // a card with neither a version nor a sentence says nothing at all.
        assert_eq!(
            card_meta(&entry(true, true, false)).as_deref(),
            Some("1.0.0")
        );
        let mut bare = entry(true, true, false);
        bare.installed_version = None;
        assert_eq!(card_meta(&bare), None);
        assert_eq!(card_summary(&bare, SettingsLanguage::English), None);
    }

    #[test]
    fn a_cards_meta_line_names_its_author_and_its_version() {
        let mut authored = entry(true, true, false);
        authored.author = "BongoCat".to_string();
        assert_eq!(card_meta(&authored).as_deref(), Some("BongoCat · 1.0.0"));

        let mut versionless = entry(false, false, false);
        versionless.author = "BongoCat".to_string();
        assert_eq!(
            card_meta(&versionless).as_deref(),
            Some("BongoCat"),
            "a plugin the catalog only offers has no version on disk, so it names its author alone \
             rather than a bullet with nothing after it"
        );
    }

    #[test]
    fn a_card_offers_its_settings_whether_or_not_the_plugin_is_running() {
        // The dead end this replaced. A plugin's schema arrives with its handshake, so a
        // switched-off plugin has no `fields` — and a card that drew its configure button
        // only when there were fields left a user who had just installed a plugin with a
        // delete button and no way to configure anything, and no way to find out why.
        // The button is unconditional now; what pressing it does is what varies.
        let stopped = entry(true, false, false);
        assert!(
            stopped.fields.is_empty(),
            "a stopped plugin has no schema yet"
        );
        assert!(
            !stopped.settings_available,
            "so there is nothing to open, and the button has to say so rather than vanish"
        );
        assert!(
            entry(true, true, false).settings_available
                || entry(true, true, false).fields.is_empty(),
            "and a running plugin with no settings has nothing to open either"
        );
    }

    #[test]
    fn a_running_plugin_with_a_schema_can_show_its_form() {
        let running = with_fields(entry(true, true, false));
        assert!(
            running.settings_available,
            "which is the state the settings button opens the form in"
        );
    }

    #[test]
    fn the_controls_a_plugin_offered_are_the_ones_its_card_draws() {
        // The label is the user's only clue to what a press will do, so a timer that
        // offers "Start" and then "Pause" arrives here as two different labels — and the
        // card draws whichever the plugin last said.
        let offered = with_actions(entry(true, true, false), "Start", SettingsActionGlyph::Play);
        assert_eq!(offered.actions.len(), 1);
        assert_eq!(offered.actions[0].label, "Start");
        assert_eq!(offered.actions[0].glyph, SettingsActionGlyph::Play);

        let paused = with_actions(
            entry(true, true, false),
            "Pause",
            SettingsActionGlyph::Pause,
        );
        assert_ne!(offered.actions[0].label, paused.actions[0].label);

        // A plugin that offered nothing is a plugin whose card shows nothing extra,
        // which is how a plugin that was never meant to be pressed from a card opts out.
        assert!(entry(true, true, false).actions.is_empty());
    }

    #[test]
    fn only_an_installed_plugin_is_running_or_stopped() {
        let locale = SettingsLanguage::English.catalog_locale();

        // The bug the screenshot showed: a list of plugins the user had not installed,
        // every card reporting "Stopped". There is no process to have stopped, so the
        // badge had no sentence to say and said the wrong one anyway.
        let uninstalled = CardState {
            installed: false,
            running: false,
        };
        let running = CardState {
            installed: true,
            running: true,
        };
        let stopped = CardState {
            installed: true,
            running: false,
        };
        assert!(
            state_badge(uninstalled, locale).is_none(),
            "a plugin that is not installed has no process, so its card says nothing about one"
        );
        assert!(
            state_badge(running, locale).is_some(),
            "an installed, running plugin does have a process to report on"
        );
        assert!(
            state_badge(stopped, locale).is_some(),
            "and so does an installed one that is not running — that is the crash a user \
             has to be able to see"
        );
    }

    #[test]
    fn a_cards_name_is_the_plugins_own_and_its_id_when_it_declared_none() {
        assert_eq!(card_heading(&entry(true, true, false)), "Pomodoro");
        let mut nameless = entry(true, true, false);
        nameless.name = String::new();
        assert_eq!(card_heading(&nameless), "pomodoro");
    }

    #[test]
    fn a_card_with_no_icon_gets_a_letter_rather_than_a_blank() {
        let bare = entry(true, true, false);
        assert!(bare.icon.is_empty());
        assert_eq!(
            bare.initial(),
            "P",
            "and a plugin that declared no icon gets a letter, because a grid of cards with a \
             blank where the icon goes is a grid of blanks"
        );

        let mut emoji = entry(true, true, false);
        emoji.icon = SettingsPluginIcon {
            emoji: Some("🍅".to_string()),
            image: None,
        };
        assert_eq!(emoji.initial(), "P");
        assert_eq!(emoji.icon.emoji.as_deref(), Some("🍅"));

        let mut image = entry(true, true, false);
        image.icon = SettingsPluginIcon {
            emoji: None,
            image: Some("icon.png".to_string()),
        };
        assert_eq!(
            image.initial(),
            "P",
            "an image falls back to the letter today, and the field it travels in is already the \
             one the host reads"
        );
    }

    #[test]
    fn every_localized_key_the_page_declares_exists_in_every_language() {
        // The declared list is the page's own contract: a key the page renders but does
        // not declare is one the catalogs can lose without anything noticing.
        for language in SettingsLanguage::ALL {
            let locale = language.catalog_locale();
            for key in PLUGINS_LOCALIZED_KEYS {
                assert!(
                    !bongocat_i18n::text(locale, key).is_empty(),
                    "{key} has no copy in {locale}"
                );
            }
        }
    }

    #[test]
    fn every_plugin_error_code_names_a_message() {
        for code in [
            SettingsPluginErrorCode::HostUnavailable,
            SettingsPluginErrorCode::NotPublished,
            SettingsPluginErrorCode::AlreadyInstalled,
            SettingsPluginErrorCode::CatalogUnavailable,
            SettingsPluginErrorCode::NetworkUnavailable,
            SettingsPluginErrorCode::ChecksumMismatch,
            SettingsPluginErrorCode::SignatureInvalid,
            SettingsPluginErrorCode::StoreWriteFailed,
            SettingsPluginErrorCode::TooManyEnabled,
            SettingsPluginErrorCode::InvalidManifest,
            SettingsPluginErrorCode::RenderFailed,
            SettingsPluginErrorCode::PluginFailed,
            SettingsPluginErrorCode::Other,
        ] {
            let key = error_key(code);
            assert!(key.starts_with("settings.plugins.error."));
            assert!(
                !bongocat_i18n::text(SettingsLanguage::English.catalog_locale(), key).is_empty(),
                "{key} has no English copy"
            );
        }
    }
}
