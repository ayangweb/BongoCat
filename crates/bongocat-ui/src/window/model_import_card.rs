//! The single model-import entry: a card that opens the folder picker.
//!
//! Importing used to be three controls — a title field, one button per source
//! kind and a commit button — even though choosing the folder to import is the
//! only decision the user actually makes. The card collapses that into one
//! affordance: pressing it opens the native folder picker, and the selection
//! starts the import on the spot.
//!
//! The card is also where the import reports itself. While the run is in flight
//! the prompt is replaced by the one step that is happening right now, drawn as
//! the component library's [`Spinner`] above that step's own line. A later step
//! *replaces* the line rather than stacking under it: the card is one grid cell
//! the size of a model card, so a growing list of finished steps would turn a
//! two-phase wait into a log the cell cannot hold, and a check beside a stale
//! line says less than the line that is actually running.
//!
//! Nothing here talks to the settings service. The card reports that the user
//! pressed it and the cancel request, and the page owns everything the import
//! then does.

use super::models::{MODEL_CARD_MIN_HEIGHT, MODEL_CARD_WIDTH};
use super::*;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{Sizable as _, Size, button::Button, spinner::Spinner};
use gpui_kit::{ElementId, RenderOnce};

/// The width the card occupies in the model grid.
///
/// It is the model card's own width, taken from the grid rather than repeated —
/// the two are cells in the same grid, so they are not allowed to disagree. Its
/// height is not set here either: the grid stretches every cell on a row to that
/// row's tallest one, which is a model card, and [`MODEL_CARD_MIN_HEIGHT`] is
/// only the floor the card keeps when it is alone on its row.
const CARD_WIDTH: Pixels = px(MODEL_CARD_WIDTH);

/// The spacing between the spinner and the step line under it.
///
/// This is the tip offset Ant Design uses under a `Spin` indicator, and it is the
/// whole reason the two read as one block rather than as two centred lines.
const STEP_GAP: Pixels = px(8.0);

/// The extra space between the step line and the cancel button under it.
///
/// A control needs more air from the sentence it belongs to than the sentence
/// needs from its indicator, so the button takes [`STEP_GAP`] from the column's
/// own gap plus this again — 16px in total, twice the tip offset above it.
const CANCEL_GAP: Pixels = px(8.0);

type ActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// The import entry.
///
/// `step` decides which of the two faces the card shows: `None` is the pressable
/// upload prompt, `Some` is the progress face for that single step. The caller
/// owns the state behind both, so the card never keeps a copy of an import it
/// does not own.
#[derive(IntoElement)]
pub(super) struct ModelImportCard {
    id: ElementId,
    title: SharedString,
    hint: SharedString,
    step: Option<SharedString>,
    cancel_label: Option<SharedString>,
    cancel_disabled: bool,
    /// Whether the prompt accepts a press. A run in flight or another page
    /// command in flight leaves the card visible but inert.
    interactive: bool,
    focus: Option<FocusHandle>,
    tab_index: isize,
    on_open: Option<ActionCallback>,
    on_cancel: Option<ActionCallback>,
}

impl ModelImportCard {
    pub(super) fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            hint: SharedString::default(),
            step: None,
            cancel_label: None,
            cancel_disabled: false,
            interactive: true,
            focus: None,
            tab_index: 0,
            on_open: None,
            on_cancel: None,
        }
    }

    /// The line under the title that says what a press will open.
    pub(super) fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = hint.into();
        self
    }

    /// The step that is running right now, or `None` for the upload prompt.
    ///
    /// One step, not a history: the caller passes what the run is doing at this
    /// moment, and a later step arrives as a replacement for this one.
    pub(super) fn step(mut self, step: Option<SharedString>) -> Self {
        self.step = step;
        self
    }

    /// Offer a cancel button while a step is running.
    pub(super) fn cancel(
        mut self,
        label: impl Into<SharedString>,
        disabled: bool,
        on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.cancel_label = Some(label.into());
        self.cancel_disabled = disabled;
        self.on_cancel = Some(Rc::new(on_cancel));
        self
    }

    pub(super) fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub(super) fn track_focus(mut self, focus: Option<FocusHandle>) -> Self {
        self.focus = focus;
        self
    }

    pub(super) fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    /// Called when the user presses the prompt, by pointer or by keyboard.
    pub(super) fn on_open(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(callback));
        self
    }
}

