#![allow(unsafe_code)]

//! Read-only input source observation. Native objects never leave this adapter.

/// A source identity includes its input mode where the operating system exposes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSource {
    pub id: String,
    pub name: String,
    /// Optional 32x32 straight RGBA8. A caller must use a badge if unavailable.
    pub icon_rgba: Option<Vec<u8>>,
    /// The short label the system itself shows for a source that has no icon of
    /// its own, such as the three-letter language code beside a keyboard layout.
    /// It is not a stand-in chosen by the plugin: it is the same label the system
    /// puts there, and it is used only when no icon was available.
    pub label: Option<String>,
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

    /// Whether a native observer reported a change since the last snapshot.
    ///
    /// Only macOS observes its input source: its distributed TIS notification
    /// reaches a background process, so a change can be published without asking.
    /// Windows has no notification with that reach — see `windows.rs`.
    #[cfg(target_os = "macos")]
    pub fn changed(&self) -> bool {
        self.0.changed()
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
