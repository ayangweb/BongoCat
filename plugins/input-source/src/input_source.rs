#![allow(unsafe_code)]

//! Read-only input source observation. Native objects never leave this adapter.

/// A source identity includes its input mode where the operating system exposes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSource {
    pub id: String,
    pub name: String,
    /// Optional 32x32 straight RGBA8. A caller must use a badge if unavailable.
    pub icon_rgba: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[error("current input source is unavailable")]
pub struct InputSourceError;

/// Own on the calling thread (the main thread on macOS). Queries are read-only;
/// dropping it removes the native observer before releasing callback state.
pub struct InputSourceMonitor(platform::Monitor);

impl InputSourceMonitor {
    pub fn new() -> Result<Self, InputSourceError> {
        platform::Monitor::new().map(Self)
    }

    pub fn snapshot(&mut self) -> Result<InputSource, InputSourceError> {
        self.0.snapshot()
    }
}

#[cfg(target_os = "macos")]
#[path = "input_source/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "input_source/windows.rs"]
mod platform;
