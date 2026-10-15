//! The plugin center: cards, lifecycle controls and per-plugin configuration.
//!
//! Business rules live in the plugin process; this page only renders the view
//! tree each plugin publishes and forwards typed commands to the host cache.
use super::*;
use bongocat_plugin_sdk::ConfigValue;
use gpui_kit::{AnyElement, assets::IconName, base::TestSupportExt as _};
use plugins_host::{PluginCardView, PluginHubState};

pub(super) fn content(
    view: &mut SettingsView,
    language: SettingsLanguage,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let state: PluginHubState = view.plugin_host.snapshot();
    let handle = cx.entity();
    let refresh_view = handle.clone();
    let mut column = div().flex().flex_col().gap_4().w_full().child(
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(bongocat_i18n::text(
                        locale(language),
                        "navigation.plugins.subtitle",
                    )),
            )
            .child(
                Button::new("plugin-refresh")
                    .icon(IconName::RefreshCw)
                    .tooltip(bongocat_i18n::text(locale(language), "plugins.refresh"))
                    .loading(state.loading)
                    .disabled(state.loading)
                    .on_click(move |_, _, cx| {
                        refresh_view.update(cx, |view, cx| {
                            view.update_plugin_host(|host| host.rescan(), cx)
                        });
                    }),
            ),
    );
    if state.error.is_some() {
        let retry_view = handle.clone();
        column = column.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .child(bongocat_i18n::text(
                            locale(language),
                            "plugins.discovery_failed",
                        )),
                )
                .child(
                    Button::new("plugin-retry")
                        .icon(IconName::RefreshCw)
                        .tooltip(bongocat_i18n::text(locale(language), "plugins.retry"))
                        .disabled(state.loading)
                        .on_click(move |_, _, cx| {
                            retry_view.update(cx, |view, cx| {
                                view.update_plugin_host(move |host| host.rescan(), cx);
                            });
                        }),
                ),
        );
    }
    if state.loading {
        column = column.child(
            div()
                .text_sm()
                .child(bongocat_i18n::text(locale(language), "plugins.loading")),
        );
    } else if state.cards.is_empty() && state.error.is_none() {
        column = column.child(
            div()
                .p_4()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .text_sm()
                .child(bongocat_i18n::text(
                    locale(language),
                    "navigation.plugins.empty",
                )),
        );
    }
    for card in state.cards {
        column = column.child(card_view(card, handle.clone(), language, state.loading, cx));
    }
    column.into_any_element()
}

fn locale(language: SettingsLanguage) -> &'static str {
    language.catalog_locale()
}

fn card_view(
    card: PluginCardView,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
    busy: bool,
    cx: &App,
) -> impl IntoElement {
    let installed = card.version.is_some();
    let enabled = card.enabled;
    let icon = match &card.icon {
        bongocat_plugin_sdk::PluginIcon::Emoji(emoji) => emoji.clone(),
        bongocat_plugin_sdk::PluginIcon::LocalImage(path) => path.clone(),
        bongocat_plugin_sdk::PluginIcon::RemoteImage(url) => url.clone(),
    };
    let toggle_view = view.clone();
    let toggle_id = card.id.clone();
    let reset_view = view.clone();
    let reset_id = card.id.clone();
    let uninstall_view = view.clone();
    let uninstall_id = card.id.clone();
    let mut heading = div()
        .min_w_0()
        .flex_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().font_semibold().child(card.name.clone()));
    if let Some(version) = card.version.clone() {
        heading = heading.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(bongocat_i18n::format_text(
                    locale(language),
                    "plugins.version",
                    &[("version", version)],
                )),
        );
    }
    if let Some(version) = card
        .available_version
        .filter(|_| !installed || card.update_available)
    {
        heading = heading.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(bongocat_i18n::format_text(
                    locale(language),
                    "plugins.available_version",
                    &[("version", version)],
                )),
        );
    }
    let mut body = div()
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
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size_12()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().secondary)
                        .text_xl()
                        .child(icon),
                )
                .child(heading),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(card.description),
        );
    if card.failed {
        body = body.child(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(bongocat_i18n::text(locale(language), "plugins.failed")),
        );
    }
    let mut actions = div()
        .id(SharedString::from(format!("plugin-actions-{}", card.id)))
        .test_support()
        .min_w_0()
        .flex_1()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_2();
    // Keep plugin-authored commands opaque: their labels supply the tooltips,
    // not a host-side guess about an action ID or translated text.
    if enabled {
        for node in card.actions.into_iter().chain(card.settings) {
            if matches!(node, bongocat_plugin_sdk::UiNode::Button { .. }) {
                actions = actions.child(node_view(node, card.id.clone(), view.clone(), busy));
            } else {
                body = body.child(node_view(node, card.id.clone(), view.clone(), busy));
            }
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
                    locale(language),
                    if installed {
                        "plugins.update"
                    } else {
                        "plugins.install"
                    },
                ))
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    install_view.update(cx, |view, cx| {
                        view.update_plugin_host(move |host| host.install_available(&id), cx)
                    });
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
        .flex()
        .items_center()
        .justify_between()
        .gap_3();
    if installed {
        footer = footer.child(
            Switch::new(SharedString::from(format!("plugin-toggle-{}", card.id)))
                .flex_shrink_0()
                .checked(enabled)
                .disabled(busy)
                .tooltip(bongocat_i18n::text(
                    locale(language),
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
                        view.update_plugin_host(move |host| host.set_enabled(&id, enabled), cx);
                    });
                }),
        );
        actions = actions
            .child(
                Button::new(SharedString::from(format!("plugin-reset-{}", card.id)))
                    .disabled(busy)
                    .icon(IconName::RotateCcw)
                    .tooltip(bongocat_i18n::text(locale(language), "plugins.reset_data"))
                    .on_click(move |_, _, cx| {
                        let id = reset_id.clone();
                        reset_view.update(cx, |view, cx| {
                            view.update_plugin_host(move |host| host.reset(&id), cx);
                        });
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("plugin-uninstall-{}", card.id)))
                    .disabled(busy)
                    .icon(IconName::Trash)
                    .tooltip(bongocat_i18n::text(locale(language), "plugins.uninstall"))
                    .on_click(move |_, _, cx| {
                        let id = uninstall_id.clone();
                        uninstall_view.update(cx, |view, cx| {
                            view.update_plugin_host(move |host| host.uninstall(&id), cx);
                        });
                    }),
            );
    }
    body.child(footer.child(actions))
}

