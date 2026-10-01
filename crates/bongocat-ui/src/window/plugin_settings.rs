//! One plugin's own settings, drawn from the schema the plugin declared.
//!
//! This is the whole of the "every plugin has its own configuration panel"
//! requirement, and it is worth being precise about what makes it work: **the window
//! draws controls and the plugin owns the values.** A plugin sends a schema — a
//! closed set of typed fields, each with a default, a range where the kind has one,
//! and a label in the plugin's own copy — and every row below is chosen by a field's
//! `kind` rather than by anything about the plugin.
//!
//! That is what keeps the two sides apart:
//!
//! * The window never interprets a value. It checks that what the user pressed fits the
//!   field the plugin declared, and it does not decide what a value means.
//! * The plugin never draws a control. It cannot: there is nothing in the protocol
//!   that names a widget, only a kind.
//! * `config.json` gains no plugin section, because the plugin writes its own file.
//!
//! So adding a plugin that wants a switch, a number, a line of text and a menu adds
//! no code to this file, and a plugin that wants something this window cannot draw is
//! refused at load time rather than shown as a hole.

use super::*;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::setting::{AnySettingField, SettingFieldType};
use std::collections::BTreeMap;

/// One plugin's open settings, as the page's panel holds them.
///
/// The values are a copy of the host's completed document, so a plugin's form shows
/// what its own file holds rather than a draft that could drift from it. A change goes
/// out through [`super::SettingsView::set_plugin_field`] and comes back on the next
/// snapshot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginSettingsDraft {
    /// Which plugin's settings these are.
    pub(crate) plugin: String,
    /// Every field's current value, complete because the host completes it.
    pub(crate) values: BTreeMap<String, SettingsFieldValue>,
}

impl PluginSettingsDraft {
    /// Whether there is anything to show.
    ///
    /// A panel that grew by nothing is a panel the user opened and saw no change,
    /// which is worse than a control that was not there.
    #[allow(
        dead_code,
        reason = "read by the page once a plugin has a field it cannot draw"
    )]
    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Every field of a plugin, as the rows the panel draws.
