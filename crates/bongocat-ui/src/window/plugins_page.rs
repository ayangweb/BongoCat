//! The plugin center: cards, lifecycle controls and per-plugin configuration.
//!
//! Business rules live in the plugin process; this page only renders the view
//! tree each plugin publishes and forwards typed commands to the host cache.
use super::*;
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
                            let environment = view.seed.environment;
                            view.update_plugin_host(move |host| host.initialize(environment), cx);
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
        column = column.child(card_view(card, handle.clone(), language));
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
) -> impl IntoElement {
    let enabled = card.enabled;
    let icon = match &card.icon {
        bongocat_plugin_sdk::PluginIcon::Emoji(emoji) => emoji.clone(),
        bongocat_plugin_sdk::PluginIcon::LocalImage(path) => path.clone(),
        bongocat_plugin_sdk::PluginIcon::RemoteImage(url) => url.clone(),
    };
    let toggle_view = view.clone();
    let toggle_id = card.id.clone();
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
        )
        .child(
            Button::new(SharedString::from(format!("plugin-toggle-{}", card.id)))
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
                        view.update_plugin_host(move |host| host.toggle(&id), cx);
                    });
                }),
        );
    if card.failed {
        body = body.child(
            div()
                .text_sm()
                .child(bongocat_i18n::text(locale(language), "plugins.failed")),
        );
    }
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
        bongocat_plugin_sdk::UiNode::Toggle { key, label, value } => div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_sm().child(format!("{label}: {key}")))
            .child(div().text_sm().child(if value { "on" } else { "off" }))
            .into_any_element(),
        bongocat_plugin_sdk::UiNode::Integer {
            key, label, value, ..
        } => div()
            .text_sm()
            .child(format!("{label}: {value} ({key})"))
            .into_any_element(),
    }
}
