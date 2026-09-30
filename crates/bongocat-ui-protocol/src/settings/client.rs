//! The window end of the bounded channel: one method per command.
//!
//! Every command has an async and a blocking spelling, and they are kept side
//! by side on purpose. They are the same request over the same channel, so a
//! reader changing one has to change the other, and the only way that pairing
//! stays visible is if both are in the same file.

use super::*;
use crate::settings::endpoint::PreparedModelImport;

#[derive(Clone)]
pub struct SettingsClient {
    pub(crate) commands: Sender<SettingsCommand>,
    pub(crate) next_operation_id: Arc<AtomicU64>,
}

impl SettingsClient {
    pub fn bounded(capacity: usize) -> (Self, SettingsServiceEndpoint) {
        assert!(capacity > 0, "settings command capacity must be positive");
        let (commands, receiver) = async_channel::bounded(capacity);
        (
            Self {
                commands,
                next_operation_id: Arc::new(AtomicU64::new(1)),
            },
            SettingsServiceEndpoint { commands: receiver },
        )
    }

    pub fn track_window_state(
        &self,
        placement: Option<SettingsWindowPlacement>,
    ) -> SettingsWindowState {
        SettingsWindowState::tracked(placement, self.commands.clone())
    }

    pub fn update_overlay_window_placement(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), SettingsServiceClosed> {
        self.commands
            .try_send(SettingsCommand::OverlayWindowPlacementChanged {
                x,
                y,
                width,
                height,
            })
            .map_err(|_| SettingsServiceClosed)
    }

