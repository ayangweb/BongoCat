//! The left-button event monitor.
//!
//! A panel's buttons are part of the model window, so pressing one is a press on
//! the model window — and the panel has no window of its own to be clicked in. The
//! monitor is how the model window learns about it, the same way the right-button
//! monitor is how it learns about a resize.
//!
//! The monitor hit-tests against the placement the frame loop published, rather
//! than the frame loop hit-testing a click the monitor forwarded. It has to be
//! this way round: `NSPanel` reports a press for the whole window, and the decision
//! between "a button was pressed" and "the user clicked the model window for no
//! reason" is only available here, while the event is still being handled.

use super::*;

/// The left-button event monitor for one panel, and the token that removes it.
pub(crate) struct ClickMonitor {
    /// The token `NSEvent::removeMonitor` needs at shutdown.
    pub(crate) token: Retained<AnyObject>,
}

/// What the monitor needs to turn an event into a press.
struct ClickMonitorState {
    panel: Retained<NSPanel>,
    /// The placement to hit-test against, republished by the frame loop each tick.
    placed: PlacedLayers,
    /// Where a press inside a layer goes. Never blocks.
    press_sink: Arc<dyn bongocat_render::OverlayPressSink>,
}

/// Install the left-button monitor for one panel.
///
/// `NSEventType::LeftMouseDown` only: a button is pressed, not held. There is no
/// drag to end and no release to pair with, so a press-driven panel needs one
/// message, and a drag would only repeat it sixty times a second.
pub(crate) fn install_click_monitor(
    _: MainThreadMarker,
    panel: Retained<NSPanel>,
    placed: PlacedLayers,
    press_sink: Arc<dyn bongocat_render::OverlayPressSink>,
) -> Option<ClickMonitor> {
    let window_number = panel.windowNumber();
    let state = ClickMonitorState {
        panel,
        placed,
        press_sink,
    };
    let handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
        RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit supplies a valid NSEvent pointer for the duration of
            // this local event-monitor callback, and it is returned to continue
            // normal dispatch.
            let event_ref = unsafe { event.as_ref() };
            if event_ref.windowNumber() == window_number
                && event_ref.r#type() == objc2_app_kit::NSEventType::LeftMouseDown
            {
                report_press(&state, event_ref);
            }
            event.as_ptr()
        });
    // SAFETY: the handler returns AppKit's original valid event pointer and is
    // retained by the returned monitor token until session shutdown.
    let token = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseDown, &handler)
    }?;
    Some(ClickMonitor { token })
}

/// Report the press, if the click landed inside a layer.
///
/// AppKit's window coordinates are points measured from the bottom left, and the
/// drawable is pixels measured from the top left, so both the scale and the y axis
/// are converted here. The drawable's size is measured the way the renderer
/// measures it — the panel frame times the panel's own backing scale factor — so
/// the two cannot disagree about a window that moved to a display with a different
/// scale factor.
fn report_press(state: &ClickMonitorState, event: &NSEvent) {
    let scale = state.panel.backingScaleFactor();
    let frame = state.panel.frame().size;
    let location = event.locationInWindow();
    let drawable = (
        (frame.width * scale).round().max(0.0) as u32,
        (frame.height * scale).round().max(0.0) as u32,
    );
    let x = location.x * scale;
    let y = frame.height * scale - location.y * scale;
    if let Some(press) = state
        .placed
        .press(drawable.0, drawable.1, x as f32, y as f32)
    {
        state.press_sink.press(press.layer_id, press.x, press.y);
    }
}
