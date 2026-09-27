//! OS-registered global shortcuts backed by the `global-hotkey` crate
//! (ADR-0044). A single owner thread holds the platform manager, mirrors
//! the shared [`bongocat_config::ShortcutTable`] into real OS
//! registrations, and forwards pressed events to the dispatcher.
//!
//! Platform notes:
//! - Windows: `RegisterHotKey` posts `WM_HOTKEY` to the message queue of
//!   the thread that created the manager's hidden window, so the owner
//!   thread runs a Win32 message pump.
//! - macOS: hot key events arrive through the Carbon handler on the main
//!   event loop; creation and registration from the owner thread were
//!   verified against `RegisterEventHotKey` in a controlled experiment.
//!   Bindings whose keys have no Carbon scancode (ScrollLock, Pause)
//!   cannot register and are reported as registration failures instead of
//!   blocking the remaining bindings.
#[cfg(test)]
#[cfg(test)]
use super::ShortcutDispatch;
use super::{ShortcutDispatchError, ShortcutDispatcher};
use bongocat_config::{
    CompiledShortcuts, ShortcutChord, ShortcutModifiers, ShortcutTable, ShortcutTarget,
};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::{BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) mod hotkey;
mod owner;
mod registration;
pub(crate) mod service;

#[cfg(test)]
mod tests;

// The four are one vocabulary — the owner thread registers, serves, and maps
// keys for the same service — so each reaches the others through this one
// prelude rather than naming three modules apiece. The globs are `pub(crate)`
// because nothing outside this crate implements a registrar or pumps the
// platform's messages.
pub(crate) use hotkey::*;
pub(crate) use owner::*;
pub(crate) use registration::*;
pub(crate) use service::*;
