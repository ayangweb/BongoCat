//! OS-registered global shortcuts backed by `global-hotkey` (ADR-0044) on
//! Windows/macOS and the XDG Desktop Portal on Linux. A single owner thread
//! holds the platform manager, mirrors the shared
//! [`bongocat_config::ShortcutTable`] into real OS registrations, and forwards
//! pressed events to the dispatcher.
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
//! - Linux: the XDG Desktop Portal owns the global registration and emits
//!   activation/deactivation signals, so pure Wayland does not depend on X11.
#[cfg(all(test, not(target_os = "linux")))]
use super::ShortcutDispatch;
use super::{ShortcutDispatchError, ShortcutDispatcher};
use bongocat_config::{
    CompiledShortcuts, ShortcutChord, ShortcutModifiers, ShortcutTable, ShortcutTarget,
};
#[cfg(any(not(target_os = "linux"), test))]
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
#[cfg(any(not(target_os = "linux"), test))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::{BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) mod hotkey;
#[cfg(any(not(target_os = "linux"), test))]
mod owner;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(any(not(target_os = "linux"), test))]
mod registration;
pub(crate) mod service;

#[cfg(test)]
mod tests;

// The four are one vocabulary — the owner thread registers, serves, and maps
// keys for the same service — so each reaches the others through this one
// prelude rather than naming three modules apiece. The globs are `pub(crate)`
// because nothing outside this crate implements a registrar or pumps the
// platform's messages.
#[cfg(any(not(target_os = "linux"), test))]
pub(crate) use hotkey::*;
#[cfg(any(not(target_os = "linux"), test))]
pub(crate) use owner::*;
#[cfg(target_os = "linux")]
pub(crate) use portal::*;
#[cfg(any(not(target_os = "linux"), test))]
pub(crate) use registration::*;
pub(crate) use service::*;
