#![forbid(unsafe_code)]

#[cfg(all(
    feature = "storage-test-injection",
    bongocat_build_environment = "production"
))]
compile_error!("storage-test-injection cannot be enabled for Production builds");

use bongocat_audio::{MotionAudioService, MotionAudioShutdownError};
use bongocat_config::{
    ApplicationState, BuildEnvironment, CompiledShortcuts, ConfigError, ConfigRecovery,
    ConfigRevision, ConfigStore, InterruptedConfigRecovery, Language, ModelBehaviorBinding,
    NativeConfig, OverlayWindowPlacement, PlatformStorageError, SelectedModelOrigin,
    ShortcutBinding, ShortcutConfig, ShortcutTable, StateError, StateStore, StorageLayout,
    Theme as ConfigTheme, WindowPlacement, platform_layout,
};
use bongocat_model::{
    CommittedModel, InstalledModel, ModelCatalogEntry, ModelError, ModelId, ModelImportProgress,
    ModelOrigin, ModelPackageLimits, ModelStore, ModelStoreError, PresetModelCatalog,
};
use bongocat_render::{ModelCommitToken, RenderConsumer};
use bongocat_runtime::{
    CursorProducer, ExpressionId, ExpressionIdError, GamepadAxisProducer, GamepadAxisSettings,
    GamepadButton, HandSide, InputBindings, InputProducer, ModelSettings, MotionId, MotionIdError,
    MotionPriority, OverlaySettings, PhysicalKey, RuntimeClient, RuntimeCommand,
    RuntimeCommandFailure, RuntimeOwner, RuntimeRenderErrorCode, RuntimeSnapshot, SendError,
    ShutdownError, maximum_fps_is_valid, release_fallback_timeout_is_valid,
};
use std::{
    collections::BTreeMap,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

mod app_log;
#[cfg(test)]
mod build_environment_contract;
#[cfg(test)]
mod product_icon_contract;
mod settings;
use app_log::ApplicationRunMarker;
pub use app_log::{
    ApplicationLogCode, ApplicationLogComponent, ApplicationLogDiagnostics, ApplicationLogError,
    ApplicationLogEvent, ApplicationLogEventCounts, ApplicationLogHandle, ApplicationLogLevel,
    ApplicationPanicHook,
};
pub use settings::{
    ApplicationSettingsService, SettingsServiceJoinError, StatusIconCapability,
    TaskbarIconCapability,
};

#[derive(Clone, Default)]
pub struct ApplicationShortcutSignals {
    open_settings: Arc<AtomicBool>,
}

impl ApplicationShortcutSignals {
    pub fn request_open_settings(&self) {
        self.open_settings.store(true, Ordering::Release);
    }

    pub fn take_open_settings_request(&self) -> bool {
        self.open_settings.swap(false, Ordering::AcqRel)
    }
}

const COMMAND_CAPACITY: usize = 64;
const AUDIO_COMMAND_CAPACITY: usize = 16;
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(bongocat_build_environment = "development")]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Development;

#[cfg(bongocat_build_environment = "production")]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Production;

#[derive(Debug)]
pub enum ApplicationError {
    PlatformStorage(PlatformStorageError),
    Config(ConfigError),
    Model(ModelError),
    ModelStore(ModelStoreError),
    MotionId(MotionIdError),
    ExpressionId(ExpressionIdError),
    PresetModelDeletion(ModelId),
    SelectedModelDeletion(ModelId),
    RuntimeCommand(SendError),
    RuntimeCommandFailed(RuntimeCommandFailure),
    RuntimeDidNotPublish,
    RuntimeDidNotPrepareModel,
    RenderConsumerUnavailable,
    Shutdown(ShutdownError),
    MotionAudioShutdown(MotionAudioShutdownError),
    ApplicationLog(ApplicationLogError),
    ConfigRollback(ConfigError),
    State(StateError),
    ConfigurationRecoveryRequired,
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformStorage(error) => write!(formatter, "storage setup failed: {error}"),
            Self::Config(error) => write!(formatter, "configuration failed: {error}"),
            Self::Model(error) => write!(formatter, "model preparation failed: {error}"),
            Self::ModelStore(error) => write!(formatter, "model store failed: {error}"),
            Self::MotionId(error) => write!(formatter, "motion id failed: {error}"),
            Self::ExpressionId(error) => write!(formatter, "expression id failed: {error}"),
            Self::PresetModelDeletion(id) => {
                write!(formatter, "preset model cannot be deleted: {}", id.as_str())
            }
            Self::SelectedModelDeletion(id) => {
                write!(
                    formatter,
                    "selected model cannot be deleted: {}",
                    id.as_str()
                )
            }
            Self::RuntimeCommand(error) => write!(formatter, "runtime command failed: {error}"),
            Self::RuntimeCommandFailed(failure) => write!(
                formatter,
                "runtime command {} failed: {:?}",
                failure.sequence, failure.code
            ),
            Self::RuntimeDidNotPublish => {
                formatter.write_str("runtime did not publish the requested revision")
            }
            Self::RuntimeDidNotPrepareModel => {
                formatter.write_str("runtime did not prepare the requested render model")
            }
            Self::RenderConsumerUnavailable => {
                formatter.write_str("application render consumer is unavailable")
            }
            Self::Shutdown(error) => write!(formatter, "shutdown failed: {error}"),
            Self::MotionAudioShutdown(error) => {
                write!(formatter, "motion audio shutdown failed: {error}")
            }
            Self::ApplicationLog(error) => write!(formatter, "application logging failed: {error}"),
            Self::ConfigRollback(error) => {
                write!(formatter, "model selection config rollback failed: {error}")
            }
            Self::State(error) => write!(formatter, "application state failed: {error}"),
            Self::ConfigurationRecoveryRequired => {
                formatter.write_str("configuration recovery is required")
            }
        }
    }
}

impl std::error::Error for ApplicationError {}

impl From<PlatformStorageError> for ApplicationError {
    fn from(error: PlatformStorageError) -> Self {
        Self::PlatformStorage(error)
    }
}

impl From<ConfigError> for ApplicationError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<StateError> for ApplicationError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<ModelError> for ApplicationError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<MotionIdError> for ApplicationError {
    fn from(error: MotionIdError) -> Self {
        Self::MotionId(error)
    }
}

impl From<ExpressionIdError> for ApplicationError {
    fn from(error: ExpressionIdError) -> Self {
        Self::ExpressionId(error)
    }
}

impl From<ModelStoreError> for ApplicationError {
    fn from(error: ModelStoreError) -> Self {
        Self::ModelStore(error)
    }
}

