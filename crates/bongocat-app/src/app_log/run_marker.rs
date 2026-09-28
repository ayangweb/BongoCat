//! How the last run ended, recorded on disk so the next one can say so.
//!
//! The marker is written when the application starts and removed when it finishes
//! cleanly, so a marker found at startup is a run that did not finish. It says
//! whether that run panicked or was interrupted, and nothing more: a marker
//! carries no stack and no path, because it outlives the run and is read by the
//! next one.

use super::*;

pub(crate) const RUN_MARKER_PANICKED: &[u8] = b"{\"schema_version\":1,\"phase\":\"panicked\"}\n";

#[derive(Debug)]
pub(crate) struct ApplicationRunMarker {
    pub(crate) path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreviousRunState {
    ForcedOrUnknown,
    Panic,
    ShutdownInterrupted,
}

impl ApplicationRunMarker {
    pub(crate) fn mark_shutdown_started(&self) -> Result<(), ApplicationLogError> {
        write_run_marker(&self.path, RUN_MARKER_SHUTTING_DOWN)
    }

    pub(crate) fn complete(self) -> Result<(), ApplicationLogError> {
        fs::remove_file(&self.path).map_err(ApplicationLogError::RemoveRunMarker)
    }
}

pub(crate) fn write_run_marker(path: &Path, contents: &[u8]) -> Result<(), ApplicationLogError> {
    let mut marker = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(ApplicationLogError::WriteRunMarker)?;
    set_private_file(&marker).map_err(ApplicationLogError::WriteRunMarker)?;
    marker
        .write_all(contents)
        .and_then(|()| marker.sync_all())
        .map_err(ApplicationLogError::WriteRunMarker)
}
