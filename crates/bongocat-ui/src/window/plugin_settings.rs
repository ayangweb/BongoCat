//! One plugin's configuration panel, opened from its card.
//!
//! Every plugin gets the same entry and the same surface: a modal holding the
//! controls that plugin published, in the order it published them. A modal is
//! the right shape here because a plugin declares a handful of fields at most —
//! there is nothing to scroll and nothing to switch between — and because the
//! panel is about one plugin, so it should not become a second column of the
//! window that has to say whose settings it is showing.
//!
//! The panel owns no business rules. It writes through the view's typed command
//! and redraws from what the plugin publishes next, exactly as the card does.

use super::*;
use bongocat_plugin_sdk::{ConfigValue, UiNode};
use gpui_kit::{AnyElement, assets::IconName, base::TestSupportExt as _};
use plugins_page::action_icon;

/// The width the panel reads at.
///
/// It is a reading measure, not a layout constraint: a plugin declares a handful
/// of controls, so the panel is sized so a labelled control and its value fit on
/// one line.
const PLUGIN_SETTINGS_PANEL_WIDTH: Pixels = px(440.0);

/// The one plugin whose configuration panel is open.
///
/// The open panel is a value rather than a window flag because the view has to
/// know *which* plugin it belongs to before it opens the layer at all — the same
/// reason the model rename and the Mver conversion dialog keep their drafts here.
pub(crate) struct PluginSettingsDraft {
    pub(crate) plugin: String,
    /// Set when the panel is built during render; the component owns the surface
    /// itself, and this records that the view put something into it.
    pub(crate) open: bool,
    /// The frame's copy of what the panel draws.
    ///
    /// `Root` owns the dialog layer inside `SettingsView::render`, so the
    /// builder must not read the `SettingsView` that `render` already holds. The
    /// published title, controls and in-flight commands are the frame's inputs.
    pub(crate) snapshot: Rc<RefCell<PluginSettingsSnapshot>>,
}

/// What one frame of a plugin's configuration panel draws.
#[derive(Clone, PartialEq)]
pub(crate) struct PluginSettingsSnapshot {
    pub(crate) locale: &'static str,
    pub(crate) title: String,
    pub(crate) nodes: Vec<UiNode>,
    /// Whether the center as a whole is still discovering, which holds every
    /// control the panel offers.
    pub(crate) busy: bool,
    /// The configuration keys of this plugin whose write has not been answered.
    ///
    /// It is per key rather than per plugin because two fields of one panel can
    /// be written at the same time, and only the field that was edited is busy.
    pub(crate) pending_keys: BTreeSet<String>,
}