///
/// One row per field rather than a tabbed form: a settings form with more than a dozen
/// fields is a page the user scrolls rather than a page they navigate, and the scroll
/// is one control instead of a tab bar the plugin would also have to order.
///
/// Every control is a `SettingField`, so the panel is made of exactly the same parts
/// as the rest of the settings window. That is the point of a schema-driven form: a
/// plugin's panel is not a different kind of panel.
pub(super) fn field_rows(
    entry: &SettingsPluginEntry,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> Vec<SettingItem> {
    // Every value is cloned into its control's getter, so a row owns its own copy of
    // what it draws. That is what lets a `SettingItem` outlive the borrow of the
    // snapshot it was built from — which it must, because the page holds rows across
    // a snapshot that will be replaced under it.
    let mut rows = position_row(entry, view.clone(), language);
    rows.extend(entry.fields.iter().map(|field| {
        let mut row = SettingItem::new(
            SharedString::from(field.label.clone()),
            field_control(entry, field, view.clone()),
        );
        if let Some(description) = &field.description {
            row = row.description(SharedString::from(description.clone()));
        }
        row
    }));
    rows
}

/// The row that says where this plugin's panel is, for a plugin that draws one.
///
/// First, because it is the one thing on the form that is not the plugin's: placement is
/// the host's — one plugin per corner of the model window, chosen by the user — and a row
/// about the model's own arrangement belongs above the plugin's own settings rather than
/// among them.
///
/// Absent for a plugin that draws no panel, which is the whole of how a sound plugin stays
/// out of this: there is nothing on the model window to move, so there is no row to offer.
/// The menu carries only the positions that are free, because a menu that offered a corner
/// and then refused it would be a control that lies.
fn position_row(
    entry: &SettingsPluginEntry,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> Vec<SettingItem> {
    let Some(current) = entry.position.clone() else {
        return Vec::new();
    };
    let options: Vec<(SharedString, SharedString)> = entry
        .positions
        .iter()
        .map(|position| {
            (
                SharedString::from(position.value.clone()),
                SharedString::from(position.label.clone()),
            )
        })
        .collect();
    let current = SharedString::from(current.value);
    let plugin = entry.id.clone();
    let control = AnyField::dropdown(SettingField::dropdown(
        options,
        move |_app| current.clone(),
        {
            let plugin = plugin.clone();
            let view = view.clone();
            move |value, app| {
                view.update(app, |view, cx| {
                    view.set_plugin_position(&plugin, &value, cx);
                });
            }
        },
    ));
    vec![
        SettingItem::new(
            SharedString::from(bongocat_i18n::text(
                language.catalog_locale(),
                "settings.plugins.position_label",
            )),
            control,
        )
        .description(SharedString::from(bongocat_i18n::text(
            language.catalog_locale(),
            "settings.plugins.position_help",
        ))),
    ]
}

/// One plugin's open settings, as the page's items: the header, then the fields.
///
/// The panel is a sibling of the card grid rather than a section of one card,
/// because `gpui-kit` renders a `SettingItem` only inside a `SettingGroup` — a card
/// cannot host a field row at all. So the page holds one panel, under the grid, for
/// whichever plugin is open, and the header is what says whose it is.
///
/// The rows come from the schema the *running* plugin declared, so a plugin that
/// improved its settings in a later version is configured against the version that
/// is actually running.
pub(super) fn panel(
    entry: &SettingsPluginEntry,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> Vec<SettingItem> {
    let mut items = vec![header(entry.clone(), view.clone(), language)];
    items.extend(field_rows(entry, view, language));
    items
}

/// The row that names the plugin whose settings are open, and closes them.
///
/// It carries the rule above it as well as the name, because the panel appears under
/// a grid of cards rather than in a place of its own: without the rule, the first
/// field row would read as another row of the page instead of the start of
/// something.
///
/// The close control is a ghost button at the trailing edge, beside the name it
/// belongs to, so the panel is dismissed from where it was opened rather than from
/// somewhere below it.
fn header(
    entry: SettingsPluginEntry,
    view: Entity<SettingsView>,
    language: SettingsLanguage,
) -> SettingItem {
    let close_label: SharedString =
        bongocat_i18n::text(language.catalog_locale(), "actions.close").into();
    SettingItem::render(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
        let tokens = Tokens::from_theme(app);
        let close_view = view.clone();
        let close_id = entry.id.clone();
        div()
            .id(SharedString::from(format!(
                "plugin-settings-header-{close_id}"
            )))
            .w_full()
            .mt_4()
            .pt_4()
            .border_t_1()
            .border_color(tokens.border)
            // Observed so a test can tell an open panel from a closed one: the panel's
            // whole shape is that these rows appear and go away with the view's state.
            .test_support()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(super::plugins::card_icon(&entry, tokens))
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .font_semibold()
                            .truncate()
                            .child(super::plugins::card_heading(&entry)),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "plugin-settings-close-{close_id}"
                )))
                .icon(gpui_kit::assets::IconName::X)
                .tooltip(close_label.clone())
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _, app| {
                    let plugin = close_id.to_string();
                    close_view.update(app, |view, cx| {
                        view.toggle_plugin_settings(plugin, cx);
                    });
                }),
            )
    })
}

/// The control one field is edited with, whichever kind that is.
///
/// `SettingField` is generic over its value's type, so a function that answers "which
/// control is this field?" cannot return one of them directly. This is the five-armed
/// answer, and it is why the switch, the number, the menu and the line below are the
/// same controls the rest of the settings window uses rather than four
/// hand-rolled lookalikes.
enum PluginControl {
    Toggle(SettingField<bool>),
    Number(SettingField<f64>),
    Dropdown(SettingField<SharedString>),
    Input(SettingField<SharedString>),
    /// A field whose value is not of the kind the field declares.
    ///
    /// The host fits every value to its field before the window sees it, so this is a
    /// document from a newer build rather than something a user produced — and it
    /// draws nothing rather than drawing the wrong control.
    Empty(SettingField<SharedString>),
}

