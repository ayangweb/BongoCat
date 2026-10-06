//! Renaming one motion or expression.
//!
//! The rows and the random-playback checkboxes both name a behavior by position —
//! "Motion 3" — because the resource names inside a package are internal numbering the
//! user cannot see. A name is the only way to find the behavior you meant without
//! counting, so each row carries an optional one.
//!
//! The rename is a dialog rather than an inline field for one reason: a shortcut row's
//! label sits next to a control that records a chord on any click inside it, and a text
//! field in that row would have to share focus and tab order with a capture that opens
//! on Enter. A separate surface keeps the row's interaction exactly what it was, and
//! keeps a half-typed name from ever being drawn as the row's label.
//!
//! One draft exists at a time, and it belongs to the view rather than to the dialog
//! layer, because the page has to know whether a rename is pending before it opens the
//! layer at all — the same reason the Mver conversion dialog keeps its draft there.

use super::*;

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
    pub(crate) title: String,
    pub(crate) input: Entity<InputState>,
    /// Whether the rename surface has been put on screen for this draft.
    pub(crate) opened: bool,
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

/// Build one frame of the rename dialog.
///
/// The builder runs whenever the dialog layer paints, which happens inside
/// `SettingsView::render`, so it must not read the view: the draft and the language
/// are passed in instead. The footer is built by hand because the stock button pair has
/// no disabled state, and saving a name the field never accepted has to read as
/// disabled. Enter still routes through [`Dialog::on_ok`], so the keyboard path reads
/// the same predicate.
pub(super) fn build_behavior_name_dialog(
    draft: Rc<RefCell<BehaviorNameDialogSnapshot>>,
    locale: &'static str,
    view: WeakEntity<SettingsView>,
    dialog: Dialog,
    _window: &mut Window,
    cx: &mut App,
) -> Dialog {
    let text = |key: &str| SharedString::from(bongocat_i18n::text(locale, key));
    let title = draft.borrow().title.clone();
    let description = text("shortcuts.behavior_names.rename.description");
    let save_label = text("actions.save");
    let cancel_label = text("actions.cancel");
    let input = draft.borrow().input.clone();
    // Read once here, while the builder still has an `App`: a save is offered only
    // when the field holds a name the row does not already show, because saving the
    // current label writes nothing.
    let can_save = draft.borrow().can_save(cx);

    let on_ok_view = view.clone();
    let on_cancel_view = view.clone();
    let footer_save_view = view.clone();
    let footer_cancel_view = view.clone();
    let ok_draft = draft.clone();
    let save_draft = draft.clone();

    let viewport_height = _window.viewport_size().height;
    // GPUI Kit's Dialog positions from a top inset, before the surface has measured
    // its content. The surface here is a fixed width with one field and a description,
    // so the measured height is derived the same way the Mver dialog's is.
    let dialog_height = px(178.);
    let centered_margin_top = ((viewport_height - dialog_height) / 2.).max(px(16.));

    dialog
        .title(title)
        .w(px(440.))
        .margin_top(centered_margin_top)
        .button_props(
            DialogButtonProps::default()
                .ok_text(save_label.clone())
                .cancel_text(cancel_label.clone())
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    // Enter and the footer's save read the same shared draft, so a
                    // name the field never accepted can never be stored.
                    if !ok_draft.borrow().can_save(cx) {
                        return false;
                    }
                    let _ = on_ok_view.update(cx, |view, cx| view.confirm_behavior_name(cx));
                    window.close_dialog(cx);
                    true
                })
                .on_cancel(move |_, window, cx| {
                    let _ = on_cancel_view.update(cx, |view, cx| view.cancel_behavior_name(cx));
                    window.close_dialog(cx);
                    true
                }),
        )
        .footer(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("behavior-name-cancel")
                        .label(cancel_label.clone())
                        .ghost()
                        .tab_stop(true)
                        .on_click(move |_, window, cx| {
                            let _ = footer_cancel_view
                                .update(cx, |view, cx| view.cancel_behavior_name(cx));
                            window.close_dialog(cx);
                        }),
                )
                .child(
                    Button::new("behavior-name-save")
                        .label(save_label.clone())
                        .disabled(!can_save)
                        .tab_stop(can_save)
                        .on_click(move |_, window, cx| {
                            if !save_draft.borrow().can_save(cx) {
                                return;
                            }
                            let _ = footer_save_view
                                .update(cx, |view, cx| view.confirm_behavior_name(cx));
                            window.close_dialog(cx);
                        }),
                ),
        )
        .content(move |content, _window, cx| {
            let muted = cx.theme().muted_foreground;
            content.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(div().text_sm().text_color(muted).child(description.clone()))
                    .child(
                        Input::new(&input)
                            .with_size(BEHAVIOR_NAME_SIZE)
                            .line_height(BEHAVIOR_NAME_LINE_HEIGHT)
                            .text_base(),
                    ),
            )
        })
}