fn node_view(
    node: bongocat_plugin_sdk::UiNode,
    plugin: String,
    view: Entity<SettingsView>,
    busy: bool,
) -> impl IntoElement {
    match node {
        bongocat_plugin_sdk::UiNode::Text { text } => {
            div().text_sm().child(text).into_any_element()
        }
        bongocat_plugin_sdk::UiNode::Button { id, label } => {
            let target = view.clone();
            Button::new(SharedString::from(format!("plugin-action-{plugin}-{id}")))
                .icon(IconName::MousePointerClick)
                .tooltip(label)
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    let plugin = plugin.clone();
                    let id = id.clone();
                    target.update(cx, |view, cx| {
                        view.update_plugin_host(move |host| host.action(&plugin, &id), cx);
                    });
                })
                .into_any_element()
        }
        bongocat_plugin_sdk::UiNode::Toggle { key, label, value } => {
            let target = view.clone();
            let plugin_id = plugin.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().min_w_0().flex_1().text_sm().child(label.clone()))
                .child(
                    Switch::new(SharedString::from(format!(
                        "plugin-toggle-field-{plugin}-{key}"
                    )))
                    .flex_shrink_0()
                    .checked(value)
                    .tooltip(label)
                    .disabled(busy)
                    .on_click(move |checked, _, cx| {
                        let key = key.clone();
                        let plugin = plugin_id.clone();
                        let value = *checked;
                        target.update(cx, |view, cx| {
                            view.update_plugin_host(
                                move |host| {
                                    host.configure(&plugin, &key, ConfigValue::Boolean(value));
                                },
                                cx,
                            );
                        });
                    }),
                )
                .into_any_element()
        }
        bongocat_plugin_sdk::UiNode::Integer {
            key,
            label,
            value,
            min,
            max,
        } => {
            // The plugin declares the range and the host only offers steps inside
            // it; the plugin validates the value it is handed. A stepper rather
            // than a free-text field keeps the value inside `min..=max` by
            // construction, which is the same guarantee the model's own number
            // fields get from their engine.
            let row = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().min_w_0().flex_1().text_sm().child(label.clone()));
            let mut controls = div().flex().flex_shrink_0().items_center().gap_2();
            for (suffix, delta) in [("decrease", -1), ("increase", 1)] {
                let target = view.clone();
                let plugin_id = plugin.clone();
                let step_key = key.clone();
                let next = value.saturating_add(delta).clamp(min, max);
                let enabled = next != value;
                if delta > 0 {
                    controls = controls.child(div().text_sm().child(value.to_string()));
                }
                controls = controls.child(
                    Button::new(SharedString::from(format!(
                        "plugin-integer-{plugin}-{key}-{suffix}"
                    )))
                    .icon(if delta < 0 {
                        IconName::Minus
                    } else {
                        IconName::Plus
                    })
                    .tooltip(format!("{label} {} 1", if delta < 0 { "−" } else { "+" }))
                    .disabled(busy || !enabled)
                    .on_click(move |_, _, cx| {
                        let key = step_key.clone();
                        let plugin = plugin_id.clone();
                        target.update(cx, |view, cx| {
                            view.update_plugin_host(
                                move |host| {
                                    host.configure(&plugin, &key, ConfigValue::Integer(next));
                                },
                                cx,
                            );
                        });
                    }),
                );
            }
            row.child(controls).into_any_element()
        }
    }
}
