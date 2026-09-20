//! The failure vocabulary of the window-level native calls.
//!
//! The product keeps one pre-rendered window per purpose (settings, update) and swaps
//! its visibility instead of destroying it, so both first-release platforms implement
//! the same hide/show pair and report the same failures. The variants stay in one
//! shared type rather than one per platform: `bongocat-ui` calls the pair through a
//! single code path and only maps the error to a stable settings error code.
//!
//! `WrongThread` is produced by the macOS calls, which are AppKit main-thread calls;
//! `CloseRequestFailed` and `TaskbarVisibilityUpdateFailed` are produced by the
//! Windows calls that own those concepts.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWindowError {
    HandleUnavailable,
    UnsupportedHandle,
    WrongThread,
    CloseRequestFailed,
    TaskbarVisibilityUpdateFailed,
}

impl fmt::Display for NativeWindowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::HandleUnavailable => "the native window handle is unavailable",
            Self::UnsupportedHandle => {
                "the native window handle is not a supported platform window"
            }
            Self::WrongThread => "the native window call was made off the owner thread",
            Self::CloseRequestFailed => "the native window rejected the close request",
            Self::TaskbarVisibilityUpdateFailed => {
                "the native window taskbar visibility did not update"
            }
        })
    }
}

impl std::error::Error for NativeWindowError {}
