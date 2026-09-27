//! The surface is a focus scope, so the keyboard reaches both decisions.

use super::*;

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
