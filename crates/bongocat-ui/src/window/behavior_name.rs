//! Naming one motion or expression, in place.
//!
//! The rows and the random-playback checkboxes both name a behavior by position —
//! "Motion 3" — because the resource names inside a package are internal numbering the
//! user cannot see. A name is the only way to find the behavior you meant without
//! counting, so each row carries an optional one.
//!
//! The name is edited where it is shown: the pencil beside the label turns the label
//! into a text field on a click, the way a piece of editable text does in the component
//! libraries this window takes its controls from. The pencil is the only entry — the
//! name itself stays plain text, so reading or selecting a row never starts an edit.
//! Nothing is drawn in a second surface,
//! so the thing being renamed and the thing being typed are the same object in the same
//! place on screen — and a half-typed name is never drawn as the row's label, because
//! the label is not on screen while the field is.
//!
//! One draft exists at a time and it belongs to the view rather than to the row,
//! because a row is rebuilt every frame and the field has to survive that — the same
//! reason the model rename keeps its draft there.

use super::*;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::{Rems, component::Size};

/// The one behavior whose row is open for renaming.
///
/// `current` is the label the row shows today, so the field opens on something the
/// user recognises; saving it unchanged is a no-op rather than a write.
pub(crate) struct BehaviorNameDraft {
    pub(crate) model: SettingsModelKey,
    /// The `behavior_id` spelling the name is stored under.
    pub(crate) behavior_id: String,
    /// The numbered label, which is also what clearing the field returns to.
    pub(crate) current: String,
    pub(crate) input: Entity<InputState>,
    /// The field's focus handle, which the label gives up its click to.
    pub(crate) input_focus: FocusHandle,
}

impl BehaviorNameDraft {
    /// Whether this draft is the one a row would open.
    ///
    /// Only one editor exists at a time, and the pencil that opens it disappears
    /// while its own row is being edited, so this is a guard rather than a path the
    /// pointer can retrace: it makes a second arrival a no-op instead of a restart
    /// that discards what was typed.
    pub(crate) fn is_for(&self, model: &SettingsModelKey, behavior_id: &str) -> bool {
        self.model == *model && self.behavior_id == behavior_id
    }

    /// The draft's value as it will be stored.
    pub(crate) fn value(&self, cx: &App) -> String {
        sanitize_behavior_name_input(&self.input.read(cx).value())
    }

    /// Whether saving would write anything.
    ///
    /// Saving the name a row already shows writes nothing, so Enter on an unchanged
    /// field closes it without sending a request the service would answer by storing
    /// what it already has.
    pub(crate) fn can_save(&self, cx: &App) -> bool {
        self.value(cx) != self.current
    }
}

/// The longest name one row may carry, matching the configuration's own bound.
///
/// The field and the document agree on the number, so a name the field accepts is a
/// name the document accepts — and the page never has to decide what to do with a
/// value the service will reject.
pub(crate) const BEHAVIOR_NAME_MAXIMUM_CHARS: usize =
    bongocat_config::MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS;

/// The field's height, the same one the model-title field uses.
///
/// Reusing the number is deliberate: two rename fields at different sizes would make
/// the same text look like two different kinds of value.
const BEHAVIOR_NAME_SIZE: Size = models::MODEL_TITLE_SIZE;
const BEHAVIOR_NAME_LINE_HEIGHT: Rems = models::MODEL_TITLE_LINE_HEIGHT;

/// The draft's value as it will be stored.
///
/// The name is free-form display text, so control characters are dropped and the value
/// is trimmed and bounded before it is stored, exactly as a model title is. An empty
/// result is not a name: the request that carries it removes the row, which is what
/// clearing the field means.
pub(crate) fn sanitize_behavior_name_input(value: &str) -> String {
    let filtered: String = value.chars().filter(|c| !c.is_control()).collect();
    let filtered = filtered.trim();
    filtered.chars().take(BEHAVIOR_NAME_MAXIMUM_CHARS).collect()
}