impl SettingsView {
    /// Keep the configuration panel and the draft that asked for it in step.
    ///
    /// The draft says what should be on screen and the window owns the surface.
    /// A panel dismissed by the overlay or Escape never reaches a callback, so
    /// its close is noticed here on the next frame and the draft is dropped then.
    pub(super) fn sync_plugin_settings_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.plugin_settings.as_ref() else {
            return;
        };
        if draft.open && !window.has_active_dialog(cx) {
            self.plugin_settings = None;
            return;
        }
        let plugin = draft.plugin.clone();
        let state = self.plugin_host.snapshot();
        let locale = self.display_language().catalog_locale();
        // A plugin that was uninstalled, switched off or that failed while its
        // panel was open has nothing left to configure: the panel closes with the
        // plugin rather than showing controls nothing will answer.
        let Some(card) = state
            .cards
            .iter()
            .find(|card| card.id == plugin && card.enabled && !card.failed)
        else {
            self.plugin_settings = None;
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
            }
            return;
        };
        let (name, _) = card.copy(locale);
        let snapshot = PluginSettingsSnapshot {
            locale,
            title: bongocat_i18n::format_text(
                locale,
                "plugins.settings.title",
                &[("name", name.to_owned())],
            ),
            nodes: card.settings.clone(),
            busy: state.loading,
            pending_keys: card
                .settings
                .iter()
                .filter_map(|node| match node {
                    UiNode::Toggle { key, .. } | UiNode::Integer { key, .. } => Some(key.clone()),
                    _ => None,
                })
                .filter(|key| state.pending_key(&plugin, key))
                .collect(),
        };
        let draft = self
            .plugin_settings
            .as_mut()
            .expect("the draft was just read");
        // The builder runs again on every frame the dialog layer paints and reads
        // this cell each time, so a new frame is written into the panel already on
        // screen. The layer itself is only opened once: reopening it would restart
        // the dialog's own presentation for what is only a value change inside it.
        *draft.snapshot.borrow_mut() = snapshot;
        if draft.open {
            return;
        }
        draft.open = true;
        let snapshot = draft.snapshot.clone();
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            build_plugin_settings_dialog(snapshot.clone(), view.clone(), dialog, cx)
        });
    }

    /// Put one plugin's configuration panel on screen.
    pub(super) fn open_plugin_settings(
        &mut self,
        plugin: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .plugin_settings
            .as_ref()
            .is_some_and(|draft| draft.plugin == plugin)
        {
            return;
        }
        let state = self.plugin_host.snapshot();
        let locale = self.display_language().catalog_locale();
        let Some(card) = state
            .cards
            .iter()
            .find(|card| card.id == plugin && card.enabled && !card.failed)
        else {
            return;
        };
        let (name, _) = card.copy(locale);
        self.plugin_settings = Some(PluginSettingsDraft {
            plugin: plugin.to_owned(),
            open: false,
            snapshot: Rc::new(RefCell::new(PluginSettingsSnapshot {
                locale,
                title: bongocat_i18n::format_text(
                    locale,
                    "plugins.settings.title",
                    &[("name", name.to_owned())],
                ),
                nodes: card.settings.clone(),
                busy: state.loading,
                pending_keys: BTreeSet::new(),
            })),
        });
        self.sync_plugin_settings_dialog(window, cx);
    }

    /// Close the panel and drop its draft.
    pub(super) fn close_plugin_settings(&mut self, cx: &mut Context<Self>) {
        self.plugin_settings = None;
        cx.notify();
    }

    /// Send one action published by the plugin whose panel is open.
    pub(super) fn run_settings_action(&mut self, action: &str, cx: &mut Context<Self>) {
        let Some(plugin) = self
            .plugin_settings
            .as_ref()
            .map(|draft| draft.plugin.clone())
        else {
            return;
        };
        self.run_plugin_action(&plugin, action, cx);
    }

    /// Write one key of the plugin whose panel is open.
    pub(super) fn configure_open_plugin(
        &mut self,
        key: &str,
        value: ConfigValue,
        cx: &mut Context<Self>,
    ) {
        let Some(plugin) = self
            .plugin_settings
            .as_ref()
            .map(|draft| draft.plugin.clone())
        else {
            return;
        };
        self.configure_plugin(&plugin, key, value, cx);
    }
}

/// Build one frame of a plugin's configuration panel.
///
/// The builder reruns whenever the window's dialog layer repaints, so its inputs
/// come from the shared snapshot rather than from the `SettingsView` that
/// `render` already holds. Every control writes through the view's typed
/// command, and the panel redraws from what the plugin publishes next: the plugin
/// stays the only thing that decides what its configuration is.
fn build_plugin_settings_dialog(
    snapshot: Rc<RefCell<PluginSettingsSnapshot>>,
    view: WeakEntity<SettingsView>,
    dialog: Dialog,
    _cx: &mut App,
) -> Dialog {
    let (locale, title, nodes, busy, pending_keys) = {
        let snapshot = snapshot.borrow();
        (
            snapshot.locale,
            snapshot.title.clone(),
            snapshot.nodes.clone(),
            snapshot.busy,
            snapshot.pending_keys.clone(),
        )
    };
    let close_label = bongocat_i18n::text(locale, "actions.close");
    // The panel writes every control as it is used, so there is nothing to
    // commit: the one action it needs is leaving it, and both the dialog's own
    // cancel action and the footer button are that same action.
    let cancel_view = view.clone();
    let footer_view = view.clone();
    dialog
        .title(title)
        .w(PLUGIN_SETTINGS_PANEL_WIDTH)
        .button_props(
            DialogButtonProps::default()
                .cancel_text(close_label)
                .show_cancel(true)
                .on_cancel(move |_, window, cx| {
                    let _ = cancel_view.update(cx, |view, cx| view.close_plugin_settings(cx));
                    window.close_dialog(cx);
                    true
                }),
        )
        .footer(
            div()
                .id("plugin-settings-footer")
                .flex()
                .w_full()
                .justify_end()
                .child(
                    Button::new("plugin-settings-close")
                        .label(close_label)
                        .on_click(move |_, window, cx| {
                            let _ =
                                footer_view.update(cx, |view, cx| view.close_plugin_settings(cx));
                            window.close_dialog(cx);
                        }),
                ),
        )
        .content(move |content, _, cx| {
            if nodes.is_empty() {
                // A plugin with a gear but nothing behind it says so rather than
                // opening onto an empty surface the user has to interpret.
                return content.child(
                    div()
                        .id("plugin-settings-empty")
                        .test_support()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(bongocat_i18n::text(locale, "plugins.settings.empty")),
                );
            }
            content.child(
                div()
                    .id("plugin-settings-fields")
                    .test_support()
                    .flex()
                    .flex_col()
                    .w_full()
                    .gap_3()
                    .children(nodes.iter().enumerate().map(|(index, node)| {
                        settings_field(node, index, locale, busy, &pending_keys, view.clone())
                    })),
            )
        })
}

