//! The confirmation surface's tests, split by what they are about.
//!
//! The harness is here rather than in one of the files because every question
//! needs a window: the tests read what was painted, and a test that opened the
//! surface differently would be measuring its own setup.

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
    arrow: bool,
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
            .arrow(self.arrow)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.log.borrow_mut().opened.push(*open);
                this.open = *open;
                cx.notify();
            }))
            .on_confirm(move |_, _| confirmed.borrow_mut().confirmed += 1);
        if self.with_icon {
            confirm = confirm.icon(gpui_kit::assets::IconName::TriangleAlert, cx.theme().danger);
        }
        div().p_4().child(confirm)
    }
}

fn harness(cx: &mut TestAppContext, with_icon: bool) -> (Entity<Harness>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.add_window_view(move |_, _| Harness {
        log: Rc::new(RefCell::new(Log::default())),
        open: false,
        with_icon,
        arrow: false,
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

fn keyboard_harness(cx: &mut TestAppContext) -> (Entity<KeyboardHarness>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.add_window_view(move |_, cx| KeyboardHarness {
        log: Rc::new(RefCell::new(Log::default())),
        open: false,
        trigger_focus: cx.focus_handle(),
    })
}

mod keyboard;
mod labels;
mod layout;
mod open;
