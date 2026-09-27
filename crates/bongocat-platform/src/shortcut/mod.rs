//! Shortcuts: the platform-neutral dispatcher, and the OS-registered global
//! shortcuts behind it.
//!
//! The two are separate because they answer different questions. The dispatcher
//! is what the operating-system adapter calls once it has already matched a
//! chord, and it knows nothing about configuration strings or key codes. The
//! global service is what holds those registrations, and it knows nothing about
//! what a matched chord means.

use bongocat_config::ShortcutTarget;
use std::sync::Arc;

type ShortcutHandler =
    dyn Fn(&ShortcutTarget) -> Result<ShortcutDispatch, ShortcutDispatchError> + Send + Sync;

/// Dispatches a matched shortcut target without exposing configuration strings
/// or platform key codes to the operating-system adapter. The application owns
/// the typed mapping and supplies this callback; the platform owner only
/// forwards the already-matched target.
#[derive(Clone)]
pub struct ShortcutDispatcher {
    handler: Arc<ShortcutHandler>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatch {
    Triggered,
    ApplicationQueued,
    IgnoredApplicationCommand,
    IgnoredInactiveModel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatchError {
    ApplicationQueueFull,
    RuntimeQueueFull,
    RuntimeStopped,
}

impl ShortcutDispatcher {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&ShortcutTarget) -> Result<ShortcutDispatch, ShortcutDispatchError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn execute(
        &self,
        target: &ShortcutTarget,
    ) -> Result<ShortcutDispatch, ShortcutDispatchError> {
        (self.handler)(target)
    }
}

mod global;
#[cfg(test)]
mod tests;

// The public surface is named through the modules that hold it, so the
// crate-private globs inside `global` do not narrow what leaves the crate.
pub use global::hotkey::ShortcutHotkeyError;
pub use global::service::{
    GlobalShortcutCounters, GlobalShortcutService, GlobalShortcutServiceError,
};
