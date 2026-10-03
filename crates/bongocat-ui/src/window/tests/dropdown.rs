//! The width the settings dropdowns open with.
//!
//! `gpui-component` sizes a `Select`'s menu from its trigger, so without a width
//! of its own a dropdown opened over a short value ellipsizes every longer
//! option. The rule these tests pin is the replacement: the longest option
//! decides, and there is room around it.

use super::*;
use gpui_kit::component::h_flex;

/// One option label drawn the way a dropdown row draws it.
///
/// The rows under test live inside the component's popup, which a test cannot
/// reach, so the probe is the closest honest stand-in: the same text at the same
/// `text_sm` size the rows use, painted by the same text system.
struct OptionLabelProbe {
    label: SharedString,
}

impl Render for OptionLabelProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex().child(
            div()
                .id("option-label-probe")
                .test_support()
                .text_sm()
                .child(self.label.clone()),
        )
    }
}

/// The menu is as wide as the longest option, whatever the current value is.
///
/// The labels are the ones the issue reports: with "off" selected, the menu used
/// to be too short for every other choice in the list.
#[gpui_kit::test]
fn a_dropdown_menu_is_as_wide_as_its_longest_option(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    let options = ["关闭", "仅表情", "仅动作", "表情和动作"];
    let (all_options, longest, shortest) = visual.update(|window, _| {
        (
            dropdown_menu_width(window, options),
            dropdown_menu_width(window, ["表情和动作"]),
            dropdown_menu_width(window, ["关闭"]),
        )
    });

    assert_eq!(
        all_options, longest,
        "the width must follow the longest option, not the first row"
    );
    assert!(
        shortest < all_options,
        "a menu sized by the selected value is the ellipsis this width replaces"
    );
}

/// The rendered label of the longest option fits inside the menu, with the chrome
/// the component's option rows draw around it.
#[gpui_kit::test]
fn the_open_menu_covers_the_rendered_label_and_its_row_chrome(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let label = "表情和动作";
    let (_, visual) = cx.add_window_view(|_, _| OptionLabelProbe {
        label: label.into(),
    });
    visual.update(|window, cx| window.render_frame(cx));
    let rendered = rendered_bounds(visual, ElementId::from("option-label-probe"))
        .size
        .width;
    let menu = visual.update(|window, _| dropdown_menu_width(window, [label]));

    // What a row draws around its label: 4px of list padding, 8px of row padding
    // on each side, the 4px gap before the trailing check and its 12px box.
    assert!(
        menu >= rendered + px(40.),
        "the menu ({menu:?}) must leave room for the rendered label ({rendered:?}) and the row chrome"
    );
}
