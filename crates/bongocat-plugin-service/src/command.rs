//! What a window may ask the plugin service for, and what it reads back.
//!
//! The service outlives every window, so the window cannot own the plugins and
//! cannot hold their manager. It asks for a change and reads the center's current
//! state back, exactly as the settings window asks the settings service for a
//! change and reads the resulting snapshot. That is what makes a plugin keep
//! running while the settings window is closed, and what makes two windows see
//! the same plugins.

use async_channel::{Receiver, Sender};
use bongocat_plugin_sdk::{ConfigValue, PluginCopy, PluginIcon, UiNode};
use std::collections::BTreeMap;

/// One plugin as the center shows it.
///
/// It is a projection, not the plugin: the plugin's own view tree is the only
/// source of its controls, and everything else here is metadata the manager owns.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginCard {
    pub id: String,
    /// The name and description the plugin shows when its locale map has no
    /// entry for the language on screen.
    pub name: String,
    pub description: String,
    /// The plugin's own name and description per locale.
    ///
    /// They travel with the card rather than being resolved here because the
    /// window is what knows which language it is showing, and it switches
    /// without the service having to republish anything: the map is a property
    /// of the plugin, and picking from it is presentation.
    pub localizations: BTreeMap<String, PluginCopy>,
    pub icon: PluginIcon,
    /// A glyph shown when `icon` is a local image that cannot be read.
    pub icon_fallback: Option<PluginIcon>,
    /// Absolute path to the installed local icon, when one is available.
    pub icon_path: Option<std::path::PathBuf>,
    pub enabled: bool,
    pub failed: bool,
    /// The version the selected package names, when one is installed.
    pub version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
    /// The controls the plugin publishes for the model window.
    pub actions: Vec<UiNode>,
    /// The controls the plugin publishes for the center.
    pub settings: Vec<UiNode>,
    pub errors: Vec<bongocat_plugin_sdk::PluginError>,
    pub interaction_failure: Option<PluginInteractionFailure>,
}

/// A model-window operation has no request reply for a settings window to consume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginInteractionFailure {
    Action {
        action: String,
        code: PluginCommandError,
    },
    Configure {
        key: String,
        code: PluginCommandError,
    },
}

/// Everything the plugin center renders in one read.
///
/// `error` is the last refusal the service reported, not a property of any one
/// card, because the page's retry affordance is about the center rather than
/// about a plugin. `loading` is true until the first discovery has finished.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginCenterSnapshot {
    pub cards: Vec<PluginCard>,
    pub error: Option<String>,
    pub loading: bool,
}

impl PluginCenterSnapshot {
    /// The state a center is in before it has read anything.
    pub const fn loading() -> Self {
        Self {
            cards: Vec::new(),
            error: None,
            loading: true,
        }
    }
}

impl Default for PluginCenterSnapshot {
    fn default() -> Self {
        Self::loading()
    }
}

/// Why the service refused a command.
///
/// The code is the stable part: a window maps it to a localized message, and a
/// code that is added is a message that has to be added.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PluginCommandError {
    #[error("the plugin service is unavailable")]
    ServiceUnavailable,
    #[error("the plugin is not installed")]
    NotFound,
    #[error("the plugin is not running")]
    NotRunning,
    #[error("the plugin operation failed")]
    Operation,
}

impl PluginCommandError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ServiceUnavailable => "plugin_service_unavailable",
            Self::NotFound => "plugin_not_found",
            Self::NotRunning => "plugin_not_running",
            Self::Operation => "plugin_operation_failed",
        }
    }
}

pub struct PluginReply<T>(pub(crate) Sender<T>);

impl<T> PluginReply<T> {
    pub fn respond(self, value: T) -> Result<(), PluginServiceClosed> {
        self.0.send_blocking(value).map_err(|_| PluginServiceClosed)
    }
}