/// A render-safe copy of the rename draft.
///
/// The dialog builder runs while `SettingsView` is borrowed for rendering, so this
/// carries exactly the state the surface needs: the field, the title it was opened
/// with, and whether the value may be saved.
#[derive(Clone)]
pub(super) struct BehaviorNameDialogSnapshot {
    pub(crate) title: String,
    pub(crate) input: Entity<InputState>,
    pub(crate) current: String,
}

impl BehaviorNameDialogSnapshot {
    pub(super) fn from_draft(draft: &BehaviorNameDraft) -> Self {
        Self {
            title: draft.title.clone(),
            input: draft.input.clone(),
            current: draft.current.clone(),
        }
    }

    pub(super) fn can_save(&self, cx: &App) -> bool {
        // Saving the name a row already shows writes nothing, so the button reads as
        // disabled rather than as a command that would silently do nothing.
        sanitize_behavior_name_input(&self.input.read(cx).value()) != self.current
    }
}

impl SettingsView {
    /// Open the rename surface for one behavior row.
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
        let language = self
            .snapshot
            .as_ref()
            .map_or(SettingsLanguage::English, |snapshot| {
                snapshot.resolved_language
            });
        let locale = language.catalog_locale();
        let current = row.name(language);
        // The title names the subject rather than repeating the row's label: the field
        // already shows the label, and a title that quoted it would read as a second
        // copy of the same thing.
        let title = bongocat_i18n::text(locale, "shortcuts.behavior_names.rename.title").to_owned();
        let placeholder = bongocat_i18n::text(locale, "shortcuts.behavior_names.rename.field");
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(current.clone())
        });
        let input_focus = input.read(cx).focus_handle(cx);
        // The dialog reads the field live when it paints, so the only thing a change
        // has to do is ask for that repaint: it is what turns the save button on once
        // the field holds something the row does not already show.
        cx.subscribe(&input, |_view, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        window.focus(&input_focus, cx);
        self.behavior_name = Some(BehaviorNameDraft {
            model: playable.model.clone(),
            behavior_id,
            current,
            title,
            input,
            opened: false,
        });
        cx.notify();
    }

    /// Send one name, without going through the dialog.
    ///
    /// The dialog is the product path; this is the seam a rendered test drives, so the
    /// command, the revision it carries and the row identity it names can be asserted
    /// without a text field in the way. It takes the same guard as
    /// [`Self::confirm_behavior_name`], so a rename cannot overtake a request in flight.
    #[cfg(test)]
    pub(super) fn set_model_behavior_name_for_test(
        &mut self,
        model: SettingsModelKey,
        behavior_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.start_request(
            PendingOperation::ModelBehaviorName,
            Some(SettingValue::ModelBehaviorName {
                expected_config_revision,
                model,
                behavior_id,
                name,
            }),
            cx,
        );
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
    /// would leave the page deciding whether an empty name is a name.
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
        let name = sanitize_behavior_name_input(&draft.input.read(cx).value());
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

impl SettingsView {
    /// Put the rename surface on screen for a draft that has just been created.
    ///
    /// The layer is opened from render rather than from the click, because the click
    /// arrives on a row that is rebuilt every frame and the surface needs the
    /// render-safe copy of the draft rather than the draft itself — the same reason the
    /// Mver conversion dialog opens here. The draft records that it has been opened, so
    /// this runs once per rename rather than once per frame.
    pub(super) fn sync_behavior_name_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.behavior_name.as_ref() else {
            return;
        };
        if draft.opened {
            return;
        }
        let locale = self.display_language().catalog_locale();
        let snapshot = Rc::new(RefCell::new(BehaviorNameDialogSnapshot::from_draft(draft)));
        if let Some(draft) = self.behavior_name.as_mut() {
            draft.opened = true;
        }
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            // The builder runs again on every frame the dialog is on screen, reading
            // the snapshot rather than the view, which is already borrowed for render.
            build_behavior_name_dialog(snapshot.clone(), locale, view.clone(), dialog, window, cx)
        });
    }
}