impl RenderOnce for ModelImportCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            title,
            hint,
            step,
            cancel_label,
            cancel_disabled,
            interactive,
            focus,
            tab_index,
            on_open,
            on_cancel,
        } = self;
        let tokens = Tokens::from_theme(cx);

        if let Some(step) = step {
            return progress_card(id, step, cancel_label, cancel_disabled, on_cancel, tokens)
                .into_any_element();
        }

        let card = div()
            .id((id, "trigger"))
            .w(CARD_WIDTH)
            .min_h(px(MODEL_CARD_MIN_HEIGHT))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_dashed()
            .border_color(tokens.border)
            .bg(tokens.canvas)
            .key_context("SettingsControl")
            .tab_index(tab_index)
            .child(
                Icon::new(gpui_kit::assets::IconName::Upload)
                    .with_size(Size::Medium)
                    .text_color(tokens.muted),
            )
            .child(div().text_color(tokens.text).child(title))
            .child(div().text_sm().text_color(tokens.muted).child(hint))
            .test_support();
        let card = match focus {
            Some(focus) => card.track_focus(&focus),
            None => card,
        };
        match (interactive, on_open) {
            (true, Some(on_open)) => {
                let key_open = on_open.clone();
                card.cursor_pointer()
                    .hover(|style| style.border_color(tokens.accent))
                    .on_click(move |_, window, cx| on_open(window, cx))
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if is_activation_key(event) {
                            cx.stop_propagation();
                            key_open(window, cx);
                        }
                    })
                    .into_any_element()
            }
            // The prompt keeps its place while another command settles; it just
            // opens nothing until that command is done.
            _ => card.opacity(0.6).into_any_element(),
        }
    }
}

