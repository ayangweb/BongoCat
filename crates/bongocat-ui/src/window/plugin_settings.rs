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
use gpui_kit::{AnyElement, Task, assets::IconName, base::TestSupportExt as _};
use image::ImageEncoder as _;
use plugins_page::action_icon;

type PreviewCallback = Rc<dyn Fn(SharedString, bool, &mut App)>;

/// The standard image cache keys encoded artwork by content. A fresh RenderImage
/// on each dialog repaint would instead allocate a fresh sprite-atlas entry.
fn status_icon_image(
    pixels: &[u8],
    tint: Option<[u8; 3]>,
) -> Option<std::sync::Arc<gpui_kit::Image>> {
    if pixels.len() != 4096 {
        return None;
    }
    let mut bytes = Vec::new();
    let mut tinted;
    let pixels = if let Some(tint) = tint {
        tinted = pixels.to_vec();
        for pixel in tinted.chunks_exact_mut(4) {
            pixel[..3].copy_from_slice(&tint);
        }
        tinted.as_slice()
    } else {
        pixels
    };
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(pixels, 32, 32, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(std::sync::Arc::new(gpui_kit::Image::from_bytes(
        gpui_kit::ImageFormat::Png,
        bytes,
    )))
}

#[derive(gpui_kit::IntoElement)]
struct PluginStatus {
    icon: bongocat_plugin_sdk::StatusIcon,
    text: String,
}

impl RenderOnce for PluginStatus {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        use bongocat_plugin_sdk::StatusIcon;
        let color = cx.theme().foreground.to_rgb();
        let tint = self
            .icon
            .is_template()
            .then(|| [color.r, color.g, color.b].map(|c| (c * 255.0).round() as u8));
        let artwork = match &self.icon {
            StatusIcon::Rgba(pixels) => status_icon_image(pixels, tint).map_or_else(
                || div().into_any_element(),
                |image| img(image).size(px(24.0)).into_any_element(),
            ),
            StatusIcon::Badge(badge) => div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded_md()
                .bg(cx.theme().secondary)
                .text_xs()
                .child(badge.clone())
                .into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(artwork)
            .child(self.text)
    }
}

/// The width the panel reads at.
///
/// It is a reading measure, not a layout constraint: a plugin declares a handful
/// of controls, so the panel is sized so a labelled control and its value fit on
/// one line.
const PLUGIN_SETTINGS_PANEL_WIDTH: Pixels = px(440.0);

/// One option of a dropdown a plugin published.
///
/// The value is what the plugin is handed back and the label is what the row
/// draws, which is the same split the protocol makes: a plugin's option list is
/// its own vocabulary, and the panel never has to guess a value from a label it
/// is showing.
#[derive(Clone)]
pub(crate) struct PluginSettingsSelectChoice {
    value: SharedString,
    label: SharedString,
    /// The optional category row this choice belongs under.
    group: Option<SharedString>,
    preview: Option<PreviewCallback>,
}

impl PluginSettingsSelectChoice {
    fn new(value: &str, label: &str) -> Self {
        Self {
            value: SharedString::from(value.to_owned()),
            label: SharedString::from(label.to_owned()),
            group: None,
            preview: None,
        }
    }

    fn with_group(mut self, group: Option<&str>) -> Self {
        self.group = group
            .filter(|group| !group.is_empty())
            .map(SharedString::from);
        self
    }
}

impl SearchableListItem for PluginSettingsSelectChoice {
    /// The option's value rather than the whole row, so a plugin that relabels
    /// an option it still holds keeps the selection instead of losing it.
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let value = self.value.clone();
        div()
            .child(self.label.clone())
            .when_some(self.preview.clone(), |row, preview| {
                // Keep stock row selection and click handling; only add a hover target.
                row.child(
                    div()
                        .id(SharedString::from(format!(
                            "plugin-option-preview-{}",
                            value
                        )))
                        .test_support()
                        .absolute()
                        .inset_0()
                        .on_hover(move |hovered, _, cx| preview(value.clone(), *hovered, cx)),
                )
            })
    }
}

/// Delegate filtering, indexing and header styling to the stock component.
/// Older plugins retain a flat list without a synthetic empty heading.
#[derive(Clone)]
pub(crate) enum PluginSettingsSelectGroups {
    Flat(SearchableVec<PluginSettingsSelectChoice>),
    Grouped(SearchableVec<SearchableGroup<PluginSettingsSelectChoice>>),
}

