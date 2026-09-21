//! A confirmation surface for actions that cannot be undone.
//!
//! GPUI Kit ships no `PopConfirm`, so this is built on the `Popover` it does
//! ship: the trigger stays where the caller already draws it, and one sentence
//! with a confirm/cancel pair appears next to it. Nothing here is specific to a
//! particular action — the caller supplies the trigger, the sentence, the icon
//! and both labels.
//!
//! The surface reports the decision and nothing else. It never runs the action,
//! so the caller keeps ownership of what happens on confirm, on cancel and on
//! failure.
//!
//! There is no arrow. GPUI Kit's `Popover` gained one after the version this
//! workspace pins (0.6.4 has neither `arrow` nor `offset`), so a tail here would
//! be a hand-rolled shape that the next upgrade replaces. The surface works
//! without it; add `.arrow(true)` at the call site once the dependency moves.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::actions::Confirm;
use gpui_kit::component::{
    Icon, Selectable, Sizable as _, Size,
    button::{Button, ButtonVariant, ButtonVariants as _},
    popover::{Popover, PopoverState},
};
use gpui_kit::{
    Anchor, AnyElement, App, Context, Div, ElementId, Hsla, IntoElement, Pixels, RenderOnce,
    SharedString, Window, div, prelude::*, px,
};

use crate::SettingsLanguage;

/// The width the sentence is laid out in.
///
/// Left to size itself, the surface grows to whatever its longest line wants,
/// and a confirmation is opened from a card that is narrower than a long
/// sentence would make it. A fixed width keeps the wrap predictable and the
/// surface smaller than the thing that asked for it.
const PANEL_WIDTH: Pixels = px(220.);

/// The size of both buttons.
///
/// [`Size::Medium`], the component default, makes the pair wider than the
/// sentence it is answering. A confirmation is a footnote to the control it
/// guards, not a dialog of its own, so it is drawn at [`Size::Small`].
const BUTTON_SIZE: Size = Size::Small;

/// The style of the button that accepts.
///
/// Accepting is the emphasised choice and declining is the plain one. The
/// destructive reading is carried by the warning icon the caller supplies
/// rather than by the button's colour, so accepting is drawn in the primary
/// variant and not the danger one. Nothing here forces a caller into that
/// reading: a confirmation with no consequence is a bad confirmation, not a
/// different widget.
const ACCEPT_VARIANT: ButtonVariant = ButtonVariant::Primary;

/// The locale a label falls back to when the caller does not supply one.
///
/// Both buttons have to say *something*, and the only text this crate can read
/// without a language in hand is the catalog's own. Every call site in this app
/// passes both labels in the resolved language, so this is a safety net rather
/// than a path anything takes.
const FALLBACK_LABEL_LOCALE: &str = SettingsLanguage::EnglishUnitedStates.catalog_locale();

/// What the surface reports when its open state changes.
type OpenChangeCallback = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// What the surface reports when the user accepts.
type ConfirmCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// One sentence, a leading icon, and a confirm/cancel pair, opened from a
/// trigger the caller already draws.
///
/// The open state is the popover's unless [`PopConfirm::open`] is used, in which
/// case the caller owns it — which is what a trigger that is also reachable from
/// the keyboard needs, because the click that toggles the surface is the
/// popover's own.
#[derive(IntoElement)]
pub struct PopConfirm {
    id: ElementId,
    title: SharedString,
    icon: Option<(Icon, Hsla)>,
    confirm_label: Option<SharedString>,
    cancel_label: Option<SharedString>,
    anchor: Anchor,
    trigger: Option<AnyElement>,
    open: Option<bool>,
    on_open_change: Option<OpenChangeCallback>,
    on_confirm: Option<ConfirmCallback>,
}