impl From<ApplicationLogError> for ApplicationError {
    fn from(error: ApplicationLogError) -> Self {
        Self::ApplicationLog(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationConfigStatus {
    Ready,
    RecoveryRequired { checked_backups: usize },
    DefaultsRestoredRestartRequired,
}

pub struct Application {
    config_store: ConfigStore,
    state_store: StateStore,
    state: ApplicationState,
    config: NativeConfig,
    config_revision: Option<ConfigRevision>,
    config_status: ApplicationConfigStatus,
    system_language: Language,
    config_recovery: Option<ConfigRecovery>,
    interrupted_config_recovery: Option<InterruptedConfigRecovery>,
    preset_models: PresetModelCatalog,
    model_store: ModelStore,
    active_model_origin: Option<ModelOrigin>,
    runtime: RuntimeOwner,
    motion_audio: Option<MotionAudioService>,
    render_consumer: Option<RenderConsumer>,
    application_log: ApplicationLogHandle,
    run_marker: ApplicationRunMarker,
    panic_hook: Option<ApplicationPanicHook>,
    shortcut_table: ShortcutTable,
}

impl Application {
    pub fn start(preset_root: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(
            platform_layout(BUILD_ENVIRONMENT)?,
            preset_root.as_ref(),
            true,
            system_language(),
        )
    }

    #[cfg(feature = "storage-test-injection")]
    #[doc(hidden)]
    pub fn start_with_layout_for_smoke(
        layout: StorageLayout,
        preset_root: impl AsRef<Path>,
    ) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(layout, preset_root.as_ref(), true, system_language())
    }

    #[cfg(test)]
    fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            false,
            Language::EnglishUnitedStates,
        )
    }

