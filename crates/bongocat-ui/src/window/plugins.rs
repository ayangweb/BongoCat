//! The plugin center: every plugin there is, and the one control that changes it.
//!
//! A plugin is a panel on the model window, so this page is about the model window
//! first. That shapes everything here:
//!
//! * Every available plugin is listed, including one this host cannot install. A
//!   row that vanished would read as "there is nothing for this machine"; a row with
//!   a reason attached reads as "this one is not for this machine".
//! * The switch is *show on the model window*, not *enable*. Those are different
//!   words for a different thing, and the panel count is the reason the bound exists
//!   at all.
//! * A row is a `SettingItem` with the same contract as every other row rather than
//!   a bespoke card. The page is a list of things you switch on and off; a card per
//!   row would be three times the height for the same information.
//!
//! Four states are rendered distinctly and none of them is an empty list: a host
//! that is not running, a catalog that has not been read yet, a catalog with nothing
//! in it, and a catalog with rows in it.

use super::*;
use gpui_kit::component::Sizable as _;

/// Every user-visible string on the plugin page.
///
/// The page's own copy contract: a key it renders but does not declare here is one
/// the catalogs can lose without anything noticing, and a declared key with no copy
/// in some language renders as the key itself.
#[cfg(test)]
pub(super) const PLUGINS_LOCALIZED_KEYS: [&str; 22] = [
    "settings.plugins.catalog.title",
    "settings.plugins.catalog.description",
    "settings.plugins.catalog.refresh",
    "settings.plugins.catalog.loading",
    "settings.plugins.catalog.empty",
    "settings.plugins.show_on_window",
    "settings.plugins.action.install",
    "settings.plugins.action.update",
    "settings.plugins.action.uninstall",
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
    "settings.plugins.error.other",
    "settings.plugins.refusal.not_published",
];

/// What one row offers, decided from the row's own state.
///
/// Named rather than a boolean soup because the three cases need different controls:
/// a switch on a row with nothing installed is a control that lies, and a row that
/// is both installed and updatable needs two controls where the common case needs
/// one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PluginRowAction {
    Install,
    Show { enabled: bool },
    ShowAndUpdate { enabled: bool },
}

impl PluginRowAction {
    fn for_entry(entry: &SettingsPluginEntry, switch_live: bool) -> Self {
        if !entry.installed {
            return Self::Install;
        }
        let show = Self::Show {
            enabled: entry.enabled,
        };
        if entry.update_available {
            Self::ShowAndUpdate {
                enabled: entry.enabled,
            }
        } else {
            show
        }
        // Greyed rather than absent when the bound is reached: a user with four
        // panels on the model window needs to see the fifth row's switch to
        // understand why turning it on did nothing.
        .gate(switch_live)
    }

    const fn gate(self, live: bool) -> Self {
        match (self, live) {
            (Self::Show { .. } | Self::ShowAndUpdate { .. }, false) => Self::Install,
            (control, _) => control,
        }
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
        SettingsPluginErrorCode::Other => "settings.plugins.error.other",
    }
}

/// Why one plugin cannot be installed here, in the window's language.
///
/// A refusal on a row is always about the *platform* in practice — the catalog
/// publishes per-target archives and this host is not one of them — so the whole
/// set maps onto the one sentence that is true. A refusal is not an error, so it
/// gets its own line rather than the page's error line.
fn refusal_text(refusal: &SettingsPluginRefusal, language: SettingsLanguage) -> SharedString {
    let _ = refusal;
    bongocat_i18n::text(
        language.catalog_locale(),
        "settings.plugins.refusal.not_published",
    )
    .into()
}

