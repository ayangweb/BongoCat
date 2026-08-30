#![forbid(unsafe_code)]

use async_channel::{Receiver, Sender};
use std::fmt;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod window;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use window::{SettingsView, open_settings_window};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHealth {
    Starting,
    Ready,
    Degraded,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub runtime_health: RuntimeHealth,
    pub overlay_visible: bool,
    pub motion_audio_enabled: bool,
    pub active_model_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsErrorCode {
    ServiceUnavailable,
    RuntimeUnavailable,
    ConfigPersistFailed,
    ShutdownFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsError {
    code: SettingsErrorCode,
}

impl SettingsError {
    pub const fn new(code: SettingsErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> SettingsErrorCode {
        self.code
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.code {
            SettingsErrorCode::ServiceUnavailable => "settings service is unavailable",
            SettingsErrorCode::RuntimeUnavailable => "runtime did not apply the setting",
            SettingsErrorCode::ConfigPersistFailed => "setting could not be saved",
            SettingsErrorCode::ShutdownFailed => "application shutdown did not complete",
        })
    }
}

impl std::error::Error for SettingsError {}

pub struct SettingsReply<T>(Sender<T>);

impl<T> SettingsReply<T> {
    pub fn respond(self, value: T) -> Result<(), SettingsServiceClosed> {
        self.0
            .send_blocking(value)
            .map_err(|_| SettingsServiceClosed)
    }
}

pub enum SettingsCommand {
    ReadSnapshot {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetOverlayVisible {
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetMotionAudioEnabled {
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    Shutdown {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsServiceClosed;

impl fmt::Display for SettingsServiceClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("settings command channel is closed")
    }
}

impl std::error::Error for SettingsServiceClosed {}

#[derive(Clone)]
pub struct SettingsClient {
    commands: Sender<SettingsCommand>,
}

pub struct SettingsServiceEndpoint {
    commands: Receiver<SettingsCommand>,
}

impl SettingsClient {
    pub fn bounded(capacity: usize) -> (Self, SettingsServiceEndpoint) {
        assert!(capacity > 0, "settings command capacity must be positive");
        let (commands, receiver) = async_channel::bounded(capacity);
        (
            Self { commands },
            SettingsServiceEndpoint { commands: receiver },
        )
    }

    pub async fn read_snapshot(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ReadSnapshot { reply })
            .await
    }

    pub async fn set_overlay_visible(
        &self,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetOverlayVisible { visible, reply })
            .await
    }

    pub async fn set_motion_audio_enabled(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMotionAudioEnabled { enabled, reply })
            .await
    }

    pub async fn shutdown(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::Shutdown { reply })
            .await
    }

    pub fn read_snapshot_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ReadSnapshot { reply })
    }

    pub fn set_overlay_visible_blocking(
        &self,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetOverlayVisible { visible, reply })
    }

    pub fn set_motion_audio_enabled_blocking(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMotionAudioEnabled { enabled, reply })
    }

    pub fn shutdown_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::Shutdown { reply })
    }

    async fn request(
        &self,
        command: impl FnOnce(SettingsReply<Result<SettingsSnapshot, SettingsError>>) -> SettingsCommand,
    ) -> Result<SettingsSnapshot, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(command(SettingsReply(reply)))
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    fn request_blocking(
        &self,
        command: impl FnOnce(SettingsReply<Result<SettingsSnapshot, SettingsError>>) -> SettingsCommand,
    ) -> Result<SettingsSnapshot, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(command(SettingsReply(reply)))
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }
}

impl SettingsServiceEndpoint {
    pub fn recv_blocking(&self) -> Result<SettingsCommand, SettingsServiceClosed> {
        self.commands
            .recv_blocking()
            .map_err(|_| SettingsServiceClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn commands_are_bounded_ordered_and_receive_typed_replies() {
        let (client, endpoint) = SettingsClient::bounded(2);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetOverlayVisible { visible, reply } =
                endpoint.recv_blocking().expect("first command")
            else {
                panic!("unexpected first command");
            };
            assert!(!visible);
            reply
                .respond(Ok(snapshot(2, false, true)))
                .expect("first reply");

            let SettingsCommand::SetMotionAudioEnabled { enabled, reply } =
                endpoint.recv_blocking().expect("second command")
            else {
                panic!("unexpected second command");
            };
            assert!(!enabled);
            reply
                .respond(Ok(snapshot(3, false, false)))
                .expect("second reply");
        });

        let first = client.set_overlay_visible_blocking(false);
        let second = client.set_motion_audio_enabled_blocking(false);
        assert_eq!(first.expect("first snapshot").revision, 2);
        assert_eq!(second.expect("second snapshot").revision, 3);
        worker.join().expect("worker join");
    }

    #[test]
    fn a_closed_service_returns_a_stable_error() {
        let (client, endpoint) = SettingsClient::bounded(1);
        drop(endpoint);
        let result = client.read_snapshot_blocking();
        assert_eq!(
            result.expect_err("closed service").code(),
            SettingsErrorCode::ServiceUnavailable
        );
    }

    fn snapshot(
        revision: u64,
        overlay_visible: bool,
        motion_audio_enabled: bool,
    ) -> SettingsSnapshot {
        SettingsSnapshot {
            revision,
            runtime_health: RuntimeHealth::Ready,
            overlay_visible,
            motion_audio_enabled,
            active_model_id: Some("standard".to_owned()),
        }
    }
}