/// The progress face: the step that is running right now, plus the cancel control
/// while that step can still be stopped.
///
/// The arrangement follows Ant Design's `Spin` with a tip: the indicator sits
/// above the step's line, the two are centred as one block, and the block is
/// centred in the card. `gpui-component`'s [`Spinner`] at [`Size::Medium`] is the
/// 32px indicator Ant calls "large", and [`STEP_GAP`] is the tip offset it puts
/// under the indicator; the line itself is the secondary text a tip uses, so it
/// never competes with the spinner for attention.
///
/// The cancel button joins that same block instead of being pinned to a corner:
/// centred under the step line it belongs to, with [`CANCEL_GAP`] of extra air,
/// so the whole wait reads as one group the user can read top to bottom — an
/// off-to-the-side control would read as belonging to the card, not the wait.
fn progress_card(
    id: ElementId,
    step: SharedString,
    cancel_label: Option<SharedString>,
    cancel_disabled: bool,
    on_cancel: Option<ActionCallback>,
    tokens: Tokens,
) -> impl IntoElement {
    // The wait block is built first so the cancel control can join it as a
    // third centred child, then handed to the card that centres it.
    let root_id = id.clone();
    let mut wait = div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(STEP_GAP)
        .child(Spinner::new().with_size(Size::Medium).color(tokens.accent))
        .child(
            div()
                .id((id.clone(), "step"))
                .w_full()
                .text_center()
                .text_sm()
                .text_color(tokens.muted)
                .test_support()
                .child(step),
        );
    if let (Some(label), Some(on_cancel)) = (cancel_label, on_cancel) {
        wait = wait.child(
            // The column's `items_center` is what centres the button; the
            // wrapper exists for observability, since a bare [`Button`] does
            // not register in the test snapshot on its own.
            div()
                .id((id, "cancel"))
                .mt(CANCEL_GAP)
                .test_support()
                .child(
                    Button::new("cancel-import")
                        .label(label)
                        .with_size(Size::Small)
                        .disabled(cancel_disabled)
                        .on_click(move |_, window, cx| on_cancel(window, cx)),
                ),
        );
    }
    div()
        .id(root_id)
        .w(CARD_WIDTH)
        .min_h(px(MODEL_CARD_MIN_HEIGHT))
        .flex_none()
        .flex()
        .flex_col()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas)
        .test_support()
        // `flex_1` hands the block whatever height the cell was stretched to
        // by the grid, so the wait is centred in the card instead of hanging
        // under its top edge when the card is as tall as a model card.
        .child(wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, FocusHandle, Render, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    /// The card id the harness hands the component; its children hang off it.
    const CARD: &str = "import-card";

    /// The same construction the component uses for its children, so a query
    /// here and a registration there cannot drift apart.
    fn part(name: &'static str) -> ElementId {
        ElementId::from((ElementId::from(CARD), name))
    }

    /// What the card reported, and how it is currently configured.
    struct Harness {
        opened: Rc<RefCell<usize>>,
        step: Option<SharedString>,
        interactive: bool,
        focus: FocusHandle,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let opened = self.opened.clone();
            div().p_4().child(
                ModelImportCard::new(CARD, "Import model")
                    .hint("Click to choose a model folder")
                    .step(self.step.clone())
                    .interactive(self.interactive)
                    .track_focus(Some(self.focus.clone()))
                    .tab_index(20)
                    .on_open(move |_, _| *opened.borrow_mut() += 1),
            )
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        step: Option<SharedString>,
        interactive: bool,
    ) -> (Entity<Harness>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(move |_, cx| Harness {
            opened: Rc::new(RefCell::new(0)),
            step,
            interactive,
            focus: cx.focus_handle(),
        })
    }

    /// The prompt is the picker's trigger, by pointer and by keyboard.
    #[gpui_kit::test]
    fn pressing_the_card_opens_the_picker_once(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, None, true);
        visual.update(|window, cx| window.render_frame(cx));

        visual.update(|window, cx| window.click(part("trigger"), cx));
        assert_eq!(
            view.read_with(visual, |view, _| *view.opened.borrow()),
            1,
            "a press on the prompt must open the picker exactly once"
        );

        let focus = view.read_with(visual, |view, _| view.focus.clone());
        visual.update(|window, cx| window.focus(&focus, cx));
        visual.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(visual, |view, _| *view.opened.borrow()),
            2,
            "Enter on the focused prompt must open the picker too"
        );
    }

    /// The progress face replaces the prompt, and the second half of a two-phase
    /// wait replaces the first.
    ///
    /// What a step *says* is pinned by `import_card_step`'s own tests, and that
    /// only one step can be on screen is a property of the card's shape — it
    /// takes a single step rather than a list — so neither is asserted here. What
    /// this pins is the part the card owns: the prompt gives way to the step, and
    /// a later step leaves the same single line on screen with no prompt under it
    /// and no second row beside it.
    #[gpui_kit::test]
    fn the_running_step_replaces_the_prompt_and_the_step_before_it(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, Some("Importing model…".into()), true);
        visual.update(|window, cx| window.render_frame(cx));
        assert!(
            visual.update(|window, _| window.try_find(part("step")).is_some()),
            "the step that is running must be drawn"
        );
        assert!(
            visual.update(|window, _| window.try_find(part("trigger")).is_none()),
            "a run in flight must not draw the upload prompt underneath"
        );

        view.update(visual, |view, cx| {
            view.step = Some("Capturing cover…".into());
            cx.notify();
        });
        visual.update(|window, cx| window.render_frame(cx));
        assert!(
            visual.update(|window, _| window.try_find(part("step")).is_some()),
            "the next step must be drawn in the same place"
        );
        assert!(
            visual.update(|window, _| window.try_find(part("trigger")).is_none()),
            "the next step must not bring the prompt back"
        );
    }

    /// The prompt keeps its place while another command settles, but opens
    /// nothing: the page's commands are serialized, and this card is not exempt.
    #[gpui_kit::test]
    fn the_prompt_opens_nothing_while_a_command_is_in_flight(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, None, false);
        visual.update(|window, cx| window.render_frame(cx));
        assert!(
            visual.update(|window, _| window.try_find(part("trigger")).is_some()),
            "an in-flight command must not make the prompt disappear"
        );

        visual.update(|window, cx| window.click(part("trigger"), cx));
        assert_eq!(
            view.read_with(visual, |view, _| *view.opened.borrow()),
            0,
            "a press must not open the picker while another command is in flight"
        );
    }

    /// A harness whose run in flight offers the cancel control.
    struct CancelHarness {
        cancelled: Rc<RefCell<usize>>,
    }

    impl Render for CancelHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let cancelled = self.cancelled.clone();
            div().p_4().child(
                ModelImportCard::new(CARD, "Import model")
                    .step(Some("Importing model…".into()))
                    .cancel("Cancel", false, move |_, _| *cancelled.borrow_mut() += 1),
            )
        }
    }

    /// The cancel control is part of the wait, not a corner of the card: it
    /// sits centred under the step line it can stop, with the line and the
    /// indicator it groups with.
    #[gpui_kit::test]
    fn the_cancel_button_is_centred_under_the_step_line(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(800.0), px(600.0)), |_, _| CancelHarness {
            cancelled: Rc::new(RefCell::new(0)),
        });
        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.update(|window, cx| window.render_frame(cx));

        let (card, step, cancel) = visual.update(|window, _| {
            (
                window.find(CARD).bounds(),
                window.find(part("step")).bounds(),
                window.find(part("cancel")).bounds(),
            )
        });
        assert_eq!(
            f32::from(cancel.center().x),
            f32::from(card.center().x),
            "the cancel button must be centred on the card, not pushed to a corner"
        );
        assert!(
            cancel.center().y > step.center().y,
            "the cancel button must sit below the step line it belongs to"
        );
    }

    /// The grid the page lays its cells out in: the card in both of its faces,
    /// and a neighbour standing in for a model card.
    struct GridHarness;

    impl Render for GridHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            super::models::model_grid()
                .child(
                    ModelImportCard::new("prompt-cell", "Import model")
                        .hint("Click to choose a model folder"),
                )
                .child(
                    ModelImportCard::new("progress-cell", "Import model")
                        .step(Some("Importing model…".into())),
                )
                .child(
                    div()
                        .id("neighbour-cell")
                        .flex_none()
                        .w(px(240.0))
                        .h(tall_cell_height())
                        .test_support(),
                )
        }
    }

    /// A cell taller than the import card's own floor, which is what a model card
    /// with a status line under its title amounts to.
    fn tall_cell_height() -> Pixels {
        px(MODEL_CARD_MIN_HEIGHT + 40.0)
    }

    /// The card takes the height of the model cards on its row.
    ///
    /// It is a grid cell like any other, so it carries no height of its own: the
    /// grid stretches every cell on a line to that line's tallest one, and the
    /// tallest one is a model card. The neighbour here is that card's height, so
    /// a card that kept its own two lines would come out short — and the check
    /// covers both faces, because the progress face replaces the prompt inside
    /// the same cell.
    #[gpui_kit::test]
    fn the_card_takes_the_height_of_the_model_cards_beside_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(800.0), px(600.0)), |_, _| GridHarness);
        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.update(|window, cx| window.render_frame(cx));

        let expected = tall_cell_height();
        for (cell, id) in [
            (
                "the upload prompt",
                ElementId::from((ElementId::from("prompt-cell"), "trigger")),
            ),
            ("the running step", ElementId::from("progress-cell")),
            ("the model card", ElementId::from("neighbour-cell")),
        ] {
            let height = visual.update(|window, _| {
                window
                    .try_find(id)
                    .unwrap_or_else(|| panic!("{cell} must be drawn"))
                    .bounds()
                    .size
                    .height
            });
            assert_eq!(
                height, expected,
                "{cell} must be as tall as the tallest cell on its row"
            );
        }
    }
}
