//! The right-button event monitor.
//!
//! The overlay has no window to hit-test against, so a local `NSEvent` monitor
//! is the only way it learns about a right-button drag. The monitor sees every
//! right-button event in the process, which is also why it has to hand the
//! context menu back to the application instead of opening it: the monitor and
//! the tray menu would otherwise both claim the same click.

use super::*;

/// State shared by the right-button event monitor.
///
/// The monitor sees every right-button event in the process, so it holds the
/// panel it belongs to, the base size a drag scales from, and the sinks that
/// carry the resulting request back to the application. `Cell` is what lets a
/// plain `Fn` callback own the drag: AppKit's local monitor API takes no
/// mutable context, and every callback runs on the same main thread.
pub(crate) struct ResizeMonitorState {
    pub(crate) panel: Retained<NSPanel>,
    /// `None` when the panel's base size could not be derived, which leaves the
    /// right button a plain context-menu click.
    pub(crate) base: Option<ResizeBase>,
    pub(crate) drag: Cell<Option<ResizeDrag>>,
    pub(crate) context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub(crate) resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    /// The current plugin layer's hit-test data. A plugin that draws a control
    /// claims the click that lands on it; the panel keeps every other click.
    pub(crate) plugin_targets: Cell<OverlayPluginTargets>,
    /// Where a plugin's click is reported.
    pub(crate) plugin_interaction_sender: Cell<Option<SyncSender<OverlayPluginInteraction>>>,
}

/// The right-button event monitor for one panel, together with the state its
/// callbacks read.
pub(crate) struct RightButtonMonitor {
    /// The token `NSEvent::removeMonitor` needs at shutdown.
    pub(crate) token: Retained<AnyObject>,
    /// Shared with the monitor callbacks, which run on the same main thread.
    pub(crate) state: Rc<ResizeMonitorState>,
}

impl RightButtonMonitor {
    /// Whether a right-button resize drag is in progress.
    ///
    /// The hover hide has to stand down while it is: the overlay fades out and
    /// starts passing pointer events through, which would end the drag.
    pub(crate) fn is_resize_dragging(&self) -> bool {
        self.state.drag.get().is_some()
    }
}

/// Install the right-button monitor for one panel.
///
/// A right button that moves past the drag threshold resizes the window; one
/// that does not is still the context-menu request it always was. The monitor
/// is installed whenever either handoff exists, because the two share the same
/// button and the same event.
pub(crate) fn install_context_menu_monitor(
    _: MainThreadMarker,
    panel: Retained<NSPanel>,
    base: Option<ResizeBase>,
    context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    plugin_interaction_sender: Option<SyncSender<OverlayPluginInteraction>>,
) -> Option<RightButtonMonitor> {
    if context_menu_sender.is_none()
        && resize_sender.is_none()
        && plugin_interaction_sender.is_none()
    {
        return None;
    }
    let window_number = panel.windowNumber();
    let state = Rc::new(ResizeMonitorState {
        panel,
        base,
        drag: Cell::new(None),
        context_menu_sender,
        resize_sender,
        plugin_targets: Cell::new(OverlayPluginTargets::default()),
        plugin_interaction_sender: Cell::new(plugin_interaction_sender),
    });
    let shared = Rc::clone(&state);
    let handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
        RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit supplies a valid NSEvent pointer for the duration of
            // this local event-monitor callback, and returns it to continue normal dispatch.
            let event_ref = unsafe { event.as_ref() };
            if event_ref.windowNumber() == window_number {
                match event_ref.r#type() {
                    objc2_app_kit::NSEventType::RightMouseDown => begin_resize(&state),
                    objc2_app_kit::NSEventType::RightMouseDragged => drag_resize(&state),
                    objc2_app_kit::NSEventType::RightMouseUp => finish_resize(&state),
                    objc2_app_kit::NSEventType::LeftMouseDown => {
                        handle_plugin_click(&state, event_ref);
                    }
                    _ => {}
                }
            }
            event.as_ptr()
        });
    // SAFETY: the handler returns AppKit's original valid event pointer and is
    // retained by the returned monitor token until session shutdown.
    let token = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::RightMouseDown
                | NSEventMask::RightMouseDragged
                | NSEventMask::RightMouseUp
                | NSEventMask::LeftMouseDown,
            &handler,
        )
    }?;
    Some(RightButtonMonitor {
        token,
        state: shared,
    })
}

