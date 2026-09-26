//! What the application is told while an update runs.
//!
//! The update window's whole vocabulary is here. A phase is a closed set so a new
//! one cannot be added without the window learning to render it, and the outcome
//! says what the user may do next rather than what the machine did.

use super::*;

/// A step of the install pipeline a caller can observe while it runs.
///
/// The library verifies the payload immediately after reading it, so the three
/// events are the only points at which progress is knowable from outside: bytes
/// arrive, the transfer ends, and the payload is authenticated. Everything after
/// `Verified` is the install itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateEvent {
    /// Bytes arrived; `downloaded_bytes` is cumulative for this transfer.
    Progress(UpdateProgress),
    /// The payload has been read in full and its signature is about to be checked.
    DownloadFinished,
    /// The payload is authenticated and is about to be installed.
    Verified,
}

/// The outcome of a completed update check or install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateOutcome {
    /// The running build is already the newest release.
    UpToDate,
    /// A newer release exists; nothing was installed.
    Available { release: UpdateRelease },
    /// The release was installed.
    Installed { version: String },
}

impl UpdateOutcome {
    /// The release this outcome is about, for callers that only need the metadata.
    pub fn release(&self) -> Option<UpdateRelease> {
        match self {
            Self::UpToDate => None,
            Self::Available { release } => Some(release.clone()),
            Self::Installed { version } => Some(UpdateRelease {
                version: version.clone(),
                notes: None,
            }),
        }
    }
}
