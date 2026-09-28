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
//! GPUI Kit's `Popover` draws the optional anchor-aligned arrow, so this
//! wrapper only forwards whether the caller wants one.

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

#[cfg(test)]
mod tests;
mod trigger;

use trigger::TriggerSlot;

/// The width the sentence is laid out in.
///
/// Left to size itself, the surface grows to whatever its longest line wants,
/// and a confirmation is opened from a card that is narrower than a long
/// sentence would make it. A fixed width keeps the wrap predictable and the
/// surface smaller than the thing that asked for it.
pub(crate) const PANEL_WIDTH: Pixels = px(220.);

/// The size of both buttons.
///
/// [`Size::Medium`], the component default, makes the pair wider than the
/// sentence it is answering. A confirmation is a footnote to the control it
/// guards, not a dialog of its own, so it is drawn at [`Size::Small`].
pub(crate) const BUTTON_SIZE: Size = Size::Small;

/// The style of the button that accepts.
///
/// Accepting is the emphasised choice and declining is the plain one. The
/// destructive reading is carried by the warning icon the caller supplies
/// rather than by the button's colour, so accepting is drawn in the primary
/// variant and not the danger one. Nothing here forces a caller into that
/// reading: a confirmation with no consequence is a bad confirmation, not a
/// different widget.
pub(crate) const ACCEPT_VARIANT: ButtonVariant = ButtonVariant::Primary;

/// The locale a label falls back to when the caller does not supply one.
///
/// Both buttons have to say *something*, and the only text this crate can read
/// without a language in hand is the catalog's own. Every call site in this app
/// passes both labels in the resolved language, so this is a safety net rather
/// than a path anything takes.
pub(crate) const FALLBACK_LABEL_LOCALE: &str =
    SettingsLanguage::EnglishUnitedStates.catalog_locale();

/// What the surface reports when its open state changes.
pub(crate) type OpenChangeCallback = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// What the surface reports when the user accepts.
pub(crate) type ConfirmCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// One sentence, a leading icon, and a confirm/cancel pair, opened from a
/// trigger the caller already draws.
///
/// The open state is the popover's unless [`PopConfirm::open`] is used, in which
/// case the caller owns it — which is what a trigger that is also reachable from
/// the keyboard needs, because the click that toggles the surface is the
/// popover's own.
#[derive(IntoElement)]
pub struct PopConfirm {
    pub(crate) id: ElementId,
    pub(crate) title: SharedString,
    pub(crate) icon: Option<(Icon, Hsla)>,
    pub(crate) confirm_label: Option<SharedString>,
    pub(crate) cancel_label: Option<SharedString>,
    pub(crate) anchor: Anchor,
    pub(crate) arrow: bool,
    pub(crate) trigger: Option<AnyElement>,
    pub(crate) open: Option<bool>,
    pub(crate) on_open_change: Option<OpenChangeCallback>,
    pub(crate) on_confirm: Option<ConfirmCallback>,
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
            arrow: false,
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

    /// Show an anchor-aligned arrow pointing toward the trigger.
    pub fn arrow(mut self, arrow: bool) -> Self {
        self.arrow = arrow;
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
pub(crate) fn sentence(id: &ElementId, title: &SharedString, icon: Option<(Icon, Hsla)>) -> Div {
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
pub(crate) fn deciding_button<T: Into<ElementId>>(
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
pub(crate) fn decide(
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
pub(crate) fn resolved_labels(
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
            arrow,
            trigger,
            open,
            on_open_change,
            on_confirm,
        } = self;

        let (confirm_label, cancel_label) = resolved_labels(confirm_label, cancel_label);

        Popover::new(id.clone())
            .anchor(anchor)
            .arrow(arrow)
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