/// One request to the plugin service.
pub enum PluginCommand {
    /// Read the current center state without changing anything.
    ReadCenter {
        reply: PluginReply<PluginCenterSnapshot>,
    },
    /// Scan the package store again, for the page's retry affordance.
    Refresh {
        reply: PluginReply<PluginCenterSnapshot>,
    },
    /// Start or stop a plugin's process.
    SetEnabled {
        id: String,
        enabled: bool,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Install a package archive the user chose.
    ///
    /// The bytes travel with the command rather than a path: the window has
    /// already read the file the user picked, and the service is what validates
    /// and unpacks it, so a package the user chose and a package the product
    /// downloaded cannot diverge in how they are checked.
    Install {
        bytes: Vec<u8>,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Install/update one entry from the current verified catalog.
    InstallAvailable {
        id: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Remove a plugin's executable package. Its data is kept.
    Uninstall {
        id: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Drop a plugin's persisted data, keeping its package.
    ResetData {
        id: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Send one action to a running plugin.
    Action {
        plugin: String,
        action: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Preview one select option without committing it to plugin configuration.
    Preview {
        plugin: String,
        key: String,
        value: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Set one declared configuration key on a running plugin.
    Configure {
        plugin: String,
        key: String,
        value: ConfigValue,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Tell every running plugin that localizes its views which catalog tag the
    /// host is showing.
    ///
    /// It is a suggestion rather than a control: a plugin that declares no
    /// localization keeps the copy it has always published, so the service never
    /// fails a command because one plugin cannot be localized.
    SetLocale {
        locale: String,
        reply: PluginReply<Result<PluginCenterSnapshot, PluginCommandError>>,
    },
    /// Stop every plugin and leave the worker.
    Shutdown { reply: PluginReply<()> },
}

/// The service's command channel is closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the plugin service channel is closed")]
pub struct PluginServiceClosed;

/// The window end of the plugin service channel.
#[derive(Clone)]
pub struct PluginServiceClient {
    commands: Sender<PluginCommand>,
}

impl PluginServiceClient {
    pub fn bounded(capacity: usize) -> (Self, PluginServiceEndpoint) {
        assert!(capacity > 0, "plugin command capacity must be positive");
        let (commands, receiver) = async_channel::bounded(capacity);
        (
            Self { commands },
            PluginServiceEndpoint { commands: receiver },
        )
    }

    pub async fn read_center(&self) -> Result<PluginCenterSnapshot, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send(PluginCommand::ReadCenter {
            reply: PluginReply(reply),
        })
        .await?;
        receiver.recv().await.map_err(|_| closed())
    }

    pub async fn refresh(&self) -> Result<PluginCenterSnapshot, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send(PluginCommand::Refresh {
            reply: PluginReply(reply),
        })
        .await?;
        receiver.recv().await.map_err(|_| closed())
    }

    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::SetEnabled {
            id: id.to_owned(),
            enabled,
            reply,
        })
        .await
    }

    pub async fn install(
        &self,
        bytes: Vec<u8>,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::Install { bytes, reply })
            .await
    }

    pub async fn install_available(
        &self,
        id: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::InstallAvailable {
            id: id.to_owned(),
            reply,
        })
        .await
    }

    pub fn install_available_blocking(
        &self,
        id: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::InstallAvailable {
            id: id.to_owned(),
            reply,
        })
    }

    pub async fn uninstall(&self, id: &str) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::Uninstall {
            id: id.to_owned(),
            reply,
        })
        .await
    }

    pub async fn reset_data(&self, id: &str) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::ResetData {
            id: id.to_owned(),
            reply,
        })
        .await
    }

    pub async fn action(
        &self,
        plugin: &str,
        action: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::Action {
            plugin: plugin.to_owned(),
            action: action.to_owned(),
            reply,
        })
        .await
    }

    pub async fn configure(
        &self,
        plugin: &str,
        key: &str,
        value: ConfigValue,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request(|reply| PluginCommand::Configure {
            plugin: plugin.to_owned(),
            key: key.to_owned(),
            value,
            reply,
        })
        .await
    }

    /// Ask the service to stop and wait for it to have stopped.
    ///
    /// The reply is what makes this a shutdown rather than a request: the plugin
    /// processes have been joined by the time it arrives.
    pub async fn shutdown(&self) -> Result<(), PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send(PluginCommand::Shutdown {
            reply: PluginReply(reply),
        })
        .await?;
        receiver.recv().await.map_err(|_| closed())
    }

    /// The same requests, for a caller that is not a task.
    ///
    /// The settings worker owns one blocking receive loop, so it cannot await a
    /// reply; these are the same commands over the same channel, kept beside their
    /// async twins so a reader changing one has to change the other.
    pub fn read_center_blocking(&self) -> Result<PluginCenterSnapshot, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send_blocking(PluginCommand::ReadCenter {
            reply: PluginReply(reply),
        })?;
        receiver.recv_blocking().map_err(|_| closed())
    }

    pub fn refresh_blocking(&self) -> Result<PluginCenterSnapshot, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send_blocking(PluginCommand::Refresh {
            reply: PluginReply(reply),
        })?;
        receiver.recv_blocking().map_err(|_| closed())
    }

    pub fn set_enabled_blocking(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::SetEnabled {
            id: id.to_owned(),
            enabled,
            reply,
        })
    }

    pub fn install_blocking(
        &self,
        bytes: Vec<u8>,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::Install { bytes, reply })
    }

    pub fn uninstall_blocking(&self, id: &str) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::Uninstall {
            id: id.to_owned(),
            reply,
        })
    }

    pub fn reset_data_blocking(
        &self,
        id: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::ResetData {
            id: id.to_owned(),
            reply,
        })
    }

    pub fn action_blocking(
        &self,
        plugin: &str,
        action: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::Action {
            plugin: plugin.to_owned(),
            action: action.to_owned(),
            reply,
        })
    }

    pub fn configure_blocking(
        &self,
        plugin: &str,
        key: &str,
        value: ConfigValue,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::Configure {
            plugin: plugin.to_owned(),
            key: key.to_owned(),
            value,
            reply,
        })
    }

    pub fn preview_blocking(
        &self,
        plugin: &str,
        key: &str,
        value: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::Preview {
            plugin: plugin.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
            reply,
        })
    }

    pub fn set_locale_blocking(
        &self,
        locale: &str,
    ) -> Result<PluginCenterSnapshot, PluginCommandError> {
        self.request_blocking(|reply| PluginCommand::SetLocale {
            locale: locale.to_owned(),
            reply,
        })
    }

    /// Ask the service to stop, and wait for the answer that says it has.
    pub fn shutdown_blocking(&self) -> Result<(), PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send_blocking(PluginCommand::Shutdown {
            reply: PluginReply(reply),
        })?;
        receiver.recv_blocking().map_err(|_| closed())
    }

    fn send_blocking(&self, command: PluginCommand) -> Result<(), PluginCommandError> {
        self.commands
            .send_blocking(command)
            .map_err(|_| PluginCommandError::ServiceUnavailable)
    }

    fn request_blocking<T>(
        &self,
        command: impl FnOnce(PluginReply<Result<T, PluginCommandError>>) -> PluginCommand,
    ) -> Result<T, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send_blocking(command(PluginReply(reply)))?;
        receiver.recv_blocking().map_err(|_| closed())?
    }

    async fn send(&self, command: PluginCommand) -> Result<(), PluginCommandError> {
        self.commands
            .send(command)
            .await
            .map_err(|_| PluginCommandError::ServiceUnavailable)
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(PluginReply<Result<T, PluginCommandError>>) -> PluginCommand,
    ) -> Result<T, PluginCommandError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.send(command(PluginReply(reply))).await?;
        receiver.recv().await.map_err(|_| closed())?
    }
}

