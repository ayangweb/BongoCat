//! The plugin center: a responsive card grid, package lifecycle controls, and
//! one configuration panel per plugin.
//!
//! The cards are laid out the way the model library lays out its catalog — the
//! same window-width-to-column-count rule, the same scrolling grid, the same
//! per-row template — so two catalogs in one window do not disagree about what a
//! catalog looks like. What a card carries is deliberately different: a plugin's
//! published view tree is the only thing that can say what the plugin does, so
//! the center renders that tree rather than inventing controls for it.
//!
//! Business rules stay in the plugin process. This page reads the center's
//! cached state, draws it, and forwards typed commands.
use super::*;
use bongocat_plugin_sdk::{ActionIcon, ConfigValue, UiNode};
use gpui_kit::component::{Sizable as _, skeleton::Skeleton, tag::Tag};
use gpui_kit::{AnyElement, assets::IconName, base::TestSupportExt as _};
use plugins_host::{PluginCardView, PluginHubState, PluginInFlight, PluginPackageOperation};

/// The narrowest column the plugin grid allows.
///
/// It is wider than the model grid's floor because a plugin card carries a
/// title, a version line, a description and a footer of controls, where a model
/// card is mostly cover: at the model floor a plugin card's footer would wrap
/// onto three rows on every card.
pub(super) const PLUGIN_CARD_MIN_WIDTH: f32 = 300.0;
pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    language: SettingsLanguage,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let state: PluginHubState = view.plugin_host.snapshot();
    let locale = language.catalog_locale();
    let handle = cx.entity();
    let update_view = handle.clone();
    let update_all = Button::new("plugin-update-all")
        .primary()
        .label(bongocat_i18n::text(locale, "plugins.update_all"))
        .loading(state.updating_all())
        .disabled(!state.can_update_all())
        .on_click(move |_, _, cx| {
            update_view.update(cx, |view, cx| view.update_all_plugins(cx));
        });
    // The batch control is drawn only when an update is waiting. A page with
    // nothing newer than what is installed cannot enable it by any later state,
    // whereas loading, refreshing and a running package operation are temporary
    // and keep the button on screen, disabled, so the user can still read why.
    let tools = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_3()
        .children(state.has_updates().then_some(update_all));
    let mut column = div()
        .flex()
        .flex_col()
        .gap_4()
        .w_full()
        .children(state.has_updates().then_some(tools));
    if state.loading && state.cards.is_empty() {
        column = column.child(
            div()
                .text_sm()
                .child(bongocat_i18n::text(locale, "plugins.loading")),
        );
    } else if state.cards.is_empty() {
        column = column.child(
            div()
                .p_4()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .text_sm()
                .child(bongocat_i18n::text(locale, "navigation.plugins.empty")),
        );
    }
    let cards = state
        .cards
        .iter()
        .map(|card| card_view(card, &state, handle.clone(), locale, cx).into_any_element())
        .collect::<Vec<_>>();
    column
        .child(models::grid_with_id(
            "plugin-catalog",
            models::grid_columns_for_window(window.viewport_size().width, PLUGIN_CARD_MIN_WIDTH),
            cards,
        ))
        .into_any_element()
}

/// The glyph the host draws for one plugin-declared action.
///
/// The protocol's icon set is closed and the plugin owns it, so this is a
/// translation table rather than a guess: the host never derives an icon from an
/// action id or a translated label. Only artwork the bundled component assets
/// ship is used, so a button never depends on a font glyph for its meaning.
pub(super) fn action_icon(icon: ActionIcon) -> IconName {
    match icon {
        ActionIcon::Play => IconName::Play,
        ActionIcon::Pause => IconName::Pause,
        ActionIcon::Restart => IconName::RefreshCw,
        ActionIcon::Volume => IconName::AudioLines,
    }
}

