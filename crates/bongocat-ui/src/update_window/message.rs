//! The one line each phase says, and the sizes a changelog reports in.
//!
//! These are pure: a phase and a locale in, a localized string out. Keeping them
//! apart from the frame that draws them is what makes them testable without a
//! window, which is why the stage table and the byte formatter have their own
//! tests.

use super::*;

pub(crate) fn status_line(message: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_sm()
        .text_color(tokens.text)
        .child(message.into())
}

pub(crate) fn hint_line(message: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_xs()
        .text_color(tokens.muted)
        .child(message.into())
}

/// The changelog, rendered from the Markdown the release manifest announced.
///
/// The notes are untrusted input. [`crate::update_markdown`] bounds them and hands them to
/// `gpui-kit` with the plugins that keep a manifest from making this application fetch
/// anything or offer a target it will not open.
///
/// The scroll box is sized by its content up to [`NOTES_MAX_HEIGHT`] and no
/// further, which is what makes the window's own height measurable: the height the
/// box reports is the changelog's intrinsic height capped at a constant, so it does
/// not depend on how tall the window happens to be this frame. Past the cap the box
/// holds its height and scrolls. Sizing it to the window instead is what would make
/// the two feed each other and never settle.
pub(crate) fn notes_section(locale: &str, notes: Option<String>, tokens: Tokens, _cx: &App) -> Div {
    let Some(notes) = notes else {
        return div();
    };
    if notes.trim().is_empty() {
        return div();
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(tokens.muted)
                .child(text(locale, "update.notes.title").to_owned()),
        )
        .child(
            div()
                .id("update-release-notes-body")
                .test_support()
                .w_full()
                .h_auto()
                .min_h_0()
                .max_h(px(NOTES_MAX_HEIGHT))
                .overflow_y_scroll()
                .child(crate::update_markdown::render(&notes)),
        )
}

/// A footer action.
///
/// `test_support()` registers the element for the headless window tests without
/// changing anything in a normal build: the trait's method is the identity function
/// when `gpui-kit`'s `test-support` feature is off.
pub(crate) fn command_button(
    label: &'static str,
    id: &'static str,
    focus: &FocusHandle,
    _tokens: Tokens,
    disabled: bool,
) -> impl IntoElement + StatefulInteractiveElement {
    div()
        .key_context("UpdateControl")
        .track_focus(focus)
        // The wrapper owns the queryable id; the inner control gets a derived one so
        // the two registrations cannot be ambiguous.
        .child(
            Button::new(SharedString::from(format!("{id}-control")))
                .label(label)
                .disabled(disabled),
        )
        .id(id)
        // `test_support()` needs the element to already have an id.
        .test_support()
}

pub(crate) fn stage_message_key(stage: UpdateFailureStage) -> &'static str {
    match stage {
        UpdateFailureStage::Check => "update.error.stage.check",
        UpdateFailureStage::Download => "update.error.stage.download",
        UpdateFailureStage::Verify => "update.error.stage.verify",
        UpdateFailureStage::Install => "update.error.stage.install",
    }
}

pub(crate) fn unavailable_message(locale: &str, reason: UpdateUnavailableReason) -> String {
    let key = match reason {
        UpdateUnavailableReason::DevelopmentBuild => "update.unavailable.development_build",
        UpdateUnavailableReason::SigningKeyMissing => "update.unavailable.signing_key_missing",
    };
    text(locale, key).to_owned()
}

pub(crate) fn download_message(locale: &str, progress: UpdateProgressInfo) -> String {
    match progress.percent() {
        Some(percent) => format_text(
            locale,
            "update.status.downloading",
            &[("percent", percent.to_string())],
        ),
        None => text(locale, "update.status.downloading_unknown").to_owned(),
    }
}

/// Render a byte count without pulling in a formatting dependency.
pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