impl PluginSettingsSelectGroups {
    fn new(choices: Vec<PluginSettingsSelectChoice>) -> Self {
        // Keep partial metadata readable without inventing a host-owned group.
        if choices.iter().any(|choice| choice.group.is_none()) {
            Self::Flat(SearchableVec::new(choices))
        } else {
            Self::Grouped(SearchableVec::new(plugin_select_groups(choices)))
        }
    }
}

impl SearchableListDelegate for PluginSettingsSelectGroups {
    type Item = PluginSettingsSelectChoice;

    fn sections_count(&self, cx: &App) -> usize {
        match self {
            Self::Flat(items) => items.sections_count(cx),
            Self::Grouped(groups) => groups.sections_count(cx),
        }
    }

    fn items_count(&self, section: usize) -> usize {
        match self {
            Self::Flat(items) => items.items_count(section),
            Self::Grouped(groups) => groups.items_count(section),
        }
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        match self {
            Self::Flat(items) => items.item(ix),
            Self::Grouped(groups) => groups.item(ix),
        }
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        match self {
            Self::Flat(items) => items.position(value),
            Self::Grouped(groups) => groups.position(value),
        }
    }

    fn perform_search(&mut self, query: &str, window: &mut Window, cx: &mut App) -> Task<()> {
        match self {
            Self::Flat(items) => items.perform_search(query, window, cx),
            Self::Grouped(groups) => groups.perform_search(query, window, cx),
        }
    }

    #[allow(deprecated)]
    fn section(&self, section: usize) -> Option<AnyElement> {
        match self {
            Self::Flat(_) => None,
            Self::Grouped(groups) => groups.section(section),
        }
    }
}

/// Build the component's grouped data from the plugin's flat option list.
///
/// A plugin publishes its preferred order; choices that name the same category
/// share a section. Lists with no groups (or only a partial set of groups) use
/// the stock flat list so older and malformed plugin output remains readable.
fn plugin_select_groups(
    choices: Vec<PluginSettingsSelectChoice>,
) -> Vec<SearchableGroup<PluginSettingsSelectChoice>> {
    let mut groups: Vec<SearchableGroup<PluginSettingsSelectChoice>> = Vec::new();
    for choice in choices {
        let title = choice.group.clone().unwrap_or_default();
        match groups.iter_mut().find(|group| group.title == title) {
            Some(group) => group.items.push(choice),
            None => groups.push(SearchableGroup::new(title).item(choice)),
        }
    }
    groups
}

/// The dropdowns one open panel draws, keyed by the config key each writes.
///
/// A select is an `Entity` because that is what the component's state is, and
/// the panel is rebuilt from the plugin's published tree on every frame: keeping
/// the entities here is what lets a plugin republish its option list without the
/// open menu losing its place. The subscriptions are held beside them because a
/// dropped `Subscription` stops delivering the choice the user made.
#[derive(Default)]
struct PluginSettingsSelects {
    entities: BTreeMap<String, Entity<PluginSettingsSelectState>>,
    subscriptions: Vec<gpui_kit::Subscription>,
    previews: BTreeMap<String, PreviewCallback>,
    nodes: Vec<UiNode>,
}

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
    /// The dropdowns this panel draws, one per select the plugin published.
    selects: Rc<RefCell<PluginSettingsSelects>>,
    preview_lifetime: std::sync::Arc<()>,
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
                    UiNode::Toggle { key, .. }
                    | UiNode::Integer { key, .. }
                    | UiNode::Select { key, .. } => Some(key.clone()),
                    _ => None,
                })
                .filter(|key| state.pending_key(&plugin, key))
                .collect(),
        };
        let selects = draft.selects.clone();
        sync_plugin_selects(
            &selects,
            &plugin,
            &snapshot.nodes,
            cx.entity().downgrade(),
            std::sync::Arc::downgrade(&draft.preview_lifetime),
            window,
            cx,
        );
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
        let selects = draft.selects.clone();
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            build_plugin_settings_dialog(
                snapshot.clone(),
                selects.clone(),
                view.clone(),
                dialog,
                cx,
            )
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
            selects: Rc::new(RefCell::new(PluginSettingsSelects::default())),
            preview_lifetime: std::sync::Arc::default(),
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