fn card_view(
    card: &PluginCardView,
    state: &PluginHubState,
    view: Entity<SettingsView>,
    locale: &'static str,
    cx: &App,
) -> impl IntoElement {
    // The center is busy only while it has nothing to show: what a plugin is
    // still publishing is the plugin's own state, not the catalog's.
    let install_pending =
        state.pending_package(&card.id, PluginPackageOperation::Install) || card.installing;
    let busy = (state.loading && state.cards.is_empty())
        || install_pending
        || state.refreshing()
        || state.updating_all();
    let installed = card.version.is_some();
    let enabled = card.enabled;
    let (name, description) = card.copy(locale);
    let (name, description) = (name.to_owned(), description.to_owned());
    let mut heading = div()
        .min_w_0()
        .flex_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().min_w_0().font_semibold().child(name.clone()));
    if let Some(version) = card.version.clone() {
        heading =
            heading.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(bongocat_i18n::format_text(
                        locale,
                        "plugins.version",
                        &[("version", version)],
                    ))
                    .when(installed && card.update_available, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!(
                                    "plugin-update-badge-{}",
                                    card.id
                                )))
                                .test_support()
                                .text_xs()
                                .child(Tag::info().outline().small().child(bongocat_i18n::text(
                                    locale,
                                    "plugins.update_available",
                                ))),
                        )
                    }),
            );
    }
    if let Some(version) = card
        .available_version
        .clone()
        .filter(|_| !installed || card.update_available)
    {
        heading = heading.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(bongocat_i18n::format_text(
                    locale,
                    "plugins.available_version",
                    &[("version", version)],
                )),
        );
    }
    // Every installed, running plugin offers one settings entry, and it is drawn
    // the same way on every card: a gear beside the identity it configures,
    // rather than one more button in the row of package lifecycle commands.
    let settings_entry = (installed && enabled && !card.failed).then(|| {
        let target = view.clone();
        let id = card.id.clone();
        Button::new(SharedString::from(format!("plugin-settings-{}", card.id)))
            .icon(IconName::Settings)
            .tooltip(bongocat_i18n::text(locale, "plugins.settings.open"))
            .disabled(busy)
            .on_click(move |_, window, cx| {
                let id = id.clone();
                target.update(cx, |view, cx| view.open_plugin_settings(&id, window, cx));
            })
    });
    let body = div()
        .id(SharedString::from(format!("plugin-card-{}", card.id)))
        .test_support()
        .w_full()
        .min_w_0()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(plugin_icon(card, cx))
                .child(heading)
                .children(settings_entry),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        );
    let mut actions = div()
        .id(SharedString::from(format!("plugin-actions-{}", card.id)))
        .test_support()
        .min_w_0()
        .flex_1()
        .flex_basis(gpui_kit::Length::Auto)
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_2();
    // The plugin's own model-window controls stay reachable from its card: the
    // same typed command the model window sends, carrying the plugin's own label
    // as its tooltip and the plugin's own glyph on the button.
    if enabled {
        for node in &card.actions {
            let UiNode::Button { id, label, .. } = node else {
                continue;
            };
            actions = actions.child(action_button(
                node,
                id,
                label,
                &card.id,
                state,
                view.clone(),
                busy,
            ));
        }
    }
    if !installed || card.update_available {
        let install_view = view.clone();
        let id = card.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("plugin-install-{}", card.id)))
                .icon(if installed {
                    IconName::RefreshCw
                } else {
                    IconName::Download
                })
                .tooltip(bongocat_i18n::text(
                    locale,
                    if installed {
                        "plugins.update"
                    } else {
                        "plugins.install"
                    },
                ))
                .loading(install_pending)
                .disabled(busy || state.updating_all())
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    install_view.update(cx, |view, cx| view.install_plugin_available(&id, cx));
                }),
        );
    }
    let mut footer = div()
        .id(SharedString::from(format!("plugin-footer-{}", card.id)))
        .test_support()
        .mt_auto()
        .pt_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .min_w_0()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_3();
    if installed {
        // A switch cannot spin, so the command it starts holds it inert while it
        // runs and its tooltip keeps naming what the next click would do.
        let toggle_pending = state.pending_package(&card.id, PluginPackageOperation::Enable);
        let toggle_view = view.clone();
        let toggle_id = card.id.clone();
        footer = footer.child(
            Switch::new(SharedString::from(format!("plugin-toggle-{}", card.id)))
                .flex_shrink_0()
                .checked(enabled)
                .disabled(busy || toggle_pending)
                .tooltip(bongocat_i18n::text(
                    locale,
                    if enabled {
                        "plugins.disable"
                    } else {
                        "plugins.enable"
                    },
                ))
                .on_click(move |checked, _, cx| {
                    let id = toggle_id.clone();
                    let enabled = *checked;
                    toggle_view.update(cx, |view, cx| {
                        view.set_plugin_enabled(&id, enabled, cx);
                    });
                }),
        );
        let uninstall_pending = state.pending_package(&card.id, PluginPackageOperation::Uninstall);
        let uninstall_view = view.clone();
        let uninstall_id = card.id.clone();
        actions = actions.child(
            PopConfirm::new(
                SharedString::from(format!("plugin-uninstall-confirmation-{}", card.id)),
                bongocat_i18n::format_text(locale, "plugins.uninstall_confirm", &[("name", name)]),
            )
            .anchor(Anchor::BottomRight)
            .arrow(true)
            .icon(IconName::TriangleAlert, cx.theme().danger)
            .confirm_label(bongocat_i18n::text(locale, "actions.confirm"))
            .cancel_label(bongocat_i18n::text(locale, "actions.cancel"))
            .trigger(
                Button::new(SharedString::from(format!("plugin-uninstall-{}", card.id)))
                    .danger()
                    .outline()
                    .icon(IconName::Trash)
                    .tooltip(bongocat_i18n::text(locale, "plugins.uninstall"))
                    .loading(uninstall_pending)
                    .disabled(busy || uninstall_pending),
            )
            .on_confirm(move |_, cx| {
                uninstall_view.update(cx, |view, cx| {
                    view.uninstall_plugin(&uninstall_id, cx);
                });
            }),
        );
    }
    body.child(footer.child(actions))
}

