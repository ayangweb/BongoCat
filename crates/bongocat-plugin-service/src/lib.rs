#![forbid(unsafe_code)]

//! The one owner of the installed plugins, and the one thing that drives them.
//!
//! Plugins are separate processes with their own lifecycle, so something outside
//! the settings window has to own them: the model window shows their panels while
//! the settings window is closed, and a timer that stopped counting when the user
//! dismissed the window would not be a timer. This crate is that owner.
//!
//! It is deliberately free of GPUI and of the operating system: it holds the
//! manager, ticks it on a monotonic clock, composes every running plugin's panel
//! into the one layer the model window draws, routes a click back to the plugin
//! that drew the control, and answers a small typed command vocabulary. The two
//! ends of the command channel and of the layer channel are the only things it
//! shares with the rest of the product, which is what keeps the plugin system out
//! of the settings window's lifetime.
//!
//! The owner waits with a bounded timeout for clock and input-counter work.
//! Capability-gated plugin output wakes it immediately rather than waiting for
//! that timeout; unchanged views still do not cause HUD composition.

mod command;
mod service;

pub use command::{
    PluginCard, PluginCenterSnapshot, PluginCommand, PluginCommandError, PluginInteractionFailure,
    PluginReply, PluginServiceClient, PluginServiceClosed, PluginServiceEndpoint,
};
pub use service::{PluginOverlayHandles, PluginService, PluginServiceStartError, plugin_directory};

/// How often the service looks for work.
///
/// One loop serves both cadences: it drains commands and clicks, ticks the
/// plugins when their own interval has passed, and composes a new layer only when
/// something changed. A panel that is not changing therefore costs nothing but
/// the timer, and a click on a plugin's control is applied within this interval
/// rather than at the next tick.
pub(crate) const LOOP_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// How often every running plugin is told the clock moved.
///
/// A plugin's own timer is a function of these ticks, so this is the resolution a
/// countdown counts at. Faster would spend a process round trip per frame for a
/// label that changes once a second.
pub(crate) const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

pub use bongocat_plugin_manager::PluginSource;
/// The anonymous device edges a plugin may be fed, re-exported for the caller
/// that measures them.
///
/// The service reads its activity source rather than owning it, so the type of
/// what that source returns belongs to the protocol both are written against;
/// the binary that wires the two together should not have to depend on the SDK
/// itself to name it.
pub use bongocat_plugin_sdk::InputActivity;

/// An event feed attached once to the owner thread, never polled for input.
pub trait PluginInputEvents: Send {
    fn set_waker(&self, thread: std::thread::Thread);
    fn drain(&mut self) -> (Vec<bongocat_plugin_sdk::PluginInputEvent>, u64);
}

pub use bongocat_plugin_sdk::{InputEventControl, PluginInputEvent};