/// Build the row's label as the field that replaces it while the row is renamed.
///
/// The editor takes the label's place in the row and nothing else about the row moves:
/// the chord frame, its play control and its clear control keep their positions, their
/// tab indices and their handlers, because this surface is built from the label rather
/// than added beside them. Nothing here joins the tab order — the row's three controls
/// are still the row's three tab stops — so the keyboard path to a rename is the
/// pointer one, exactly as it was when the label alone was the entry.
///
/// The surface carries no controls and no copy of its own: Enter saves, Escape
/// discards, and the field's focus being taken away saves. What the field says is only
/// its placeholder — set where the draft is opened — which names the value it holds.
pub(super) fn behavior_name_editor(
    draft: &BehaviorNameDraft,
    cx: &mut Context<SettingsView>,
) -> gpui_kit::AnyElement {
    let input = draft.input.clone();
    let focus = draft.input_focus.clone();
    let focus_for_click = focus.clone();
    let key = behavior_name_key(&ShortcutCaptureTarget::ModelBehavior {
        model: draft.model.clone(),
        behavior_id: draft.behavior_id.clone(),
    });

    div()
        .id(behavior_name_part_id(&key, "editor"))
        .key_context("SettingsControl")
        .track_focus(&focus)
        .min_w_0()
        .flex_1()
        .on_click(cx.listener(move |_view, _, window, cx| {
            window.focus(&focus_for_click, cx);
        }))
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                view.cancel_behavior_name(cx);
            }
        }))
        .child(
            div()
                .id(behavior_name_part_id(&key, "field"))
                .flex_1()
                .min_w_0()
                .child(
                    Input::new(&input)
                        .with_size(BEHAVIOR_NAME_SIZE)
                        .line_height(BEHAVIOR_NAME_LINE_HEIGHT)
                        .text_base(),
                )
                .test_support(),
        )
        .into_any_element()
}

/// One stable element id per part of a renamable row's name surface.
///
/// `key` is [`behavior_name_key`]: the behavior a row names, or the command it runs.
/// The parts are spelled out rather than derived from a position, because these
/// elements are queried by what they belong to rather than by where the row happens to
/// sit. `gpui-kit` needs an `ElementId`, and a target is a typed value that would have
/// to be flattened into one.
pub(super) fn behavior_name_part_id(key: &str, part: &str) -> gpui_kit::ElementId {
    format!("behavior-name-{key}-{part}").into()
}

/// The stable name one row's rename surface is identified by.
///
/// A behavior is named by the `behavior_id` it is stored under, so a query and a
/// registration agree on the row without either counting positions. A command has no
/// behavior to name, so it carries its own spelling — it never renders a rename
/// surface, but the label shares this id space.
pub(super) fn behavior_name_key(target: &ShortcutCaptureTarget) -> String {
    match target {
        ShortcutCaptureTarget::Command(command) => format!("command-{command}"),
        ShortcutCaptureTarget::ModelBehavior { behavior_id, .. } => behavior_id.clone(),
    }
}

/// The pencil that turns a row's label into the field above.
///
/// It sits after the name rather than replacing anything, so a row that has never been
/// renamed still reads as plain text with one extra glyph — and the glyph is the only
/// thing that says the name can be edited at all, and the only thing that starts one.
/// It carries no keyboard handler: it is a pointer affordance, so it stays out of the
/// tab order rather than becoming a fourth stop in a row that has three.
pub(super) fn behavior_name_edit_control(
    target: &ShortcutCaptureTarget,
    label: &'static str,
    color: Hsla,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let click_target = target.clone();
    div()
        .id(behavior_name_part_id(&behavior_name_key(target), "edit"))
        .flex_none()
        .child(
            Button::new(label)
                .ghost()
                .xsmall()
                .icon(Icon::new(gpui_kit::assets::IconName::SquarePen).text_color(color))
                .tooltip(label)
                .tab_stop(false),
        )
        .on_click(cx.listener(move |view, _, window, cx| {
            let Some(row) = view.shortcut_rows.get(&click_target).cloned() else {
                return;
            };
            view.open_behavior_name(&row, window, cx);
        }))
        .test_support()
}