/// Keep one dropdown per select node the panel draws in step with the plugin.
///
/// A dropdown is an entity, so it is created once for the lifetime of the open
/// panel and only its items and selection are refreshed afterwards: recreating it
/// on every frame would close a menu the user has open, and rebuilding the items
/// is what makes a plugin's republished option list show up. The selection is
/// written back through the same typed command every other control uses, so the
/// plugin stays the one that decides whether a value is acceptable.
fn sync_plugin_selects(
    selects: &Rc<RefCell<PluginSettingsSelects>>,
    plugin: &str,
    nodes: &[UiNode],
    view: WeakEntity<SettingsView>,
    lifetime: std::sync::Weak<()>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    let changed = selects.borrow().nodes != nodes;
    if changed {
        selects.borrow_mut().nodes = nodes.to_vec();
    }
    for node in nodes {
        let UiNode::Select {
            key,
            value,
            options,
            preview: supports_preview,
            ..
        } = node
        else {
            continue;
        };
        let mut choices: Vec<PluginSettingsSelectChoice> = options
            .iter()
            .map(|option| {
                PluginSettingsSelectChoice::new(&option.value, &option.label)
                    .with_group(option.group.as_deref())
            })
            .collect();
        let selected = SharedString::from(value.clone());
        let mut selects = selects.borrow_mut();
        let lifetime = lifetime.clone();
        let preview = supports_preview.then(|| {
            selects
                .previews
                .entry(key.clone())
                .or_insert_with(|| {
                    let plugin = plugin.to_owned();
                    let key = key.clone();
                    let view = view.clone();
                    Rc::new(move |value, hovered, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.preview_plugin(
                                &plugin,
                                &key,
                                value.as_ref(),
                                hovered,
                                lifetime.clone(),
                                cx,
                            );
                        });
                    })
                })
                .clone()
        });
        for choice in &mut choices {
            choice.preview = preview.clone();
        }
        let data = PluginSettingsSelectGroups::new(choices);
        let selected_index = data.position(&selected).unwrap_or_default();
        match selects.entities.get(key) {
            Some(entity) => {
                entity.update(cx, |state, cx| {
                    if changed {
                        state.set_items(data, window, cx);
                    }
                    // A refused write republishes the same nodes. Restore its
                    // value too, without resetting an unchanged menu cursor.
                    if changed || state.selected_value() != Some(&selected) {
                        state.set_selected_value(&selected, window, cx);
                    }
                });
            }
            None => {
                let entity = cx.new(|cx| SelectState::new(data, Some(selected_index), window, cx));
                let (field_plugin, field_key) = (plugin.to_owned(), key.clone());
                let subscription = cx.subscribe_in(
                    &entity,
                    window,
                    move |view: &mut SettingsView,
                          _: &Entity<PluginSettingsSelectState>,
                          event: &SelectEvent<PluginSettingsSelectGroups>,
                          _: &mut Window,
                          cx: &mut Context<SettingsView>| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        view.configure_plugin(
                            &field_plugin,
                            &field_key,
                            ConfigValue::Text(value.to_string()),
                            cx,
                        );
                    },
                );
                selects.subscriptions.push(subscription);
                let (field_plugin, field_key) = (plugin.to_owned(), key.clone());
                let dismissed =
                    cx.subscribe(&entity, move |view, _, _: &gpui_kit::DismissEvent, _| {
                        view.plugin_host.cancel_preview(&field_plugin, &field_key);
                    });
                selects.subscriptions.push(dismissed);
                selects.entities.insert(key.clone(), entity);
            }
        }
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
    selects: Rc<RefCell<PluginSettingsSelects>>,
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
                        settings_field(
                            node,
                            index,
                            locale,
                            busy,
                            &pending_keys,
                            view.clone(),
                            &selects,
                        )
                    })),
            )
        })
}

/// One plugin dropdown, drawn with the window it is painted in.
///
/// The element exists for the menu width alone: `Select` sizes its menu from its
/// trigger, and the trigger is only as wide as the option the plugin currently
/// holds, so every longer option — the one a user scanning the list is looking
/// for — would be ellipsized. The width is measured at paint time, when there is
/// a window with a text system to measure through, which is what a `RenderOnce`
/// element is handed.
#[derive(gpui_kit::IntoElement)]
struct PluginSettingsSelect {
    state: Entity<PluginSettingsSelectState>,
    labels: Vec<SharedString>,
    disabled: bool,
}

impl RenderOnce for PluginSettingsSelect {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .menu_width(dropdown_menu_width(window, self.labels))
            .disabled(self.disabled)
    }
}