    fn start_with_layout_internal(
        layout: StorageLayout,
        preset_root: &Path,
        enable_rendering: bool,
        system_language: Language,
    ) -> Result<Self, ApplicationError> {
        let preset_models = PresetModelCatalog::open(preset_root, ModelPackageLimits::default())?;
        let model_store = ModelStore::new(
            &layout.models,
            layout.locks.join("models.writer.lock"),
            ModelPackageLimits::default(),
        )?;
        let config_store = ConfigStore::new(layout.clone())?;
        let application_log = ApplicationLogHandle::install(&layout.logs)?;
        let (run_marker, previous_run) = application_log.begin_run()?;
        let state_store = StateStore::new(layout);
        let state = state_store.load_or_default().state;
        let (config, config_revision, config_recovery, interrupted_config_recovery, config_status) =
            match config_store.load_or_default() {
                Ok(loaded) => (
                    loaded.config,
                    Some(loaded.revision),
                    loaded.recovery,
                    loaded.interrupted_recovery,
                    ApplicationConfigStatus::Ready,
                ),
                Err(ConfigError::NoValidRecoveryBackup { candidates }) => (
                    NativeConfig::default(),
                    None,
                    None,
                    None,
                    ApplicationConfigStatus::RecoveryRequired {
                        checked_backups: candidates,
                    },
                ),
                Err(error) => return Err(error.into()),
            };
        let operational = config_status == ApplicationConfigStatus::Ready;
        let shortcut_table = ShortcutTable::new(active_shortcuts(&config)?);
        let (motion_audio, motion_audio_client) =
            match MotionAudioService::start(AUDIO_COMMAND_CAPACITY) {
                Ok(service) => {
                    let client = service.client();
                    (Some(service), client)
                }
                Err(_) => (None, bongocat_audio::MotionAudioClient::unavailable()),
            };
        let runtime_overlay_visible = operational && config.overlay.visible;
        let runtime_motion_audio_enabled = operational && config.model.play_motion_audio;
        let (runtime, render_consumer) = if enable_rendering && operational {
            let (runtime, consumer) = RuntimeOwner::start_with_rendering_and_audio(
                runtime_overlay_visible,
                runtime_motion_audio_enabled,
                COMMAND_CAPACITY,
                motion_audio_client,
            );
            (runtime, Some(consumer))
        } else {
            (
                RuntimeOwner::start_with_audio(
                    runtime_overlay_visible,
                    runtime_motion_audio_enabled,
                    COMMAND_CAPACITY,
                    motion_audio_client,
                ),
                None,
            )
        };
        runtime
            .client()
            .wait_for_revision(1, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if operational {
            let client = runtime.client();
            let sequence = client
                .send(RuntimeCommand::SetGamepadAxisSettings(
                    gamepad_axis_settings_from_config(&config)?,
                ))
                .map_err(ApplicationError::RuntimeCommand)?;
            client
                .wait_for_command(sequence, RUNTIME_TIMEOUT)
                .ok_or(ApplicationError::RuntimeDidNotPublish)?;
            let sequence = client
                .send(RuntimeCommand::SetMaximumFps(config.model.maximum_fps))
                .map_err(ApplicationError::RuntimeCommand)?;
            client
                .wait_for_command(sequence, RUNTIME_TIMEOUT)
                .ok_or(ApplicationError::RuntimeDidNotPublish)?;
            let sequence = client
                .send(RuntimeCommand::SetReleaseFallbackTimeout(
                    config.model.release_fallback_timeout_ms,
                ))
                .map_err(ApplicationError::RuntimeCommand)?;
            client
                .wait_for_command(sequence, RUNTIME_TIMEOUT)
                .ok_or(ApplicationError::RuntimeDidNotPublish)?;
            let sequence = client
                .send(RuntimeCommand::SetOverlaySettings(
                    overlay_settings_from_config(&config),
                ))
                .map_err(ApplicationError::RuntimeCommand)?;
            client
                .wait_for_command(sequence, RUNTIME_TIMEOUT)
                .ok_or(ApplicationError::RuntimeDidNotPublish)?;
            let sequence = client
                .send(RuntimeCommand::SetModelSettings(
                    model_settings_from_config(&config),
                ))
                .map_err(ApplicationError::RuntimeCommand)?;
            client
                .wait_for_command(sequence, RUNTIME_TIMEOUT)
                .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        }
        let active_model_origin = config
            .model
            .selected_model_origin
            .map(model_origin_from_config);
        let application = Self {
            config_store,
            state_store,
            state,
            config,
            config_revision,
            config_status,
            system_language,
            config_recovery,
            interrupted_config_recovery,
            preset_models,
            model_store,
            active_model_origin,
            runtime,
            motion_audio,
            render_consumer,
            application_log,
            run_marker,
            panic_hook: None,
            shortcut_table,
        };
        if previous_run.is_some() {
            application
                .application_log
                .record(ApplicationLogEvent::previous_run_unclean());
        }
        application
            .application_log
            .record(ApplicationLogEvent::started());
        Ok(application)
    }

    pub fn runtime_client(&self) -> RuntimeClient {
        self.runtime.client()
    }

    pub fn config_revision(&self) -> Option<u64> {
        self.config_revision.map(ConfigRevision::value)
    }

    pub fn input_producer(&self) -> InputProducer {
        self.runtime.input_producer()
    }

    pub fn cursor_producer(&self) -> CursorProducer {
        self.runtime.cursor_producer()
    }

    pub fn gamepad_axis_producer(&self) -> GamepadAxisProducer {
        self.runtime.gamepad_axis_producer()
    }

    pub fn take_render_consumer(&mut self) -> Result<RenderConsumer, ApplicationError> {
        self.render_consumer
            .take()
            .ok_or(ApplicationError::RenderConsumerUnavailable)
    }

    pub fn config(&self) -> &NativeConfig {
        &self.config
    }

    pub fn effective_language(&self) -> Language {
        self.config
            .appearance
            .language
            .resolve(self.system_language)
    }

    /// Compile the currently committed shortcut bindings for a platform
    /// adapter. This is read-only and never performs registration or capture.
    pub fn compiled_shortcuts(&self) -> Result<CompiledShortcuts, ApplicationError> {
        active_shortcuts(&self.config).map_err(ApplicationError::Config)
    }

    pub fn shortcut_table(&self) -> ShortcutTable {
        self.shortcut_table.clone()
    }

    pub fn logs_directory(&self) -> &Path {
        &self.config_store.layout().logs
    }

    pub fn application_log_diagnostics(&self) -> ApplicationLogDiagnostics {
        self.application_log.diagnostics()
    }

    pub fn record_log(&self, event: ApplicationLogEvent) {
        self.application_log.record(event);
    }

    pub fn install_process_panic_hook(&mut self) {
        if self.panic_hook.is_none() {
            self.panic_hook = Some(self.application_log.install_panic_hook());
        }
    }

    pub const fn config_recovery(&self) -> Option<ConfigRecovery> {
        self.config_recovery
    }

    pub const fn interrupted_config_recovery(&self) -> Option<InterruptedConfigRecovery> {
        self.interrupted_config_recovery
    }

    pub const fn config_status(&self) -> ApplicationConfigStatus {
        self.config_status
    }

    pub const fn settings_window_placement(&self) -> Option<WindowPlacement> {
        self.state.settings_window
    }

    pub const fn overlay_window_placement(&self) -> Option<OverlayWindowPlacement> {
        self.state.overlay_window
    }

    pub fn persist_settings_window_placement(
        &mut self,
        placement: Option<WindowPlacement>,
    ) -> Result<(), ApplicationError> {
        if self.state.settings_window == placement {
            return Ok(());
        }
        let state = ApplicationState::with_windows(placement, self.state.overlay_window);
        self.state_store.commit(&state)?;
        self.state = state;
        Ok(())
    }

    pub fn persist_overlay_window_placement(
        &mut self,
        placement: OverlayWindowPlacement,
    ) -> Result<(), ApplicationError> {
        if self.state.overlay_window == Some(placement) {
            return Ok(());
        }
        let state = ApplicationState::with_windows(self.state.settings_window, Some(placement));
        self.state_store.commit(&state)?;
        self.state = state;
        Ok(())
    }

    pub(crate) fn config_backup_directory(&self) -> &Path {
        &self.config_store.layout().backups
    }

    pub const fn is_operational(&self) -> bool {
        matches!(self.config_status, ApplicationConfigStatus::Ready)
    }

    pub fn restore_default_configuration(&mut self) -> Result<(), ApplicationError> {
        if !matches!(
            self.config_status,
            ApplicationConfigStatus::RecoveryRequired { .. }
        ) {
            return Err(ApplicationError::Config(ConfigError::RecoveryNotRequired));
        }
        let loaded = self.config_store.restore_default_after_failed_recovery()?;
        self.config = loaded.config;
        self.config_revision = Some(loaded.revision);
        self.config_recovery = loaded.recovery;
        self.interrupted_config_recovery = loaded.interrupted_recovery;
        self.config_status = ApplicationConfigStatus::DefaultsRestoredRestartRequired;
        Ok(())
    }

    fn ready_config_revision(&self) -> Result<ConfigRevision, ApplicationError> {
        if self.config_status != ApplicationConfigStatus::Ready {
            return Err(ApplicationError::ConfigurationRecoveryRequired);
        }
        self.config_revision
            .ok_or(ApplicationError::ConfigurationRecoveryRequired)
    }

    pub fn set_appearance_theme(&mut self, theme: ConfigTheme) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.appearance.theme = theme;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_status_icon_visible(&mut self, visible: bool) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.application.show_status_icon = visible;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_taskbar_icon_visible(&mut self, visible: bool) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.application.show_taskbar_icon = visible;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_check_for_updates_automatically(
        &mut self,
        enabled: bool,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.application.check_for_updates_automatically = enabled;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_language(&mut self, language: Language) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.appearance.language = language;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_overlay_visible(
        &mut self,
        visible: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.overlay.visible = visible;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetOverlayVisible(visible))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_overlay_settings(
        &mut self,
        settings: OverlaySettings,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        if !settings.is_valid() {
            return Err(ApplicationError::RuntimeCommandFailed(
                RuntimeCommandFailure {
                    sequence: 0,
                    code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
                },
            ));
        }
        let mut next_config = self.config.clone();
        next_config.overlay.click_through = settings.click_through;
        next_config.overlay.always_on_top = settings.always_on_top;
        next_config.overlay.scale_percent = settings.scale_percent;
        next_config.overlay.opacity_percent = settings.opacity_percent;
        next_config.overlay.keep_inside_work_area = settings.keep_inside_work_area;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetOverlaySettings(settings))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_motion_audio_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.play_motion_audio = enabled;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetMotionAudioEnabled(enabled))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_maximum_fps(
        &mut self,
        maximum_fps: u16,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        if !maximum_fps_is_valid(maximum_fps) {
            return Err(ApplicationError::RuntimeCommandFailed(
                RuntimeCommandFailure {
                    sequence: 0,
                    code: RuntimeRenderErrorCode::MaximumFpsInvalid,
                },
            ));
        }
        let mut next_config = self.config.clone();
        next_config.model.maximum_fps = maximum_fps;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetMaximumFps(maximum_fps))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_release_fallback_timeout(
        &mut self,
        timeout_ms: u32,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        if !release_fallback_timeout_is_valid(timeout_ms) {
            return Err(ApplicationError::RuntimeCommandFailed(
                RuntimeCommandFailure {
                    sequence: 0,
                    code: RuntimeRenderErrorCode::ReleaseFallbackTimeoutInvalid,
                },
            ));
        }
        let mut next_config = self.config.clone();
        next_config.model.release_fallback_timeout_ms = timeout_ms;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetReleaseFallbackTimeout(timeout_ms))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_model_settings(
        &mut self,
        settings: ModelSettings,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.mirror = settings.mirror;
        next_config.model.mirror_pointer_tracking = settings.mirror_pointer_tracking;
        next_config.model.ignore_pointer = settings.ignore_pointer;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetModelSettings(settings))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_gamepad_axis_settings(
        &mut self,
        settings: GamepadAxisSettings,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.input.gamepad_stick_dead_zone = persistent_dead_zone(settings.stick_dead_zone);
        next_config.input.gamepad_trigger_dead_zone =
            persistent_dead_zone(settings.trigger_dead_zone);
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetGamepadAxisSettings(settings))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_shortcuts(
        &mut self,
        shortcuts: bongocat_ui::SettingsShortcuts,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.shortcuts = ShortcutConfig {
            commands: shortcuts
                .commands
                .into_iter()
                .map(|binding| ShortcutBinding {
                    command: binding.command,
                    shortcut: binding.shortcut,
                })
                .collect(),
            model_behaviors: shortcuts
                .model_behaviors
                .into_iter()
                .map(|binding| ModelBehaviorBinding {
                    model_id: binding.model_id,
                    behavior_id: binding.behavior_id,
                    shortcut: binding.shortcut,
                })
                .collect(),
        };
        next_config.shortcuts = next_config.shortcuts.canonicalized()?;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config)?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn set_behavior_shortcuts_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.enable_behavior_shortcuts = enabled;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config)?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn restore_default_shortcuts(&mut self) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.shortcuts = ShortcutConfig::default();
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config)?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    pub fn model_catalog(&self) -> Result<Vec<ModelCatalogEntry>, ApplicationError> {
        let mut entries = self.preset_models.list()?;
        entries.extend(self.model_store.list()?);
        entries.sort_by(|left, right| {
            left.id().as_str().cmp(right.id().as_str()).then_with(|| {
                model_origin_order(left.origin()).cmp(&model_origin_order(right.origin()))
            })
        });
        Ok(entries)
    }

    pub const fn active_model_origin(&self) -> Option<ModelOrigin> {
        self.active_model_origin
    }

    pub fn start_motion(
        &self,
        group: impl Into<String>,
        index: usize,
        priority: MotionPriority,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::StartMotion {
            motion: MotionId::new(group, index)?,
            priority,
        })
    }

    pub fn stop_motion(
        &self,
        group: impl Into<String>,
        index: usize,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::StopMotion(MotionId::new(group, index)?))
    }

    pub fn set_expression(
        &self,
        name: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::SetExpression(ExpressionId::new(name)?))
    }

    pub fn prepare_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<ModelCommitToken, ApplicationError> {
        if self.render_consumer.is_none() {
            return Err(ApplicationError::RenderConsumerUnavailable);
        }
        let id = ModelId::parse(id)?;
        let committed = self.load_model(origin, &id)?;
        let input_bindings = input_bindings_for_model(origin, id.as_str());
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(committed),
                input_bindings: Arc::new(input_bindings),
            })
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPrepareModel)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        let token = snapshot
            .pending_model
            .filter(|pending| pending.token.command_sequence == sequence)
            .map(|pending| pending.token)
            .ok_or(ApplicationError::RuntimeDidNotPrepareModel)?;
        self.active_model_origin = Some(origin);
        Ok(token)
    }

    pub fn select_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let id = ModelId::parse(id)?;
        let committed = self.load_model(origin, &id)?;
        let mut next_config = self.config.clone();
        next_config.model.selected_model_id = Some(id.as_str().to_owned());
        next_config.model.selected_model_origin = Some(config_origin_from_model(origin));
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let result = self.wait_for_model_command(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(committed),
            input_bindings: Arc::new(input_bindings_for_model(origin, id.as_str())),
        });
        match result {
            Ok(snapshot) => {
                self.config = next_config;
                self.config_revision = Some(next_revision);
                self.active_model_origin = Some(origin);
                Ok(snapshot)
            }
            Err(error) => {
                self.config_revision = Some(
                    self.config_store
                        .commit_if_revision(&self.config, next_revision)
                        .map_err(ApplicationError::ConfigRollback)?,
                );
                Err(error)
            }
        }
    }

    fn load_model(
        &self,
        origin: ModelOrigin,
        id: &ModelId,
    ) -> Result<CommittedModel, ApplicationError> {
        match origin {
            ModelOrigin::Preset => self.preset_models.load(id).map_err(ApplicationError::Model),
            ModelOrigin::Installed => self
                .model_store
                .load(id)
                .map(CommittedModel::from)
                .map_err(ApplicationError::ModelStore),
        }
    }

    fn wait_for_model_command(
        &self,
        command: RuntimeCommand,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let client = self.runtime.client();
        let sequence = client
            .send(command)
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        Ok(snapshot)
    }

    pub fn delete_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let id = ModelId::parse(id)?;
        if origin == ModelOrigin::Preset {
            return Err(ApplicationError::PresetModelDeletion(id));
        }
        let active_installed = self.active_model_origin == Some(ModelOrigin::Installed)
            && self
                .runtime
                .client()
                .snapshot()
                .active_model
                .as_ref()
                .is_some_and(|active| active.id == id);
        let configured_installed = self.config.model.selected_model_origin
            == Some(SelectedModelOrigin::Installed)
            && self.config.model.selected_model_id.as_deref() == Some(id.as_str());
        if active_installed || configured_installed {
            return Err(ApplicationError::SelectedModelDeletion(id));
        }
        self.model_store
            .delete(&id)
            .map_err(ApplicationError::ModelStore)
    }

    pub fn import_model(
        &mut self,
        id: impl Into<String>,
        source_root: impl AsRef<Path>,
    ) -> Result<InstalledModel, ApplicationError> {
        self.import_model_with_observer(id, source_root, |_| {}, || false)
    }

    pub fn import_model_with_observer<Observe, IsCancelled>(
        &mut self,
        id: impl Into<String>,
        source_root: impl AsRef<Path>,
        observe: Observe,
        is_cancelled: IsCancelled,
    ) -> Result<InstalledModel, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let id = ModelId::parse(id)?;
        self.model_store
            .import_with_observer(id, source_root, observe, is_cancelled)
            .map_err(ApplicationError::ModelStore)
    }

    pub fn shutdown(self) -> Result<RuntimeSnapshot, ApplicationError> {
        self.application_log
            .record(ApplicationLogEvent::shutdown_started());
        self.run_marker.mark_shutdown_started()?;
        let runtime_result = self.runtime.shutdown(RUNTIME_TIMEOUT);
        let audio_result = self
            .motion_audio
            .map(|service| service.shutdown(RUNTIME_TIMEOUT))
            .transpose();
        let stopped = match runtime_result {
            Ok(stopped) => stopped,
            Err(error) => {
                self.application_log
                    .record(ApplicationLogEvent::shutdown_failed());
                return Err(ApplicationError::Shutdown(error));
            }
        };
        if let Err(error) = audio_result {
            self.application_log
                .record(ApplicationLogEvent::shutdown_failed());
            return Err(ApplicationError::MotionAudioShutdown(error));
        }
        self.run_marker.complete()?;
        self.application_log
            .record(ApplicationLogEvent::shutdown_completed());
        Ok(stopped)
    }
}

