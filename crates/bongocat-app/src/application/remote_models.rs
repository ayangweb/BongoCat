//! Where the remote model library worker puts its downloads.
//!
//! The directory lives under the environment's storage root like every other
//! per-environment artifact, so a Development build never touches Production
//! downloads and the other way around.

use std::path::PathBuf;

use super::Application;
use crate::remote_models::REMOTE_MODELS_DIRECTORY_NAME;

impl Application {
    pub(crate) fn remote_models_directory(&self) -> PathBuf {
        self.config_store
            .layout()
            .root
            .join(REMOTE_MODELS_DIRECTORY_NAME)
    }

    /// The application log handle the remote worker records its failures into.
    ///
    /// The worker runs on its own thread and never owns the `Application`, but a
    /// download it could not finish is exactly the kind of fact the log exists
    /// for: the card only ever says "failed", and the unpacked evidence is
    /// deleted the moment the failure is published.
    pub(crate) fn remote_models_log_handle(&self) -> crate::app_log::ApplicationLogHandle {
        self.application_log.clone()
    }
}