/// The pointer position the resize math measures against.
///
/// AppKit's screen coordinates grow upwards while the drag math assumes a
/// downwards-positive axis (the legacy webview and the Windows adapter both use
/// one), so the vertical component is negated once, here. The window's own
/// coordinate space is deliberately not used: the drag changes the frame while
/// it runs, which would make the same screen position read differently from one
/// event to the next.
pub(crate) fn resize_pointer() -> (f64, f64) {
    let location = NSEvent::mouseLocation();
    (location.x, -location.y)
}

pub(crate) fn begin_resize(state: &ResizeMonitorState) {
    let Some(base) = state.base else {
        return;
    };
    // The drag starts from the width the panel actually has, so a box that
    // drifted from the stored scale does not jump on the first pointer move.
    let width = state.panel.frame().size.width.max(0.0).round() as u32;
    state.drag.set(Some(ResizeDrag::begin(
        resize_pointer(),
        base,
        base.scale_percent_for_width(width),
    )));
}

pub(crate) fn drag_resize(state: &ResizeMonitorState) {
    let Some(mut drag) = state.drag.take() else {
        return;
    };
    let outcome = drag.observe(resize_pointer());
    state.drag.set(Some(drag));
    if let Some(outcome) = outcome {
        apply_resize(state, outcome);
    }
}

pub(crate) fn finish_resize(state: &ResizeMonitorState) {
    let Some(drag) = state.drag.take() else {
        // A right button that was never observed going down is still a click:
        // the monitor can miss the press when the panel was replaced mid-click.
        request_context_menu(state);
        return;
    };
    if !drag.dragging() {
        request_context_menu(state);
        return;
    }
    let Some(scale_percent) = drag.finish() else {
        return;
    };
    if let Some(sender) = &state.resize_sender {
        let _ = sender.try_send(OverlayResizeOutcome { scale_percent });
    }
}

pub(crate) fn request_context_menu(state: &ResizeMonitorState) {
    if let Some(sender) = &state.context_menu_sender {
        let _ = sender.try_send(OverlayContextMenuRequest);
    }
}

/// Report a left click that landed on one of a plugin's controls.
///
/// The panel is movable by its background and the window has no explicit hit
/// test, so a click is the panel's unless a plugin drew a control under the
/// pointer. The event's location is converted the same way the drag pointer is:
/// AppKit's screen coordinates grow upwards, and the layer's device pixels grow
/// downwards from the panel's top-left corner.
///
/// The event is forwarded either way: a click on a plugin's panel is not a
/// window click, but it must not be swallowed either, because AppKit's window
/// machinery still needs to see the mouse down.
pub(crate) fn handle_plugin_click(state: &ResizeMonitorState, event: &NSEvent) {
    let Some(sender) = state.plugin_interaction_sender.take() else {
        return;
    };
    let targets = state.plugin_targets.take();
    let location = event.locationInWindow();
    let frame = state.panel.frame();
    let scale = state.panel.backingScaleFactor().max(1.0);
    let x = ((location.x - frame.origin.x) * scale) as f32;
    let y = ((frame.origin.y + frame.size.height - location.y) * scale) as f32;
    if let Some(interaction) = targets.interaction_at(x, y) {
        // A plugin service that has gone away takes the control with it; the
        // click then falls back to the window, which is what a plugin that
        // unloaded mid-drag already sees.
        let _ = sender.try_send(interaction);
    }
    state.plugin_interaction_sender.set(Some(sender));
    state.plugin_targets.set(targets);
}

/// Apply one drag step to the panel, keeping the window's top edge where it is.
///
/// AppKit window frames are anchored at their bottom-left corner, so growing a
/// window from a fixed origin would move its top edge up the screen. The origin
/// is corrected instead, which is what makes a right-button drag feel like the
/// legacy `setSize` call: the top-left corner the user grabbed stays put.
pub(crate) fn apply_resize(state: &ResizeMonitorState, outcome: ResizeOutcome) {
    let frame = state.panel.frame();
    let top = frame.origin.y + frame.size.height;
    let size = NSSize::new(f64::from(outcome.width), f64::from(outcome.height));
    state.panel.setFrame_display(
        NSRect::new(NSPoint::new(frame.origin.x, top - size.height), size),
        true,
    );
}