impl PopConfirm {
    /// Create a confirmation that asks `title`.
    ///
    /// Nothing opens it until a trigger is set, and nothing happens on confirm
    /// until [`PopConfirm::on_confirm`] is.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            confirm_label: None,
            cancel_label: None,
            anchor: Anchor::TopLeft,
            trigger: None,
            open: None,
            on_open_change: None,
            on_confirm: None,
        }
    }

    /// The element the surface hangs off, and opens from when it is clicked.
    ///
    /// A trigger is an ordinary element rather than a `Button`: the model card's
    /// delete control is a focus-tracked wrapper around an icon button, and the
    /// surface has no business requiring a particular shape of it.
    pub fn trigger(mut self, trigger: impl IntoElement + 'static) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    /// Draw `icon` in `color` before the sentence.
    ///
    /// An icon is how the surface says what kind of action it is guarding — a
    /// warning triangle for a destructive one — so the colour is the caller's
    /// decision rather than the component's.
    pub fn icon(mut self, icon: impl Into<Icon>, color: Hsla) -> Self {
        self.icon = Some((icon.into(), color));
        self
    }

    /// The label of the button that accepts. Defaults to the catalog's.
    pub fn confirm_label(mut self, label: impl Into<SharedString>) -> Self {
        self.confirm_label = Some(label.into());
        self
    }

    /// The label of the button that declines. Defaults to the catalog's.
    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = Some(label.into());
        self
    }

    /// Which corner of the trigger the surface is anchored to, `TopLeft` by
    /// default. A control at the trailing edge of its container wants `TopRight`
    /// so the surface grows back over the container instead of past it.
    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    /// Take ownership of the open state.
    ///
    /// Set this together with [`PopConfirm::on_open_change`]: the surface then
    /// reports every transition — a press on the trigger, Enter or Space on it,
    /// Escape, a press outside — and waits for the caller to write it back.
    ///
    /// Writing the state back does not report again, so the caller can clear the
    /// surface from its own side without the clear bouncing off `on_open_change`.
    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Called with the new open state on every transition.
    pub fn on_open_change(
        mut self,
        callback: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(callback));
        self
    }

    /// Called when the user accepts, before the surface closes.
    ///
    /// The surface closes either way, so a caller that wants the confirmation to
    /// stay up after a failed action has to open it again.
    pub fn on_confirm(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(callback));
        self
    }
}

/// The sentence, with the leading icon when one was asked for.
///
/// A free function rather than a method because the popover rebuilds its content
/// on every frame: the closure that draws the surface is `Fn`, so it has to be
/// able to build the sentence again rather than move one in.
///
/// The icon and the text are centred against each other rather than aligned at
/// the top. An icon box is shorter than a line of text, so top alignment leaves
/// the glyph sitting visibly above the words it belongs to.
fn sentence(id: &ElementId, title: &SharedString, icon: Option<(Icon, Hsla)>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .when_some(icon, |this, (icon, color)| {
            this.child(
                div()
                    .id((id.clone(), "icon"))
                    .flex_shrink_0()
                    .child(Icon::new(icon).text_color(color))
                    .test_support(),
            )
        })
        .child(
            div()
                .id((id.clone(), "title"))
                .flex_1()
                .min_w_0()
                .child(title.clone())
                .test_support(),
        )
}

