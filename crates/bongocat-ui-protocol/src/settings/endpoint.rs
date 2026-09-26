//! Receiving the other end of the channel.
//!
//! A service owns the receiver and the window owns the sender; the endpoint is
//! the handle the service keeps, and dropping it is what tells a waiting window
//! that nothing is coming.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("settings command channel is closed")]
pub struct SettingsServiceClosed;

pub struct SettingsServiceEndpoint {
    pub(crate) commands: Receiver<SettingsCommand>,
}

pub(crate) type PreparedModelImport = (
    SettingsModelImportOperation,
    SettingsModelImportControl,
    SettingsReply<Result<SettingsSnapshot, SettingsError>>,
);

impl SettingsServiceEndpoint {
    pub fn recv_blocking(&self) -> Result<SettingsCommand, SettingsServiceClosed> {
        self.commands
            .recv_blocking()
            .map_err(|_| SettingsServiceClosed)
    }

    /// Receive the next command without blocking.
    pub fn try_recv(&self) -> Result<SettingsCommand, SettingsServiceClosed> {
        self.commands.try_recv().map_err(|_| SettingsServiceClosed)
    }
}
