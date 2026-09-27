//! Where the pieces sit, and which one carries the primary variant.

use super::*;

#[gpui_kit::test]
fn the_arrow_reserves_space_between_the_trigger_and_surface(cx: &mut TestAppContext) {
    let (view, visual) = harness(cx, false);
    visual.update(|window, cx| window.render_frame(cx));

    let mut gaps = Vec::new();
    for arrow in [false, true] {
        view.update(visual, |view, cx| {
            view.open = true;
            view.arrow = arrow;
            cx.notify();
        });
        visual.update(|window, cx| window.render_frame(cx));
        let trigger_bottom = visual.update(|window, _| {
            window
                .try_find(TRIGGER)
                .map(|snapshot| snapshot.bounds().bottom())
                .expect("the trigger is drawn")
        });
        let surface_top = visual.update(|window, _| {
            window
                .try_find(part("surface"))
                .map(|snapshot| snapshot.bounds().top())
                .expect("the open surface is drawn")
        });
        gaps.push(surface_top - trigger_bottom);
    }

    assert!(
        gaps[1] > gaps[0],
        "the arrow must reserve space between the trigger and the surface"
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