/// A plugin icon occupies one fixed square without changing its aspect ratio.
///
/// The catalog supplies a URL in Production and a file path in Development.
/// Both identify the same PNG and neither depends on whether the package is
/// installed. A malformed or missing source shows a static neutral square
/// rather than substituting a font glyph.
fn plugin_icon(card: &PluginCardView, cx: &App) -> AnyElement {
    let frame = div().size_12().flex_shrink_0();
    let frame_id = SharedString::from(format!("plugin-icon-{}", card.id));
    match card.icon_url.clone() {
        Some(url) => {
            // `img(String)` treats a non-URL as an embedded asset. Development
            // catalogs name a local path, which GPUI can only read as one when
            // the source is a `PathBuf`.
            let source = if url.starts_with("https://") {
                ImageSource::from(url)
            } else {
                ImageSource::from(PathBuf::from(url))
            };
            let image_id = SharedString::from(format!("plugin-icon-image-{}", card.id));
            let loading_id = SharedString::from(format!("plugin-icon-loading-{}", card.id));
            let fallback_id = SharedString::from(format!("plugin-icon-fallback-{}", card.id));
            let fallback_color = cx.theme().muted;
            frame
                .id(frame_id)
                .test_support()
                .child(
                    img(source)
                        .id(image_id)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_loading(move || {
                            div()
                                .id(loading_id.clone())
                                .test_support()
                                .size_12()
                                .child(Skeleton::new().size_full().rounded_md())
                                .into_any_element()
                        })
                        .with_fallback(move || {
                            div()
                                .id(fallback_id.clone())
                                .test_support()
                                .size_12()
                                .rounded_md()
                                .bg(fallback_color)
                                .into_any_element()
                        }),
                )
                .into_any_element()
        }
        None => frame
            .id(frame_id)
            .test_support()
            .child(
                div()
                    .id(SharedString::from(format!(
                        "plugin-icon-fallback-{}",
                        card.id
                    )))
                    .test_support()
                    .size_12()
                    .rounded_md()
                    .bg(cx.theme().muted),
            )
            .into_any_element(),
    }
}