/// One control of a plugin's configuration panel.
///
/// Each node keeps the meaning the plugin gave it: a button runs the action it
/// published, a toggle writes its key, and a number steps inside the range the
/// plugin declared. A node carrying its own in-flight state draws that state on
/// the control itself rather than beside it.
fn settings_field(
    node: &UiNode,
    index: usize,
    locale: &'static str,
    busy: bool,
    pending_keys: &BTreeSet<String>,
    view: WeakEntity<SettingsView>,
) -> AnyElement {
    match node {
        UiNode::Text { text } => div()
            .id(("plugin-setting-text", index))
            .test_support()
            .text_sm()
            .child(text.clone())
            .into_any_element(),
        UiNode::Button {
            id,
            label,
            icon,
            loading,
        } => {
            let target = view.clone();
            let action = id.clone();
            Button::new(SharedString::from(format!("plugin-setting-button-{id}")))
                .icon(action_icon(*icon))
                .label(label.clone())
                .tooltip(label.clone())
                .loading(*loading)
                .disabled(busy || *loading)
                .on_click(move |_, _, cx| {
                    let action = action.clone();
                    let _ = target.update(cx, |view, cx| {
                        view.run_settings_action(&action, cx);
                    });
                })
                .into_any_element()
        }
        UiNode::Toggle { key, label, value } => {
            let target = view.clone();
            let field = key.clone();
            let field_label = label.clone();
            let pending = pending_keys.contains(key);
            div()
                .id(("plugin-setting-toggle", index))
                .test_support()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .child(field_label.clone()),
                )
                .child(
                    Switch::new(SharedString::from(format!("plugin-setting-toggle-{key}")))
                        .flex_shrink_0()
                        .checked(*value)
                        .tooltip(field_label)
                        .disabled(busy || pending)
                        .on_click(move |checked, _, cx| {
                            let field = field.clone();
                            let value = *checked;
                            let _ = target.update(cx, |view, cx| {
                                view.configure_open_plugin(&field, ConfigValue::Boolean(value), cx);
                            });
                        }),
                )
                .into_any_element()
        }
        UiNode::Integer {
            key,
            label,
            value,
            min,
            max,
        } => {
            // The plugin declares the range and validates the value it is handed;
            // a stepper keeps the value inside `min..=max` by construction, which
            // is the same guarantee the model library's number fields get from
            // its own engine.
            let row = div()
                .id(("plugin-setting-integer", index))
                .test_support()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().min_w_0().flex_1().text_sm().child(label.clone()));
            let mut controls = div().flex().flex_shrink_0().items_center().gap_2();
            let pending = pending_keys.contains(key);
            for (suffix, delta) in [("decrease", -1), ("increase", 1)] {
                let target = view.clone();
                let step_key = key.clone();
                let next = value.saturating_add(delta).clamp(*min, *max);
                let enabled = next != *value;
                // The stepper's own label is the field it steps, so the tooltip
                // only has to name the direction: a translated word rather than
                // a `-`/`+` glyph glued onto a plugin-supplied label.
                let direction = bongocat_i18n::text(
                    locale,
                    if delta < 0 {
                        "plugins.settings.decrease"
                    } else {
                        "plugins.settings.increase"
                    },
                );
                if delta > 0 {
                    controls = controls.child(div().text_sm().child(value.to_string()));
                }
                controls = controls.child(
                    Button::new(SharedString::from(format!(
                        "plugin-setting-integer-{key}-{suffix}"
                    )))
                    .icon(if delta < 0 {
                        IconName::Minus
                    } else {
                        IconName::Plus
                    })
                    .tooltip(direction)
                    .disabled(busy || pending || !enabled)
                    .on_click(move |_, _, cx| {
                        let key = step_key.clone();
                        let _ = target.update(cx, |view, cx| {
                            view.configure_open_plugin(&key, ConfigValue::Integer(next), cx);
                        });
                    }),
                );
            }
            row.child(controls).into_any_element()
        }
    }
}
