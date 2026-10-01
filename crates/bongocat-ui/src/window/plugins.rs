//! The plugin center: a card per plugin, and the settings the plugin declared.
//!
//! A plugin is a program with its own logic, its own state and its own settings, and
//! this page is where the product meets that. Three decisions shape everything here,
//! and each one exists because the alternative was tried:
//!
//! * **A card, not a row.** A plugin is a thing a user *installs*, and an install list
//!   of one-line rows reads as a list of switches. A card carries an icon, a name, a
//!   version, a sentence, whether its process is alive, and its own controls.
//! * **An icon the plugin declares.** An emoji today and a PNG beside it tomorrow,
//!   drawn by the host either way. The plugin says *what the icon is*; the host draws
//!   it, which is the same division the panel follows.
//! * **A settings form built from a schema.** Every control in
//!   [`super::plugin_settings`] is chosen by a field's `kind`, not by the plugin's
//!   name. Nothing on this page knows what a pomodoro is, and adding a plugin that
//!   wants a switch, a number, a line of text and a menu adds no code to the window.
//!   The form expands under its own card rather than opening somewhere else, so the
//!   card stays a card and the user can see the rest of the page beside it.
//!
//! Four states are rendered distinctly and none of them is an empty list: a host that
//! is not running, a catalog that has not been read yet, a catalog with nothing in it,
//! and a catalog with cards in it.

use super::*;
use gpui_kit::AnyElement;
use gpui_kit::component::Sizable as _;