/// One of the surface's two decisions, wired to a press and to the keyboard.
///
/// A press is the obvious path; the keyboard one is not. The popover binds Enter
/// and Space on its own key context to "toggle the surface", and gpui dispatches
/// an action *before* it dispatches `on_key_down`, so a key handler on the button
/// would never run: the surface would close without reporting anything, which is
/// indistinguishable from a decline. The action is claimed on the wrapper, which
/// sits between the focused control and the popover's own handler — the only
/// position in the dispatch path that is reached first.
fn deciding_button<T: Into<ElementId>>(
    id: T,
    label: SharedString,
    variant: Option<ButtonVariant>,
    report: Option<ConfirmCallback>,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<T> {
    let id = id.into();
    let mut button = Button::new((id.clone(), "control"))
        .label(label)
        .with_size(BUTTON_SIZE);
    if let Some(variant) = variant {
        button = button.with_variant(variant);
    }
    let on_click = {
        let report = report.clone();
        cx.listener(move |state, _: &gpui_kit::ClickEvent, window, cx| {
            decide(state, window, cx, report.as_ref());
        })
    };
    let on_confirm = {
        let report = report.clone();
        cx.listener(move |state, _: &Confirm, window, cx| {
            cx.stop_propagation();
            decide(state, window, cx, report.as_ref());
        })
    };
    div()
        .id(id)
        .test_support()
        .on_click(on_click)
        .on_action(on_confirm)
        .child(button)
}

/// Report a decision and close the surface.
///
/// Both paths come through here, so "accepted" cannot mean one thing to a mouse
/// and another to the keyboard.
fn decide(
    state: &mut PopoverState,
    window: &mut Window,
    cx: &mut Context<PopoverState>,
    report: Option<&ConfirmCallback>,
) {
    if let Some(report) = report {
        report(window, cx);
    }
    state.dismiss(window, cx);
}

/// The pair of labels the buttons carry.
///
/// Both buttons have to say *something*, and the only text this crate can read
/// without a language in hand is the catalog's own. Every call site in this app
/// passes both labels in the resolved language, so the fallback is a safety net
/// rather than a path anything takes.
fn resolved_labels(
    confirm: Option<SharedString>,
    cancel: Option<SharedString>,
) -> (SharedString, SharedString) {
    let confirm = confirm.unwrap_or_else(|| {
        SharedString::from(bongocat_i18n::text(
            FALLBACK_LABEL_LOCALE,
            "actions.confirm",
        ))
    });
    let cancel = cancel.unwrap_or_else(|| {
        SharedString::from(bongocat_i18n::text(FALLBACK_LABEL_LOCALE, "actions.cancel"))
    });
    (confirm, cancel)
}

impl RenderOnce for PopConfirm {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let Self {
            id,
            title,
            icon,
            confirm_label,
            cancel_label,
            anchor,
            trigger,
            open,
            on_open_change,
            on_confirm,
        } = self;

        let (confirm_label, cancel_label) = resolved_labels(confirm_label, cancel_label);

        Popover::new(id.clone())
            .anchor(anchor)
            .when_some(trigger, |this, trigger| {
                this.trigger(TriggerSlot {
                    element: trigger,
                    open: false,
                })
            })
            .when_some(open, |this, open| this.open(open))
            .when_some(on_open_change, |this, callback| {
                this.on_open_change(move |open, window, cx| callback(open, window, cx))
            })
            .content(move |_, _, cx| {
                let confirm = deciding_button(
                    (id.clone(), "confirm"),
                    confirm_label.clone(),
                    Some(ACCEPT_VARIANT),
                    on_confirm.clone(),
                    cx,
                );
                let cancel =
                    deciding_button((id.clone(), "cancel"), cancel_label.clone(), None, None, cx);

                div()
                    .id((id.clone(), "surface"))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w(PANEL_WIDTH)
                    .child(sentence(&id, &title, icon.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(cancel)
                            .child(confirm),
                    )
                    .test_support()
            })
    }
}

/// Carries a caller's trigger into `Popover::trigger`.
///
/// `Popover::trigger` asks for a `Selectable` element because it marks the
/// trigger while the surface is open — the call a `Button` turns into its
/// selected style. A confirmation's trigger is an ordinary element, so this
/// satisfies the bound and keeps the selection it is handed without anywhere to
/// paint it: the open state is visible as the surface itself.
struct TriggerSlot {
    element: AnyElement,
    open: bool,
}

impl Selectable for TriggerSlot {
    fn selected(self, selected: bool) -> Self {
        Self {
            open: selected,
            ..self
        }
    }

    fn is_selected(&self) -> bool {
        self.open
    }
}

impl IntoElement for TriggerSlot {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        self.element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::ActiveTheme as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, Entity, FocusHandle, Render, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    /// The id the harness hands the component; its children hang off it.
    const PANEL: &str = "confirm";
    const TRIGGER: &str = "confirm-trigger";

    /// The same construction the component uses for its own children, so a
    /// query here and a registration there cannot drift apart silently.
    fn part(name: &'static str) -> ElementId {
        ElementId::from((ElementId::from(PANEL), name))
    }

    /// What the surface did, in the order it did it.
    #[derive(Default)]
    struct Log {
        confirmed: u32,
        opened: Vec<bool>,
    }

    struct Harness {
        log: Rc<RefCell<Log>>,
        open: bool,
        with_icon: bool,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let confirmed = self.log.clone();
            let mut confirm = PopConfirm::new(PANEL, "Delete this model?")
                .trigger(
                    div()
                        .id(TRIGGER)
                        .w(px(24.))
                        .h(px(24.))
                        .child("delete")
                        .test_support(),
                )
                .open(self.open)
                .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                    this.log.borrow_mut().opened.push(*open);
                    this.open = *open;
                    cx.notify();
                }))
                .on_confirm(move |_, _| confirmed.borrow_mut().confirmed += 1);
            if self.with_icon {
                confirm =
                    confirm.icon(gpui_kit::assets::IconName::TriangleAlert, cx.theme().danger);
            }
            div().p_4().child(confirm)
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        with_icon: bool,
    ) -> (Entity<Harness>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(move |_, _| Harness {
            log: Rc::new(RefCell::new(Log::default())),
            open: false,
            with_icon,
        })
    }

    /// A harness whose trigger carries a focus handle.
    ///
    /// The popover binds Enter on its own context to "toggle the surface" and the
    /// buttons bind it to "press me", so which one wins where can only be asked
    /// of a trigger that can hold focus — which is what the model card's delete
    /// control is, and what a control that is only clickable is not.
    struct KeyboardHarness {
        log: Rc<RefCell<Log>>,
        open: bool,
        trigger_focus: FocusHandle,
    }

    impl Render for KeyboardHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let confirmed = self.log.clone();
            div().p_4().child(
                PopConfirm::new(PANEL, "Delete this model?")
                    .trigger(
                        div()
                            .id(TRIGGER)
                            .w(px(24.))
                            .h(px(24.))
                            .track_focus(&self.trigger_focus)
                            .child("delete")
                            .test_support(),
                    )
                    .open(self.open)
                    .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                        this.log.borrow_mut().opened.push(*open);
                        this.open = *open;
                        cx.notify();
                    }))
                    .on_confirm(move |_, _| confirmed.borrow_mut().confirmed += 1),
            )
        }
    }

    fn keyboard_harness(
        cx: &mut TestAppContext,
    ) -> (Entity<KeyboardHarness>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(move |_, cx| KeyboardHarness {
            log: Rc::new(RefCell::new(Log::default())),
            open: false,
            trigger_focus: cx.focus_handle(),
        })
    }

    /// A closed surface draws nothing, and the trigger is what opens it.
    #[gpui_kit::test]
    fn the_trigger_opens_the_surface_and_reports_it(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, true);
        visual.update(|window, cx| window.render_frame(cx));
        assert!(
            visual.update(|window, _| window.try_find(part("surface")).is_none()),
            "a confirmation nobody opened must not be on screen"
        );

        visual.update(|window, cx| window.click(TRIGGER, cx));
        assert!(
            visual.update(|window, _| window.try_find(part("surface")).is_some()),
            "clicking the trigger must open the surface"
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.log.borrow().opened.clone()),
            vec![true],
            "the surface must report opening exactly once"
        );
    }

    /// The icon is drawn only when one was asked for.
    ///
    /// Its colour is not observable here: the snapshot reports identity, state
    /// and geometry, never painted pixels.
    #[gpui_kit::test]
    fn the_icon_follows_the_caller(cx: &mut TestAppContext) {
        for with_icon in [false, true] {
            let (view, visual) = harness(cx, with_icon);
            view.update(visual, |view, cx| {
                view.open = true;
                cx.notify();
            });
            visual.update(|window, cx| window.render_frame(cx));
            assert_eq!(
                visual.update(|window, _| window.try_find(part("icon")).is_some()),
                with_icon,
                "an icon configured={with_icon} rendered the wrong leading element"
            );
        }
    }

    /// The icon sits on the same visual line as the words it belongs to.
    ///
    /// Top alignment is the tempting default and it reads as broken: an icon box
    /// is shorter than a line of text, so the glyph ends up sitting above the
    /// sentence. Geometry is the only way to see this — the snapshot reports
    /// bounds, never painted pixels — and a test that merely found the icon would
    /// pass with either alignment.
    #[gpui_kit::test]
    fn the_icon_is_centred_against_the_sentence(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, true);
        view.update(visual, |view, cx| {
            view.open = true;
            cx.notify();
        });
        visual.update(|window, cx| window.render_frame(cx));

        let icon = visual.update(|window, _| {
            window
                .try_find(part("icon"))
                .map(|snapshot| f32::from(snapshot.bounds().center().y))
        });
        let title = visual.update(|window, _| {
            window
                .try_find(part("title"))
                .map(|snapshot| f32::from(snapshot.bounds().center().y))
        });
        let icon = icon.expect("an open surface with an icon draws one");
        let title = title.expect("an open surface draws its sentence");
        assert!(
            (icon - title).abs() < 1.0,
            "the icon's centre sits at {icon} and the sentence's at {title}; \
             a gap that size reads as a glyph floating above the words"
        );
    }

    /// An open surface draws the sentence and both decisions.
    ///
    /// The labels the buttons carry are not readable from here: a `Button`'s
    /// element type cannot be registered for observation, and the wrapper that
    /// can carries no label of its own. They are pinned in
    /// [`a_label_is_the_callers_or_the_catalogs`] instead.
    #[gpui_kit::test]
    fn an_open_surface_draws_the_sentence_and_both_decisions(cx: &mut TestAppContext) {
        let (view, visual) = harness(cx, false);
        view.update(visual, |view, cx| {
            view.open = true;
            cx.notify();
        });
        visual.update(|window, cx| window.render_frame(cx));
        for (name, what) in [
            ("title", "the sentence it was given"),
            ("confirm", "a button that accepts"),
            ("cancel", "a button that declines"),
        ] {
            assert!(
                visual.update(|window, _| window.try_find(part(name)).is_some()),
                "an open surface must draw {what}"
            );
        }
    }

    /// The surface can be opened and answered without a mouse.
    ///
    /// Two Enter handlers meet inside this component: the popover's, which
    /// toggles the surface, and the one each decision claims, which answers it.
    /// The trigger is asked to open the surface the way a keyboard user would;
    /// the decisions are then asked to answer it the same way.
    ///
    /// The decline half is the weaker of the two by nature: the popover's own
    /// handler closes the surface and reports the close, which is exactly what
    /// declining does, so that half pins "Enter does not accidentally accept".
    /// The accept half is the one with teeth — a surface that only closes
    /// reports nothing, so a decision that never runs cannot pass it.
    #[gpui_kit::test]
    fn the_keyboard_can_open_and_answer_the_surface(cx: &mut TestAppContext) {
        let (view, visual) = keyboard_harness(cx);
        visual.update(|window, cx| window.render_frame(cx));

        let trigger_focus = view.read_with(visual, |view, _| view.trigger_focus.clone());
        visual.update(|window, cx| window.focus(&trigger_focus, cx));
        visual.simulate_keystrokes("enter");
        assert!(
            visual.update(|window, _| window.try_find(part("surface")).is_some()),
            "Enter on the focused control must open the surface"
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.log.borrow().opened.clone()),
            vec![true],
            "opening by keyboard must report the same transition a press does"
        );

        // The surface takes focus when it opens, so the first tab stop inside it
        // is the decline button and the second is the accept one. Both are asked,
        // because "the wrong button answered" is exactly the failure that would
        // otherwise look like a successful cancel. A frame has to be drawn before
        // walking the tab stops: they are read off the last frame, and a surface
        // that has just opened is not in one yet.
        visual.update(|window, cx| window.render_frame(cx));
        visual.update(|window, cx| window.focus_next(cx));
        visual.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(visual, |view, _| view.log.borrow().confirmed),
            0,
            "Enter on the decline button must not accept"
        );
        assert!(
            visual.update(|window, _| window.try_find(part("surface")).is_none()),
            "Enter on the decline button must close the surface"
        );

        visual.update(|window, cx| window.focus(&trigger_focus, cx));
        visual.simulate_keystrokes("enter");
        visual.update(|window, cx| window.render_frame(cx));
        assert!(
            visual.update(|window, _| window.try_find(part("surface")).is_some()),
            "the surface must open again after it was declined"
        );
        visual.update(|window, cx| window.focus_next(cx));
        visual.update(|window, cx| window.focus_next(cx));
        visual.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(visual, |view, _| view.log.borrow().confirmed),
            1,
            "Enter on the accept button must accept"
        );
    }

    /// Accepting reports once and closes; declining only closes.
    ///
    /// The two paths differ in exactly one thing, which is the whole contract of
    /// the component, so both are driven through a real click.
    #[gpui_kit::test]
    fn accepting_reports_and_declining_does_not(cx: &mut TestAppContext) {
        for (button, expected) in [("confirm", 1), ("cancel", 0)] {
            let (view, visual) = harness(cx, false);
            view.update(visual, |view, cx| {
                view.open = true;
                cx.notify();
            });
            visual.update(|window, cx| window.render_frame(cx));
            visual.update(|window, cx| window.click(part(button), cx));

            assert_eq!(
                view.read_with(visual, |view, _| view.log.borrow().confirmed),
                expected,
                "clicking {button} reported the wrong number of confirmations"
            );
            assert!(
                visual.update(|window, _| window.try_find(part("surface")).is_none()),
                "clicking {button} must close the surface"
            );
            assert_eq!(
                view.read_with(visual, |view, _| view.log.borrow().opened.clone()),
                vec![false],
                "clicking {button} must report the close, and nothing else"
            );
        }
    }

    /// A label is the caller's, or the catalog's when the caller gave none.
    ///
    /// The catalog returns the key itself for a missing entry, so comparing the
    /// resolved label against the raw key is what catches a label nobody wrote.
    /// Comparing it against the same lookup would agree with itself.
    #[test]
    fn a_label_is_the_callers_or_the_catalogs() {
        let (confirm, cancel) = resolved_labels(Some("Delete it".into()), Some("Keep it".into()));
        assert_eq!(confirm, "Delete it");
        assert_eq!(cancel, "Keep it");

        let (confirm, cancel) = resolved_labels(None, None);
        for (label, key) in [
            (confirm.as_ref(), "actions.confirm"),
            (cancel.as_ref(), "actions.cancel"),
        ] {
            assert_ne!(
                label, key,
                "a catalog entry nobody wrote leaves the raw key on the button"
            );
            assert!(
                !label.trim().is_empty(),
                "a blank label tells the user nothing about what the button does"
            );
        }
    }

    /// Accepting wears the primary variant, not the danger one.
    ///
    /// The destructive reading belongs to the warning icon the caller passes in.
    /// Painting the button red as well says the same thing twice and makes the
    /// surface louder than the control it guards. No rendering assertion here
    /// could catch a later edit that reaches for `Danger` again — the variant is
    /// applied inside the button and never read back out — so the constant is
    /// asserted directly.
    #[test]
    fn the_accepting_button_is_the_primary_variant() {
        assert_eq!(ACCEPT_VARIANT, ButtonVariant::Primary);
    }
}