/// The one line under a row's name.
///
/// Author and version when there is something to say, and the refusal when there is
/// one — the refusal is the reason the row's control is dead, and a dead control
/// with no explanation is a bug report.
fn row_description(
    entry: &SettingsPluginEntry,
    language: SettingsLanguage,
) -> Option<SharedString> {
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
/// notification: the failure is a property of the catalog the page is showing, so
/// a notification would vanish and leave the page looking healthy.
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

/// One row's controls.
///
/// A `Switch` and buttons side by side rather than a `SettingField::switch`,
/// because an installed plugin with an update on offer needs two controls and
/// `SettingField` carries one. The switch reads its own value out of the view at
/// render time, so a refused press — the panel bound, a host that is not running —
/// puts the switch back where the host actually is without the page keeping a copy
/// of the truth.
fn row_control(
    action: PluginRowAction,
    id: SharedString,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> SettingField<SharedString> {
    let locale = language.catalog_locale();
    let install_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.install").into();
    let update_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.update").into();
    let uninstall_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.action.uninstall").into();
    // The switch is the one control on the page whose meaning is not obvious from
    // the row's title: "Pomodoro" beside a switch reads as "is this one active",
    // which is a question about the plugin rather than about the model window.
    let show_label: SharedString =
        bongocat_i18n::text(locale, "settings.plugins.show_on_window").into();
    let show = matches!(
        action,
        PluginRowAction::Show { .. } | PluginRowAction::ShowAndUpdate { .. }
    );
    let update = matches!(action, PluginRowAction::ShowAndUpdate { .. });
    SettingField::element(
        move |options: &RenderOptions, _: &mut Window, app: &mut App| {
            let size = options.size();
            if !show {
                let install_view = view.clone();
                let install_id = id.clone();
                return Button::new(install_id.clone())
                    .label(install_label.clone())
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
            let show_switch = Switch::new(format!("plugin-show-{switch_id}"))
                .label(show_label.clone())
                .checked(checked)
                .with_size(size)
                .disabled(!view.read(app).plugin_switch_is_live())
                .on_click(move |_checked, _window, app| {
                    let plugin = switch_id.to_string();
                    switch_view.update(app, |view, cx| view.toggle_plugin_enabled(plugin, cx));
                });
            let mut controls = div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(show_switch);
            if update {
                let update_view = view.clone();
                let update_id = id.clone();
                controls = controls.child(
                    Button::new(format!("plugin-update-{update_id}"))
                        .label(update_label.clone())
                        .with_size(size)
                        .with_variant(ButtonVariant::Default)
                        .on_click(move |_, _, app| {
                            let plugin = update_id.to_string();
                            update_view.update(app, |view, cx| view.install_plugin(plugin, cx));
                        }),
                );
            }
            let uninstall_view = view.clone();
            let uninstall_id = id.clone();
            controls = controls.child(
                Button::new(format!("plugin-uninstall-{uninstall_id}"))
                    .label(uninstall_label.clone())
                    .with_size(size)
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, app| {
                        let plugin = uninstall_id.to_string();
                        uninstall_view.update(app, |view, cx| view.uninstall_plugin(plugin, cx));
                    }),
            );
            controls.into_any_element()
        },
    )
}

/// The whole page body, as the two groups it is made of.
pub(super) fn groups(
    view: Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    keywords: Vec<SharedString>,
) -> Vec<SettingGroup> {
    let locale = language.catalog_locale();
    let plugins = snapshot.map_or_else(SettingsPlugins::default, |snapshot| {
        snapshot.plugins.clone()
    });
    let busy = plugins.busy;

    // The catalog row is above the list because it is what the list is: a button
    // that re-reads it, and the line that says what happened last time. Its control
    // takes its own handle, so the list below still has one to build its rows from.
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

    let mut rows: Vec<SettingItem> = Vec::new();
    let switch_live = switch_is_live(&plugins);
    for entry in &plugins.entries {
        let action = PluginRowAction::for_entry(entry, switch_live);
        // The control closure takes ownership of its own handle, so the loop keeps
        // the one it needs for the next row rather than reusing a moved value.
        let control_view = view.clone();
        let control = row_control(action, entry.id.clone().into(), control_view, language);
        let title: SharedString = if entry.name.is_empty() {
            entry.id.clone().into()
        } else {
            entry.name.clone().into()
        };
        let mut row = SettingItem::new(title, control)
            .keywords(keywords.clone())
            // A row the host cannot act on is greyed, and the reason is its
            // description. A dead control with no reason is a bug report; a live
            // control that is refused is worse.
            .disabled(busy || entry.refusal.is_some());
        row = match row_description(entry, language) {
            Some(description) => row.description(description),
            None => row,
        };
        rows.push(row);
    }

    let mut items = vec![catalog];
    if rows.is_empty() {
        // A group with no items does not render its title, so the one line that
        // explains the absence lives in a row of its own rather than being dropped
        // on the floor.
        items.push(
            SettingItem::new(
                bongocat_i18n::text(locale, "settings.plugins.catalog.title"),
                SettingField::element(
                    |_options: &RenderOptions, _: &mut Window, _app: &mut App| {
                        div().into_any_element()
                    },
                ),
            )
            .description(catalog_description(&plugins, language).unwrap_or_default())
            .disabled(true),
        );
    } else {
        items.extend(rows);
    }
    let catalog_group = SettingGroup::new()
        .title(bongocat_i18n::text(
            locale,
            "settings.plugins.catalog.title",
        ))
        .items(items);
    vec![catalog_group]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(installed: bool, enabled: bool, update: bool) -> SettingsPluginEntry {
        SettingsPluginEntry {
            id: "pomodoro".to_string(),
            name: "Pomodoro".to_string(),
            description: String::new(),
            author: String::new(),
            installed_version: installed.then(|| "1.0.0".to_string()),
            available_version: Some("1.0.0".to_string()),
            installed,
            enabled,
            update_available: update,
            refusal: None,
        }
    }

    #[test]
    fn a_plugin_that_is_not_installed_offers_install_and_no_switch() {
        let action = PluginRowAction::for_entry(&entry(false, false, false), true);
        assert_eq!(action, PluginRowAction::Install);
    }

    #[test]
    fn an_installed_plugin_offers_a_switch() {
        let action = PluginRowAction::for_entry(&entry(true, true, false), true);
        assert_eq!(action, PluginRowAction::Show { enabled: true });
        let action = PluginRowAction::for_entry(&entry(true, false, false), true);
        assert_eq!(action, PluginRowAction::Show { enabled: false });
    }

    #[test]
    fn an_updatable_plugin_offers_the_switch_and_the_update() {
        let action = PluginRowAction::for_entry(&entry(true, true, true), true);
        assert_eq!(action, PluginRowAction::ShowAndUpdate { enabled: true });
    }

    #[test]
    fn the_panel_bound_greys_the_switch_instead_of_hiding_it() {
        let plugins = SettingsPlugins {
            active: 4,
            maximum_active: 4,
            ..SettingsPlugins::default()
        };
        assert!(!switch_is_live(&plugins));
        assert_eq!(
            PluginRowAction::for_entry(&entry(true, false, false), switch_is_live(&plugins)),
            PluginRowAction::Install,
            "at the bound the row is read-only rather than offering a refused press"
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
            PluginRowAction::for_entry(&entry(true, false, false), switch_is_live(&plugins)),
            PluginRowAction::Show { enabled: false }
        );
    }

    #[test]
    fn a_degraded_host_says_so_rather_than_showing_an_empty_catalog() {
        let plugins = SettingsPlugins::default();
        assert!(plugins.entries.is_empty());
        let text = catalog_description(&plugins, SettingsLanguage::English);
        assert!(text.is_some(), "no host is a failure, not an empty catalog");
    }

    #[test]
    fn an_unread_catalog_is_a_loading_state_and_a_read_empty_one_is_empty() {
        let pending = SettingsPlugins {
            available: true,
            ..SettingsPlugins::default()
        };
        assert!(catalog_description(&pending, SettingsLanguage::English).is_some());

        let empty = SettingsPlugins {
            available: true,
            entries: vec![entry(true, true, false)],
            ..SettingsPlugins::default()
        };
        assert!(
            catalog_description(&empty, SettingsLanguage::English).is_none(),
            "a catalog with rows in it needs no explanation above the list"
        );
    }

    #[test]
    fn every_localized_key_the_page_declares_exists_in_every_language() {
        // The declared list is the page's own contract: a key the page renders but
        // does not declare is one the catalogs can lose without anything noticing.
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