fn system_language() -> Language {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        bongocat_platform::system_language()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Language::EnglishUnitedStates
    }
}

fn model_origin_order(origin: ModelOrigin) -> u8 {
    match origin {
        ModelOrigin::Preset => 0,
        ModelOrigin::Installed => 1,
    }
}

const fn config_origin_from_model(origin: ModelOrigin) -> SelectedModelOrigin {
    match origin {
        ModelOrigin::Preset => SelectedModelOrigin::Preset,
        ModelOrigin::Installed => SelectedModelOrigin::Installed,
    }
}

const fn model_origin_from_config(origin: SelectedModelOrigin) -> ModelOrigin {
    match origin {
        SelectedModelOrigin::Preset => ModelOrigin::Preset,
        SelectedModelOrigin::Installed => ModelOrigin::Installed,
    }
}

fn overlay_settings_from_config(config: &NativeConfig) -> OverlaySettings {
    OverlaySettings {
        click_through: config.overlay.click_through,
        always_on_top: config.overlay.always_on_top,
        scale_percent: config.overlay.scale_percent,
        opacity_percent: config.overlay.opacity_percent,
        keep_inside_work_area: config.overlay.keep_inside_work_area,
    }
}

const fn model_settings_from_config(config: &NativeConfig) -> ModelSettings {
    ModelSettings {
        mirror: config.model.mirror,
        mirror_pointer_tracking: config.model.mirror_pointer_tracking,
        ignore_pointer: config.model.ignore_pointer,
    }
}