    pub async fn read_snapshot(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ReadSnapshot { reply })
            .await
    }

    /// The snapshot revision, without building the snapshot.
    ///
    /// The answer costs the service one comparison against state it already holds, so a
    /// change detector can poll it at a display cadence instead of rebuilding and
    /// comparing whole snapshots.
    pub async fn read_snapshot_revision(&self) -> Result<u64, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(SettingsCommand::ReadSnapshotRevision {
                reply: SettingsReply(reply),
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))
    }

    pub async fn read_automatic_update_settings(
        &self,
    ) -> Result<AutomaticUpdateSettings, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(SettingsCommand::ReadAutomaticUpdateSettings {
                reply: SettingsReply(reply),
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    pub async fn set_overlay_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetOverlayVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_appearance_theme(
        &self,
        expected_config_revision: u64,
        theme: SettingsTheme,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetAppearanceTheme {
            expected_config_revision,
            theme,
            reply,
        })
        .await
    }

    pub async fn set_language(
        &self,
        expected_config_revision: u64,
        language: SettingsLanguage,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetLanguage {
            expected_config_revision,
            language,
            reply,
        })
        .await
    }

    pub async fn set_status_icon_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetStatusIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_taskbar_icon_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetTaskbarIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_dock_icon_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetDockIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_check_for_updates_automatically(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetCheckForUpdatesAutomatically {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    pub async fn set_check_for_updates_interval_hours(
        &self,
        expected_config_revision: u64,
        interval_hours: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetCheckForUpdatesIntervalHours {
            expected_config_revision,
            interval_hours,
            reply,
        })
        .await
    }

    pub async fn set_motion_audio_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMotionAudioEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    pub async fn set_behavior_shortcuts_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetBehaviorShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    /// Whether the application command bindings reach the platform table.
    ///
    /// The recorded bindings stay in the configuration and the model behaviour
    /// gate is untouched: this switch only decides whether the command half of
    /// the live table is populated.
    pub async fn set_command_shortcuts_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetCommandShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    pub async fn set_maximum_fps(
        &self,
        expected_config_revision: u64,
        maximum_fps: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMaximumFps {
            expected_config_revision,
            maximum_fps,
            reply,
        })
        .await
    }

    pub async fn set_random_behavior_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsRandomBehavior,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetRandomBehaviorSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_model_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_gamepad_axis_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetGamepadAxisSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_gamepad_auto_switch(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAutoSwitch,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetGamepadAutoSwitch {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    /// Whether a model returns to the expression the user last chose for it.
    pub async fn set_remember_last_expression(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetRememberLastExpression {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    /// Tell the settings service that gamepad connectivity changed.
    ///
    /// The caller is the product frame source, which observes the runtime's
    /// connected count without doing any work for it. A closed channel is the
    /// only failure: the caller retries on its next frame, and a service that
    /// never hears the notice simply has nothing to reconcile.
    pub fn notify_gamepad_connection_changed(&self) -> Result<(), SettingsServiceClosed> {
        self.commands
            .try_send(SettingsCommand::GamepadConnectionChanged)
            .map_err(|_| SettingsServiceClosed)
    }

    /// Tell the settings service that the remote worker has a package ready to
    /// import.
    ///
    /// The caller is the remote worker, and there is no reply: the import runs
    /// on the settings thread and its outcome is the entry's status in the next
    /// snapshot. A closed channel is the only failure.
    pub fn notify_remote_model_downloaded(
        &self,
        request: SettingsRemoteModelImportRequest,
    ) -> Result<(), SettingsServiceClosed> {
        self.commands
            .try_send(SettingsCommand::RemoteModelDownloaded { request })
            .map_err(|_| SettingsServiceClosed)
    }

    /// Ask the remote worker to re-read the remote model library document.
    pub async fn refresh_remote_models(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::RefreshRemoteModels { reply })
            .await
    }

    /// Download one remote model catalog entry; the import follows on its own.
    pub async fn download_remote_model(&self, id: u64) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::DownloadRemoteModel { id, reply })
            .await
    }

    /// Persist the multiplayer room service URL.
    pub async fn set_multiplayer_server_url(
        &self,
        expected_config_revision: u64,
        server_url: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMultiplayerServerUrl {
            expected_config_revision,
            server_url,
            reply,
        })
        .await
    }

    /// Persist the nickname shown to other room members.
    pub async fn set_multiplayer_nickname(
        &self,
        expected_config_revision: u64,
        nickname: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMultiplayerNickname {
            expected_config_revision,
            nickname,
            reply,
        })
        .await
    }

    /// Refresh the lobby room list; the rows arrive through later revisions.
    pub async fn connect_multiplayer_service(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ConnectMultiplayerService { reply })
            .await
    }

    /// Disconnect from the room service and leave the current room.
    pub async fn disconnect_multiplayer_service(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::DisconnectMultiplayerService { reply })
            .await
    }

    pub async fn refresh_multiplayer_lobby(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::RefreshMultiplayerLobby { reply })
            .await
    }

    /// Create a room and join it as its host; the outcome travels through the
    /// multiplayer projection.
    pub async fn create_multiplayer_room(
        &self,
        room_name: String,
        password: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::CreateMultiplayerRoom {
            room_name,
            password,
            reply,
        })
        .await
    }

    /// Join an existing room; the outcome travels through the multiplayer
    /// projection.
    pub async fn join_multiplayer_room(
        &self,
        room_id: String,
        password: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::JoinMultiplayerRoom {
            room_id,
            password,
            reply,
        })
        .await
    }

    /// Leave the current room.
    pub async fn leave_multiplayer_room(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::LeaveMultiplayerRoom { reply })
            .await
    }

    /// Send one chat line to the room.
    pub async fn send_multiplayer_chat(
        &self,
        content: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SendMultiplayerChat { content, reply })
            .await
    }

    /// Remove one member from the current room.
    pub async fn kick_multiplayer_member(
        &self,
        member_id: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::KickMultiplayerMember { member_id, reply })
            .await
    }

    pub async fn set_logging_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsLogging,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetLoggingSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_shortcuts(
        &self,
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetShortcuts {
            expected_config_revision,
            shortcuts,
            reply,
        })
        .await
    }

    pub async fn suspend_shortcut_capture(
        &self,
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SuspendShortcutCapture {
            expected_config_revision,
            shortcuts_without_capture_target,
            reply,
        })
        .await
    }

    pub async fn resume_shortcut_capture(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ResumeShortcutCapture { reply })
            .await
    }

    pub async fn set_overlay_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsOverlay,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetOverlaySettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_startup_item_enabled(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetStartupItemEnabled { enabled, reply })
            .await
    }

    pub async fn select_model(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SelectModel {
            expected_config_revision,
            model,
            reply,
        })
        .await
    }

    pub async fn set_model_title(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelTitle {
            expected_config_revision,
            model,
            title,
            reply,
        })
        .await
    }

    pub async fn preview_model_behavior(
        &self,
        model: SettingsModelKey,
        behavior: SettingsModelBehavior,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::PreviewModelBehavior {
            model,
            behavior,
            reply,
        })
        .await
    }

    pub async fn set_model_cover(
        &self,
        model: SettingsModelKey,
        source: PathBuf,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelCover {
            model,
            source,
            reply,
        })
        .await
    }

    /// Install a cover the product captured itself.
    ///
    /// Called from the thread that owns the overlay windows, right after it rendered
    /// the model into that cover.
    pub async fn replace_model_cover(
        &self,
        model: SettingsModelKey,
        png: Vec<u8>,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ReplaceModelCover { model, png, reply })
            .await
    }

    pub async fn open_model_location(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::OpenModelLocation { model, reply })
            .await
    }

    pub async fn inspect_model_source(
        &self,
        source_root: PathBuf,
    ) -> Result<SettingsModelSourceContent, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(SettingsCommand::InspectModelSource {
                source_root,
                reply: SettingsReply(reply),
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    pub async fn start_model_import(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsModelImportOperation, SettingsError> {
        let (operation, control, reply) = self.prepare_model_import()?;
        self.commands
            .send(SettingsCommand::ImportModel {
                request,
                operation: control,
                reply,
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        Ok(operation)
    }

    pub async fn delete_model(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::DeleteModel { model, reply })
            .await
    }

    pub async fn open_logs_location(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::OpenLogsLocation { reply })
            .await
    }

    pub async fn shutdown(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::Shutdown { reply })
            .await
    }

    pub fn read_snapshot_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ReadSnapshot { reply })
    }

    /// The snapshot revision, without building the snapshot.
    pub fn read_snapshot_revision_blocking(&self) -> Result<u64, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(SettingsCommand::ReadSnapshotRevision {
                reply: SettingsReply(reply),
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))
    }

    pub fn read_automatic_update_settings_blocking(
        &self,
    ) -> Result<AutomaticUpdateSettings, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(SettingsCommand::ReadAutomaticUpdateSettings {
                reply: SettingsReply(reply),
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    pub fn set_overlay_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetOverlayVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_appearance_theme_blocking(
        &self,
        expected_config_revision: u64,
        theme: SettingsTheme,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetAppearanceTheme {
            expected_config_revision,
            theme,
            reply,
        })
    }

    pub fn set_language_blocking(
        &self,
        expected_config_revision: u64,
        language: SettingsLanguage,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetLanguage {
            expected_config_revision,
            language,
            reply,
        })
    }

    pub fn set_status_icon_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetStatusIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_taskbar_icon_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetTaskbarIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_dock_icon_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetDockIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_check_for_updates_automatically_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetCheckForUpdatesAutomatically {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_check_for_updates_interval_hours_blocking(
        &self,
        expected_config_revision: u64,
        interval_hours: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetCheckForUpdatesIntervalHours {
            expected_config_revision,
            interval_hours,
            reply,
        })
    }

    pub fn set_motion_audio_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMotionAudioEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_behavior_shortcuts_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetBehaviorShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_command_shortcuts_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetCommandShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_maximum_fps_blocking(
        &self,
        expected_config_revision: u64,
        maximum_fps: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMaximumFps {
            expected_config_revision,
            maximum_fps,
            reply,
        })
    }

    pub fn set_random_behavior_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsRandomBehavior,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetRandomBehaviorSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_model_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_gamepad_axis_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetGamepadAxisSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_gamepad_auto_switch_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAutoSwitch,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetGamepadAutoSwitch {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_logging_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsLogging,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetLoggingSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_shortcuts_blocking(
        &self,
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetShortcuts {
            expected_config_revision,
            shortcuts,
            reply,
        })
    }

    pub fn suspend_shortcut_capture_blocking(
        &self,
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SuspendShortcutCapture {
            expected_config_revision,
            shortcuts_without_capture_target,
            reply,
        })
    }

    pub fn resume_shortcut_capture_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ResumeShortcutCapture { reply })
    }

    pub fn enqueue_application_shortcut(
        &self,
        command: SettingsApplicationShortcut,
    ) -> Result<(), SettingsServiceClosed> {
        self.commands
            .send_blocking(SettingsCommand::TriggerApplicationShortcut { command })
            .map_err(|_| SettingsServiceClosed)
    }

    pub fn set_overlay_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsOverlay,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetOverlaySettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_startup_item_enabled_blocking(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetStartupItemEnabled { enabled, reply })
    }

    pub fn select_model_blocking(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SelectModel {
            expected_config_revision,
            model,
            reply,
        })
    }

    pub fn set_model_title_blocking(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelTitle {
            expected_config_revision,
            model,
            title,
            reply,
        })
    }

    pub fn set_model_cover_blocking(
        &self,
        model: SettingsModelKey,
        source: PathBuf,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelCover {
            model,
            source,
            reply,
        })
    }

    pub fn replace_model_cover_blocking(
        &self,
        model: SettingsModelKey,
        png: Vec<u8>,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ReplaceModelCover { model, png, reply })
    }

    pub fn open_model_location_blocking(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::OpenModelLocation { model, reply })
    }

    pub fn inspect_model_source_blocking(
        &self,
        source_root: PathBuf,
    ) -> Result<SettingsModelSourceContent, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(SettingsCommand::InspectModelSource {
                source_root,
                reply: SettingsReply(reply),
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    pub fn import_model_blocking(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.start_model_import_blocking(request)?
            .final_result_blocking()
            .result
    }

    pub fn start_model_import_blocking(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsModelImportOperation, SettingsError> {
        let (operation, control, reply) = self.prepare_model_import()?;
        self.commands
            .send_blocking(SettingsCommand::ImportModel {
                request,
                operation: control,
                reply,
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        Ok(operation)
    }

    pub fn delete_model_blocking(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::DeleteModel { model, reply })
    }

    pub fn open_config_backup_location_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::OpenConfigBackupLocation { reply })
    }

    pub fn export_diagnostics_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ExportDiagnostics { reply })
    }

    pub fn open_logs_location_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::OpenLogsLocation { reply })
    }

    pub fn shutdown_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::Shutdown { reply })
    }

    pub fn set_multiplayer_server_url_blocking(
        &self,
        expected_config_revision: u64,
        server_url: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMultiplayerServerUrl {
            expected_config_revision,
            server_url,
            reply,
        })
    }

    pub fn set_multiplayer_nickname_blocking(
        &self,
        expected_config_revision: u64,
        nickname: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMultiplayerNickname {
            expected_config_revision,
            nickname,
            reply,
        })
    }

    pub fn refresh_multiplayer_lobby_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::RefreshMultiplayerLobby { reply })
    }

    pub fn create_multiplayer_room_blocking(
        &self,
        room_name: String,
        password: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::CreateMultiplayerRoom {
            room_name,
            password,
            reply,
        })
    }

    pub fn join_multiplayer_room_blocking(
        &self,
        room_id: String,
        password: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::JoinMultiplayerRoom {
            room_id,
            password,
            reply,
        })
    }

    pub fn leave_multiplayer_room_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::LeaveMultiplayerRoom { reply })
    }

    pub fn send_multiplayer_chat_blocking(
        &self,
        content: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SendMultiplayerChat { content, reply })
    }

    pub fn kick_multiplayer_member_blocking(
        &self,
        member_id: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::KickMultiplayerMember { member_id, reply })
    }

    pub(crate) async fn request(
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

    pub(crate) fn request_blocking(
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

    pub(crate) fn prepare_model_import(&self) -> Result<PreparedModelImport, SettingsError> {
        let operation_id = self
            .next_operation_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map(SettingsOperationId)
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        let control = SettingsModelImportControl {
            operation_id,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(SettingsModelImportProgress {
                stage: SettingsModelImportStage::Preparing,
                files_copied: 0,
                bytes_copied: 0,
            })),
        };
        let (reply, result) = async_channel::bounded(1);
        Ok((
            SettingsModelImportOperation {
                control: control.clone(),
                result,
            },
            control,
            SettingsReply(reply),
        ))
    }
}
