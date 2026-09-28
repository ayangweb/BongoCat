//! What the surface reports, and when it is open.

use super::*;

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