/// Every user-visible string on the plugin page and in its settings dialog.
///
/// The page's own copy contract: a key it renders but does not declare here is one the
/// catalogs can lose without anything noticing, and a declared key with no copy in
/// some language renders as the key itself.
#[cfg(test)]
pub(super) const PLUGINS_LOCALIZED_KEYS: [&str; 26] = [
    "settings.plugins.catalog.title",
    "settings.plugins.catalog.description",
    "settings.plugins.catalog.refresh",
    "settings.plugins.catalog.loading",
    "settings.plugins.catalog.empty",
    "settings.plugins.show_on_window",
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

/// What one card offers, decided from the card's own state.
///
/// Two variants and three facts rather than four variants: the facts are independent
/// — installed, updatable, pressable — and a variant per combination would be eight
/// arms of which the compiler can prove four unreachable. An earlier shape had one
/// variant for "installed" and a separate one for "installed but at the panel bound",
/// which could not say "installed, updatable *and* at the bound" and therefore dropped
/// the update button on exactly the card that needed it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CardAction {
    /// Nothing is installed, so the card offers the one action that changes that.
    ///
    /// The panel bound does not apply here, and deliberately: installing a plugin does
    /// not show its panel until the switch is turned on, so refusing the install would
    /// leave a user at the bound with no way to acquire a plugin at all.
    Install,
    Show {
        enabled: bool,
        /// Whether the catalog offers something newer.
        update: bool,
        /// Whether the switch accepts a press.
        ///
        /// False at the panel bound. The switch stays on screen and is marked
        /// unpressable rather than being replaced by an install button, because the card
        /// describes a plugin that *is* installed and an install button on it would
        /// describe a different action.
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
    /// The card reads the switch's own state out of the view rather than from this,
    /// because the host is the only side that knows it — a refused press moves the
    /// host's answer and the switch follows on the next render. This is what a *test*
    /// reads, which is why it exists rather than being a field.
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
/// report and this page cannot name is a compile error here rather than a blank line in
/// a user's settings window.
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
/// publishes per-target archives and this host is not one of them — so the whole set
/// maps onto the one sentence that is true. A refusal is not an error, so it gets its
/// own line rather than the page's error line.
fn refusal_text(_refusal: &SettingsPluginRefusal, language: SettingsLanguage) -> SharedString {
    bongocat_i18n::text(
        language.catalog_locale(),
        "settings.plugins.refusal.not_published",
    )
    .into()
}

/// A card's title, with the icon the plugin declared in front of it.
///
/// Emoji today and a PNG beside it tomorrow, and the title is where both land: it is
/// the one piece of a card that is already a label, so an icon costs no new component
/// and a plugin that ships a picture is one replacement of this function rather than a
/// change to the page. A plugin that declared neither gets a letter, because a grid of
/// cards with a blank where the icon goes is a grid of blanks.
fn card_title(entry: &SettingsPluginEntry) -> SharedString {
    let name = if entry.name.is_empty() {
        entry.id.as_str()
    } else {
        entry.name.as_str()
    };
    if let Some(emoji) = &entry.icon.emoji {
        return format!("{emoji} {name}").into();
    }
    format!("{} {name}", entry.initial()).into()
}

/// The one line under a card's name.
///
/// The failure first, then the refusal, then the description, then author and version.
/// The failure leads because a card that looks fine and is not running is the state a
/// user cannot work out from a sentence about its author.
fn card_description(
    entry: &SettingsPluginEntry,
    language: SettingsLanguage,
) -> Option<SharedString> {
    let locale = language.catalog_locale();
    if let Some(failure) = &entry.failure {
        return Some(bongocat_i18n::text(locale, error_key(failure.code)).into());
    }
    if let Some(refusal) = &entry.refusal {
        return Some(refusal_text(refusal, language));
    }
    if !entry.description.is_empty() {
        return Some(entry.description.clone().into());
    }
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

/// The page's own line: the catalog's state, or the last failure.
///
/// A plugin operation that failed leaves its reason here rather than in a
/// notification: the failure is a property of the catalog the page is showing, so a
/// notification would vanish and leave the page looking healthy.
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

/// One card's controls, as one element.
///
/// Built inside the card's own render rather than as a `SettingField`, because a card
/// is not a settings *row*: its controls sit beside its state rather than beside a
/// title, and a `SettingField` can only ever render beside a title. The switch reads
/// its own value out of the view at render time, so a refused press — the panel bound,
/// a host that is not running — puts the switch back where the host actually is
/// without the page keeping a copy of the truth.
#[allow(clippy::too_many_arguments)]
fn card_control(
    options: &RenderOptions,
    action: CardAction,
    configurable: bool,
    id: SharedString,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
    _window: &mut Window,
    app: &mut App,
) -> AnyElement {
    let locale = language.catalog_locale();
    let size = options.size();
    let install_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.install").into();
    let update_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.update").into();
    let uninstall_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.uninstall").into();
    let configure_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.configure").into();
    // The switch is the one control whose meaning is not obvious from the card's
    // title: "Pomodoro" beside a switch reads as "is this one active", which is a
    // question about the plugin rather than about the model window.
    let show_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.show_on_window").into();

    if !action.shows_switch() {
        let install_view = view.clone();
        let install_id = id.clone();
        return Button::new(install_id.clone())
            .label(install_label)
            .with_size(size)
            .with_variant(ButtonVariant::Default)
            .on_click(move |_, _, app| {
                let plugin = install_id.to_string();
                install_view.update(app, |view, cx| view.install_plugin(plugin, cx));
            })
            .into_any_element();
    }

    let checked = view.read(app).plugin_is_enabled(id.as_ref());
    let switch_view = view.clone();
    let switch_id = id.clone();
    let mut controls = div().flex().flex_row().items_center().gap_2().child(
        Switch::new(format!("plugin-show-{switch_id}"))
            .label(show_label)
            .checked(checked)
            .with_size(size)
            .disabled(!action.switch_is_live())
            .on_click(move |_checked, _window, app| {
                let plugin = switch_id.to_string();
                switch_view.update(app, |view, cx| view.toggle_plugin_enabled(plugin, cx));
            }),
    );
    if action.shows_update() {
        let update_view = view.clone();
        let update_id = id.clone();
        controls = controls.child(
            Button::new(format!("plugin-update-{update_id}"))
                .label(update_label)
                .with_size(size)
                .with_variant(ButtonVariant::Default)
                .on_click(move |_, _, app| {
                    let plugin = update_id.to_string();
                    update_view.update(app, |view, cx| view.install_plugin(plugin, cx));
                }),
        );
    }
    // A plugin with settings offers a button that opens its own form, which is a
    // second thing beside the switch rather than a control in place of it: a plugin
    // that is switched off still has settings worth changing.
    if configurable {
        let configure_view = view.clone();
        let configure_id = id.clone();
        controls = controls.child(
            Button::new(format!("plugin-configure-{configure_id}"))
                .label(configure_label)
                .with_size(size)
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _window, app| {
                    let plugin = configure_id.to_string();
                    configure_view.update(app, |view, cx| {
                        view.toggle_plugin_settings(plugin, cx);
                    });
                }),
        );
    }
    let uninstall_view = view.clone();
    let uninstall_id = id.clone();
    controls
        .child(
            Button::new(format!("plugin-uninstall-{uninstall_id}"))
                .label(uninstall_label)
                .with_size(size)
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _, app| {
                    let plugin = uninstall_id.to_string();
                    uninstall_view.update(app, |view, cx| view.uninstall_plugin(plugin, cx));
                }),
        )
        .into_any_element()
}

/// The two facts a card's state badge is decided from.
///
/// A copy rather than a reference into the entry, because the badge is rebuilt on
/// every render and a rendered element cannot be built once and kept. Two bools is
/// the whole of it, and it is deliberately not the whole entry: anything else a card
/// shows is read where the row is built, where the view is not already borrowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CardState {
    installed: bool,
    running: bool,
}