impl SettingsView {
    /// Open the rename editor for one behavior row.
    ///
    /// The field starts on the label the row shows today, so the user edits the thing
    /// they can see rather than retyping a name they have to remember. Only a row that
    /// is a live model's behavior can be renamed: a name is stored against one model's
    /// `behavior_id`, so there is nothing to attach it to otherwise.
    pub(super) fn open_behavior_name(
        &mut self,
        row: &ShortcutRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let Some(playable) = row.playable.as_ref() else {
            return;
        };
        let behavior_id = match &row.target {
            ShortcutCaptureTarget::ModelBehavior { behavior_id, .. } => behavior_id.clone(),
            ShortcutCaptureTarget::Command(_) => return,
        };
        if self
            .behavior_name
            .as_ref()
            .is_some_and(|draft| draft.is_for(&playable.model, &behavior_id))
        {
            return;
        }
        // Opening another row's editor commits the one that was open. The editor is
        // one at a time, so the alternative to committing is discarding a name the
        // user had already typed — and leaving a row because the next one caught
        // their eye is not a reason to throw their typing away.
        if self.behavior_name.is_some() {
            self.confirm_behavior_name(cx);
        }
        let language = self
            .snapshot
            .as_ref()
            .map_or(SettingsLanguage::English, |snapshot| {
                snapshot.resolved_language
            });
        let locale = language.catalog_locale();
        let current = row.name(language);
        let placeholder = bongocat_i18n::text(locale, "shortcuts.behavior_names.rename.field");
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(current.clone())
        });
        let input_focus = input.read(cx).focus_handle(cx);
        // Enter saves, Escape discards, and losing the field to something else saves
        // rather than drops: a name the user typed is never thrown away by looking at
        // something else. The check is on the draft, so a blur from an editor that has
        // already been replaced can never write the one that replaced it.
        cx.subscribe(&input, |view, input, event: &InputEvent, cx| match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => view.confirm_behavior_name(cx),
            InputEvent::Blur => {
                let blurred = input.entity_id();
                if view
                    .behavior_name
                    .as_ref()
                    .is_some_and(|draft| draft.input.entity_id() == blurred)
                {
                    view.confirm_behavior_name(cx);
                }
            }
            InputEvent::Focus => {}
        })
        .detach();
        window.focus(&input_focus, cx);
        self.behavior_name = Some(BehaviorNameDraft {
            model: playable.model.clone(),
            behavior_id,
            current,
            input,
            input_focus,
        });
        cx.notify();
    }

    /// Drop the rename draft without writing anything.
    pub(super) fn cancel_behavior_name(&mut self, cx: &mut Context<Self>) {
        if self.behavior_name.take().is_some() {
            cx.notify();
        }
    }

    /// Store the draft's name, or clear it when the field ended up empty.
    ///
    /// An empty field is a removal rather than a stored blank: "go back to the
    /// numbered name" is what clearing a text field means, and storing an empty string
    /// would leave the page deciding whether an empty name is a name. A field still
    /// holding the label the row already shows writes nothing at all.
    pub(super) fn confirm_behavior_name(&mut self, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            self.behavior_name = None;
            return;
        };
        let Some(draft) = self.behavior_name.take() else {
            return;
        };
        if !draft.can_save(cx) {
            cx.notify();
            return;
        }
        let name = draft.value(cx);
        self.start_request(
            PendingOperation::ModelBehaviorName,
            Some(SettingValue::ModelBehaviorName {
                expected_config_revision,
                model: draft.model,
                behavior_id: draft.behavior_id,
                name,
            }),
            cx,
        );
    }
}