const fn closed() -> PluginCommandError {
    PluginCommandError::ServiceUnavailable
}

/// The service end of the plugin command channel.
pub struct PluginServiceEndpoint {
    commands: Receiver<PluginCommand>,
}

impl PluginServiceEndpoint {
    pub(crate) fn close(&self) {
        self.commands.close();
        while self.commands.try_recv().is_ok() {}
    }
    /// Take the next command, if one is waiting.
    ///
    /// The worker's loop is the service's whole clock: it is already waking on its
    /// own interval to tick plugins and to compose, so it takes commands there
    /// rather than blocking on one. A closed channel is the one thing that has to
    /// be distinguished from an empty one, because it is what tells the worker its
    /// client is gone.
    pub fn try_recv(&self) -> Result<Option<PluginCommand>, PluginServiceClosed> {
        match self.commands.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
            Err(async_channel::TryRecvError::Closed) => Err(PluginServiceClosed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_starts_loading_and_a_closed_service_is_reported() {
        assert!(PluginCenterSnapshot::default().loading);
        assert_eq!(
            PluginCommandError::ServiceUnavailable.code(),
            "plugin_service_unavailable"
        );
    }

    #[test]
    fn closing_admission_drops_queued_replies_and_refuses_new_commands() {
        let (client, endpoint) = PluginServiceClient::bounded(2);
        let (reply, receiver) = async_channel::bounded(1);
        client
            .send_blocking(PluginCommand::ReadCenter {
                reply: PluginReply(reply),
            })
            .unwrap();
        endpoint.close();
        assert!(receiver.recv_blocking().is_err());
        assert_eq!(
            client.read_center_blocking(),
            Err(PluginCommandError::ServiceUnavailable)
        );
        assert!(endpoint.try_recv().is_err());
    }
}