fn active_shortcuts(config: &NativeConfig) -> Result<CompiledShortcuts, ConfigError> {
    let mut shortcuts = config.shortcuts.clone();
    if !config.model.enable_behavior_shortcuts {
        shortcuts.model_behaviors.clear();
    }
    shortcuts.compile()
}

fn gamepad_axis_settings_from_config(
    config: &NativeConfig,
) -> Result<GamepadAxisSettings, ConfigError> {
    let stick_dead_zone = runtime_dead_zone(
        config.input.gamepad_stick_dead_zone,
        "input.gamepad_stick_dead_zone",
    )?;
    let trigger_dead_zone = runtime_dead_zone(
        config.input.gamepad_trigger_dead_zone,
        "input.gamepad_trigger_dead_zone",
    )?;
    GamepadAxisSettings::new(stick_dead_zone, trigger_dead_zone)
        .ok_or(ConfigError::InvalidValue("input.gamepad_dead_zone"))
}

fn runtime_dead_zone(value: f64, field: &'static str) -> Result<f32, ConfigError> {
    let value = value as f32;
    if value.is_finite() && (0.0..1.0).contains(&value) {
        Ok(value)
    } else {
        Err(ConfigError::InvalidValue(field))
    }
}

fn persistent_dead_zone(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
fn repository_preset_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("native/resources/models")
}

