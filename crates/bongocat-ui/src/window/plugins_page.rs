//! The plugin center: cards, lifecycle controls and per-plugin configuration.
//!
//! Business rules live in the plugin process; this page only renders the view
//! tree each plugin publishes and forwards typed commands to the host cache.
use super::*;
use bongocat_plugin_sdk::ConfigValue;
use gpui_kit::AnyElement;
#[cfg(test)]
use gpui_kit::base::TestSupportExt as _;
use plugins_host::{PluginCardView, PluginHubState};

pub(super) fn content(
    view: &mut SettingsView,
    language: SettingsLanguage,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let state: PluginHubState = view.plugin_host.snapshot();
    let handle = cx.entity();
    let mut column = div().flex().flex_col().gap_4().w_full().child(
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(bongocat_i18n::text(
                locale(language),
                "navigation.plugins.subtitle",
            )),
    );
    let refresh_view = handle.clone();
    column = column.child(
        Button::new("plugin-refresh")
            .label(bongocat_i18n::text(locale(language), "plugins.refresh"))
            .disabled(state.loading)
            .on_click(move |_, _, cx| {
                refresh_view.update(cx, |view, cx| {
                    view.update_plugin_host(|host| host.rescan(), cx)
                });
            }),
    );
    if state.error.is_some() {
        let retry_view = handle.clone();
        column = column
            .child(div().text_sm().child(bongocat_i18n::text(
                locale(language),
                "plugins.discovery_failed",
            )))
            .child(
                Button::new("plugin-retry")
                    .label(bongocat_i18n::text(locale(language), "plugins.retry"))
                    .on_click(move |_, _, cx| {
                        retry_view.update(cx, |view, cx| {
                            view.update_plugin_host(move |host| host.rescan(), cx);
                        });
                    }),
            );
    }
    if state.loading {
        column = column.child(
            div()
                .text_sm()
                .child(bongocat_i18n::text(locale(language), "plugins.loading")),
        );
    } else if state.cards.is_empty() && state.error.is_none() {
        column = column.child(div().p_4().rounded_lg().border_1().text_sm().child(
            bongocat_i18n::text(locale(language), "navigation.plugins.empty"),
        ));
    }
    for card in state.cards {
        column = column.child(card_view(card, handle.clone(), language, state.loading));
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
) -> impl IntoElement {
    let installed = card.version.is_some();
    let enabled = card.enabled;
    let next_enabled = !enabled;
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
    let mut body = div()
        .id(SharedString::from(format!("plugin-card-{}", card.id)))
        .map(|body| {
            #[cfg(test)]
            let body = body.test_support();
            body
        })
        .w_full()
        .p_4()
        .rounded_lg()
        .border_1()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().text_xl().child(icon))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_semibold().child(card.name.clone()))
                        .child(div().text_sm().child(card.description.clone())),
                ),
        );
    if !installed || card.update_available {
        let install_view = view.clone();
        let id = card.id.clone();
        body = body.child(
            Button::new(SharedString::from(format!("plugin-install-{}", card.id)))
                .label(bongocat_i18n::text(
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
    if installed {
        body = body
            .child(
                Button::new(SharedString::from(format!("plugin-toggle-{}", card.id)))
                    .disabled(busy)
                    .label(bongocat_i18n::text(
                        locale(language),
                        if enabled {
                            "plugins.disable"
                        } else {
                            "plugins.enable"
                        },
                    ))
                    .on_click(move |_, _, cx| {
                        let id = toggle_id.clone();
                        toggle_view.update(cx, |view, cx| {
                            view.update_plugin_host(
                                move |host| host.set_enabled(&id, next_enabled),
                                cx,
                            );
                        });
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("plugin-reset-{}", card.id)))
                            .disabled(busy)
                            .label(bongocat_i18n::text(locale(language), "plugins.reset_data"))
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
                            .label(bongocat_i18n::text(locale(language), "plugins.uninstall"))
                            .on_click(move |_, _, cx| {
                                let id = uninstall_id.clone();
                                uninstall_view.update(cx, |view, cx| {
                                    view.update_plugin_host(move |host| host.uninstall(&id), cx);
                                });
                            }),
                    ),
            );
    }
    if let Some(version) = card.version.clone() {
        body = body.child(div().text_xs().child(bongocat_i18n::format_text(
            locale(language),
            "plugins.version",
            &[("version", version)],
        )));
    }
    if let Some(version) = card
        .available_version
        .filter(|_| !installed || card.update_available)
    {
        body = body.child(div().text_xs().child(bongocat_i18n::format_text(
            locale(language),
            "plugins.available_version",
            &[("version", version)],
        )));
    }
    if card.failed {
        body = body.child(
            div()
                .text_sm()
                .child(bongocat_i18n::text(locale(language), "plugins.failed")),
        );
    }
    // A plugin's controls are its own: the center renders the bounded tree the
    // plugin published, and each control sends one typed command back to the
    // plugin. Nothing here decides what a control means.
    for node in card.actions {
        if enabled {
            body = body.child(node_view(node, card.id.clone(), view.clone()));
        }
    }
    for node in card.settings {
        if enabled {
            body = body.child(node_view(node, card.id.clone(), view.clone()));
        }
    }
    body
}

fn node_view(
    node: bongocat_plugin_sdk::UiNode,
    plugin: String,
    view: Entity<SettingsView>,
) -> impl IntoElement {
    match node {
        bongocat_plugin_sdk::UiNode::Text { text } => {
            div().text_sm().child(text).into_any_element()
        }
        bongocat_plugin_sdk::UiNode::Button { id, label } => {
            let target = view.clone();
            Button::new(SharedString::from(format!("plugin-action-{plugin}-{id}")))
                .label(label)
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
                .gap_2()
                .child(div().text_sm().child(label))
                .child(
                    Switch::new(SharedString::from(format!(
                        "plugin-toggle-field-{plugin}-{key}"
                    )))
                    .checked(value)
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
            let mut row = div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().text_sm().child(label));
            for (suffix, delta) in [("decrease", -1), ("increase", 1)] {
                let target = view.clone();
                let plugin_id = plugin.clone();
                let step_key = key.clone();
                let next = (value + delta).clamp(min, max);
                let enabled = next != value;
                row = row.child(
                    Button::new(SharedString::from(format!(
                        "plugin-integer-{plugin}-{key}-{suffix}"
                    )))
                    .label(if delta < 0 { "−" } else { "+" })
                    .disabled(!enabled)
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
            row = row.child(div().text_sm().child(value.to_string()));
            row.into_any_element()
        }
    }
}
