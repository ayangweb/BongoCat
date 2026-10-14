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
    // Installing is choosing a package the user already has; the catalog download
    // is a separate, later path. The button is always present so the page is not
    // empty of actions when nothing is installed yet.
    let install_view = handle.clone();
    column = column.child(
        Button::new("plugin-install")
            .label(bongocat_i18n::text(locale(language), "plugins.install"))
            .on_click(move |_, _, cx| {
                install_view.update(cx, |view, cx| view.choose_plugin_package(cx));
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
        column = column.child(card_view(card, handle.clone(), language));
    }
    column.into_any_element()
}

fn locale(language: SettingsLanguage) -> &'static str {
    language.catalog_locale()
}

impl SettingsView {
    /// Surface a plugin error in the center and repaint.
    pub(super) fn report_plugin_error(&self, message: String, cx: &mut Context<Self>) {
        self.plugin_host.report_error(message);
        cx.notify();
    }

    /// Open the native picker for a plugin package, then install what it returns.
    ///
    /// The bytes are read on the background executor and installed through the
    /// same manager call a catalog download uses, so a package the user chose and
    /// a package the product downloaded cannot diverge in how they are validated.
    pub(super) fn choose_plugin_package(&mut self, cx: &mut Context<Self>) {
        let (sender, receiver) = async_channel::bounded(1);
        let picking = move |result| {
            let _ = sender.try_send(result);
        };
        if let Err(error) = bongocat_platform::pick_plugin_package(picking) {
            self.report_plugin_error(error.to_string(), cx);
            return;
        }
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let outcome = receiver.recv().await.unwrap_or(Err(
                bongocat_platform::ModelSourcePickerError::BackendUnavailable,
            ));
            let path = match outcome {
                Ok(bongocat_platform::ModelSourcePickerOutcome::Selected(path)) => path,
                // Cancelling is not an error, and a dialog backend failure is
                // reported the same way any other plugin failure is.
                Ok(bongocat_platform::ModelSourcePickerOutcome::Cancelled) => return,
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.report_plugin_error(error.to_string(), cx)
                    });
                    return;
                }
            };
            let read = executor
                .spawn(async move { std::fs::read(&path).map_err(|error| error.to_string()) })
                .await;
            let _ = this.update(cx, |view, cx| match read {
                Ok(bytes) => view.update_plugin_host(move |host| host.install(bytes), cx),
                Err(error) => view.report_plugin_error(error, cx),
            });
        })
        .detach();
    }
}

fn card_view(
    card: PluginCardView,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> impl IntoElement {
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
                        .label(bongocat_i18n::text(locale(language), "plugins.uninstall"))
                        .on_click(move |_, _, cx| {
                            let id = uninstall_id.clone();
                            uninstall_view.update(cx, |view, cx| {
                                view.update_plugin_host(move |host| host.uninstall(&id), cx);
                            });
                        }),
                ),
        );
    if let Some(version) = card.version.clone() {
        body = body.child(div().text_xs().child(bongocat_i18n::format_text(
            locale(language),
            "plugins.version",
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
