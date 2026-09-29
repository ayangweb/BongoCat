//! The remote model library: what the catalog advertises and where each entry
//! stands.
//!
//! The settings window reads this as one more projection in the snapshot. The
//! catalog itself lives in a worker the settings service owns; the snapshot
//! carries the last published state, and the snapshot clock watches the
//! publisher's version so a download that quietly moves its progress bar still
//! advances the revision the window polls.

use super::*;

/// Whether the settings window has catalog content to render at all.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsRemoteCatalogStatus {
    /// The window has not asked for the catalog yet.
    #[default]
    Unloaded,
    /// A refresh is fetching the catalog document.
    Loading,
    /// The catalog is on screen, possibly with per-entry download state.
    Ready,
    /// The last refresh could not produce a catalog.
    Failed,
}

/// The image encoding of a downloaded preview, so the window can hand the bytes
/// to the renderer without sniffing them again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRemoteImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
}

/// One downloaded preview image.
///
/// Equality is pointer-based: two states point at the same preview exactly when
/// they share the same fetched bytes, and the worker never re-fetches a preview
/// it already published, so an ordinary snapshot comparison never pays for
/// comparing megabytes of pixels.
#[derive(Clone, Debug)]
pub struct SettingsRemotePreviewImage {
    format: SettingsRemoteImageFormat,
    bytes: Arc<[u8]>,
}

impl SettingsRemotePreviewImage {
    pub fn new(format: SettingsRemoteImageFormat, bytes: Arc<[u8]>) -> Self {
        Self { format, bytes }
    }

    pub const fn format(&self) -> SettingsRemoteImageFormat {
        self.format
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
}

impl PartialEq for SettingsRemotePreviewImage {
    fn eq(&self, other: &Self) -> bool {
        self.format == other.format
            && (Arc::ptr_eq(&self.bytes, &other.bytes)
                || (self.bytes.len() == other.bytes.len() && self.bytes == other.bytes))
    }
}

impl Eq for SettingsRemotePreviewImage {}

/// Where one entry's preview image stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsRemotePreview {
    /// The preview URL is known but its bytes have not arrived.
    Pending,
    /// The preview image is on screen.
    Ready(SettingsRemotePreviewImage),
    /// The catalog row ships no preview, or the preview could not be fetched.
    Unavailable,
}

/// Why one entry's download or import gave up. The codes are stable vocabulary
/// the window translates into copy; they never carry error text or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRemoteModelFailure {
    /// The catalog row could not be downloaded over the network.
    DownloadFailed,
    /// The download exceeded the size bound a model package may have.
    DownloadTooLarge,
    /// The downloaded package was refused by the model import.
    ImportFailed,
}

/// Where one catalog entry stands. The download progress travels in the same
/// projection as everything else on the page so one poll renders all of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsRemoteModelStatus {
    Available,
    Downloading {
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Importing(SettingsModelImportStage),
    Installed,
    Failed(SettingsRemoteModelFailure),
}

impl SettingsRemoteModelStatus {
    /// Whether pressing download again would start a new download.
    pub const fn is_downloadable(&self) -> bool {
        matches!(self, Self::Available | Self::Installed | Self::Failed(_))
    }

    /// Whether the entry is in the middle of an operation that must not be
    /// interrupted by starting a second one.
    pub const fn is_in_flight(&self) -> bool {
        !self.is_downloadable()
    }
}

/// One entry of the remote model library catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRemoteModelEntry {
    /// The stable entry identity, derived from the download URL.
    pub id: u64,
    pub name: String,
    pub author: String,
    pub preview: SettingsRemotePreview,
    pub status: SettingsRemoteModelStatus,
}

/// The whole remote model library projection the settings window renders.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsRemoteModels {
    pub catalog: SettingsRemoteCatalogStatus,
    pub entries: Vec<SettingsRemoteModelEntry>,
}

impl SettingsRemoteModels {
    /// The entry with this identity, if the catalog carries one.
    pub fn entry(&self, id: u64) -> Option<&SettingsRemoteModelEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Whether any entry is downloading or importing. The service runs one
    /// operation at a time, and the window reads this to refuse a second one.
    pub fn has_entry_in_flight(&self) -> bool {
        self.entries.iter().any(|entry| entry.status.is_in_flight())
    }
}

/// The handoff from the remote worker to the settings service: the package is
/// on disk, unpacked and validated in shape, and the settings service imports it
/// on the thread that owns the model store.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsRemoteModelImportRequest {
    pub id: u64,
    pub title: String,
    pub source_root: PathBuf,
}
