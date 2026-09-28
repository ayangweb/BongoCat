//! The small controls the pages build out of.
//!
//! Every icon-only action in the window goes through here, which is what makes
//! them all reachable by keyboard and all carry a tooltip: a control with no text
//! label has nothing else to be named by.

use super::*;

pub(crate) fn command_button(
    label: &'static str,
    focus: &FocusHandle,
    tab_index: isize,
    _window: &Window,
    _tokens: Tokens,
    disabled: bool,
) -> Div {
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(tab_index)
        .child(Button::new(label).label(label).disabled(disabled))
}

/// Wrap one icon button in the element that owns its settings identity.
///
/// The wrapper carries the key context and the keyboard tab position, and the
/// caller gives it the id it is queried by; the button inside is what the
/// pointer presses. The caller builds that button, because its size and variant
/// are the only things that tell one icon control apart from another — the
/// models page's card controls and a shortcut row's compact ones share this
/// wrapper and nothing else.
pub(crate) fn icon_command_control(focus: &FocusHandle, tab_index: isize, button: Button) -> Div {
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(tab_index)
        .child(button)
}

pub(crate) fn icon_command_button(
    id: &'static str,
    label: &'static str,
    icon: impl Into<Icon>,
    focus: &FocusHandle,
    tab_index: isize,
    disabled: bool,
) -> Div {
    icon_command_control(
        focus,
        tab_index,
        Button::new(id).icon(icon).tooltip(label).disabled(disabled),
    )
}