/// A [`PluginControl`] behind the object-safe field trait.
///
/// Eight delegating methods and no logic: every question `SettingItem` asks about a
/// field — what is it, is it dirty, reset it — is answered by whichever variant holds,
/// and asking it here rather than at each row is what keeps a five-armed match out of
/// the row builder. The constructors exist so the field builder reads
/// `AnyField::toggle(…)` rather than `AnyField(PluginControl::Toggle(…))`, which is the
/// same variant named twice in every arm.
struct AnyField(PluginControl);

impl AnyField {
    fn toggle(field: SettingField<bool>) -> Self {
        Self(PluginControl::Toggle(field))
    }
    fn number(field: SettingField<f64>) -> Self {
        Self(PluginControl::Number(field))
    }
    fn dropdown(field: SettingField<SharedString>) -> Self {
        Self(PluginControl::Dropdown(field))
    }
    fn input(field: SettingField<SharedString>) -> Self {
        Self(PluginControl::Input(field))
    }
    fn empty(field: SettingField<SharedString>) -> Self {
        Self(PluginControl::Empty(field))
    }
}

impl AnySettingField for AnyField {
    fn as_any(&self) -> &dyn std::any::Any {
        match &self.0 {
            PluginControl::Toggle(field) => field.as_any(),
            PluginControl::Number(field) => field.as_any(),
            PluginControl::Dropdown(field) => field.as_any(),
            PluginControl::Input(field) => field.as_any(),
            PluginControl::Empty(field) => field.as_any(),
        }
    }

    fn type_name(&self) -> &'static str {
        match &self.0 {
            PluginControl::Toggle(field) => field.type_name(),
            PluginControl::Number(field) => field.type_name(),
            PluginControl::Dropdown(field) => field.type_name(),
            PluginControl::Input(field) => field.type_name(),
            PluginControl::Empty(field) => field.type_name(),
        }
    }

    fn type_id(&self) -> std::any::TypeId {
        match &self.0 {
            PluginControl::Toggle(field) => field.type_id(),
            PluginControl::Number(field) => field.type_id(),
            PluginControl::Dropdown(field) => field.type_id(),
            PluginControl::Input(field) => field.type_id(),
            PluginControl::Empty(field) => field.type_id(),
        }
    }

    fn field_type(&self) -> &SettingFieldType {
        match &self.0 {
            PluginControl::Toggle(field) => field.field_type(),
            PluginControl::Number(field) => field.field_type(),
            PluginControl::Dropdown(field) => field.field_type(),
            PluginControl::Input(field) => field.field_type(),
            PluginControl::Empty(field) => field.field_type(),
        }
    }

    fn style(&self) -> &gpui_kit::StyleRefinement {
        match &self.0 {
            PluginControl::Toggle(field) => field.style(),
            PluginControl::Number(field) => field.style(),
            PluginControl::Dropdown(field) => field.style(),
            PluginControl::Input(field) => field.style(),
            PluginControl::Empty(field) => field.style(),
        }
    }

    fn is_resettable(&self, cx: &App) -> bool {
        match &self.0 {
            PluginControl::Toggle(field) => field.is_resettable(cx),
            PluginControl::Number(field) => field.is_resettable(cx),
            PluginControl::Dropdown(field) => field.is_resettable(cx),
            PluginControl::Input(field) => field.is_resettable(cx),
            PluginControl::Empty(field) => field.is_resettable(cx),
        }
    }

    fn reset(&self, window: &mut Window, cx: &mut App) {
        match &self.0 {
            PluginControl::Toggle(field) => field.reset(window, cx),
            PluginControl::Number(field) => field.reset(window, cx),
            PluginControl::Dropdown(field) => field.reset(window, cx),
            PluginControl::Input(field) => field.reset(window, cx),
            PluginControl::Empty(field) => field.reset(window, cx),
        }
    }
}