/// One plugin-published model-window control, as the card shows it.
///
/// The button the plugin published and the button the card draws are one command
/// with one label; only the surface differs. Its glyph and its in-flight state
/// both come from the plugin, so a plugin that reports an audition still running
/// shows its button carrying the action rather than a line of placeholder text
/// beside it.
fn action_button(
    node: &UiNode,
    id: &str,
    label: &str,
    plugin: &str,
    state: &PluginHubState,
    view: Entity<SettingsView>,
    busy: bool,
) -> AnyElement {
    let UiNode::Button { icon, loading, .. } = node else {
        return div().into_any_element();
    };
    let pending = *loading || state.pending_action(plugin, id);
    let target = view.clone();
    let element_id = SharedString::from(format!("plugin-action-{plugin}-{id}"));
    let plugin = plugin.to_owned();
    let id = id.to_owned();
    Button::new(element_id)
        .icon(action_icon(*icon))
        .tooltip(label.to_owned())
        .loading(pending)
        .disabled(busy || pending)
        .on_click(move |_, _, cx| {
            let plugin = plugin.clone();
            let id = id.clone();
            target.update(cx, |view, cx| view.run_plugin_action(&plugin, &id, cx));
        })
        .into_any_element()
}

impl SettingsView {
    /// Send one plugin-published action.
    ///
    /// The command is marked on this thread before it is handed to the executor,
    /// because nothing between the click and the reply would otherwise redraw:
    /// the button a user pressed would jump from idle to idle and never show that
    /// it was working. The plugin's own `loading` flag covers the work the plugin
    /// keeps doing after the reply — an audition that is still playing — so the
    /// two cover the whole life of the action.
    pub(super) fn run_plugin_action(&mut self, plugin: &str, action: &str, cx: &mut Context<Self>) {
        self.plugin_host
            .begin(PluginInFlight::action(plugin, action));
        let (plugin, action) = (plugin.to_owned(), action.to_owned());
        self.update_plugin_host(move |host| host.action(&plugin, &action), cx);
        cx.notify();
    }

    /// Send one configuration write.
    pub(super) fn configure_plugin(
        &mut self,
        plugin: &str,
        key: &str,
        value: ConfigValue,
        cx: &mut Context<Self>,
    ) {
        self.plugin_host
            .begin(PluginInFlight::configure(plugin, key));
        let (plugin, key) = (plugin.to_owned(), key.to_owned());
        self.update_plugin_host(move |host| host.configure(&plugin, &key, value), cx);
        cx.notify();
    }

    /// Turn one plugin on or off.
    pub(super) fn set_plugin_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        self.plugin_host
            .begin(PluginInFlight::package(id, PluginPackageOperation::Enable));
        let id = id.to_owned();
        self.update_plugin_host(move |host| host.set_enabled(&id, enabled), cx);
        cx.notify();
    }

    /// Install or update one entry the catalog offers.
    pub(super) fn install_plugin_available(&mut self, id: &str, cx: &mut Context<Self>) {
        let state = self.plugin_host.snapshot();
        let already_installing = state
            .cards
            .iter()
            .find(|card| card.id == id)
            .is_some_and(|card| card.installing);
        if state.updating_all()
            || already_installing
            || state.pending_package(id, PluginPackageOperation::Install)
        {
            return;
        }
        self.plugin_host
            .begin(PluginInFlight::package(id, PluginPackageOperation::Install));
        let id = id.to_owned();
        self.update_plugin_host(
            move |host| {
                host.install_available(&id);
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn update_all_plugins(&mut self, cx: &mut Context<Self>) {
        let targets = self.plugin_host.begin_update_all();
        if targets.is_empty() {
            return;
        }
        self.update_plugin_host(move |host| host.update_all(targets), cx);
        cx.notify();
    }

    /// Remove one plugin's package, keeping its data.
    pub(super) fn uninstall_plugin(&mut self, id: &str, cx: &mut Context<Self>) {
        self.plugin_host.begin(PluginInFlight::package(
            id,
            PluginPackageOperation::Uninstall,
        ));
        let id = id.to_owned();
        self.update_plugin_host(move |host| host.uninstall(&id), cx);
        cx.notify();
    }
}
