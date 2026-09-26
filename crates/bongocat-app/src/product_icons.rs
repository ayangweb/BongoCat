//! The status and taskbar icons, as the settings worker's capabilities.
//!
//! Both sit behind a request channel rather than a direct platform call: the icon
//! belongs to the GPUI thread that owns the window, while the settings worker is
//! the one asking for the change, so the request carries the answer back.

use super::*;

pub(crate) struct StatusIconRequest {
    pub(crate) visible: bool,
    pub(crate) reply: std::sync::mpsc::SyncSender<Result<(), SettingsError>>,
}

#[derive(Clone)]
pub(crate) struct ProductStatusIcon {
    pub(crate) sender: std::sync::mpsc::SyncSender<StatusIconRequest>,
}

impl bongocat_app::StatusIconCapability for ProductStatusIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .try_send(StatusIconRequest { visible, reply })
            .map_err(|_| SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed))?
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct TaskbarIconRequest {
    pub(crate) visible: bool,
    pub(crate) reply: std::sync::mpsc::SyncSender<Result<(), SettingsError>>,
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
pub(crate) struct ProductTaskbarIcon {
    pub(crate) sender: std::sync::mpsc::SyncSender<TaskbarIconRequest>,
}

#[cfg(target_os = "windows")]
impl bongocat_app::TaskbarIconCapability for ProductTaskbarIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .try_send(TaskbarIconRequest { visible, reply })
            .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?
    }
}