/// One field's control, chosen by the field's own kind.
///
/// Exhaustive on purpose: a sixth kind without an arm here would be a field that
/// renders with nothing on its right, which is a compile error here rather than a
/// blank row in a user's dialog. The four real arms are the four kinds a plugin's
/// schema can declare, and each is a control the rest of the settings window already
/// uses.
fn field_control(
    entry: &SettingsPluginEntry,
    field: &SettingsPluginField,
    view: Entity<SettingsView>,
) -> AnyField {
    let plugin = entry.id.clone();
    let key = field.key.clone();
    let value = entry
        .value_of(&field.key)
        .cloned()
        .unwrap_or_else(|| field.default.clone());
    match (control_kind(field.kind, &value), value) {
        (ControlKind::Toggle, SettingsFieldValue::Bool(current)) => AnyField::toggle(
            SettingField::switch(move |_app| current, report_flag(&plugin, &key, view)),
        ),
        (ControlKind::Number, SettingsFieldValue::Integer(current)) => {
            // A whole number is edited on the same number input as a real one and
            // rounded on the way out, because a spinner that could produce `25.5`
            // minutes would be a value the plugin then has to refuse.
            let current = current as f64;
            AnyField::number(SettingField::number_input(
                number_options(field),
                move |_app| current,
                {
                    let plugin = plugin.clone();
                    let key = key.clone();
                    let view = view.clone();
                    move |value, app| {
                        view.update(app, |view, cx| {
                            view.set_plugin_field(
                                &plugin,
                                &key,
                                SettingsFieldValue::Integer(value.round() as i64),
                                cx,
                            );
                        });
                    }
                },
            ))
        }
        (ControlKind::Number, SettingsFieldValue::Decimal(current)) => {
            AnyField::number(SettingField::number_input(
                number_options(field),
                move |_app| current,
                report_number(&plugin, &key, view),
            ))
        }
        (ControlKind::Choice, SettingsFieldValue::Text(current)) => {
            // A menu offers the plugin's own options with its own labels, already
            // resolved for the user's language — so the window never has a list of
            // choices to localize on a plugin's behalf.
            let options: Vec<(SharedString, SharedString)> = field
                .options
                .iter()
                .map(|option| {
                    (
                        SharedString::from(option.value.clone()),
                        SharedString::from(option.label.clone()),
                    )
                })
                .collect();
            let current = SharedString::from(current);
            AnyField::dropdown(SettingField::dropdown(
                options,
                move |_app| current.clone(),
                report(&plugin, &key, view),
            ))
        }
        (ControlKind::Input, SettingsFieldValue::Text(current)) => {
            let current = SharedString::from(current);
            AnyField::input(SettingField::input(
                move |_app| current.clone(),
                report(&plugin, &key, view),
            ))
        }
        (ControlKind::Empty, _) => AnyField::empty(SettingField::element(
            |_: &RenderOptions, _: &mut Window, _: &mut App| div().into_any_element(),
        )),
        // A value whose kind the plugin's own schema did not declare draws nothing
        // rather than drawing the wrong control. The host fits every value to its
        // field before the window sees it, so this is a document from a newer build
        // rather than something a user produced.
        (_, _) => AnyField::empty(SettingField::element(
            |_: &RenderOptions, _: &mut Window, _: &mut App| div().into_any_element(),
        )),
    }
}

/// Which control a field of `kind` holding `value` is edited with.
///
/// Split from the control itself so the mapping can be asserted without a view: the
/// kind is the whole of the decision, and the arms below are the four controls the
/// settings window already has.
fn control_kind(kind: SettingsFieldKind, value: &SettingsFieldValue) -> ControlKind {
    match (kind, value) {
        (SettingsFieldKind::Toggle, SettingsFieldValue::Bool(_)) => ControlKind::Toggle,
        (SettingsFieldKind::Integer | SettingsFieldKind::Decimal, _) => ControlKind::Number,
        (SettingsFieldKind::Choice, SettingsFieldValue::Text(_)) => ControlKind::Choice,
        (SettingsFieldKind::Text, SettingsFieldValue::Text(_)) => ControlKind::Input,
        _ => ControlKind::Empty,
    }
}