/// The badge that says whether a plugin's process is alive, or that there is none.
///
/// Separate from the switch because the two answer different questions: the switch is
/// what the user asked for and the badge is what happened. A card whose plugin is
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

/// One card's body: its state on the left and its own controls on the right.
fn card_body(
    entry: &SettingsPluginEntry,
    action: CardAction,
    keywords: Vec<SharedString>,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> SettingItem {
    let locale = language.catalog_locale();
    // Read before any row is built, and kept as the two facts rather than as a
    // rendered element: a card's closure runs again on every render, and a rendered
    // element cannot be built once and reused. Which badge this is — none, "running",
    // "stopped" — is therefore decided from these two inside the closure, which is
    // one cheap comparison rather than a second source of truth.
    let state = CardState {
        installed: entry.installed,
        running: entry.running,
    };
    let id: SharedString = entry.id.clone().into();
    // A plugin with settings offers a button that opens its own form, which is a
    // second thing beside the switch rather than a control in place of it: a plugin
    // that is switched off still has settings worth changing.
    let configurable = entry.installed && !entry.fields.is_empty();
    // A whole card in one item, because a card *is* the unit: a title, a sentence, a
    // state and its own controls. Splitting it across a group's rows would put the
    // controls a screen away from the name they belong to.
    SettingItem::render(
        move |options: &RenderOptions, window: &mut Window, app: &mut App| {
            div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .justify_between()
                // The badge, or a growing spacer in its place: without it the row's
                // `justify_between` would put the controls on the *left*, so an
                // install button would sit under the title instead of under the
                // install button on the card below it.
                .child(
                    state_badge(state, locale).unwrap_or_else(|| div().flex_1().into_any_element()),
                )
                .child(card_control(
                    options,
                    action,
                    configurable,
                    id.clone(),
                    view.clone(),
                    language,
                    window,
                    app,
                ))
                .into_any_element()
        },
    )
    .keywords(keywords)
    .disabled(false)
}

/// The whole page body: the catalog's own row, then a card per plugin.
/// `expanded` is which plugin's settings are open, read by the caller.
///
/// A parameter rather than a view read because a card's render closure runs while the
/// view is already borrowed: the one thing the page needs from the view has to be
/// decided before any row is built, and a card that asked for it would be asking the
/// entity it belongs to from inside its own render.
#[allow(clippy::too_many_arguments)]
pub(super) fn groups(
    view: Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    keywords: Vec<SharedString>,
    expanded: Option<String>,
) -> Vec<SettingGroup> {
    let locale = language.catalog_locale();
    let plugins = snapshot.map_or_else(SettingsPlugins::default, |snapshot| {
        snapshot.plugins.clone()
    });
    let busy = plugins.busy;
    // Read once, before any row is built: a card's render closure runs while the view
    // is already borrowed, so the one thing it needs from the view has to be decided
    // here rather than asked for inside it.

    // The catalog row is above the cards because it is what they are: a button that
    // re-reads the catalog, and the line that says what happened last time. Its
    // control takes its own handle, so the cards below still have one to build from.
    let catalog_view = view.clone();
    let catalog = SettingItem::new(
        bongocat_i18n::text(locale, "settings.plugins.catalog.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _app: &mut App| {
                let view = catalog_view.clone();
                Button::new("plugin-catalog-refresh")
                    .label(bongocat_i18n::text(
                        locale,
                        "settings.plugins.catalog.refresh",
                    ))
                    .with_size(options.size())
                    .with_variant(ButtonVariant::Default)
                    .disabled(busy)
                    .on_click(move |_, _, app| {
                        view.update(app, |view, cx| view.refresh_plugin_catalog(cx));
                    })
            },
        ),
    )
    .description(catalog_description(&plugins, language).unwrap_or_else(|| {
        bongocat_i18n::text(locale, "settings.plugins.catalog.description").into()
    }))
    .keywords(keywords.clone());

    // The catalog row carries the whole state — loading, empty, failed, ready — in its
    // description, and the cards follow it when there are any. An empty state is
    // therefore that one row and no more: a second row repeating the catalog's own
    // title and description would render the same sentence twice and make a settled
    // page look like two things happened.
    let mut groups = vec![
        SettingGroup::new()
            .title(bongocat_i18n::text(
                locale,
                "settings.plugins.catalog.title",
            ))
            .items(vec![catalog]),
    ];

    // One group per plugin, so a card is a card: its own surface, its own title with
    // its icon, its own sentence and its own controls. A single group with every card
    // in it would read as one long list, which is what this page stopped being.
    let switch_live = switch_is_live(&plugins);
    for entry in &plugins.entries {
        let action = CardAction::for_entry(entry, switch_live);
        // The control closure takes ownership of its own handle, so the loop keeps the
        // one it needs for the next card rather than reusing a moved value.
        let card_view = view.clone();
        let card = card_body(entry, action, keywords.clone(), card_view, language);
        let description = card_description(entry, language);
        let mut group = SettingGroup::new()
            .title(card_title(entry))
            .variant(GroupBoxVariant::Outline);
        if let Some(description) = description {
            group = group.description(description);
        }
        let mut items = vec![card];
        // The plugin's own settings, expanded under the card that owns them.
        //
        // Which plugin is expanded is the *view's* state rather than this function's,
        // because a card cannot read the view while it is being rendered — so the rows
        // are chosen here from a flag the render closure was handed, and the flag
        // itself comes from the draft the view holds.
        if expanded.as_deref() == Some(entry.id.as_str()) && !entry.fields.is_empty() {
            items.extend(plugin_settings::field_rows(entry, view.clone()));
        }
        groups.push(group.items(items));
    }
    groups
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
            values: Default::default(),
            log: Vec::new(),
            refusal: None,
            failure: None,
        }
    }

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
    fn a_cards_description_leads_with_its_failure_then_its_description() {
        let mut failing = entry(true, true, false);
        failing.failure = Some(SettingsPluginError {
            code: SettingsPluginErrorCode::PluginFailed,
            detail: None,
        });
        failing.description = "A focus timer.".to_string();
        assert_eq!(
            card_description(&failing, SettingsLanguage::English).as_deref(),
            Some(bongocat_i18n::text(
                SettingsLanguage::English.catalog_locale(),
                "settings.plugins.error.plugin"
            )),
            "because a card that looks fine and is not running is the state a user cannot work \
             out from a sentence about its author"
        );
        // With nothing wrong and nothing to say, the version is the one line that
        // still tells the user what they have — so a settled card is not silent, and a
        // card with neither a version nor a failure says nothing at all.
        assert_eq!(
            card_description(&entry(true, true, false), SettingsLanguage::English).as_deref(),
            Some("1.0.0")
        );
        let mut bare = entry(true, true, false);
        bare.installed_version = None;
        assert_eq!(card_description(&bare, SettingsLanguage::English), None);
    }

    #[test]
    fn a_card_with_settings_is_configurable_and_one_without_is_not() {
        let with = with_fields(entry(true, true, false));
        assert!(
            !with.fields.is_empty(),
            "so its card offers the configure button"
        );
        assert!(entry(true, true, false).fields.is_empty());
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
    fn a_cards_title_carries_the_icon_the_plugin_declared() {
        let mut emoji = entry(true, true, false);
        emoji.icon = SettingsPluginIcon {
            emoji: Some("🍅".to_string()),
            image: None,
        };
        assert_eq!(card_title(&emoji), "🍅 Pomodoro");

        let bare = entry(true, true, false);
        assert!(bare.icon.is_empty());
        assert_eq!(
            card_title(&bare),
            "P Pomodoro",
            "and a plugin that declared no icon gets a letter, because a grid of cards with a \\
             blank where the icon goes is a grid of blanks"
        );

        let mut image = entry(true, true, false);
        image.icon = SettingsPluginIcon {
            emoji: None,
            image: Some("icon.png".to_string()),
        };
        assert_eq!(
            card_title(&image),
            "P Pomodoro",
            "an image falls back to the letter today, and the field it travels in is already the \\
             one the host reads"
        );
    }

    #[test]
    fn a_card_with_no_name_uses_its_id() {
        let mut nameless = entry(true, true, false);
        nameless.name = String::new();
        assert_eq!(card_title(&nameless), "P pomodoro");
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
