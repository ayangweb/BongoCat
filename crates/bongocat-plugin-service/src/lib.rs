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
//! The loop is polled rather than event-driven for one reason: a plugin's tick is
//! a clock, and a clock is not an event. Everything else the loop does — draining
//! commands, applying a click, publishing a changed layer — is work that a poll
//! already covers, so a second wake-up source would add a race without removing a
//! wake-up.

mod command;
mod service;

pub use command::{
    PluginCard, PluginCenterSnapshot, PluginCommand, PluginCommandError, PluginReply,
    PluginServiceClient, PluginServiceClosed, PluginServiceEndpoint,
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
