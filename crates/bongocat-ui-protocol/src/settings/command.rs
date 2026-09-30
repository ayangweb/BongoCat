//! What the UI can ask for.
//!
//! One enum rather than a trait per capability, so adding a command is one
//! variant and one match rather than a new trait object the client has to carry.

use super::*;

pub struct SettingsReply<T>(pub(crate) Sender<T>);

impl<T> SettingsReply<T> {
    pub fn respond(self, value: T) -> Result<(), SettingsServiceClosed> {
        self.0
            .send_blocking(value)
            .map_err(|_| SettingsServiceClosed)
    }
}

pub enum SettingsCommand {
    SettingsWindowPlacementChanged,
    OverlayWindowPlacementChanged {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    ReadSnapshot {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// The snapshot revision alone, for pollers that only need to know whether anything
    /// changed.
    ///
    /// A full [`Self::ReadSnapshot`] also scans the model catalog, which is filesystem
    /// work a poller cannot act on. Poll this until it moves, then read the snapshot.
    ReadSnapshotRevision {
        reply: SettingsReply<u64>,
    },
    /// Read only the persisted automatic-update schedule without rebuilding the
    /// model-catalog snapshot.
    ReadAutomaticUpdateSettings {
        reply: SettingsReply<Result<AutomaticUpdateSettings, SettingsError>>,
    },
    SetOverlayVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetAppearanceTheme {
        expected_config_revision: u64,
        theme: SettingsTheme,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetLanguage {
        expected_config_revision: u64,
        language: SettingsLanguage,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetStatusIconVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetTaskbarIconVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetDockIconVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetCheckForUpdatesAutomatically {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetCheckForUpdatesIntervalHours {
        expected_config_revision: u64,
        interval_hours: u16,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetOverlaySettings {
        expected_config_revision: u64,
        settings: SettingsOverlay,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetMotionAudioEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetBehaviorShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetCommandShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetMaximumFps {
        expected_config_revision: u64,
        maximum_fps: u16,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetRandomBehaviorSettings {
        expected_config_revision: u64,
        settings: SettingsRandomBehavior,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetModelSettings {
        expected_config_revision: u64,
        settings: SettingsModelSettings,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetGamepadAxisSettings {
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Persist the gamepad-connection model switch as one atomic change.
    SetGamepadAutoSwitch {
        expected_config_revision: u64,
        settings: SettingsGamepadAutoSwitch,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Whether a model returns to the expression the user last chose for it.
    ///
    /// The switch alone: the remembered expressions themselves are never shown or
    /// edited, and turning the restore off keeps them so turning it back on
    /// restores what the user had rather than nothing.
    SetRememberLastExpression {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// The number of connected gamepads changed; the settings service
    /// reconciles the configured switch against the runtime's own answer.
    ///
    /// This carries no state on purpose. The runtime owns the connected set, so
    /// the service reads that answer while it handles the notice instead of
    /// trusting an observation that may already be stale: a notice the service
    /// could not queue is retried by the next frame, and a late notice still
    /// switches on the state that holds when it is handled.
    GamepadConnectionChanged,
    SetLoggingSettings {
        expected_config_revision: u64,
        settings: SettingsLogging,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetShortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SuspendShortcutCapture {
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    ResumeShortcutCapture {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    TriggerApplicationShortcut {
        command: SettingsApplicationShortcut,
    },
    SetStartupItemEnabled {
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SelectModel {
        expected_config_revision: u64,
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetModelTitle {
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Play one declared behavior of the model in use, without persisting
    /// anything.
    ///
    /// The shortcut rows carry this because a recorded chord is only meaningful
    /// against the motion or expression it fires: the row that holds the
    /// binding is the one place a user can hear and see what it does. The
    /// command is deliberately revision-free — it changes no configuration, so
    /// there is nothing for a stale revision to protect.
    PreviewModelBehavior {
        model: SettingsModelKey,
        behavior: SettingsModelBehavior,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetModelCover {
        model: SettingsModelKey,
        source: PathBuf,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Install cover bytes the product captured from the model itself.
    ///
    /// A user-chosen cover travels as a path ([`Self::SetModelCover`]) because the
    /// file already exists. A captured cover does not: it is produced in memory by
    /// the renderer, which writes the window it captured from and nothing else. Both
    /// end at the same store replacement, under the same PNG contract.
    ReplaceModelCover {
        model: SettingsModelKey,
        png: Vec<u8>,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    OpenModelLocation {
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Describe a user-picked folder without installing it.
    ///
    /// The settings page calls this before showing a BongoCat Mver conversion
    /// dialog, because only the service can ask the model crate what a source
    /// actually carries.
    InspectModelSource {
        source_root: PathBuf,
        reply: SettingsReply<Result<SettingsModelSourceContent, SettingsError>>,
    },
    ImportModel {
        request: SettingsModelImportRequest,
        operation: SettingsModelImportControl,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    DeleteModel {
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    OpenConfigBackupLocation {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    ExportDiagnostics {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Open the application-owned log directory in the system file manager.
    ///
    /// The path stays inside the settings service; the UI only receives the
    /// resulting settings snapshot and never needs a filesystem path.
    OpenLogsLocation {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Ask the remote worker to re-read the remote model library document.
    ///
    /// The reply carries the snapshot with the catalog back in its loading state;
    /// the fetched entries arrive through later snapshot revisions, like every
    /// other projection the window does not compute itself.
    RefreshRemoteModels {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Download a remote model catalog entry and import it when it lands.
    ///
    /// The reply reports that the operation was accepted; its progress and its
    /// outcome travel through the entry's status in later snapshot revisions.
    DownloadRemoteModel {
        id: u64,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// The remote worker has a downloaded package unpacked and ready.
    ///
    /// Only the remote worker sends this: the settings service owns the model
    /// store, so the import itself runs on the settings thread. There is no
    /// reply — the outcome is the entry's status in the next snapshot.
    RemoteModelDownloaded {
        request: SettingsRemoteModelImportRequest,
    },
    /// Persist the multiplayer room service URL. Empty means unconfigured, and
    /// the multiplayer page refuses room operations until a URL is set.
    SetMultiplayerServerUrl {
        expected_config_revision: u64,
        server_url: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Persist the nickname shown to other room members.
    SetMultiplayerNickname {
        expected_config_revision: u64,
        nickname: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Connect to the configured room service and load its lobby.
    ConnectMultiplayerService {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Disconnect from the room service and leave the current room.
    DisconnectMultiplayerService {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Refresh the lobby room list over the service's REST document.
    ///
    /// The reply carries the list back in its loading state; the rows arrive
    /// through later snapshot revisions.
    RefreshMultiplayerLobby {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Create a room and join it as its host.
    ///
    /// The reply reports that the request was accepted; the room itself, a
    /// server-side refusal or a connection failure arrive through later
    /// snapshot revisions, because the settings thread never blocks on the
    /// network.
    CreateMultiplayerRoom {
        room_name: String,
        password: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Join an existing room by its id, with its password when it has one.
    JoinMultiplayerRoom {
        room_id: String,
        password: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Leave the current room, handing the host role over server-side.
    LeaveMultiplayerRoom {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Send one chat line to the room. The overlay bubble and the chat history
    /// both come from the server's broadcast, including for the sender.
    SendMultiplayerChat {
        content: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Remove one member from the current room. Only the host may ask, and the
    /// server decides; the removal arrives through the member list.
    KickMultiplayerMember {
        member_id: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    Shutdown {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsApplicationShortcut {
    ToggleOverlay,
    ToggleIgnoreMouseInput,
    ToggleIgnoreKeyboardInput,
    ToggleIgnoreGamepadInput,
    OpenSettings,
}