/// The control one field is edited with, named rather than matched on inline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlKind {
    Toggle,
    Number,
    Choice,
    Input,
    Empty,
}

/// A setter for the switch, which produces a flag rather than text.
fn report_flag(
    plugin: &str,
    key: &str,
    view: Entity<SettingsView>,
) -> impl Fn(bool, &mut App) + 'static {
    let plugin = plugin.to_string();
    let key = key.to_string();
    move |value, app| {
        view.update(app, |view, cx| {
            view.set_plugin_field(&plugin, &key, SettingsFieldValue::Bool(value), cx);
        });
    }
}

/// A setter for a real number, which produces a number rather than text.
fn report_number(
    plugin: &str,
    key: &str,
    view: Entity<SettingsView>,
) -> impl Fn(f64, &mut App) + 'static {
    let plugin = plugin.to_string();
    let key = key.to_string();
    move |value, app| {
        view.update(app, |view, cx| {
            view.set_plugin_field(&plugin, &key, SettingsFieldValue::Decimal(value), cx);
        });
    }
}

/// A setter that reports one field's new value to the view as text.
///
/// The menu and the line share this because they both produce a `SharedString` and the
/// only difference is what the value means — which is the plugin's business, not the
/// window's.
fn report(
    plugin: &str,
    key: &str,
    view: Entity<SettingsView>,
) -> impl Fn(SharedString, &mut App) + 'static {
    let plugin = plugin.to_string();
    let key = key.to_string();
    move |value, app| {
        view.update(app, |view, cx| {
            view.set_plugin_field(
                &plugin,
                &key,
                SettingsFieldValue::Text(value.to_string()),
                cx,
            );
        });
    }
}

