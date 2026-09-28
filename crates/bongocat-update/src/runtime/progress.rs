//! What a release offers and how far a download has got.
//!
//! The progress is a value rather than a stream of increments, so a window that
//! polls cannot miss one and cannot double-count one. A release carries its own
//! notes and page rather than the runtime reaching back for them, which is what
//! lets the same value be handed to the window and to the log.

/// A published release this build could move to.
///
/// `notes` is the release changelog the shared manifest announces. It is optional
/// because the manifest treats it as optional: a release published without notes
/// still offers a valid update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateRelease {
    pub version: String,
    pub notes: Option<String>,
}

/// Transfer progress of an in-flight update download.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateProgress {
    pub downloaded_bytes: u64,
    /// The payload size the server announced, when it announced one.
    pub total_bytes: Option<u64>,
}

impl UpdateProgress {
    /// The completed fraction of the transfer, when the total size is known.
    pub fn fraction(self) -> Option<f32> {
        let total = self.total_bytes.filter(|total| *total > 0)?;
        Some((self.downloaded_bytes as f64 / total as f64).min(1.0) as f32)
    }
}