/// One control of a plugin's configuration panel.
///
/// Each node keeps the meaning the plugin gave it: a button runs the action it
/// published, a toggle writes its key, a number steps inside the range the plugin
/// declared, and a select writes the option the user picked. A node carrying its
/// own in-flight state draws that state on the control itself rather than beside
/// it.
fn settings_field(
    node: &UiNode,
    index: usize,
    locale: &'static str,
    busy: bool,
    pending_keys: &BTreeSet<String>,
    view: WeakEntity<SettingsView>,
    selects: &Rc<RefCell<PluginSettingsSelects>>,
) -> AnyElement {
    match node {
        UiNode::IconText { icon, text } => PluginStatus {
            icon: icon.clone(),
            text: text.clone(),
        }
        .into_any_element(),
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
        UiNode::Select {
            key,
            label,
            options,
            ..
        } => {
            let row = div()
                .id(("plugin-setting-select", index))
                .test_support()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().min_w_0().flex_1().text_sm().child(label.clone()));
            // The dropdown is created by `sync_plugin_selects` before the panel
            // paints, and its own list is what sizes the menu: a plugin's option
            // labels are its own, so the menu has to be measured from them rather
            // than from the trigger the component would otherwise size itself by.
            let select = selects
                .borrow()
                .entities
                .get(key)
                .cloned()
                .expect("every select node has its dropdown before the panel draws it");
            let labels: Vec<SharedString> = options
                .iter()
                .flat_map(|option| std::iter::once(&option.label).chain(option.group.iter()))
                .cloned()
                .map(SharedString::from)
                .collect();
            row.child(
                div()
                    .flex_shrink_0()
                    .max_w(px(220.0))
                    .child(PluginSettingsSelect {
                        state: select,
                        labels,
                        disabled: busy || pending_keys.contains(key),
                    }),
            )
            .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt as _};

    #[test]
    fn unchanged_status_artwork_reuses_the_content_addressed_image_cache() {
        let pixels = [255, 0, 0, 128].repeat(1024);
        let first = status_icon_image(&pixels, None).unwrap();
        let second = status_icon_image(&pixels, None).unwrap();
        assert_eq!(first.id(), second.id());
        let decoded = image::load_from_memory(&first.bytes).unwrap().into_rgba8();
        assert_eq!(decoded.as_raw(), &pixels);
        assert!(status_icon_image(&[], None).is_none());
        for tint in [[240, 240, 240], [20, 20, 20]] {
            let image = status_icon_image(&[0, 0, 0, 128].repeat(1024), Some(tint)).unwrap();
            let decoded = image::load_from_memory(&image.bytes).unwrap().into_rgba8();
            assert_eq!(decoded.get_pixel(0, 0).0, [tint[0], tint[1], tint[2], 128]);
        }
    }

    struct SelectHarness(Entity<PluginSettingsSelectState>);

    impl Render for SelectHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().p_4().w(px(300.)).child(Select::new(&self.0))
        }
    }

    #[gpui_kit::test]
    fn hovering_stock_select_options_previews_without_confirming(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let previews = Rc::new(RefCell::new(Vec::new()));
        let received = previews.clone();
        let capture = Rc::new(RefCell::new(None));
        let selected = capture.clone();
        let window = cx.open_window(size(px(600.), px(500.)), move |window, cx| {
            let callback: PreviewCallback = Rc::new(move |value, hovered, _| {
                if hovered {
                    received.borrow_mut().push(value.to_string());
                }
            });
            let choices = ["first", "second"]
                .into_iter()
                .map(|value| {
                    let mut choice = PluginSettingsSelectChoice::new(value, value);
                    choice.group = Some(choice.label.clone());
                    choice.preview = Some(callback.clone());
                    choice
                })
                .collect::<Vec<_>>();
            let state = cx.new(|cx| {
                SelectState::new(
                    PluginSettingsSelectGroups::new(choices),
                    Some(IndexPath::default().row(0)),
                    window,
                    cx,
                )
            });
            selected.borrow_mut().replace(state.clone());
            Root::new(cx.new(|_| SelectHarness(state)), window, cx)
        });
        let state = capture.borrow_mut().take().unwrap();
        let visual = VisualTestContext::from_window(*window, cx).into_mut();
        visual.run_until_parked();
        visual.update(|window, cx| window.click("input", cx));
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        for value in ["second", "first", "second"] {
            visual.update(|window, cx| {
                let bounds = window
                    .find(SharedString::from(format!("plugin-option-preview-{value}")))
                    .bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                window.simulate_mouse_move(bounds.center(), cx);
            });
            visual.update(|window, cx| window.render_frame(cx));
            visual.run_until_parked();
            assert_eq!(previews.borrow().last().map(String::as_str), Some(value));
            state.read_with(visual, |state, _| {
                assert_eq!(
                    state.selected_value().map(|value| value.as_ref()),
                    Some("first")
                );
            });
        }
        for right_edge in [false, true] {
            visual.update(|window, cx| window.simulate_mouse_move(point(px(10.), px(10.)), cx));
            let before = previews.borrow().len();
            visual.update(|window, cx| {
                let bounds = window.find("plugin-option-preview-second").bounds();
                let x = if right_edge {
                    bounds.size.width - px(1.)
                } else {
                    px(1.)
                };
                window.simulate_mouse_move(bounds.origin + point(x, bounds.size.height / 2.), cx);
            });
            assert_eq!(
                previews.borrow().len(),
                before + 1,
                "the whole option row previews, including its edges"
            );
        }
        visual.update(|window, cx| window.click("plugin-option-preview-second", cx));
        state.read_with(visual, |state, _| {
            assert_eq!(
                state.selected_value().map(|value| value.as_ref()),
                Some("second")
            );
        });
        // Relabeling and regrouping must restore by value, not a stale flat row.
        visual.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_items(
                    PluginSettingsSelectGroups::new(vec![
                        PluginSettingsSelectChoice::new("second", "Second localized")
                            .with_group(Some("New category")),
                        PluginSettingsSelectChoice::new("first", "First localized")
                            .with_group(Some("Another category")),
                    ]),
                    window,
                    cx,
                );
                state.set_selected_value(&SharedString::from("second"), window, cx);
                assert_eq!(state.selected_value(), Some(&SharedString::from("second")));
                assert_eq!(state.selected_index(cx), Some(IndexPath::default()));
            });
        });
    }

    #[gpui_kit::test]
    #[allow(deprecated)]
    fn select_groups_keep_the_plugins_categories_and_flat_legacy_lists(cx: &mut TestAppContext) {
        let grouped = vec![
            PluginSettingsSelectChoice::new("auto", "Follow").with_group(Some("Follow")),
            PluginSettingsSelectChoice::new("blue", "SKCM Blue").with_group(Some("Keyboard")),
            PluginSettingsSelectChoice::new("navy", "Box Navy").with_group(Some("Keyboard")),
        ];
        let mut grouped = PluginSettingsSelectGroups::new(grouped);
        let legacy = PluginSettingsSelectGroups::new(vec![
            PluginSettingsSelectChoice::new("auto", "Follow"),
            PluginSettingsSelectChoice::new("blue", "SKCM Blue"),
        ]);
        let mixed = PluginSettingsSelectGroups::new(vec![
            PluginSettingsSelectChoice::new("auto", "Follow"),
            PluginSettingsSelectChoice::new("blue", "SKCM Blue").with_group(Some("Keyboard")),
        ]);

        let visual = cx.add_empty_window();
        visual.update(|window, cx| {
            assert_eq!(SearchableListDelegate::sections_count(&grouped, cx), 2);
            assert_eq!(SearchableListDelegate::items_count(&grouped, 1), 2);
            assert!(SearchableListDelegate::section(&grouped, 0).is_some());
            assert_eq!(
                SearchableListDelegate::position(&grouped, &SharedString::from("navy")),
                Some(IndexPath::default().section(1).row(1))
            );
            // A plugin that has not adopted groups keeps its flat dropdown.
            assert_eq!(SearchableListDelegate::sections_count(&legacy, cx), 1);
            assert!(SearchableListDelegate::section(&legacy, 0).is_none());
            assert_eq!(SearchableListDelegate::sections_count(&mixed, cx), 1);
            assert!(SearchableListDelegate::section(&mixed, 0).is_none());
            _ = grouped.perform_search("navy", window, cx);
            assert_eq!(grouped.sections_count(cx), 1);
            assert_eq!(grouped.items_count(0), 1);
            assert_eq!(
                grouped.position(&SharedString::from("navy")),
                Some(IndexPath::default())
            );
            _ = grouped.perform_search("Keyboard", window, cx);
            assert_eq!(grouped.sections_count(cx), 1);
            assert_eq!(grouped.items_count(0), 0);
            assert!(SearchableListDelegate::section(&grouped, 0).is_some());
            _ = grouped.perform_search("missing", window, cx);
            assert_eq!(grouped.sections_count(cx), 0);
            _ = grouped.perform_search("", window, cx);
            assert_eq!(
                grouped.position(&SharedString::from("navy")),
                Some(IndexPath::default().section(1).row(1))
            );
        });
    }
}
