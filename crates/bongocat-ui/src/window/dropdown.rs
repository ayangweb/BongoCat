//! How wide a settings dropdown opens.
//!
//! `gpui-component`'s `Select` sizes its menu from its own trigger, and the
//! trigger is only as wide as the value it currently shows. A menu opened over a
//! short value therefore ellipsizes every longer option — the random behavior
//! dropdown showing "off" is the case users hit first, because the option they
//! are looking for is the longest one in the list. The pages ask for a width
//! here instead: the longest label the dropdown offers, measured through the
//! same text system the rows are painted with, plus the chrome a row draws
//! around its label.

use super::*;
// `TextRun` is what the window's shaper takes: the labels are measured through
// the same primitive the rows are painted with, rather than by guessing at an
// advance width per character.
use gpui_kit::TextRun;

/// The font size a dropdown row draws its label at.
///
/// A row takes its text size from the select's size — `Size::Medium`, the
/// component default — which is `text_sm`, 0.875rem.
const OPTION_LABEL_REM: f32 = 0.875;

/// The fixed part of the chrome around an option row's label: the list's 4px
/// padding, the row's `px_2` on both sides, and the gap before the trailing
/// check icon.
const OPTION_ROW_PADDING: f32 = 28.;

/// The trailing check icon, which is the rem-sized part of that chrome: every
/// row reserves an `XSmall` icon box, selected or not, so the labels stay
/// aligned.
const OPTION_CHECK_ICON_REM: f32 = 0.75;

/// The room the menu keeps past that chrome.
///
/// The width is a floor, not a measurement of the drawn row: a rounding
/// difference between the shaped label and the laid-out one must not put the
/// widest option back into the ellipsis this width exists to prevent, and a
/// menu whose longest option touches its edge reads as if it were still cut
/// off.
const OPTION_BREATHING_ROOM: f32 = 8.;

/// The width one dropdown has to open with so every option fits in full.
///
/// The widest label decides, and the current selection is deliberately not an
/// input: a menu sized by the selected value is the behaviour this replaces.
/// Widths are measured through the window's text system at the size the rows
/// draw with, so they follow the platform font, the active localization and the
/// theme's base font size rather than guessing at either.
///
/// An empty list yields the chrome alone, which a dropdown never asks for: every
/// settings dropdown has at least one option.
pub(super) fn dropdown_menu_width(
    window: &Window,
    labels: impl IntoIterator<Item = impl Into<SharedString>>,
) -> Pixels {
    let font_size = window.rem_size() * OPTION_LABEL_REM;
    let style = window.text_style();
    let font = style.font();
    let mut widest: Pixels = px(0.);
    for label in labels {
        let label: SharedString = label.into();
        // A label is text the row draws on one line, and a title that carries a
        // newline — user input and imported folder names both reach here — would
        // stop the shaper, so each line is measured on its own and the widest one
        // decides.
        for line in label.lines() {
            let run = TextRun {
                len: line.len(),
                font: font.clone(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let width = window
                .text_system()
                .shape_line(line.into(), font_size, std::slice::from_ref(&run), None)
                .width();
            widest = widest.max(width);
        }
    }

    let check_icon = window.rem_size() * OPTION_CHECK_ICON_REM;
    widest + check_icon + px(OPTION_ROW_PADDING + OPTION_BREATHING_ROOM)
}
