//! Why a line could not be written.
//!
//! A logging failure is almost always a bad directory rather than a bad line, so
//! the error says which path and why — the one case where a path is worth putting
//! in a message, because it is a directory the application chose, not a file the
//! user did.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ApplicationLogError {
    #[error("cannot create application log directory: {0}")]
    CreateDirectory(io::Error),
    #[error("cannot open application log file: {0}")]
    OpenFile(io::Error),
    #[error("cannot write application run marker: {0}")]
    WriteRunMarker(io::Error),
    #[error("cannot remove application run marker: {0}")]
    RemoveRunMarker(io::Error),
}