fn input_bindings_for_model(origin: ModelOrigin, model_id: &str) -> InputBindings {
    if origin == ModelOrigin::Installed {
        return InputBindings::default();
    }
    const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);
    let mut key_hands = BTreeMap::new();
    if matches!(model_id, "standard" | "keyboard") {
        for usage in 0x04..=0x27 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
        for usage in [
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x35, 0x38, 0x39, 0x4c, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4,
            0xe5, 0xe6, 0xe7,
        ] {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
    } else {
        key_hands.insert(PhysicalKey::KEY_A, HandSide::Left);
    }
    if matches!(model_id, "keyboard" | "gamepad") {
        for usage in RIGHT_ARROW.hid_usage()..=0x52 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Right);
        }
    }
    let gamepad_hands = if model_id == "gamepad" {
        BTreeMap::from([
            (GamepadButton::South, HandSide::Left),
            (GamepadButton::East, HandSide::Right),
        ])
    } else {
        BTreeMap::new()
    };
    InputBindings::with_gamepad_hands(key_hands, gamepad_hands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use bongocat_render::{ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome};
    use bongocat_runtime::{
        GamepadAxis, GamepadAxisKey, GamepadAxisSample, GamepadButton, GamepadButtonKey,
        InputControl, InputEdge, InputEvent, InputSource, MonotonicMillis, RuntimeState,
    };
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use std::time::Instant;
    use tempfile::tempdir;

    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    #[test]
    fn build_environment_is_compiled_into_the_application() {
        assert!(matches!(
            BUILD_ENVIRONMENT,
            BuildEnvironment::Development | BuildEnvironment::Production
        ));
    }

    #[test]
    fn application_compiles_committed_shortcuts_for_platform_adapters() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let shortcuts = bongocat_ui::SettingsShortcuts {
            commands: vec![bongocat_ui::SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "ctrl+shift+b".to_owned(),
            }],
            model_behaviors: vec![bongocat_ui::SettingsModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: "expression:happy".to_owned(),
                shortcut: "alt+m".to_owned(),
            }],
        };
        application
            .set_shortcuts(shortcuts)
            .expect("persist shortcuts");
        let compiled = application.shortcut_table().load();
        let modifiers = bongocat_config::ShortcutModifiers::from_bits(
            bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::SHIFT,
        )
        .expect("valid modifiers");
        assert!(compiled.resolve(modifiers, "B").is_some());
        assert!(compiled.resolve(modifiers, "C").is_none());
        let alt =
            bongocat_config::ShortcutModifiers::from_bits(bongocat_config::ShortcutModifiers::ALT)
                .expect("valid modifiers");
        assert!(compiled.resolve(alt, "M").is_some());

        application
            .set_behavior_shortcuts_enabled(false)
            .expect("disable behavior shortcuts");
        let disabled = application.shortcut_table().load();
        assert!(disabled.resolve(modifiers, "B").is_some());
        assert!(disabled.resolve(alt, "M").is_none());
        assert!(!application.config().model.enable_behavior_shortcuts);
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(
            restarted
                .shortcut_table()
                .load()
                .resolve(alt, "M")
                .is_none()
        );
        restarted
            .set_behavior_shortcuts_enabled(true)
            .expect("re-enable behavior shortcuts");
        let reenabled = restarted.shortcut_table().load();
        assert!(reenabled.resolve(modifiers, "B").is_some());
        assert!(reenabled.resolve(alt, "M").is_some());
        restarted.shutdown().expect("clean restarted shutdown");
    }

    #[test]
    fn application_loads_config_updates_runtime_and_stops() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        assert!(application.config().overlay.visible);
        assert_eq!(application.config().appearance.theme, ConfigTheme::System);
        assert!(!application.config().overlay.click_through);
        assert!(
            !application
                .runtime_client()
                .snapshot()
                .overlay_settings
                .click_through
        );

        let snapshot = application
            .set_overlay_visible(false)
            .expect("update overlay visibility");
        assert!(!snapshot.overlay_visible);
        assert!(!application.config().overlay.visible);

        let overlay_settings = OverlaySettings {
            click_through: true,
            always_on_top: false,
            scale_percent: 150,
            opacity_percent: 75,
            keep_inside_work_area: false,
        };
        let settings_snapshot = application
            .set_overlay_settings(overlay_settings)
            .expect("update overlay settings");
        assert_eq!(settings_snapshot.overlay_settings, overlay_settings);
        assert_eq!(
            application.config().overlay.scale_percent,
            overlay_settings.scale_percent
        );
        assert!(application.config().overlay.click_through);
        assert!(!application.config().overlay.keep_inside_work_area);

        application
            .set_appearance_theme(ConfigTheme::Dark)
            .expect("update appearance theme");
        assert_eq!(application.config().appearance.theme, ConfigTheme::Dark);

        let audio_snapshot = application
            .set_motion_audio_enabled(false)
            .expect("disable motion audio");
        assert!(!audio_snapshot.motion_audio_enabled);
        assert!(!application.config().model.play_motion_audio);

        let frame_rate_snapshot = application
            .set_maximum_fps(120)
            .expect("update maximum FPS");
        assert_eq!(frame_rate_snapshot.maximum_fps, 120);
        assert_eq!(application.config().model.maximum_fps, 120);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
        assert!(persisted.contains("\"play_motion_audio\": false"));
        assert!(persisted.contains("\"scale_percent\": 150"));
        assert!(persisted.contains("\"click_through\": true"));
        assert!(persisted.contains("\"maximum_fps\": 120"));
        assert!(persisted.contains("\"theme\": \"dark\""));
        let stopped = application.shutdown().expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);

        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(restarted.config().overlay.click_through);
        assert!(
            restarted
                .runtime_client()
                .snapshot()
                .overlay_settings
                .click_through
        );
        assert_eq!(restarted.runtime_client().snapshot().maximum_fps, 120);
        assert_eq!(restarted.config().appearance.theme, ConfigTheme::Dark);
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[test]
    fn system_language_is_resolved_at_start_without_overwriting_the_preference() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let application = Application::start_with_layout_internal(
            layout.clone(),
            repository_preset_root().as_path(),
            false,
            Language::ChineseSimplified,
        )
        .expect("start with simplified Chinese system language");
        assert_eq!(application.config().appearance.language, Language::System);
        assert_eq!(
            application.effective_language(),
            Language::ChineseSimplified
        );
        application.shutdown().expect("first shutdown");

        let restarted = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            false,
            Language::EnglishUnitedStates,
        )
        .expect("restart with English system language");
        assert_eq!(restarted.config().appearance.language, Language::System);
        assert_eq!(
            restarted.effective_language(),
            Language::EnglishUnitedStates
        );
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn application_projects_model_interaction_settings_at_startup() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.model.mirror = true;
        config.model.mirror_pointer_tracking = true;
        config.model.ignore_pointer = true;
        store.commit(&config).expect("persist model settings");
        drop(store);

        let application = Application::start_with_layout(layout).expect("start application");
        assert_eq!(
            application.runtime_client().snapshot().model_settings,
            ModelSettings {
                mirror: true,
                mirror_pointer_tracking: true,
                ignore_pointer: true,
            }
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn configured_gamepad_dead_zones_apply_at_start_and_persist_updates() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.input.gamepad_stick_dead_zone = 0.4;
        config.input.gamepad_trigger_dead_zone = 0.2;
        store.commit(&config).expect("custom input config");
        drop(store);

        let mut application = Application::start_with_layout(layout.clone()).expect("start app");
        let axis = application.gamepad_axis_producer();
        let connection = axis.connect(0).expect("gamepad connection");
        let input = application.input_producer();
        input
            .publish(InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            })
            .expect("connection event");
        for (axis_kind, value) in [
            (GamepadAxis::LeftStickX, 0.3),
            (GamepadAxis::LeftTrigger, 0.1),
        ] {
            axis.publish(GamepadAxisSample {
                key: GamepadAxisKey {
                    connection,
                    axis: axis_kind,
                },
                value,
                at: MonotonicMillis::new(1),
            })
            .expect("axis sample");
        }
        let edge = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey {
                    connection,
                    button: GamepadButton::South,
                }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(2),
            })
            .expect("button edge");
        let initial = application
            .runtime_client()
            .wait_for_input_sequence(edge, RUNTIME_TIMEOUT)
            .expect("initial axis projection");
        assert_eq!(initial.model_input.stick_left_x, 0.0);
        assert_eq!(initial.model_input.left_trigger, 0.0);

        let updated = application
            .set_gamepad_axis_settings(GamepadAxisSettings::new(0.1, 0.05).expect("valid settings"))
            .expect("update dead zones");
        assert!((updated.model_input.stick_left_x - (0.2 / 0.9)).abs() < 0.0001);
        assert!((updated.model_input.left_trigger - (0.05 / 0.95)).abs() < 0.0001);
        assert_eq!(application.config().input.gamepad_stick_dead_zone, 0.1);
        assert_eq!(application.config().input.gamepad_trigger_dead_zone, 0.05);
        application.shutdown().expect("clean shutdown");

        let restarted = Application::start_with_layout(layout).expect("restart app");
        assert_eq!(restarted.config().input.gamepad_stick_dead_zone, 0.1);
        assert_eq!(restarted.config().input.gamepad_trigger_dead_zone, 0.05);
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[test]
    fn application_starts_from_validated_config_backup_after_corruption() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.visible = false;
        store.commit(&config).expect("hidden config commit");
        config.overlay.visible = true;
        store.commit(&config).expect("visible config commit");
        std::fs::write(&layout.config, b"corrupt-current").expect("corrupt current config");

        let application = Application::start_with_layout(layout.clone()).expect("recover startup");
        assert!(!application.config().overlay.visible);
        assert!(!application.runtime_client().snapshot().overlay_visible);
        let recovery = application.config_recovery().expect("recovery diagnostic");
        assert_eq!(
            recovery.source_schema_version(),
            bongocat_config::SCHEMA_VERSION
        );
        assert_eq!(recovery.skipped_newer_backups(), 0);
        assert!(
            std::fs::read_dir(&layout.backups)
                .expect("backup directory")
                .any(|entry| {
                    entry
                        .expect("backup entry")
                        .file_name()
                        .to_string_lossy()
                        .starts_with("config-corrupt-")
                })
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_enters_restricted_recovery_until_defaults_are_explicitly_restored() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let invalid = b"invalid-current-without-backups";
        std::fs::create_dir_all(
            layout
                .config
                .parent()
                .expect("configuration parent directory"),
        )
        .expect("configuration directory");
        std::fs::write(&layout.config, invalid).expect("invalid current config");

        let mut application = Application::start_with_layout_internal(
            layout.clone(),
            repository_preset_root().as_path(),
            true,
            Language::EnglishUnitedStates,
        )
        .expect("restricted recovery application");
        assert_eq!(
            application.config_status(),
            ApplicationConfigStatus::RecoveryRequired { checked_backups: 0 }
        );
        assert!(!application.is_operational());
        assert!(!application.runtime_client().snapshot().overlay_visible);
        assert!(matches!(
            application.take_render_consumer(),
            Err(ApplicationError::RenderConsumerUnavailable)
        ));
        assert!(matches!(
            application.set_overlay_visible(true),
            Err(ApplicationError::ConfigurationRecoveryRequired)
        ));
        assert_eq!(
            std::fs::read(&layout.config).expect("preserved invalid"),
            invalid
        );

        application
            .restore_default_configuration()
            .expect("restore defaults");
        assert_eq!(
            application.config_status(),
            ApplicationConfigStatus::DefaultsRestoredRestartRequired
        );
        assert!(!application.is_operational());
        application.shutdown().expect("recovery shutdown");

        let restarted = Application::start_with_layout(layout).expect("restart after recovery");
        assert_eq!(restarted.config_status(), ApplicationConfigStatus::Ready);
        assert!(restarted.is_operational());
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn application_starts_from_interrupted_config_without_exposing_storage_details() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut current = store.load_or_default().expect("default config").config;
        let interrupted_bytes = std::fs::read(&layout.config).expect("visible config bytes");
        current.overlay.visible = false;
        store.commit(&current).expect("hidden current config");
        std::fs::write(layout.config.with_extension("json.tmp"), interrupted_bytes)
            .expect("interrupted config temp");

        let application = Application::start_with_layout(layout).expect("recover startup");
        assert!(!application.config().overlay.visible);
        assert_eq!(
            application.interrupted_config_recovery(),
            Some(InterruptedConfigRecovery::ArchivedStaleTemp)
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn development_and_production_applications_never_share_roots() {
        let base = tempdir().expect("temp directory");
        let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let development_root = development.root.clone();
        let production_root = production.root.clone();

        let mut development_app =
            Application::start_with_layout(development).expect("development application");
        let mut production_app =
            Application::start_with_layout(production).expect("production application");

        assert!(development_root.join("config.json").is_file());
        assert!(production_root.join("config.json").is_file());
        assert_ne!(development_root, production_root);

        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        development_app
            .import_model("same-id", &source)
            .expect("development import");
        production_app
            .import_model("same-id", source)
            .expect("production import");
        assert_eq!(
            installed_catalog_ids(&development_app),
            vec!["same-id".to_owned()]
        );
        assert_eq!(
            installed_catalog_ids(&production_app),
            vec!["same-id".to_owned()]
        );
        assert!(development_root.join("models/same-id").is_dir());
        assert!(production_root.join("models/same-id").is_dir());
        development_app.shutdown().expect("development shutdown");
        production_app.shutdown().expect("production shutdown");
    }

    #[test]
    fn failed_installed_model_preparation_preserves_the_active_model() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let mut application = Application::start_with_layout(layout).expect("start application");
        let fixtures = repository_root().join("shared/fixtures/model-fixtures/cases");

        application
            .import_model("active", fixtures.join("非 ASCII 模型"))
            .expect("import active model");
        application
            .import_model("broken", fixtures.join("非 ASCII 模型"))
            .expect("import model to corrupt");

        let active = application
            .select_model(ModelOrigin::Installed, "active")
            .expect("activate valid model");
        let active_revision = active.revision;
        assert_eq!(
            active
                .active_model
                .as_ref()
                .expect("active model")
                .id
                .as_str(),
            "active"
        );

        std::fs::remove_file(models_root.join("broken/模型 数据.moc3"))
            .expect("corrupt installed model");

        let error = application
            .select_model(ModelOrigin::Installed, "broken")
            .expect_err("invalid model must be rejected");
        assert!(matches!(error, ApplicationError::ModelStore(_)));
        let preserved = application.runtime_client().snapshot();
        assert_eq!(preserved.revision, active_revision);
        assert_eq!(preserved.active_model, active.active_model);
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_imports_into_its_environment_model_store() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

        let imported = application
            .import_model("unicode", source)
            .expect("import model");
        assert_eq!(
            imported.root(),
            models_root
                .canonicalize()
                .expect("canonical models root")
                .join("unicode")
        );
        assert!(imported.root().join("猫.model3.json").is_file());

        let catalog = application.model_catalog().expect("model catalog");
        assert!(catalog.iter().any(|entry| {
            entry.origin() == bongocat_model::ModelOrigin::Installed
                && entry.id().as_str() == "unicode"
        }));

        application
            .delete_model(ModelOrigin::Installed, "unicode")
            .expect("delete model");
        assert!(installed_catalog_ids(&application).is_empty());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn merged_model_catalog_retains_source_identity_for_duplicate_ids() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("install duplicate id");

        let catalog = application.model_catalog().expect("merged catalog");
        let duplicate = catalog
            .iter()
            .filter(|entry| entry.id().as_str() == "standard")
            .map(ModelCatalogEntry::origin)
            .collect::<Vec<_>>();
        assert_eq!(
            duplicate,
            [
                bongocat_model::ModelOrigin::Preset,
                bongocat_model::ModelOrigin::Installed,
            ]
        );
        assert!(catalog.windows(2).all(|entries| {
            let left = (
                entries[0].id().as_str(),
                model_origin_order(entries[0].origin()),
            );
            let right = (
                entries[1].id().as_str(),
                model_origin_order(entries[1].origin()),
            );
            left <= right
        }));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_selection_persists_its_origin_across_restart() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("install duplicate id");
        let selected = application
            .select_model(ModelOrigin::Installed, "standard")
            .expect("select installed duplicate");
        assert_eq!(
            selected
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert_eq!(
            application.active_model_origin(),
            Some(ModelOrigin::Installed)
        );
        assert_eq!(
            application.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Installed)
        );
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert_eq!(
            restarted.config().model.selected_model_id.as_deref(),
            Some("standard")
        );
        assert_eq!(
            restarted.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Installed)
        );
        restarted
            .select_model(ModelOrigin::Installed, "standard")
            .expect("reload installed duplicate");
        assert_eq!(
            restarted.active_model_origin(),
            Some(ModelOrigin::Installed)
        );
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[test]
    fn selected_installed_model_must_be_replaced_before_deletion() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("active", source)
            .expect("import model");
        application
            .select_model(ModelOrigin::Installed, "active")
            .expect("activate model");

        let error = application
            .delete_model(ModelOrigin::Installed, "active")
            .expect_err("selected model deletion must fail");
        assert!(matches!(error, ApplicationError::SelectedModelDeletion(_)));
        assert_eq!(
            installed_catalog_ids(&application),
            vec!["active".to_owned()]
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_can_be_deleted_while_same_id_preset_is_selected() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("import duplicate");
        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select preset");

        application
            .delete_model(ModelOrigin::Installed, "standard")
            .expect("delete installed duplicate");
        assert!(installed_catalog_ids(&application).is_empty());

        let preset_error = application
            .delete_model(ModelOrigin::Preset, "standard")
            .expect_err("preset deletion must fail");
        assert!(matches!(
            preset_error,
            ApplicationError::PresetModelDeletion(_)
        ));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn configured_installed_model_cannot_be_deleted_before_restart_activation() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        application
            .import_model("selected", source)
            .expect("import model");
        application
            .select_model(ModelOrigin::Installed, "selected")
            .expect("select installed model");
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(restarted.runtime_client().snapshot().active_model.is_none());
        let error = restarted
            .delete_model(ModelOrigin::Installed, "selected")
            .expect_err("configured model deletion must fail");
        assert!(matches!(error, ApplicationError::SelectedModelDeletion(_)));
        assert_eq!(
            installed_catalog_ids(&restarted),
            vec!["selected".to_owned()]
        );
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn rejected_gpu_model_switch_restores_the_previous_config_selection() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let mut application = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            true,
            Language::EnglishUnitedStates,
        )
        .expect("start rendering application");
        let initial_token = application
            .prepare_model(ModelOrigin::Preset, "standard")
            .expect("prepare initial model");
        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        let initial_frame = wait_for_model_commit_frame(&consumer, initial_token);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: initial_frame.model_commit.expect("initial commit token"),
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("commit initial model");
        application
            .runtime_client()
            .wait_for_command(initial_token.command_sequence, RUNTIME_TIMEOUT)
            .expect("initial model activation");

        let switch = std::thread::spawn(move || {
            let rejected = matches!(
                application.select_model(ModelOrigin::Preset, "keyboard"),
                Err(ApplicationError::RuntimeCommandFailed(_))
            );
            (application, rejected)
        });
        let candidate = wait_for_any_model_commit_frame(&consumer);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: candidate.model_commit.expect("candidate commit token"),
                outcome: ModelCommitOutcome::Rejected(
                    ModelCommitErrorCode::ResourcePreparationFailed,
                ),
            })
            .expect("reject candidate model");
        let (application, rejected) = switch.join().expect("selection worker");
        assert!(rejected);
        assert_eq!(
            application
                .runtime_client()
                .snapshot()
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert_eq!(application.config().model.selected_model_id, None);
        assert_eq!(application.config().model.selected_model_origin, None);
        let persisted = std::fs::read_to_string(config_path).expect("restored config");
        assert!(persisted.contains("\"selected_model_id\": null"));
        assert!(persisted.contains("\"selected_model_origin\": null"));
        application.shutdown().expect("clean shutdown");
    }

    fn installed_catalog_ids(application: &Application) -> Vec<String> {
        application
            .model_catalog()
            .expect("model catalog")
            .into_iter()
            .filter(|entry| entry.origin() == bongocat_model::ModelOrigin::Installed)
            .map(|entry| entry.id().as_str().to_owned())
            .collect()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_model_commit_frame(
        consumer: &RenderConsumer,
        token: ModelCommitToken,
    ) -> bongocat_render::RenderFrame {
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        loop {
            if let Some(frame) = consumer.take_latest()
                && frame.model_commit == Some(token)
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "model frame timed out");
            std::thread::yield_now();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_any_model_commit_frame(consumer: &RenderConsumer) -> bongocat_render::RenderFrame {
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        loop {
            if let Some(frame) = consumer.take_latest()
                && frame.model_commit.is_some()
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "model frame timed out");
            std::thread::yield_now();
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn application_owns_the_rendering_runtime_and_issues_one_consumer() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            true,
            Language::EnglishUnitedStates,
        )
        .expect("start rendering application");
        let token = application
            .prepare_model(ModelOrigin::Preset, "standard")
            .expect("prepare preset model");
        assert_eq!(token.model_generation, 0);
        assert!(
            application
                .runtime_client()
                .snapshot()
                .active_model
                .is_none()
        );

        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        assert!(matches!(
            application.take_render_consumer(),
            Err(ApplicationError::RenderConsumerUnavailable)
        ));
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        let frame = loop {
            if let Some(frame) = consumer.take_latest() {
                break frame;
            }
            assert!(
                Instant::now() < deadline,
                "runtime did not publish a render frame"
            );
            std::thread::yield_now();
        };
        assert_eq!(frame.model_generation, 0);
        assert!(!frame.snapshot.drawables.is_empty());
        assert_eq!(frame.model_commit, Some(token));
        consumer
            .report_model_commit(bongocat_render::ModelCommitFeedback {
                token,
                outcome: bongocat_render::ModelCommitOutcome::Prepared,
            })
            .expect("report prepared GPU model");
        let activated = application
            .runtime_client()
            .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
            .expect("commit preset model");
        assert_eq!(
            activated
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert!(activated.pending_model.is_none());

        let stopped = application.shutdown().expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }
}