/// The bounds a numeric field's input is given.
///
/// The plugin's own bounds, so a plugin that declares "one to a hundred and twenty"
/// gets a spinner that cannot leave that range — and one that declares no range gets
/// an unbounded one, which is its own decision rather than a host's guess.
fn number_options(field: &SettingsPluginField) -> NumberFieldOptions {
    NumberFieldOptions {
        min: field.minimum.unwrap_or(f64::MIN),
        max: field.maximum.unwrap_or(f64::MAX),
        step: field.step.unwrap_or(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_ui_protocol::SettingsFieldOption;
    use std::collections::BTreeMap;

    fn field(kind: SettingsFieldKind) -> SettingsPluginField {
        SettingsPluginField {
            key: "setting".to_string(),
            label: "Setting".to_string(),
            description: None,
            kind,
            default: match kind {
                SettingsFieldKind::Toggle => SettingsFieldValue::Bool(false),
                SettingsFieldKind::Integer => SettingsFieldValue::Integer(1),
                SettingsFieldKind::Decimal => SettingsFieldValue::Decimal(0.5),
                _ => SettingsFieldValue::Text(String::new()),
            },
            minimum: None,
            maximum: None,
            step: None,
            unit: None,
            placeholder: None,
            multiline: false,
            options: Vec::new(),
        }
    }

    fn entry_with(fields: Vec<SettingsPluginField>) -> SettingsPluginEntry {
        let mut values = std::collections::BTreeMap::new();
        for field in &fields {
            values.insert(field.key.clone(), field.default.clone());
        }
        SettingsPluginEntry {
            id: "pomodoro".to_string(),
            name: "Pomodoro".to_string(),
            installed: true,
            enabled: true,
            running: true,
            fields,
            values,
            ..SettingsPluginEntry::default()
        }
    }

    #[test]
    fn a_numeric_fields_input_is_bounded_by_the_plugins_own_range() {
        // The plugin declares "one to a hundred and twenty", so the spinner cannot
        // leave that range — and one that declares no range gets an unbounded input,
        // which is its own decision rather than a host's guess.
        let mut bounded = field(SettingsFieldKind::Integer);
        bounded.minimum = Some(1.0);
        bounded.maximum = Some(120.0);
        bounded.step = Some(5.0);
        let options = number_options(&bounded);
        assert_eq!(options.min, 1.0);
        assert_eq!(options.max, 120.0);
        assert_eq!(options.step, 5.0);

        let unbounded = field(SettingsFieldKind::Decimal);
        let options = number_options(&unbounded);
        assert_eq!(options.min, f64::MIN);
        assert_eq!(options.max, f64::MAX);
    }

    #[test]
    fn every_kind_a_schema_can_declare_reaches_a_control() {
        // The mapping is the whole of a schema-driven form, so a kind with no arm here
        // would be a field that renders with nothing on its right. This asserts the
        // decision rather than the widget, because the widget is gpui-kit's business.
        assert_eq!(SettingsFieldKind::ALL.len(), 5);
        let expected = [
            (
                SettingsFieldKind::Toggle,
                SettingsFieldValue::Bool(false),
                ControlKind::Toggle,
            ),
            (
                SettingsFieldKind::Integer,
                SettingsFieldValue::Integer(1),
                ControlKind::Number,
            ),
            (
                SettingsFieldKind::Decimal,
                SettingsFieldValue::Decimal(0.5),
                ControlKind::Number,
            ),
            (
                SettingsFieldKind::Text,
                SettingsFieldValue::Text(String::new()),
                ControlKind::Input,
            ),
            (
                SettingsFieldKind::Choice,
                SettingsFieldValue::Text(String::new()),
                ControlKind::Choice,
            ),
        ];
        for (kind, value, control) in expected {
            assert_eq!(
                control_kind(kind, &value),
                control,
                "{kind:?} holding a value of its own kind reaches {control:?}"
            );
        }
    }

    #[test]
    fn a_value_whose_kind_is_not_the_fields_draws_nothing_rather_than_the_wrong_control() {
        // The host fits every value to its field before the window sees it, so this is
        // a document from a newer build rather than something a user produced — and a
        // blank row is better than a number where a menu belongs.
        let wrong = SettingsFieldValue::Integer(1);
        for kind in [SettingsFieldKind::Choice, SettingsFieldKind::Text] {
            assert_eq!(
                control_kind(kind, &wrong),
                ControlKind::Empty,
                "{kind:?} holding a number draws nothing"
            );
        }
        assert_eq!(
            control_kind(SettingsFieldKind::Toggle, &wrong),
            ControlKind::Empty,
            "and so does a switch holding one"
        );
    }

    #[test]
    fn a_choice_is_drawn_from_the_options_the_plugin_declared() {
        let mut choice = field(SettingsFieldKind::Choice);
        choice.options = vec![
            SettingsFieldOption {
                value: "meow".to_string(),
                label: "Meow".to_string(),
            },
            SettingsFieldOption {
                value: "none".to_string(),
                label: "Silent".to_string(),
            },
        ];
        let entry = entry_with(vec![choice]);
        assert_eq!(
            entry.fields[0].options.len(),
            2,
            "and a menu offers the plugin's own options with its own labels, so the window never \\
             has a list of choices to localize on a plugin's behalf"
        );
    }

    #[test]
    fn a_plugin_with_no_fields_has_no_panel_to_open() {
        // The settings control is hidden for a plugin with nothing to change, and this
        // is the check behind that: a panel with no rows in it is a panel that grew by
        // nothing.
        let empty = PluginSettingsDraft {
            plugin: "pomodoro".to_string(),
            values: BTreeMap::new(),
        };
        assert!(empty.is_empty());
        let draft = PluginSettingsDraft {
            plugin: "pomodoro".to_string(),
            values: BTreeMap::from([("minutes".to_string(), SettingsFieldValue::Integer(25))]),
        };
        assert!(!draft.is_empty());
    }
}
