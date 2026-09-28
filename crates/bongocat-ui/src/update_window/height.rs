//! Sizing the window to what it draws, rather than to a fixed box.
//!
//! The update window's content is one phase line, sometimes a hint, and a
//! changelog of unbounded length. A fixed height would either clip the changelog
//! or waste the screen on an empty window, so the window measures its content,
//! clamps that between a floor and a ceiling, and reports the height it needs
//! before it is ever shown — otherwise the first frame would be painted at the
//! wrong size and the user would see it jump.

use super::*;

/// The height the rendered content needs, read off one laid-out frame.
///
/// The window's root has three children: the content column, which is sized by its
/// content and never shrinks, a spacer that collects whatever height is left over,
/// and the actions. Reading the height off those three is what keeps the window
/// honest — the column reports the height it needs, and the gaps around the spacer
/// are real distances the frame reports rather than constants this file has to keep
/// in step with the style.
///
/// The measurement is exact even when the window is too short for its content, which
/// is what lets the window go straight to the right size instead of growing to the
/// ceiling and then settling back down. Two things make that hold: the column does
/// not shrink, so it still reports its content height when it overflows, and the
/// spacer collapses to nothing, so both gaps around it are still the gaps.
/// `the_height_does_not_depend_on_which_height_the_window_opened_at` is what checks
/// it end to end; a layout change that broke it would send the window to the wrong
/// size rather than merely a slow one.
///
/// `children` is the root's `on_children_prepainted` bounds, in order.
pub(crate) fn required_height(children: &[Bounds<Pixels>]) -> Option<Pixels> {
    let [content, spacer, actions] = children else {
        return None;
    };
    // The padding is read off the top and applied to both sides, which is what `p_4`
    // means. It is the one number the frame cannot report: a window too small for its
    // content squeezes its own bottom padding rather than pushing the actions off
    // the bottom.
    let padding = content.origin.y;
    let above_spacer = spacer.origin.y - (content.origin.y + content.size.height);
    let below_spacer = actions.origin.y - (spacer.origin.y + spacer.size.height);
    // The spacer's own height is left out on purpose. It is the leftover, so counting
    // it would make an oversized window a fixed point and it would never come back
    // down to its content.
    Some(
        padding + content.size.height + above_spacer + below_spacer + actions.size.height + padding,
    )
}

/// The height the window should be, once a frame has said what its content needs.
///
/// The height is only knowable *after* layout, so the layout pass records it here and
/// whoever can act on it does: the opener, before the window is shown, and the
/// following frame once it is.
#[derive(Default)]
pub(crate) struct ContentHeight {
    pub(crate) required: Cell<Option<Pixels>>,
}

impl ContentHeight {
    pub(crate) fn record(&self, children: &[Bounds<Pixels>]) {
        if let Some(required) = required_height(children) {
            self.required.set(Some(required));
        }
    }

    /// The height the window should have, or `None` when it already has it.
    ///
    /// The floor is what a compact phase collapses to, and the ceiling is what a long
    /// changelog stops at rather than growing past. "Already has it" allows for the
    /// platform rounding what it was asked for, so a window is not asked to move to a
    /// height it cannot hold.
    pub(crate) fn target(&self, viewport_height: Pixels) -> Option<Pixels> {
        let required = self.required.get()?;
        let target = required.clamp(px(WINDOW_MIN_HEIGHT), px(WINDOW_MAX_HEIGHT));
        let drift = f32::from(target - viewport_height).abs();
        (drift > HEIGHT_MATCH_TOLERANCE).then_some(target)
    }
}

/// Measure the hidden window's content, set the height it needs, and show it once that
/// height has been painted.
///
/// Three things have to happen in this order, and none of them can be left to the frame
/// loop. A content-sized window's height is only knowable after a frame has laid it out.
/// The window is not being sent frames while it is hidden — macOS only runs a display
/// link, and so only asks for frames, for a window whose occlusion state says it is on
/// screen. And the paint the window is shown with has to be one made at the final height,
/// not the one made while measuring.
///
/// So the window is laid out here, resized, and then painted and shown from a task
/// queued behind the one that applies the resize. That ordering is the whole guarantee:
/// applying a resize is itself a task on the same foreground executor, so a task spawned
/// after it cannot run before it. Nothing polls and nothing times out.
///
/// The handle is the untyped one on purpose. A typed `WindowHandle::update` leases the
/// root view for the duration of its closure, and drawing re-leases it, which is a
/// double-lease panic. The frame loop makes the same call, through the untyped handle,
/// for the same reason.
pub(crate) fn prime_the_height(
    view: &gpui_kit::Entity<UpdateView>,
    window: gpui_kit::WindowHandle<Root>,
    cx: &mut App,
) -> Result<(), String> {
    let showing: gpui_kit::AnyWindowHandle = window.into();
    cx.update_window(showing, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        let content_height = view.read(cx).content_height.clone();
        let Some(target) = content_height.target(window.viewport_size().height) else {
            window.activate_window();
            return;
        };
        let width = window.viewport_size().width;
        window.resize(size(width, target));
        cx.spawn(async move |cx| {
            cx.update_window(showing, |_, window, cx| finish_sizing(window, cx))
                .ok();
        })
        .detach();
    })
    .map_err(|error| error.to_string())
}

/// The tail of priming: the resize has been applied, so paint at the height the window is
/// now and show it.
pub(crate) fn finish_sizing(window: &mut Window, cx: &mut App) {
    // The platform's own resize callback is what normally tells the window how big it
    // now is, and it need not have arrived yet. Reading the size from the platform here
    // is what makes the frame below the one the window is shown with.
    window.bounds_changed(cx);
    window.refresh();
    window.draw(cx).clear(cx);
    window.activate_window();
}
