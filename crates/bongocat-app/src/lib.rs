#![forbid(unsafe_code)]

#[cfg(all(feature = "production", feature = "storage-test-injection"))]
compile_error!("storage-test-injection cannot be enabled for Production builds");

use bongocat_audio::{MotionAudioService, MotionAudioShutdownError};
use bongocat_config::{
    BuildEnvironment, CompiledShortcuts, ConfigError, ConfigRevision, ConfigStore, Language,
    ModelBehaviorAction, ModelBehaviorBinding, ModelMetadata, NativeConfig, OverlayWindowPlacement,
    PlatformStorageError, SelectedModelOrigin, ShortcutBinding, ShortcutConfig, ShortcutModifiers,
    ShortcutTable, StorageLayout, Theme as ConfigTheme, WindowPlacement, WindowState,
    WindowStateError, WindowStateStore, platform_layout,
};
use bongocat_input::{
    CursorProducer, GamepadAxisProducer, GamepadAxisSettings, GamepadButton, HandSide,
    InputBindings, InputProducer, PhysicalKey,
};
use bongocat_live2d_render::KeyImageInventory;
use bongocat_model::{
    CommittedModel, InstalledModel, ModelBehaviorSnapshot, ModelCatalogEntry, ModelError, ModelId,
    ModelOrigin, ModelPackageLimits, PresetModelCatalog,
};
use bongocat_model_store::{
    ModelImportProgress, ModelImportStage, ModelSourceContent, ModelStore, ModelStoreError,
    MverInputMode, PresetCoverStore, preset_cover_exists,
};
use bongocat_render::{FUNCTION_KEY_USAGES, KeySide, ModelCommitToken, RenderConsumer};
use bongocat_runtime::{
    ExpressionId, ExpressionIdError, ModelSettings, MotionId, MotionIdError, MotionPriority,
    OverlaySettings, RuntimeClient, RuntimeCommand, RuntimeCommandFailure, RuntimeOwner,
    RuntimeRenderErrorCode, RuntimeSnapshot, SendError, ShutdownError, maximum_fps_is_valid,
    release_fallback_timeout_is_valid,
};
use bongocat_update::{UpdateDiagnostics, UpdateDiagnosticsTracker};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

mod app_log;
mod diagnostics_bundle;
#[cfg(test)]
mod product_icon_contract;
mod settings;
mod shortcuts;
mod startup_permission;
mod update;
use app_log::ApplicationRunMarker;
pub use app_log::{
    ApplicationLogCode, ApplicationLogComponent, ApplicationLogDiagnostics, ApplicationLogError,
    ApplicationLogEvent, ApplicationLogEventCounts, ApplicationLogHandle, ApplicationLogLevel,
    ApplicationPanicHook, CoreLogDiagnostics,
};
pub use settings::{
    ApplicationSettingsService, SettingsServiceJoinError, StatusIconCapability,
    TaskbarIconCapability,
};
pub use shortcuts::application_shortcut_dispatcher;
pub use startup_permission::ensure_startup_permission;
pub use update::{ApplicationUpdateService, UpdateServiceError, restart_required_after_install};

/// Work the settings worker hands to the thread that owns the product's windows.
///
/// Two things the application has to do can only happen on that thread: showing the
/// settings window a global shortcut asked for, and rendering a just-imported model
/// into its own cover. The worker cannot do either — the shortcut service is a
/// platform thread of its own, and a cover capture creates a native window — so it
/// raises a signal and the GPUI thread drains it. Both fields are shared handles, so
/// every clone observes the same work.
#[derive(Clone, Default)]
pub struct ApplicationMainThreadSignals {
    open_settings: Arc<AtomicBool>,
    cover_captures: Arc<Mutex<VecDeque<CoverCaptureRequest>>>,
}

impl ApplicationMainThreadSignals {
    pub fn request_open_settings(&self) {
        self.open_settings.store(true, Ordering::Release);
    }

    pub fn take_open_settings_request(&self) -> bool {
        self.open_settings.swap(false, Ordering::AcqRel)
    }

    /// Queue `model` to be rendered into its own cover by the GPUI thread.
    ///
    /// Nothing is written here: the caller has only just installed the model, and
    /// the cover it ships is left in place until a capture replaces it.
    pub fn request_model_cover_capture(
        &self,
        key: bongocat_ui_protocol::SettingsModelKey,
        model: CommittedModel,
    ) {
        self.cover_captures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(CoverCaptureRequest {
                key,
                model: Arc::new(model),
            });
    }

    /// Take the captures queued since the last call, oldest first.
    pub fn take_model_cover_captures(&self) -> Vec<CoverCaptureRequest> {
        self.cover_captures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }
}

/// One installed model whose cover has to be rendered from the model itself.
///
/// The model travels with the request because loading it is the settings worker's
/// work: the GPUI thread gets something it can render, not an id it would have to
/// re-open the store for.
pub struct CoverCaptureRequest {
    key: bongocat_ui_protocol::SettingsModelKey,
    model: Arc<CommittedModel>,
}

impl CoverCaptureRequest {
    /// The model as the settings protocol names it, for writing the cover back.
    pub fn key(&self) -> &bongocat_ui_protocol::SettingsModelKey {
        &self.key
    }

    pub fn model(&self) -> &Arc<CommittedModel> {
        &self.model
    }
}

const COMMAND_CAPACITY: usize = 64;
const AUDIO_COMMAND_CAPACITY: usize = 16;
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// PNG file signature, checked before a user-chosen image replaces a model's
/// cover. Only the signature is verified: the cover is display artwork for the
/// settings catalog, so a PNG that no decoder can read is a wrong picture, not
/// a broken model.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[cfg(feature = "production")]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Production;

#[cfg(not(feature = "production"))]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Development;

/// Whether this build can actually check for and install updates.
///
/// Updates require a Production channel and a provisioned release signing key;
/// a Development build never installs a release artifact. This is the
/// availability fact only — the system menu additionally keeps the entry
/// clickable in Development builds, because the update window is where the
/// "development build" explanation lives.
pub fn update_check_available() -> bool {
    bongocat_update::UpdateRuntime::for_current_build(
        BUILD_ENVIRONMENT,
        PRODUCT_VERSION,
        bongocat_update::UpdateDiagnosticsTracker::default(),
    )
    .is_available()
}
#[derive(Debug)]
pub enum ApplicationError {
    PlatformStorage(PlatformStorageError),
    Config(ConfigError),
    Model(ModelError),
    ModelStore(ModelStoreError),
    MotionId(MotionIdError),
    ExpressionId(ExpressionIdError),
    PresetModelDeletion(ModelId),
    /// The model a request names is not in its catalog or store.
    ///
    /// Both origins report this the same way: a preset a build no longer ships
    /// and an installed package the user removed by hand are the same fact to
    /// the request that named either one.
    ModelNotFound(ModelId),
    ModelTitleInvalid,
    ModelCoverInvalid,
    RuntimeCommand(SendError),
    RuntimeCommandFailed(RuntimeCommandFailure),
    RuntimeDidNotPublish,
    RuntimeDidNotPrepareModel,
    RenderConsumerUnavailable,
    Shutdown(ShutdownError),
    MotionAudioShutdown(MotionAudioShutdownError),
    ShutdownAggregate(ApplicationShutdownError),
    ApplicationLog(ApplicationLogError),
    ConfigRollback(ConfigError),
    WindowState(WindowStateError),
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
            Self::ModelNotFound(id) => {
                write!(formatter, "model was not found: {}", id.as_str())
            }
            Self::ModelTitleInvalid => formatter.write_str("model title is not usable"),
            Self::ModelCoverInvalid => {
                formatter.write_str("model cover must be a PNG image within the size limit")
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
            Self::ShutdownAggregate(error) => write!(formatter, "shutdown failed: {error}"),
            Self::ApplicationLog(error) => write!(formatter, "application logging failed: {error}"),
            Self::ConfigRollback(error) => {
                write!(formatter, "model selection config rollback failed: {error}")
            }
            Self::WindowState(error) => write!(formatter, "window state failed: {error}"),
        }
    }
}

impl std::error::Error for ApplicationError {}

#[derive(Debug, Eq, PartialEq)]
pub struct ApplicationShutdownError {
    pub runtime: ShutdownError,
    pub motion_audio: MotionAudioShutdownError,
}

impl fmt::Display for ApplicationShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "runtime: {}; motion audio: {}",
            self.runtime, self.motion_audio
        )
    }
}

impl std::error::Error for ApplicationShutdownError {}

fn combine_shutdown_results<T>(
    runtime_result: Result<T, ShutdownError>,
    audio_result: Result<(), MotionAudioShutdownError>,
) -> Result<T, ApplicationError> {
    match (runtime_result, audio_result) {
        (Ok(stopped), Ok(_)) => Ok(stopped),
        (Err(runtime), Ok(_)) => Err(ApplicationError::Shutdown(runtime)),
        (Ok(_), Err(motion_audio)) => Err(ApplicationError::MotionAudioShutdown(motion_audio)),
        (Err(runtime), Err(motion_audio)) => Err(ApplicationError::ShutdownAggregate(
            ApplicationShutdownError {
                runtime,
                motion_audio,
            },
        )),
    }
}

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

impl From<WindowStateError> for ApplicationError {
    fn from(error: WindowStateError) -> Self {
        Self::WindowState(error)
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

pub struct Application {
    config_store: ConfigStore,
    window_state_store: WindowStateStore,
    window_state: WindowState,
    config: NativeConfig,
    config_revision: Option<ConfigRevision>,
    system_language: Language,
    preset_models: PresetModelCatalog,
    model_store: ModelStore,
    /// The replacement covers of the presets the user customised. A preset's
    /// package is inside the application bundle, so this is where the one part
    /// of a preset that is meant to be edited lives.
    preset_covers: PresetCoverStore,
    active_model_origin: Option<ModelOrigin>,
    /// The model the runtime was actually handed, which is not always the
    /// configured selection: a fresh configuration has no selection at all and
    /// startup still activates the standard preset.
    active_model_id: Option<ModelId>,
    runtime: RuntimeOwner,
    motion_audio: Option<MotionAudioService>,
    render_consumer: Option<RenderConsumer>,
    application_log: ApplicationLogHandle,
    core_log_diagnostics: Option<Arc<dyn Fn() -> CoreLogDiagnostics + Send + Sync>>,
    update_diagnostics: Option<Arc<dyn Fn() -> UpdateDiagnostics + Send + Sync>>,
    run_marker: ApplicationRunMarker,
    panic_hook: Option<ApplicationPanicHook>,
    shortcut_table: ShortcutTable,
    shortcut_capture_suspended: bool,
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
        let preset_covers = PresetCoverStore::open(layout.model_overrides.clone())?;
        let config_store = ConfigStore::new(layout.clone())?;
        let application_log = ApplicationLogHandle::install(&layout.logs)?;
        let (run_marker, previous_run) = application_log.begin_run()?;
        let window_state_store = WindowStateStore::new(layout);
        let window_state = window_state_store.load_or_default().state;
        let loaded = config_store.load_or_default()?;
        let mut config = loaded.config;
        let mut config_revision = Some(loaded.revision);
        if !config.overlay.visible {
            // The model window always starts visible: hiding it is a
            // per-session choice, so a persisted hidden overlay is normalized
            // back to visible instead of surviving a restart. The commit is
            // best effort like other startup corrections — a storage failure
            // still leaves this session visible.
            config.overlay.visible = true;
            if let Ok(revision) = config_store.commit(&config) {
                config_revision = Some(revision);
            }
        }
        let shortcut_table = ShortcutTable::new(active_shortcuts(
            &config,
            config.model.selected_model_id.as_deref(),
        )?);
        let (motion_audio, motion_audio_client) =
            match MotionAudioService::start(AUDIO_COMMAND_CAPACITY) {
                Ok(service) => {
                    let client = service.client();
                    (Some(service), client)
                }
                Err(_) => (None, bongocat_audio::MotionAudioClient::unavailable()),
            };
        let runtime_overlay_visible = config.overlay.visible;
        let runtime_motion_audio_enabled = config.model.play_motion_audio;
        let (runtime, render_consumer) = if enable_rendering {
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
        let active_model_origin = config
            .model
            .selected_model_origin
            .map(model_origin_from_config);
        let active_model_id = config
            .model
            .selected_model_id
            .as_deref()
            .and_then(|id| ModelId::parse(id).ok());
        let mut application = Self {
            config_store,
            window_state_store,
            window_state,
            config,
            config_revision,
            system_language,
            preset_models,
            model_store,
            preset_covers,
            active_model_origin,
            active_model_id,
            runtime,
            motion_audio,
            render_consumer,
            application_log,
            core_log_diagnostics: None,
            update_diagnostics: None,
            run_marker,
            panic_hook: None,
            shortcut_table,
            shortcut_capture_suspended: false,
        };
        if previous_run.is_some() {
            application
                .application_log
                .record(ApplicationLogEvent::previous_run_unclean());
        }
        application
            .application_log
            .record(ApplicationLogEvent::started());
        application.prune_missing_installed_metadata();
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
        active_shortcuts(&self.config, self.live_model_id()).map_err(ApplicationError::Config)
    }

    /// The model whose behavior bindings are live: the one the runtime is
    /// showing.
    ///
    /// This is tracked on the application instead of being read from
    /// `config.model.selected_model_id`, because a fresh configuration has no
    /// recorded selection while `restore_startup_model` still activates the
    /// standard preset — and a selection whose resources were deleted by hand
    /// stays recorded until the fallback commit lands.
    fn live_model_id(&self) -> Option<&str> {
        self.active_model_id.as_ref().map(ModelId::as_str)
    }

    /// Rebuild the platform-facing shortcut table from the committed
    /// configuration, scoped to the model that is actually live.
    ///
    /// Best effort, like the assignments that feed it: a configuration that no
    /// longer compiles leaves the previous table in place and the platform
    /// keeps what it already registered.
    fn refresh_shortcut_table(&mut self) {
        let compiled = {
            let active_model = self.active_model_id.as_ref().map(ModelId::as_str);
            active_shortcuts(&self.config, active_model)
        };
        if let Ok(compiled) = compiled {
            self.shortcut_table.replace(compiled);
        }
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

    pub fn set_core_log_diagnostics_provider(
        &mut self,
        provider: impl Fn() -> CoreLogDiagnostics + Send + Sync + 'static,
    ) {
        self.core_log_diagnostics = Some(Arc::new(provider));
    }

    pub fn core_log_diagnostics(&self) -> Option<CoreLogDiagnostics> {
        self.core_log_diagnostics
            .as_ref()
            .map(|provider| provider())
    }

    pub fn set_update_diagnostics_provider(
        &mut self,
        provider: impl Fn() -> UpdateDiagnostics + Send + Sync + 'static,
    ) {
        self.update_diagnostics = Some(Arc::new(provider));
    }

    /// Register an app-owned tracker that update workers can share safely.
    pub fn set_update_diagnostics_tracker(&mut self, tracker: UpdateDiagnosticsTracker) {
        self.set_update_diagnostics_provider(move || tracker.snapshot());
    }

    pub fn update_diagnostics(&self) -> Option<UpdateDiagnostics> {
        self.update_diagnostics
            .as_ref()
            .map(|provider| provider().sanitized())
    }

    pub fn record_log(&self, event: ApplicationLogEvent) {
        self.application_log.record(event);
    }

    pub fn install_process_panic_hook(&mut self) {
        if self.panic_hook.is_none() {
            self.panic_hook = Some(self.application_log.install_panic_hook());
        }
    }

    pub const fn settings_window_placement(&self) -> Option<WindowPlacement> {
        self.window_state.settings_window
    }

    pub const fn overlay_window_placement(&self) -> Option<OverlayWindowPlacement> {
        self.window_state.overlay_window
    }

    pub fn persist_settings_window_placement(
        &mut self,
        placement: Option<WindowPlacement>,
    ) -> Result<(), ApplicationError> {
        if self.window_state.settings_window == placement {
            return Ok(());
        }
        let window_state = WindowState::with_windows(placement, self.window_state.overlay_window);
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub fn persist_overlay_window_placement(
        &mut self,
        placement: OverlayWindowPlacement,
    ) -> Result<(), ApplicationError> {
        if self.window_state.overlay_window == Some(placement) {
            return Ok(());
        }
        let window_state =
            WindowState::with_windows(self.window_state.settings_window, Some(placement));
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub(crate) fn config_backup_directory(&self) -> &Path {
        &self.config_store.layout().backups
    }

    fn ready_config_revision(&self) -> Result<ConfigRevision, ApplicationError> {
        self.config_revision
            .ok_or(ApplicationError::RuntimeDidNotPublish)
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
        next_config.overlay.corner_radius_percent = settings.corner_radius_percent;
        next_config.overlay.hide_on_pointer_hover = settings.hide_on_pointer_hover;
        next_config.overlay.hide_on_pointer_hover_delay_seconds =
            settings.hide_on_pointer_hover_delay_seconds;
        next_config.overlay.keep_inside_screen = settings.keep_inside_screen;
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
        shortcuts: bongocat_ui_protocol::SettingsShortcuts,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        let commands_enabled = next_config.shortcuts.commands_enabled;
        next_config.shortcuts = shortcut_config_from_settings(shortcuts, commands_enabled);
        next_config.shortcuts = next_config.shortcuts.canonicalized()?;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_id())?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_capture_suspended = false;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    /// Temporarily removes the binding being recorded from the platform-facing
    /// table without changing the persisted configuration.
    pub fn suspend_shortcut_capture(
        &mut self,
        shortcuts_without_capture_target: bongocat_ui_protocol::SettingsShortcuts,
    ) -> Result<(), ApplicationError> {
        let mut temporary = self.config.clone();
        let commands_enabled = temporary.shortcuts.commands_enabled;
        temporary.shortcuts =
            shortcut_config_from_settings(shortcuts_without_capture_target, commands_enabled);
        temporary.shortcuts = temporary.shortcuts.canonicalized()?;
        temporary.validate()?;
        let compiled = active_shortcuts(&temporary, self.live_model_id())?;
        self.shortcut_table.replace(compiled);
        self.shortcut_capture_suspended = true;
        Ok(())
    }

    /// Restores the platform-facing table from the current committed config
    /// after shortcut recording is abandoned.
    pub fn resume_shortcut_capture(&mut self) -> Result<(), ApplicationError> {
        if self.shortcut_capture_suspended {
            let compiled = active_shortcuts(&self.config, self.live_model_id())?;
            self.shortcut_table.replace(compiled);
            self.shortcut_capture_suspended = false;
        }
        Ok(())
    }

    pub fn set_behavior_shortcuts_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.enable_behavior_shortcuts = enabled;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_id())?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    /// Switches the application command shortcuts on or off.
    ///
    /// The recorded bindings stay in the configuration: the gate only decides
    /// whether [`ShortcutConfig::commands`] reaches the platform table, so
    /// turning it back on restores them without re-recording, exactly like the
    /// model behaviour gate next to it. The two gates are independent.
    pub fn set_command_shortcuts_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.shortcuts.commands_enabled = enabled;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_id())?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }

    /// Every model the Models page can show, in the order the page shows them:
    /// the build's presets first, then the models the user imported.
    ///
    /// The presets are the three input modes the build ships, and the page
    /// lists them in mode order — Standard, Keyboard, Gamepad. Their ids are
    /// `standard`, `keyboard` and `gamepad`, so sorting them by id would run
    /// the page backwards. See [`preset_model_order`].
    ///
    /// The imported models follow, in the order they were imported: the
    /// configuration's record list is append-only, so a newly imported model
    /// joins the end of the page and stays where it landed. See
    /// [`installed_model_order`].
    ///
    /// A model id present in both halves appears twice, once per origin, and
    /// the preset one always comes first because the whole preset half does.
    pub fn model_catalog(&self) -> Result<Vec<ModelCatalogEntry>, ApplicationError> {
        // Unrecognized store entries are filtered inside the store scan; the
        // merged catalog only exposes real models.
        let mut presets = self.preset_models.list()?;
        presets.sort_by(|left, right| {
            preset_model_order(left.id().as_str())
                .cmp(&preset_model_order(right.id().as_str()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        let records = &self.config.model.installed_models;
        let mut installed = self.model_store.list()?.entries;
        installed.sort_by(|left, right| {
            installed_model_order(records, left.id().as_str())
                .cmp(&installed_model_order(records, right.id().as_str()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        presets.extend(installed);
        Ok(presets)
    }

    pub const fn active_model_origin(&self) -> Option<ModelOrigin> {
        self.active_model_origin
    }

    /// Where a model's own files live, when the model is actually present.
    ///
    /// The settings catalog needs this twice: to offer "open model folder", and
    /// to find the cover image the package may ship. Nothing in the runtime or
    /// renderer path uses it, and a missing directory is reported as `None`
    /// rather than an error, because a catalog entry and the directory behind it
    /// are re-read independently.
    pub fn model_directory(&self, origin: ModelOrigin, id: &str) -> Option<PathBuf> {
        let Ok(id) = ModelId::parse(id) else {
            return None;
        };
        let root = match origin {
            ModelOrigin::Preset => self.preset_models.root(),
            ModelOrigin::Installed => self.model_store.root(),
        };
        let directory = root.join(id.as_str());
        directory.is_dir().then_some(directory)
    }

    /// Rename a model, whichever origin it came from.
    ///
    /// A title is user-editable metadata in the configuration, so this is the
    /// only model fact that lives outside the model directory. Both origins are
    /// renamed the same way, into their own list: a preset's package belongs to
    /// the build and is never written to, so its name is a customisation the
    /// configuration records instead of a property of the package.
    pub fn set_model_title(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let id = ModelId::parse(id)?;
        let title =
            normalize_model_title(&title.into()).ok_or(ApplicationError::ModelTitleInvalid)?;
        if self.model_directory(origin, id.as_str()).is_none() {
            return Err(ApplicationError::ModelNotFound(id));
        }
        self.record_model_title(origin, &id, title)
    }

    /// Replace a model's cover image with a user-chosen PNG.
    ///
    /// The cover is display artwork for the settings catalog, so the check here
    /// is the file contract the package layout implies — a PNG within the
    /// package's own per-file limit — and the bytes are installed verbatim,
    /// exactly as the BongoCatMver conversion installs a legacy cover.
    pub fn set_model_cover(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        source: impl AsRef<Path>,
    ) -> Result<PathBuf, ApplicationError> {
        let id = ModelId::parse(id)?;
        let source = source.as_ref();
        let metadata = fs::metadata(source).map_err(|_| ApplicationError::ModelCoverInvalid)?;
        if !metadata.is_file() || metadata.len() > ModelPackageLimits::default().maximum_file_bytes
        {
            return Err(ApplicationError::ModelCoverInvalid);
        }
        let bytes = fs::read(source).map_err(|_| ApplicationError::ModelCoverInvalid)?;
        self.install_model_cover(origin, &id, &bytes)
    }

    /// Replace a model's cover image with PNG bytes the product made.
    ///
    /// A captured cover never exists as a file until it is installed, so the cover
    /// the renderer just produced arrives here directly. It passes the same contract
    /// as a user-chosen one: a real PNG, within the package's per-file limit.
    pub fn set_model_cover_bytes(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
        bytes: &[u8],
    ) -> Result<PathBuf, ApplicationError> {
        let id = ModelId::parse(id)?;
        self.install_model_cover(origin, &id, bytes)
    }

    /// The contract every cover source shares, once its bytes are in hand.
    ///
    /// Where the bytes land is the one thing the origins do not share: an
    /// installed model's cover goes into its own package, and a preset's goes to
    /// the user side, because the package it belongs to sits inside the
    /// application bundle and may not be written to.
    fn install_model_cover(
        &self,
        origin: ModelOrigin,
        id: &ModelId,
        bytes: &[u8],
    ) -> Result<PathBuf, ApplicationError> {
        if bytes.len() as u64 > ModelPackageLimits::default().maximum_file_bytes
            || !bytes.starts_with(&PNG_SIGNATURE)
        {
            return Err(ApplicationError::ModelCoverInvalid);
        }
        match origin {
            ModelOrigin::Preset => {
                // The preset cover store creates the directory it writes into,
                // so unlike the model store it cannot report a missing model by
                // itself. The package has to be there before it may be
                // customised, exactly as for an installed model.
                if self.model_directory(origin, id.as_str()).is_none() {
                    return Err(ApplicationError::ModelNotFound(id.clone()));
                }
                self.preset_covers
                    .replace_cover(id, bytes)
                    .map_err(ApplicationError::ModelStore)
            }
            ModelOrigin::Installed => self
                .model_store
                .replace_cover(id, bytes)
                .map_err(ApplicationError::ModelStore),
        }
    }

    /// The display name recorded for a model, if the user ever changed it.
    ///
    /// `None` means the model has never been renamed, which is the ordinary
    /// state of a preset: its name is then the id the build shipped it under.
    pub fn recorded_model_title(&self, origin: ModelOrigin, id: &str) -> Option<&str> {
        self.model_metadata(origin)
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.title.as_str())
    }

    /// The cover the settings page should draw for a model, if it has one.
    ///
    /// A replacement wins over the artwork a preset's package ships: the bundle
    /// is read-only, so a replacement is the only cover that can reflect what
    /// the user chose. An installed model needs no such preference — its cover
    /// lives in the package either way.
    pub fn model_cover_path(&self, origin: ModelOrigin, id: &str) -> Option<PathBuf> {
        let directory = self.model_directory(origin, id)?;
        if origin == ModelOrigin::Preset {
            let replacement = self.preset_covers.cover_path(&ModelId::parse(id).ok()?);
            if preset_cover_exists(&replacement) {
                return Some(replacement);
            }
        }
        let cover = bongocat_model::package_cover_path(&directory);
        cover.is_file().then_some(cover)
    }

    /// Write one model's display name into the list that owns its lifecycle.
    fn record_model_title(
        &mut self,
        origin: ModelOrigin,
        id: &ModelId,
        title: String,
    ) -> Result<(), ApplicationError> {
        let mut records = self.model_metadata(origin).to_vec();
        match records.iter_mut().find(|record| record.id == id.as_str()) {
            Some(record) => record.title = title,
            // A model can legitimately exist without a record — a package copied
            // into the store by hand, an import interrupted after the directory
            // was committed, every preset — so naming it creates the record
            // instead of failing on a missing one.
            None => records.push(ModelMetadata {
                id: id.as_str().to_owned(),
                title,
            }),
        }
        self.commit_model_metadata(origin, records)
    }

    /// The metadata records that belong to one origin.
    ///
    /// The two lists are read by origin and never merged: they are keyed by
    /// separate id spaces, so the same id may name a preset and an installed
    /// model at once.
    fn model_metadata(&self, origin: ModelOrigin) -> &[ModelMetadata] {
        match origin {
            ModelOrigin::Preset => &self.config.model.preset_models,
            ModelOrigin::Installed => &self.config.model.installed_models,
        }
    }

    /// Persist one list of editable model metadata. The typed validation in
    /// `bongocat-config` rejects duplicate ids, blank titles, and over-long
    /// values before anything is written.
    fn commit_model_metadata(
        &mut self,
        origin: ModelOrigin,
        records: Vec<ModelMetadata>,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        match origin {
            ModelOrigin::Preset => next_config.model.preset_models = records,
            ModelOrigin::Installed => next_config.model.installed_models = records,
        }
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
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

    pub fn preview_motion(
        &self,
        group: impl Into<String>,
        index: usize,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::PreviewMotion(MotionId::new(group, index)?))
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
        self.persist_default_behavior_shortcuts(&committed);
        let input_bindings = input_bindings_for_committed_model(&committed);
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
        self.active_model_id = Some(id);
        // The model that just became live owns the behavior half of the
        // platform table. Rebuilding here is what both swaps the previous
        // model's chords out — they must stop working the moment the model
        // stops being shown — and registers the incoming model's own chords.
        self.refresh_shortcut_table();
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
        // Switching models is also when the new model's motions and expressions
        // receive the legacy default chords, so the Shortcuts page offers a
        // default for every behavior the user has not recorded yet. The
        // assignment rides on the same commit as the selection itself.
        assign_default_behavior_shortcuts(&mut next_config, &committed);
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let input_bindings = Arc::new(input_bindings_for_committed_model(&committed));
        let result = self.wait_for_model_command(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(committed),
            input_bindings,
        });
        match result {
            Ok(snapshot) => {
                self.config = next_config;
                self.config_revision = Some(next_revision);
                self.active_model_origin = Some(origin);
                self.active_model_id = Some(id);
                // Switching models swaps the behavior half of the platform
                // table: the model being left must stop answering its shortcuts
                // and the incoming one must answer its own immediately, without
                // waiting for a restart or for the behavior switch to be
                // toggled.
                self.refresh_shortcut_table();
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
        // Deleting the model the overlay is showing is allowed: the standard
        // preset takes over first, so the package being removed is never the one
        // the runtime is holding. A switch that fails aborts the delete rather
        // than pulling the files out from under the live model.
        if self.is_selected_installed_model(&id) {
            self.select_model(ModelOrigin::Preset, STANDARD_PRESET_MODEL_ID)?;
        }
        self.model_store
            .delete(&id)
            .map_err(ApplicationError::ModelStore)?;
        let mut installed_models = self.config.model.installed_models.clone();
        let before = installed_models.len();
        installed_models.retain(|metadata| metadata.id != id.as_str());
        if installed_models.len() != before {
            self.commit_model_metadata(ModelOrigin::Installed, installed_models)?;
        }
        Ok(())
    }

    /// Whether `id` is the installed model the application is showing or has
    /// selected.
    ///
    /// The overlay's identity and the recorded selection are two separate
    /// facts: a fresh configuration records no selection while the startup
    /// model is already live. Either one pointing at `id` means deleting it
    /// would remove the model the user is currently looking at, which is the
    /// case [`Application::delete_model`] has to switch away from first.
    fn is_selected_installed_model(&self, id: &ModelId) -> bool {
        let shown = self.active_model_origin == Some(ModelOrigin::Installed)
            && self
                .runtime
                .client()
                .snapshot()
                .active_model
                .as_ref()
                .is_some_and(|active| active.id.as_str() == id.as_str());
        let configured = self.config.model.selected_model_origin
            == Some(SelectedModelOrigin::Installed)
            && self.config.model.selected_model_id.as_deref() == Some(id.as_str());
        shown || configured
    }

    /// Import every model a source describes.
    ///
    /// A BongoCat package installs one model. A BongoCatMver source installs one
    /// *converted* model per input mode it carries, each with the same
    /// generated UUID store key and its own metadata record, so the three modes
    /// of a legacy model become three ordinary entries in the model list that
    /// can be activated, renamed and deleted independently.
    ///
    /// Each model is committed on its own: a source whose second mode fails
    /// still leaves the first installed and titled. That is deliberate — the
    /// models are independent, and silently discarding a mode that converted
    /// correctly would be worse than reporting a failure the user can act on by
    /// fixing that one mode.
    pub fn import_models(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
    ) -> Result<Vec<InstalledModel>, ApplicationError> {
        self.import_models_with_observer(title_hint, source_root, |_| {}, || false)
    }

    /// Inspect what a user-picked source is without installing anything.
    ///
    /// The settings page calls this to decide whether to ask the user about a
    /// BongoCat Mver conversion, and which modes to offer.
    pub fn inspect_model_source(
        &self,
        source_root: impl AsRef<Path>,
    ) -> Result<bongocat_ui_protocol::SettingsModelSourceContent, ApplicationError> {
        self.model_store
            .inspect_source(source_root)
            .map(settings_model_source_content)
            .map_err(ApplicationError::ModelStore)
    }

    /// Import every model a source carries, converting `selected_modes`.
    ///
    /// A BongoCat package installs one model and the selection is ignored. A
    /// BongoCat Mver source converts the intersection of `selected_modes` and
    /// the modes the source actually carries, taken in [`MverInputMode::ALL`]
    /// order; an empty intersection after an Mver source is a named
    /// `ModelImportSourceUnsupported` rather than a silent no-op.
    ///
    /// Kept for every caller that wants the whole source. The observer
    /// variant is the one the settings page uses.
    pub fn import_models_with_observer<Observe, IsCancelled>(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
        observe: Observe,
        is_cancelled: IsCancelled,
    ) -> Result<Vec<InstalledModel>, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        // No preference means "convert everything the source carries": reuse the
        // selection filter with all modes requested.
        self.import_models_with_selected_modes_with_observer(
            title_hint,
            source_root,
            MverInputMode::ALL.to_vec(),
            observe,
            is_cancelled,
        )
    }

    /// Import every model a source carries, converting only `selected_modes`.
    ///
    /// `selected_modes` names the BongoCat Mver modes to convert. A package
    /// source ignores them; an Mver source filters to the selected modes it
    /// actually carries, in [`MverInputMode::ALL`] order, so a request never
    /// duplicates a mode and the report order stays the one the settings
    /// contract declares. An Mver source whose selection matches no carried
    /// mode is reported as `ModelImportSourceUnsupported`.
    pub fn import_models_with_selected_modes_with_observer<Observe, IsCancelled>(
        &mut self,
        title_hint: impl Into<String>,
        source_root: impl AsRef<Path>,
        selected_modes: Vec<MverInputMode>,
        observe: Observe,
        mut is_cancelled: IsCancelled,
    ) -> Result<Vec<InstalledModel>, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let title_hint = title_hint.into();
        let source_root = source_root.as_ref();
        let language = self.effective_language();
        let mut aggregate = ImportProgressAccumulator::new(observe);

        // Which models the source describes is decided from its own bytes, not
        // from anything the caller selected: the folder is inspected for a
        // legacy key table. The store only reports a legacy source once a mode
        // really carries a usable model, so an empty mode list cannot occur —
        // treating it as a package keeps the loop below total and still reports
        // a real diagnostic from the package path.
        let content = self
            .model_store
            .inspect_source(source_root)
            .map_err(ApplicationError::ModelStore)?;
        let modes = match content {
            // A legacy source keeps the selected modes it really carries, in
            // the declared mode order.
            ModelSourceContent::Mver { modes } if !modes.is_empty() => {
                let selected = MverInputMode::ALL
                    .into_iter()
                    .filter(|mode| modes.contains(mode) && selected_modes.contains(mode))
                    .collect::<Vec<_>>();
                if selected.is_empty() {
                    return Err(ApplicationError::ModelStore(
                        ModelStoreError::source_conversion_failed(
                            "none of the selected BongoCatMver modes are present",
                        ),
                    ));
                }
                Some(selected)
            }
            // An Mver source with no convertible mode falls back to the
            // package path, exactly as before.
            ModelSourceContent::Mver { modes } => {
                debug_assert!(modes.is_empty());
                None
            }
            ModelSourceContent::Package => None,
        };

        let mut installed = Vec::new();
        let mut installed_models = self.config.model.installed_models.clone();
        let count = modes.as_ref().map_or(1, Vec::len);
        for index in 0..count {
            let id = self
                .model_store
                .allocate_unique_id()
                .map_err(ApplicationError::ModelStore)?;
            let fallback = id.as_str().to_owned();
            let model = match modes.as_ref() {
                None => self.model_store.import_with_observer(
                    id,
                    source_root,
                    |update| aggregate.report(update),
                    &mut is_cancelled,
                ),
                Some(modes) => self.model_store.import_mver_with_observer(
                    id,
                    modes[index],
                    source_root,
                    |update| aggregate.report(update),
                    &mut is_cancelled,
                ),
            }
            .map_err(ApplicationError::ModelStore)?;
            let title = match modes.as_ref() {
                None => installed_model_title(&title_hint, source_root, &fallback),
                Some(modes) => legacy_model_title(
                    &title_hint,
                    source_root,
                    &fallback,
                    legacy_mode_label(language, modes[index]),
                ),
            };
            installed_models.push(ModelMetadata {
                id: model.id().as_str().to_owned(),
                title,
            });
            self.commit_model_metadata(ModelOrigin::Installed, installed_models.clone())?;
            installed.push(model);
        }
        Ok(installed)
    }

    /// Drop metadata records whose installed model directory no longer
    /// exists. The record list stays consistent with the store even when a
    /// model was removed by hand outside the application.
    ///
    /// Preset records are deliberately not pruned by anything: a preset is
    /// shipped with the build rather than found on disk, so a record whose model
    /// this build no longer carries is not evidence of a stale customisation —
    /// it is simply not shown, and it names the model again if a later build
    /// ships it.
    fn prune_missing_installed_metadata(&mut self) {
        let Ok(catalog) = self.model_store.list() else {
            return;
        };
        let present = catalog
            .entries
            .iter()
            .map(|entry| entry.id().as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let kept = self
            .config
            .model
            .installed_models
            .iter()
            .filter(|metadata| present.contains(&metadata.id))
            .cloned()
            .collect::<Vec<_>>();
        if kept.len() == self.config.model.installed_models.len() {
            return;
        }
        let _ = self.commit_model_metadata(ModelOrigin::Installed, kept);
    }

    /// Restore the model selection at startup. The configured selection is
    /// activated when it still loads; a selection whose resources were
    /// deleted by hand or are otherwise unusable never blocks startup — the
    /// application records an anonymous fallback event, persists the
    /// always-available standard preset as the corrected selection, and
    /// activates it. Without a configured selection the standard preset is
    /// the default model. Metadata records for model directories that no
    /// longer exist are pruned while configuration is operational.
    ///
    /// The runtime keeps at most one unresolved model activation: a pending
    /// activation is only committed once the overlay frame source consumes
    /// its first prepared frame. Startup therefore prepares exactly one
    /// model and never issues a second activation while the first may still
    /// be pending.
    pub fn restore_startup_model(&mut self) -> Result<(), ApplicationError> {
        self.prune_missing_installed_metadata();
        let configured = self.config.model.selected_model_id.clone().zip(
            self.config
                .model
                .selected_model_origin
                .map(model_origin_from_config),
        );
        let Some((id, origin)) = configured else {
            // No configured selection: the standard preset is the default model.
            return self
                .prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
                .map(|_| ());
        };
        let configured_selection = match ModelId::parse(id) {
            Ok(id) => (origin, id),
            Err(_) => {
                self.fallback_to_standard_preset();
                return self
                    .prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
                    .map(|_| ());
            }
        };
        let (origin, id) = configured_selection;
        if self.prepare_model(origin, id.as_str()).is_ok() {
            return Ok(());
        }
        self.fallback_to_standard_preset();
        self.prepare_model(ModelOrigin::Preset, self.standard_preset_id().as_str())
            .map(|_| ())
    }

    /// Record the anonymous fallback event and persist the standard preset
    /// as the corrected selection. A failed commit keeps the stale selection
    /// on disk; the next startup simply retries the fallback.
    fn fallback_to_standard_preset(&mut self) {
        self.application_log
            .record(ApplicationLogEvent::model_selection_fallback());
        self.persist_model_selection(ModelOrigin::Preset, &self.standard_preset_id());
    }

    fn standard_preset_id(&self) -> ModelId {
        ModelId::parse(STANDARD_PRESET_MODEL_ID).expect("standard preset model id is valid")
    }

    /// Persist a corrected model selection without touching the runtime.
    /// A failed commit keeps the stale selection on disk; the next startup
    /// simply retries the fallback.
    fn persist_model_selection(&mut self, origin: ModelOrigin, id: &ModelId) {
        let mut next_config = self.config.clone();
        next_config.model.selected_model_id = Some(id.as_str().to_owned());
        next_config.model.selected_model_origin = Some(config_origin_from_model(origin));
        let Ok(expected_revision) = self.ready_config_revision() else {
            return;
        };
        let Ok(next_revision) = self
            .config_store
            .commit_if_revision(&next_config, expected_revision)
        else {
            return;
        };
        self.config = next_config;
        self.config_revision = Some(next_revision);
    }

    /// Persist the legacy default chords for a model that is becoming active.
    ///
    /// Best effort by design: a configuration that is not operational, or a
    /// commit that loses a revision race, leaves the previous bindings in place
    /// and lets activation continue. Nothing is written when the model already
    /// has a binding for every behavior, which keeps the ordinary startup path
    /// read-only.
    ///
    /// The shortcut table is not touched here — the caller refreshes it once the
    /// activation is known to have succeeded, so a failed activation cannot
    /// leave the previous model's chords unregistered.
    fn persist_default_behavior_shortcuts(&mut self, model: &CommittedModel) {
        let mut next_config = self.config.clone();
        if assign_default_behavior_shortcuts(&mut next_config, model) == 0 {
            return;
        }
        if next_config.validate().is_err() {
            return;
        }
        let Ok(expected_revision) = self.ready_config_revision() else {
            return;
        };
        let Ok(next_revision) = self
            .config_store
            .commit_if_revision(&next_config, expected_revision)
        else {
            return;
        };
        self.config = next_config;
        self.config_revision = Some(next_revision);
    }

    pub fn shutdown(self) -> Result<RuntimeSnapshot, ApplicationError> {
        self.application_log
            .record(ApplicationLogEvent::shutdown_started());
        self.run_marker.mark_shutdown_started()?;
        let runtime_result = self.runtime.shutdown(RUNTIME_TIMEOUT);
        let audio_result = self
            .motion_audio
            .map(|service| service.shutdown(RUNTIME_TIMEOUT))
            .transpose()
            .map(|_| ());
        match combine_shutdown_results(runtime_result, audio_result) {
            Ok(stopped) => {
                self.run_marker.complete()?;
                self.application_log
                    .record(ApplicationLogEvent::shutdown_completed());
                Ok(stopped)
            }
            Err(error) => {
                self.application_log
                    .record(ApplicationLogEvent::shutdown_failed());
                Err(error)
            }
        }
    }
}

fn system_language() -> Language {
    bongocat_platform::system_language()
}

/// Where a preset model sits on the Models page: the position of the input mode
/// it belongs to.
///
/// The build ships one preset per mode, and the modes already declare their own
/// order — [`MverInputMode::ALL`] is Standard, Keyboard, Gamepad, and that order
/// is part of the settings contract because it is the order a conversion
/// reports and titles its models in. Reading it here rather than repeating the
/// three ids keeps one list of modes instead of two that can drift.
///
/// The page has to read it at all because the ids are `standard`, `keyboard`
/// and `gamepad`: ordering by id puts Gamepad first and Standard last, which is
/// the reverse of the mode order a user expects.
///
/// A preset whose id is not one of the modes — a package a developer dropped
/// into the catalog root, or one a later build adds before this list learns
/// about it — is not part of that order, so it sorts after every mode.
fn preset_model_order(id: &str) -> usize {
    MverInputMode::ALL
        .iter()
        .position(|mode| mode.as_str() == id)
        .unwrap_or(MverInputMode::ALL.len())
}

/// Where an installed model sits on the Models page: the order the user
/// imported it in.
///
/// The configuration's record list is that order. An import appends its record
/// and a deletion only removes one, so a record's position is the model's place
/// on the page and a newly imported model always lands at the end.
///
/// An entry with no record — a package directory copied into the store root by
/// hand, which no import ever ran for — has no place in that order, so it sorts
/// after every model the user actually imported. Ordering those by id keeps the
/// page stable rather than dependent on the order the scan happened to walk the
/// directory in.
fn installed_model_order(records: &[ModelMetadata], id: &str) -> usize {
    records
        .iter()
        .position(|record| record.id == id)
        .unwrap_or(usize::MAX)
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

/// Fold the per-model import progress of one import action into one sequence.
///
/// The store reports one model at a time and starts every one of them at
/// `Preparing` with zero totals, because each is imported on its own. The
/// settings monitor drops an update that moves backwards, so without this
/// folding a legacy source's second model would report nothing and the display
/// would sit on the first model's final numbers. Carrying the finished models'
/// totals forward and never letting the stage regress keeps a single honest,
/// monotone sequence for the whole action, which is what the UI is showing.
struct ImportProgressAccumulator<Observe> {
    observe: Observe,
    completed_files: u64,
    completed_bytes: u64,
    current_files: u64,
    current_bytes: u64,
    stage: ModelImportStage,
}

impl<Observe> ImportProgressAccumulator<Observe>
where
    Observe: FnMut(ModelImportProgress),
{
    fn new(observe: Observe) -> Self {
        Self {
            observe,
            completed_files: 0,
            completed_bytes: 0,
            current_files: 0,
            current_bytes: 0,
            stage: ModelImportStage::Preparing,
        }
    }

    fn report(&mut self, update: ModelImportProgress) {
        if update.stage == ModelImportStage::Preparing {
            self.completed_files = self.completed_files.saturating_add(self.current_files);
            self.completed_bytes = self.completed_bytes.saturating_add(self.current_bytes);
            self.current_files = 0;
            self.current_bytes = 0;
        }
        self.current_files = update.files_copied;
        self.current_bytes = update.bytes_copied;
        self.stage = self.stage.max(update.stage);
        (self.observe)(ModelImportProgress {
            stage: self.stage,
            files_copied: self.completed_files.saturating_add(self.current_files),
            bytes_copied: self.completed_bytes.saturating_add(self.current_bytes),
        });
    }
}

/// The preset model that is always available as the final startup fallback.
const STANDARD_PRESET_MODEL_ID: &str = "standard";

const MODEL_TITLE_MAXIMUM_CHARS: usize = 128;

/// The editable display name for a newly imported model: the UI sends the
/// chosen title (defaulting to the source folder name). A blank hint degrades
/// to the source folder name and then to the stable model id. The id itself
/// is a service-generated UUID and never derived from any of these names.
fn installed_model_title(hint: &str, source_root: &Path, fallback: &str) -> String {
    let hint = hint.trim();
    if !hint.is_empty() {
        let clipped = clamp_model_title(hint);
        if !clipped.is_empty() {
            return clipped;
        }
    }
    installed_model_title_from_source(source_root, fallback)
}

/// The display name for one converted mode of a BongoCatMver source.
///
/// One legacy source becomes several models at once, so they need to be told
/// apart in the model list: the source's own name is kept and the mode is
/// appended, which is the naming the community already uses for exported
/// models. The mode is reserved out of the title limit before the source name is
/// clipped, because it is the only thing distinguishing the three models. The
/// label is localized here rather than stored as a stable token, because a title
/// is user-visible text the user can edit afterwards, exactly like the hint the
/// settings page sends.
fn legacy_model_title(hint: &str, source_root: &Path, fallback: &str, label: &str) -> String {
    let base = installed_model_title(hint, source_root, fallback);
    let suffix = format!(" · {label}");
    let available = MODEL_TITLE_MAXIMUM_CHARS.saturating_sub(suffix.chars().count());
    let base = base
        .chars()
        .take(available)
        .collect::<String>()
        .trim_end()
        .to_owned();
    if base.is_empty() {
        return clamp_model_title(label);
    }
    clamp_model_title(&format!("{base}{suffix}"))
}

/// The localized name of one BongoCatMver input mode.
fn legacy_mode_label(language: Language, mode: MverInputMode) -> &'static str {
    let key = match mode {
        MverInputMode::Standard => "models.mver.mode.standard",
        MverInputMode::Keyboard => "models.mver.mode.keyboard",
        MverInputMode::Gamepad => "models.mver.mode.gamepad",
    };
    bongocat_i18n::text(locale_code(language), key)
}

/// Project one model-crate source description onto the settings boundary enum.
///
/// The model crate is the one that read the bytes; the UI enum is the only
/// shape the settings page knows. Keeping the mapping here means the settings
/// page never names a model-crate type, and a later mode added upstream is a
/// compile error here rather than a silent gap in the dialog.
fn settings_model_source_content(
    content: ModelSourceContent,
) -> bongocat_ui_protocol::SettingsModelSourceContent {
    match content {
        ModelSourceContent::Package => bongocat_ui_protocol::SettingsModelSourceContent::Package,
        ModelSourceContent::Mver { modes } => {
            bongocat_ui_protocol::SettingsModelSourceContent::Mver {
                modes: modes
                    .into_iter()
                    .map(|mode| match mode {
                        MverInputMode::Standard => bongocat_ui_protocol::SettingsMverMode::Standard,
                        MverInputMode::Keyboard => bongocat_ui_protocol::SettingsMverMode::Keyboard,
                        MverInputMode::Gamepad => bongocat_ui_protocol::SettingsMverMode::Gamepad,
                    })
                    .collect(),
            }
        }
    }
}

/// The locale the embedded catalog knows for a resolved application language.
///
/// `system` never reaches this point: the application resolves it against the
/// platform language before anything reads it, so the fallback arm mirrors the
/// UI's own English default rather than a second resolution rule.
const fn locale_code(language: Language) -> &'static str {
    match language {
        Language::ChineseSimplified => "zh-CN",
        Language::System | Language::EnglishUnitedStates => bongocat_i18n::DEFAULT_LOCALE,
    }
}

/// Trim a display name to the length the configuration schema accepts.
fn clamp_model_title(value: &str) -> String {
    value
        .chars()
        .take(MODEL_TITLE_MAXIMUM_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Normalize a user-typed title before it is written to the metadata record.
///
/// The settings page sanitizes the field as it is typed, and the configuration
/// re-validates the record when it loads; this is the service's own gate in
/// between, so a caller that bypasses the page cannot store a title the next
/// config load would reject.
fn normalize_model_title(value: &str) -> Option<String> {
    let filtered: String = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let title = clamp_model_title(filtered.trim());
    (!title.is_empty()).then_some(title)
}

/// The source-folder default title; over-long or missing folder names
/// degrade to the model id.
///
/// The name itself comes from `bongocat_ui_protocol::model_source_display_name` so the
/// service's fallback and the settings page's pre-filled title agree. The source
/// is the folder a user picked; the shared rule also knows how to drop an archive
/// extension, which is what the `名字.zip` exported from that folder carries.
fn installed_model_title_from_source(source_root: &Path, fallback: &str) -> String {
    bongocat_ui_protocol::model_source_display_name(source_root, source_root.is_dir())
        .map(|name| clamp_model_title(&name))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn overlay_settings_from_config(config: &NativeConfig) -> OverlaySettings {
    OverlaySettings {
        click_through: config.overlay.click_through,
        always_on_top: config.overlay.always_on_top,
        scale_percent: config.overlay.scale_percent,
        opacity_percent: config.overlay.opacity_percent,
        corner_radius_percent: config.overlay.corner_radius_percent,
        hide_on_pointer_hover: config.overlay.hide_on_pointer_hover,
        hide_on_pointer_hover_delay_seconds: config.overlay.hide_on_pointer_hover_delay_seconds,
        keep_inside_screen: config.overlay.keep_inside_screen,
    }
}

const fn model_settings_from_config(config: &NativeConfig) -> ModelSettings {
    ModelSettings {
        mirror: config.model.mirror,
        mirror_pointer_tracking: config.model.mirror_pointer_tracking,
        ignore_pointer: config.model.ignore_pointer,
    }
}

/// The shortcut bindings the platform should have registered right now: every
/// application command, plus the behaviors of the live model when the model
/// behavior switch is on.
///
/// Model behaviors are scoped to exactly one model on purpose. The
/// configuration keeps a binding for every model the user has activated, and
/// each model counts its defaults from the first digit of the primary modifier
/// — so two models can legitimately hold the same chord. Only the live model's
/// half may reach the platform: registering every model's half would occupy
/// chords that can never fire (the dispatcher drops a target whose model is not
/// active) and would leave the platform's per-chord target map pointing at
/// whichever model registered first.
fn active_shortcuts(
    config: &NativeConfig,
    active_model: Option<&str>,
) -> Result<CompiledShortcuts, ConfigError> {
    let active_model = if config.model.enable_behavior_shortcuts {
        active_model
    } else {
        None
    };
    config.shortcuts.active_bindings(active_model).compile()
}

/// The platform's command modifier, which the legacy auto-assignment used as the
/// base of every model behaviour chord: Command on macOS, Control everywhere
/// else. `bongocat-config` takes it as a parameter so that crate stays
/// platform-free.
const fn behavior_shortcut_primary() -> u8 {
    if cfg!(target_os = "macos") {
        ShortcutModifiers::META
    } else {
        ShortcutModifiers::CONTROL
    }
}

/// The behavior ids of one committed model, in the order the model declares
/// them: every motion group in declaration order, then every expression. The
/// legacy auto-assignment walked them in this order, and so does the Shortcuts
/// page.
fn behavior_ids(model: &CommittedModel) -> Vec<String> {
    model
        .snapshot()
        .behaviors
        .into_iter()
        .map(|behavior| {
            let action = match behavior {
                ModelBehaviorSnapshot::Motion { group, index } => {
                    ModelBehaviorAction::Motion { group, index }
                }
                ModelBehaviorSnapshot::Expression { name } => {
                    ModelBehaviorAction::Expression { name }
                }
            };
            action.behavior_id()
        })
        .collect()
}

/// Fill in the legacy default chords for one model's motions and expressions,
/// leaving every binding the user already has untouched. Returns how many
/// bindings were added.
fn assign_default_behavior_shortcuts(config: &mut NativeConfig, model: &CommittedModel) -> usize {
    bongocat_config::assign_default_behavior_shortcuts(
        &mut config.shortcuts,
        model.id().as_str(),
        &behavior_ids(model),
        behavior_shortcut_primary(),
    )
}

/// Rebuild the shortcut section from what the settings window sent.
///
/// The payload carries bindings only, so the command gate is passed in
/// separately: recording or clearing a chord must never switch the window
/// shortcuts back on behind the user's back.
fn shortcut_config_from_settings(
    shortcuts: bongocat_ui_protocol::SettingsShortcuts,
    commands_enabled: bool,
) -> ShortcutConfig {
    ShortcutConfig {
        commands_enabled,
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
    }
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
        .nth(2)
        .expect("repository root")
        .join("resources/models")
}

/// The input bindings of a committed model: the static hand table,
/// intersected with the key images the model actually ships.
fn input_bindings_for_committed_model(model: &CommittedModel) -> InputBindings {
    input_bindings_for_model(
        model.origin(),
        model.id().as_str(),
        &KeyImageInventory::read(model.root()),
    )
}

/// The key images decide which keys the model may react to at all.
///
/// A key the model cannot draw is left unbound on purpose:
/// `InputState::model_snapshot` drops a press without a hand assignment, so this
/// one map decides both the key overlay layer and whether
/// `CatParamLeftHandDown` / `CatParamRightHandDown` move. Pressing a key whose
/// image the model does not ship must do nothing at all — a paw pressing down
/// for an image that can never appear is feedback for something the user cannot
/// see. The check runs here, before the press reaches the model, instead of in
/// the renderer, because the renderer is not allowed to decide actions and
/// because the runtime is the single owner of pressed state.
///
/// `bongocat-live2d-render::KeyImageInventory` answers "can this key be drawn" with the
/// same directory scan and the same candidate fallbacks the renderer uses, so a
/// key that is bound here is exactly a key that draws.
fn input_bindings_for_model(
    origin: ModelOrigin,
    model_id: &str,
    key_images: &KeyImageInventory,
) -> InputBindings {
    const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);
    // Installed models have no per-model binding configuration yet. They must
    // not fall back to an empty map: `InputState::model_snapshot` drops any
    // key press without a hand assignment, which silently disabled key
    // overlays (and paw motion) for every imported third-party model. Default
    // them to the same keyboard mapping as the "standard"/"keyboard" presets;
    // each key still has to survive the artwork check below, so a model without
    // assets for a side simply reacts to nothing on that side.
    let keyboard_model =
        origin == ModelOrigin::Installed || matches!(model_id, "standard" | "keyboard");
    let mut key_hands = BTreeMap::new();
    if keyboard_model {
        // Every key of the standard 104/105-key layout belongs to the left hand
        // and the arrow cluster to the right: the model's left paw draws the
        // keyboard block and its right paw draws the arrows. One loop covers the
        // whole block, punctuation, PrintScreen and the navigation cluster
        // included, because naming and binding have to cover the same set.
        // `bongocat-live2d::key_name_candidates` names every one of these keys,
        // and `InputState::model_snapshot` drops any press without a hand
        // assignment before the key-image resolver ever sees it — so a key that
        // is named but not bound can never draw its artwork. The vocabulary is
        // deliberately not limited to the keys the shipped models happen to
        // draw: a model that ships `Dot.png`, `Minus.png` or `Insert.png` has to
        // work without a product change, and it does — the image it ships is
        // what makes the key bindable in the first place.
        for usage in 0x04..=0x65 {
            if (RIGHT_ARROW.hid_usage()..=0x52).contains(&usage) {
                continue;
            }
            bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
        }
        // F13 … F24 sit above the block; F1 … F12 are already covered by the
        // loop above. `FUNCTION_KEY_USAGES` is the same table the key-image
        // resolver names its assets from.
        for (first, last) in FUNCTION_KEY_USAGES {
            for usage in first..=last {
                bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
            }
        }
        // Keypad `=`, which macOS reports as a usage of its own.
        bind_drawable_key(&mut key_hands, 0x67, HandSide::Left, key_images);
        // The eight modifier usages. They sit above every range in this block,
        // which is exactly why they were dropped when the block was rewritten:
        // `0x04..=0x65` stops at `0x65`, the function table resumes at `0x68`,
        // and the keypad `=` is a single usage, so `0xe0..=0xe7` matched none of
        // them. Unbound modifiers never reach `InputState::model_snapshot`, so
        // no model could draw `ShiftLeft.png`, `AltLeft.png` or the shared
        // `Meta.png`, and no paw moved for Shift, Control, Alt or Meta — the
        // exact failure ADR-0038 renamed the Alt images to prevent.
        for usage in 0xe0..=0xe7 {
            bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
        }
        // The Apple Fn / globe key. It is the one key of this block that is not
        // on the HID Keyboard/Keypad page — its usage folds Apple's vendor page
        // into the same `u16` — so every range above misses it and it has to be
        // written out, exactly as it has to be named explicitly in
        // `bongocat-live2d::key_name_candidates`. Windows never reports the key
        // (the firmware owns it), so the binding is inert there. It is still
        // gated on the model's artwork like every other key: a model without a
        // `Globe.png` (or the legacy `Function.png`) gets no reaction at all.
        bind_drawable_key(
            &mut key_hands,
            bongocat_render::GLOBE_KEY_USAGE,
            HandSide::Left,
            key_images,
        );
    } else {
        bind_drawable_key(
            &mut key_hands,
            PhysicalKey::KEY_A.hid_usage(),
            HandSide::Left,
            key_images,
        );
    }
    if origin == ModelOrigin::Installed || matches!(model_id, "keyboard" | "gamepad") {
        for usage in RIGHT_ARROW.hid_usage()..=0x52 {
            bind_drawable_key(&mut key_hands, usage, HandSide::Right, key_images);
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

/// Bind one key to one hand, but only when the model ships an image that draws
/// it. The same key on a model with the artwork and on a model without it must
/// not produce the same reaction: without the image there is nothing to show, so
/// the press is dropped here rather than animated into an invisible key.
fn bind_drawable_key(
    key_hands: &mut BTreeMap<PhysicalKey, HandSide>,
    hid_usage: u16,
    side: HandSide,
    key_images: &KeyImageInventory,
) {
    let key_side = match side {
        HandSide::Left => KeySide::Left,
        HandSide::Right => KeySide::Right,
    };
    if key_images.can_draw(key_side, hid_usage) {
        key_hands.insert(PhysicalKey::from_hid_usage(hid_usage), side);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_render::{ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome};
    use bongocat_runtime::{
        GamepadAxis, GamepadAxisKey, GamepadAxisSample, GamepadButton, GamepadButtonKey, HandSide,
        InputControl, InputEdge, InputEvent, InputSource, MonotonicMillis, PhysicalKey,
        RuntimeState,
    };
    use std::time::Instant;
    use std::{fs, path::Path};
    use tempfile::tempdir;

    /// Import a single-model package and return the one model it installed.
    ///
    /// The product's import entry point handles sources that describe several
    /// models at once. Every source in this module is a BongoCat package, so the
    /// count is asserted here instead of at each call site.
    fn import_one(
        application: &mut Application,
        title_hint: &str,
        source: impl AsRef<Path>,
    ) -> InstalledModel {
        let mut models = application
            .import_models(title_hint, source)
            .expect("import a single model package");
        assert_eq!(
            models.len(),
            1,
            "a package source installs exactly one model"
        );
        models.pop().expect("one installed model")
    }

    /// One opaque key cap and one semi-transparent paw, as exact PNG bytes so
    /// the fixture does not need a bitmap decoder in the test build.
    const LEGACY_KEY_CAP_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9,
        0xf1, 0x9e, 0x7e, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60,
        0x60, 0xf8, 0xff, 0x1f, 0x15, 0x93, 0x2c, 0x00, 0x00, 0x1c, 0x60, 0x1f, 0xe1, 0xcb, 0x7f,
        0x73, 0xf9, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const LEGACY_SECOND_KEY_CAP_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9,
        0xf1, 0x9e, 0x7e, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60,
        0xf8, 0x8f, 0x06, 0x49, 0x17, 0x00, 0x00, 0x2c, 0x50, 0x1f, 0xe1, 0x45, 0xaf, 0x33, 0x10,
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const LEGACY_PAW_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9,
        0xf1, 0x9e, 0x7e, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8,
        0xcf, 0xc0, 0xd0, 0x80, 0x8c, 0x19, 0x48, 0x17, 0x00, 0x00, 0x3a, 0x39, 0x17, 0xf1, 0x3b,
        0x56, 0x2b, 0xf5, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn write_fixture_file(root: &Path, reference: &str, bytes: &[u8]) {
        let path = root.join(reference);
        fs::create_dir_all(path.parent().expect("fixture parent"))
            .expect("create fixture directory");
        fs::write(path, bytes).expect("write fixture file");
    }

    /// Write a minimal BongoCatMver application folder.
    ///
    /// The layout is the legacy application's: the mode key table at the root,
    /// and one resource folder per mode holding the Live2D package, the paw and
    /// key-cap layers a conversion composes, and the mode's background and
    /// cover. The gamepad section addresses buttons with XInput indices, the way
    /// the legacy config does.
    fn legacy_source_fixture(root: &Path) {
        let mut config = serde_json::Map::new();
        for (mode, section) in [
            ("standard", r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#),
            (
                "keyboard",
                r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
            ),
            (
                "gamepad",
                r#"{"lefthand":[[10]],"righthand":[[0]],"keyboard":[[10],[0]]}"#,
            ),
        ] {
            config.insert(
                mode.to_owned(),
                serde_json::from_str(section).expect("legacy mode section"),
            );
        }
        write_fixture_file(
            root,
            "config.json",
            &serde_json::to_vec(&serde_json::Value::Object(config)).expect("legacy config"),
        );

        for mode in ["standard", "keyboard", "gamepad"] {
            let base = format!("img/{mode}");
            write_fixture_file(
                root,
                &format!("{base}/cat_model/cat.model3.json"),
                br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
            );
            write_fixture_file(root, &format!("{base}/cat_model/model.moc3"), b"moc");
            write_fixture_file(root, &format!("{base}/keyboard/0.png"), LEGACY_KEY_CAP_PNG);
            write_fixture_file(
                root,
                &format!("{base}/keyboard/1.png"),
                LEGACY_SECOND_KEY_CAP_PNG,
            );
            for hand in ["hand", "lefthand", "righthand"] {
                write_fixture_file(root, &format!("{base}/{hand}/0.png"), LEGACY_PAW_PNG);
                write_fixture_file(root, &format!("{base}/{hand}/1.png"), LEGACY_PAW_PNG);
            }
            for asset in ["mousebg.png", "bg.png", "cat.png"] {
                write_fixture_file(root, &format!("{base}/{asset}"), LEGACY_KEY_CAP_PNG);
            }
        }
    }

    /// One BongoCatMver source describes several models: importing it installs
    /// one converted model per mode, each with its own store key and a title
    /// that tells them apart.
    #[test]
    fn importing_a_legacy_source_installs_one_titled_model_per_mode() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        application
            .set_language(Language::ChineseSimplified)
            .expect("set language");
        let source = base.path().join("Bongo Cat Mver");
        fs::create_dir(&source).expect("legacy source");
        legacy_source_fixture(&source);

        let installed = application
            .import_models("我的猫", &source)
            .expect("import legacy source");
        assert_eq!(installed.len(), 3, "one model per configured mode");
        assert_eq!(
            application
                .config()
                .model
                .installed_models
                .iter()
                .map(|metadata| metadata.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "我的猫 · 标准模式",
                "我的猫 · 键盘模式",
                "我的猫 · 手柄模式"
            ]
        );
        let ids = installed
            .iter()
            .map(|model| model.id().as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 3, "each model has its own store key");

        // The standard mode has one paw per key and no right hand; the split
        // modes publish both sides, with the gamepad's own vocabulary.
        assert_eq!(installed[0].index().entry, "cat.model3.json");
        assert!(
            installed[0]
                .root()
                .join("resources/left-keys/KeyA.png")
                .is_file()
        );
        assert!(
            installed[0]
                .root()
                .join("resources/left-keys/KeyB.png")
                .is_file()
        );
        assert!(!installed[0].root().join("resources/right-keys").exists());
        assert!(
            installed[1]
                .root()
                .join("resources/left-keys/KeyA.png")
                .is_file()
        );
        assert!(
            installed[1]
                .root()
                .join("resources/right-keys/LeftArrow.png")
                .is_file()
        );
        assert!(
            installed[2]
                .root()
                .join("resources/left-keys/DPadLeft.png")
                .is_file()
        );
        assert!(
            installed[2]
                .root()
                .join("resources/right-keys/South.png")
                .is_file()
        );
        for model in &installed {
            assert!(model.root().join("resources/background.png").is_file());
            assert!(model.root().join("resources/cover.png").is_file());
        }

        // The legacy folder is read, never written into.
        assert!(
            source
                .join("img/standard/cat_model/cat.model3.json")
                .is_file()
        );
        let catalog = application.model_catalog().expect("model catalog");
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| entry.origin() == ModelOrigin::Installed)
                .count(),
            3
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn importing_a_legacy_source_installs_only_the_selected_modes() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        application
            .set_language(Language::ChineseSimplified)
            .expect("set language");
        let source = base.path().join("Bongo Cat Mver");
        fs::create_dir(&source).expect("legacy source");
        legacy_source_fixture(&source);

        let installed = application
            .import_models_with_selected_modes_with_observer(
                "仅选模式",
                &source,
                vec![MverInputMode::Gamepad, MverInputMode::Keyboard],
                |_| {},
                || false,
            )
            .expect("import selected legacy modes");

        // The application reports and stores the selection in the store's mode
        // order, and the unselected Standard mode never reaches the catalog.
        assert_eq!(installed.len(), 2);
        assert_eq!(
            application
                .config()
                .model
                .installed_models
                .iter()
                .map(|metadata| metadata.title.as_str())
                .collect::<Vec<_>>(),
            vec!["仅选模式 · 键盘模式", "仅选模式 · 手柄模式"]
        );
        assert!(
            installed[0]
                .root()
                .join("resources/right-keys/LeftArrow.png")
                .is_file()
        );
        assert!(
            installed[1]
                .root()
                .join("resources/left-keys/DPadLeft.png")
                .is_file()
        );
        assert_eq!(
            application
                .model_catalog()
                .expect("model catalog")
                .iter()
                .filter(|entry| entry.origin() == ModelOrigin::Installed)
                .count(),
            2
        );
        application.shutdown().expect("clean shutdown");
    }

    /// The settings monitor drops an update that moves backwards, so folding the
    /// per-model progress of one action has to keep the sequence monotone while
    /// still counting every model.
    #[test]
    fn legacy_import_progress_stays_monotone_across_models() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = base.path().join("Bongo Cat Mver");
        fs::create_dir(&source).expect("legacy source");
        legacy_source_fixture(&source);

        let updates = std::cell::RefCell::new(Vec::new());
        let installed = application
            .import_models_with_observer(
                "legacy",
                &source,
                |update| updates.borrow_mut().push(update),
                || false,
            )
            .expect("import legacy source");
        assert_eq!(installed.len(), 3);

        let updates = updates.into_inner();
        assert_eq!(
            updates.first().map(|update| update.stage),
            Some(ModelImportStage::Preparing)
        );
        assert_eq!(
            updates.last().map(|update| update.stage),
            Some(ModelImportStage::Committing)
        );
        for pair in updates.windows(2) {
            assert!(pair[0].stage <= pair[1].stage);
            assert!(pair[0].files_copied <= pair[1].files_copied);
            assert!(pair[0].bytes_copied <= pair[1].bytes_copied);
        }
        // The three conversions are counted end to end, so the last update is
        // the sum of what all of them wrote.
        let final_update = updates.last().expect("final update");
        assert_eq!(
            final_update.files_copied,
            installed
                .iter()
                .map(|model| model.index().package_file_count as u64)
                .sum::<u64>()
        );
        assert_eq!(
            final_update.bytes_copied,
            installed
                .iter()
                .map(|model| model.index().package_total_bytes)
                .sum::<u64>()
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn legacy_model_titles_stay_within_the_configuration_limit() {
        let hint = "猫".repeat(bongocat_config::MODEL_METADATA_MAXIMUM_TITLE_CHARS);
        let title = legacy_model_title(&hint, Path::new("/source"), "fallback", "标准模式");
        assert!(
            title.chars().count() <= bongocat_config::MODEL_METADATA_MAXIMUM_TITLE_CHARS,
            "{} characters",
            title.chars().count()
        );
        assert!(title.ends_with("标准模式"));
        // A blank hint still produces a distinguishable title per mode.
        assert_eq!(
            legacy_model_title("", Path::new("/source/我的猫"), "fallback", "手柄模式"),
            "我的猫 · 手柄模式"
        );
    }

    #[test]
    fn import_progress_folding_carries_finished_models_forward() {
        let mut updates = Vec::new();
        {
            let mut accumulator = ImportProgressAccumulator::new(|update| updates.push(update));
            for files in [1_u64, 2] {
                for (stage, files_copied) in [
                    (ModelImportStage::Preparing, 0),
                    (ModelImportStage::Copying, files),
                    (ModelImportStage::Validating, files),
                    (ModelImportStage::Committing, files),
                ] {
                    accumulator.report(ModelImportProgress {
                        stage,
                        files_copied,
                        bytes_copied: files_copied * 100,
                    });
                }
            }
        }
        for pair in updates.windows(2) {
            assert!(pair[0].stage <= pair[1].stage, "{pair:?}");
            assert!(pair[0].files_copied <= pair[1].files_copied, "{pair:?}");
            assert!(pair[0].bytes_copied <= pair[1].bytes_copied, "{pair:?}");
        }
        let final_update = updates.last().expect("final update");
        assert_eq!(final_update.files_copied, 3, "1 file + 2 files");
        assert_eq!(final_update.bytes_copied, 300);
        // The stage never regresses to the second model's `Preparing`.
        assert_eq!(final_update.stage, ModelImportStage::Committing);
    }

    /// The key images a shipped preset model carries. An imported model brings
    /// its own artwork, so the presets stand in for "a package that ships this
    /// side" and "a package that ships nothing".
    fn shipped_key_images(id: &str) -> KeyImageInventory {
        let model = bongocat_model::PresetModelCatalog::open(
            repository_preset_root(),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model");
        KeyImageInventory::read(model.root())
    }

    /// Every HID usage the key vocabulary covers, which is the set the binding
    /// table has to stay consistent with: the main block (punctuation,
    /// PrintScreen and the navigation cluster included), keypad `=`, F13 … F24,
    /// the eight modifier usages, and the Apple Fn / globe key. `0x66` (Power) is
    /// the only gap, because neither adapter maps it.
    ///
    /// This is a **superset** of what either adapter can produce, not a list of
    /// what they do produce. Naming and binding deliberately cover more than the
    /// hardware delivers, so a model shipping artwork for a key no keyboard can
    /// press is still honoured; each adapter's own tests pin what it can really
    /// report (`bongocat-platform`'s
    /// `this_adapter_reports_exactly_the_keycodes_the_platform_defines` for
    /// macOS, the scan-code matrix for Windows). Do not read a usage being listed
    /// here as proof that the key is reachable — `Apps` (`0x65`) sat in this list
    /// for as long as the vocabulary existed while no adapter mapped it, which is
    /// why `Apps.png` could never be drawn.
    ///
    /// The modifier block is written out instead of relying on the ranges the
    /// implementation uses: it sits above `0x04..=0x65` and below nothing else,
    /// so a rewrite of the binding loops dropped Shift, Control, Alt and Meta
    /// without any range looking wrong. The globe key is written out for the
    /// same reason and one more: it is not on the Keyboard/Keypad page at all,
    /// so no range over that page can ever be made to include it.
    fn adapter_keyboard_usages() -> Vec<u16> {
        (0x04..=0x65)
            .chain([0x67])
            .chain(0x68..=0x73)
            .chain(0xe0..=0xe7)
            .chain([bongocat_render::GLOBE_KEY_USAGE])
            .collect()
    }

    #[test]
    fn installed_models_get_default_keyboard_bindings() {
        // An imported model is bound from the same keyboard table as the
        // presets, keyed on the artwork its own package ships; the `keyboard`
        // preset stands in for one that carries both hands.
        let keyboard_images = shipped_key_images("keyboard");
        let bindings =
            input_bindings_for_model(ModelOrigin::Installed, "custom-model", &keyboard_images);
        assert_eq!(bindings.hand_for(PhysicalKey::KEY_A), Some(HandSide::Left));
        assert_eq!(
            bindings.hand_for(PhysicalKey::from_hid_usage(0x52)),
            Some(HandSide::Right)
        );

        // Preset mappings must stay exactly as before the fix.
        let standard = input_bindings_for_model(
            ModelOrigin::Preset,
            "standard",
            &shipped_key_images("standard"),
        );
        assert_eq!(standard.hand_for(PhysicalKey::KEY_A), Some(HandSide::Left));
        assert_eq!(standard.hand_for(PhysicalKey::from_hid_usage(0x4f)), None);
        // The gamepad preset ships no keyboard artwork at all, so no keyboard
        // key reaches it; its gamepad buttons are bound on their own map.
        let gamepad = input_bindings_for_model(
            ModelOrigin::Preset,
            "gamepad",
            &shipped_key_images("gamepad"),
        );
        assert_eq!(gamepad.hand_for(PhysicalKey::KEY_A), None);
        assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x4f)), None);
        assert_eq!(
            gamepad.hand_for_gamepad(GamepadButton::South),
            Some(HandSide::Left)
        );
        assert_eq!(
            gamepad.hand_for_gamepad(GamepadButton::East),
            Some(HandSide::Right)
        );
    }

    /// `InputState::model_snapshot` drops any press without a hand assignment, so
    /// a function key missing from this map can never draw its `Fn.png` overlay.
    #[test]
    fn keyboard_models_assign_the_whole_function_row_to_the_left_hand() {
        for (origin, id, images) in [
            (ModelOrigin::Installed, "custom-model", "keyboard"),
            (ModelOrigin::Preset, "standard", "standard"),
            (ModelOrigin::Preset, "keyboard", "keyboard"),
        ] {
            let bindings = input_bindings_for_model(origin, id, &shipped_key_images(images));
            for (first, last) in bongocat_render::FUNCTION_KEY_USAGES {
                for usage in first..=last {
                    assert_eq!(
                        bindings.hand_for(PhysicalKey::from_hid_usage(usage)),
                        Some(HandSide::Left),
                        "{id} 0x{usage:02x}"
                    );
                }
            }
            // PrintScreen sits directly after F12 but is not a function key: it
            // gets no `Fn` fallback from the resolver, and no shipped model
            // draws `PrintScreen.png`, so it is not bound either.
            assert_eq!(
                bongocat_render::function_key_name(0x46),
                None,
                "{id} PrintScreen must not be a function key"
            );
            assert_eq!(
                bindings.hand_for(PhysicalKey::from_hid_usage(0x46)),
                None,
                "{id} PrintScreen has no artwork to draw"
            );
        }

        // The gamepad model keeps its button-only mapping.
        let gamepad = input_bindings_for_model(
            ModelOrigin::Preset,
            "gamepad",
            &shipped_key_images("gamepad"),
        );
        assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x3a)), None);
        assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x68)), None);
    }

    /// The globe key is gated on artwork like every other key, and it is the one
    /// key outside the HID Keyboard/Keypad page, so no range over that page can
    /// reach it — it has to be bound explicitly.
    ///
    /// Every model shipped today predates the key and carries no artwork for it,
    /// so pressing it must do nothing at all: no key layer and no paw movement
    /// (ADR-0042). A model that does ship the image is bound to the left hand,
    /// under the canonical name or the pre-rename spelling, because the binding
    /// asks the same `can_draw` the renderer draws with.
    #[test]
    fn the_globe_key_binds_only_for_a_model_that_ships_its_image() {
        let globe = PhysicalKey::from_hid_usage(bongocat_render::GLOBE_KEY_USAGE);
        for (origin, id, images) in [
            (ModelOrigin::Installed, "custom-model", "keyboard"),
            (ModelOrigin::Preset, "standard", "standard"),
            (ModelOrigin::Preset, "keyboard", "keyboard"),
            (ModelOrigin::Preset, "gamepad", "gamepad"),
        ] {
            let bindings = input_bindings_for_model(origin, id, &shipped_key_images(images));
            assert_eq!(
                bindings.hand_for(globe),
                None,
                "{id} ships no Globe.png, so the key must be inert"
            );
        }

        for name in ["Globe", "Function"] {
            let root = tempdir().expect("root");
            let left_keys = root.path().join("resources/left-keys");
            fs::create_dir_all(&left_keys).expect("left keys directory");
            fs::write(left_keys.join(format!("{name}.png")), b"globe").expect("globe image");
            let inventory = KeyImageInventory::read(root.path());
            let bindings =
                input_bindings_for_model(ModelOrigin::Installed, "custom-model", &inventory);
            assert_eq!(
                bindings.hand_for(globe),
                Some(HandSide::Left),
                "{name}.png must bind the globe key"
            );
            // And the shared function-row image is still a different key's.
            assert_eq!(
                bindings.hand_for(PhysicalKey::from_hid_usage(0x3a)),
                None,
                "{name}.png must not bind a function key"
            );
        }
    }

    /// The static table still covers the whole standard 104/105-key layout plus
    /// the keypad, and the key image a model ships is the only thing that can
    /// remove a key from it. `InputState::model_snapshot` drops a press whose key
    /// has no hand assignment before the resolver ever sees it, so an unbound key
    /// is inert end to end: no key layer and no paw movement. The whole block
    /// goes to the left hand because `left-keys` is where the shipped key images
    /// live; the arrow cluster is the right hand's.
    #[test]
    fn keyboard_models_bind_every_drawable_key_of_the_standard_layout() {
        let standard_images = shipped_key_images("standard");
        let keyboard_images = shipped_key_images("keyboard");
        for (origin, id, images, binds_arrows) in [
            (
                ModelOrigin::Installed,
                "custom-model",
                &keyboard_images,
                true,
            ),
            (ModelOrigin::Preset, "standard", &standard_images, false),
            (ModelOrigin::Preset, "keyboard", &keyboard_images, true),
        ] {
            let bindings = input_bindings_for_model(origin, id, images);
            // Walk the whole adapter union, not the ranges the implementation
            // happens to use: the modifier block sits outside `0x04..=0x65` and
            // outside `0x68..=0x73`, and a rewrite of those two loops dropped all
            // eight of them once already.
            for usage in adapter_keyboard_usages() {
                let expected = if (0x4f..=0x52).contains(&usage) {
                    // The four arrows are the right hand's cluster.
                    binds_arrows
                        .then(|| images.can_draw(KeySide::Right, usage))
                        .filter(|drawable| *drawable)
                        .map(|_| HandSide::Right)
                } else {
                    images
                        .can_draw(KeySide::Left, usage)
                        .then_some(HandSide::Left)
                };
                assert_eq!(
                    bindings.hand_for(PhysicalKey::from_hid_usage(usage)),
                    expected,
                    "{id} 0x{usage:02x}"
                );
            }
            // Spot checks, so the rule stays visible instead of being only a
            // mirror of the code under test.
            for (usage, expected, why) in [
                (0x04, true, "every keyboard model ships KeyA.png"),
                (0x1e, true, "every keyboard model ships Num1.png"),
                (0x4c, true, "every keyboard model ships Delete.png"),
                (0x59, true, "keypad 1 falls back to Num1.png"),
                (0x58, true, "keypad Enter falls back to Enter.png"),
                (0xe0, true, "Control.png is the shared family image"),
                (0xe1, true, "every keyboard model ships ShiftLeft.png"),
                (0xe2, true, "every keyboard model ships AltLeft.png"),
                (0xe3, true, "MetaLeft.png is not shipped, Meta.png is"),
                (0xe5, true, "every keyboard model ships ShiftRight.png"),
                (0xe6, true, "every keyboard model ships AltRight.png"),
                (0xe7, true, "MetaRight.png is not shipped, Meta.png is"),
                (0x37, false, "no shipped model draws Dot.png"),
                (0x46, false, "no shipped model draws PrintScreen.png"),
                (0x53, false, "no shipped model draws NumLock.png"),
                (0x63, false, "keypad . has no artwork to fall back to"),
            ] {
                assert_eq!(
                    bindings
                        .hand_for(PhysicalKey::from_hid_usage(usage))
                        .is_some(),
                    expected,
                    "{id} 0x{usage:02x}: {why}"
                );
            }
        }

        // The union walk above covers the arrows too: they are the right hand's
        // cluster for the models that ship that artwork, and `standard` has no
        // `right-keys` directory at all.

        // The gamepad model keeps its button-only mapping.
        let gamepad = input_bindings_for_model(
            ModelOrigin::Preset,
            "gamepad",
            &shipped_key_images("gamepad"),
        );
        assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x58)), None);
        assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x59)), None);
    }

    /// The reason the artwork gate exists at all: `CatParamLeftHandDown` and
    /// `CatParamRightHandDown` are driven by the same hand assignment the key
    /// overlay layer is, so a key the active model has no image for must not
    /// move the paw either. The bundled `standard` model ships `KeyA.png` but no
    /// `Dot.png`, which makes the two keys a complete pair: one draws and
    /// presses, the other does nothing at all.
    #[test]
    fn a_key_the_active_model_cannot_draw_never_moves_the_paw() {
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
            .expect("prepare standard model");
        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        let frame = wait_for_model_commit_frame(&consumer, token);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: frame.model_commit.expect("commit token"),
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("commit standard model");
        application
            .runtime_client()
            .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
            .expect("standard model activation");

        let input = application.input_producer();
        let mut sequence = 0;
        // `0xe1` (left Shift) is the modifier case: the bundled model draws it
        // through `ShiftLeft.png`, and a binding table that misses the modifier
        // block drops it before the paw ever moves. `0xe7` (right Meta) has no
        // `MetaRight.png` and is drawn through the shared `Meta.png`.
        for (hid_usage, drawable) in [(0x04u16, true), (0xe1, true), (0xe7, true), (0x37, false)] {
            for edge in [InputEdge::Down, InputEdge::Up] {
                sequence += 1;
                let published = input
                    .publish(InputEvent::Edge {
                        control: InputControl::Key(PhysicalKey::from_hid_usage(hid_usage)),
                        edge,
                        source: InputSource::Capture,
                        at: MonotonicMillis::new(sequence),
                    })
                    .expect("key edge");
                let snapshot = application
                    .runtime_client()
                    .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
                    .expect("key projection");
                let pressed = edge == InputEdge::Down;
                let reason = format!("0x{hid_usage:02x} {edge:?}");
                assert_eq!(
                    snapshot.model_input.left_hand_down,
                    pressed && drawable,
                    "{reason} left paw"
                );
                assert!(
                    !snapshot.model_input.right_hand_down,
                    "{reason} right paw must stay up"
                );
                assert_eq!(
                    snapshot
                        .model_input
                        .key_presses
                        .iter()
                        .any(|press| press.hid_usage == hid_usage),
                    pressed && drawable,
                    "{reason} key overlay"
                );
            }
        }
        application.shutdown().expect("clean shutdown");
    }

    /// The binding test above proves the Map; this proves the press actually
    /// survives the whole path for the model that is active at runtime. F1 and
    /// F13 bracket the two HID function-key ranges.
    #[test]
    fn function_key_presses_reach_the_model_snapshot_with_the_left_hand() {
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
            .expect("prepare standard model");
        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        let frame = wait_for_model_commit_frame(&consumer, token);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: frame.model_commit.expect("commit token"),
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("commit standard model");
        application
            .runtime_client()
            .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
            .expect("standard model activation");

        let input = application.input_producer();
        let mut sequence = 0;
        for hid_usage in [0x3au16, 0x68] {
            for edge in [InputEdge::Down, InputEdge::Up] {
                sequence += 1;
                let published = input
                    .publish(InputEvent::Edge {
                        control: InputControl::Key(PhysicalKey::from_hid_usage(hid_usage)),
                        edge,
                        source: InputSource::Capture,
                        at: MonotonicMillis::new(sequence),
                    })
                    .expect("key edge");
                let snapshot = application
                    .runtime_client()
                    .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
                    .expect("key projection");
                let presses = snapshot.model_input.key_presses;
                if edge == InputEdge::Down {
                    assert!(snapshot.model_input.left_hand_down, "0x{hid_usage:02x}");
                    let press = presses
                        .iter()
                        .find(|press| press.hid_usage == hid_usage)
                        .unwrap_or_else(|| {
                            panic!("0x{hid_usage:02x} never reached the model snapshot")
                        });
                    assert_eq!(press.side, bongocat_render::KeySide::Left);
                } else {
                    assert!(
                        !presses.iter().any(|press| press.hid_usage == hid_usage),
                        "0x{hid_usage:02x} must be released"
                    );
                }
            }
        }
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn shutdown_results_preserve_single_failures_and_aggregate_dual_failures() {
        let runtime = combine_shutdown_results::<()>(Err(ShutdownError::TimedOut), Ok(()))
            .expect_err("runtime failure must be returned");
        assert!(matches!(
            runtime,
            ApplicationError::Shutdown(ShutdownError::TimedOut)
        ));

        let audio = combine_shutdown_results(Ok(()), Err(MotionAudioShutdownError::TimedOut))
            .expect_err("audio failure must be returned");
        assert!(matches!(audio, ApplicationError::MotionAudioShutdown(_)));

        let dual = combine_shutdown_results::<()>(
            Err(ShutdownError::WorkerPanicked),
            Err(MotionAudioShutdownError::TimedOut),
        )
        .expect_err("both service failures must be retained");
        match dual {
            ApplicationError::ShutdownAggregate(error) => {
                assert_eq!(error.runtime, ShutdownError::WorkerPanicked);
                assert_eq!(error.motion_audio, MotionAudioShutdownError::TimedOut);
                assert_eq!(
                    error.to_string(),
                    "runtime: runtime worker panicked; motion audio: motion audio shutdown timed out"
                );
            }
            other => panic!("unexpected shutdown error: {other}"),
        }
    }

    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
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

    /// The platform's command modifier, spelled the way the canonical chord
    /// strings spell it.
    fn behavior_shortcut_primary_name() -> &'static str {
        if cfg!(target_os = "macos") {
            "Meta"
        } else {
            "Control"
        }
    }

    /// The same modifier as a config bit set, for the one test that drives the
    /// runtime instead of reading a chord string back.
    fn behavior_shortcut_primary_modifiers() -> bongocat_config::ShortcutModifiers {
        let bits = if cfg!(target_os = "macos") {
            bongocat_config::ShortcutModifiers::META
        } else {
            bongocat_config::ShortcutModifiers::CONTROL
        };
        bongocat_config::ShortcutModifiers::from_bits(bits).expect("valid modifiers")
    }

    /// The legacy implementation auto-assigned a chord to every motion and
    /// expression the moment a model loaded, which is what makes its shortcuts
    /// page show a default in every row instead of an empty field. The Native
    /// rewrite does the same on activation, and the assignment rides on the
    /// same commit that selects the model.
    #[test]
    fn activating_a_model_fills_in_the_legacy_default_behavior_shortcuts() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            true,
            Language::EnglishUnitedStates,
        )
        .expect("start rendering application");
        assert!(
            application.config().shortcuts.model_behaviors.is_empty(),
            "a fresh configuration binds no model behaviour"
        );

        let token = application
            .prepare_model(ModelOrigin::Preset, "standard")
            .expect("prepare standard model");
        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        let frame = wait_for_model_commit_frame(&consumer, token);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: frame.model_commit.expect("commit token"),
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("commit standard model");
        application
            .runtime_client()
            .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
            .expect("standard model activation");

        // `standard` declares four motions in two groups and three
        // expressions. The legacy ordering walks motions before expressions,
        // so the seven behaviours land on the first seven digit slots.
        let bindings = application.config().shortcuts.model_behaviors.clone();
        assert_eq!(bindings.len(), 7);
        assert!(
            bindings
                .iter()
                .all(|binding| binding.model_id == "standard")
        );
        let primary = behavior_shortcut_primary_name();
        for (behavior_id, slot) in [
            ("motion:CAT_motion:0", 1),
            ("motion:CAT_motion:1", 2),
            ("motion:CAT_motion_lock:0", 3),
            ("motion:CAT_motion_lock:1", 4),
            ("expression:live2d_expression0.exp3.json", 5),
            ("expression:live2d_expression1.exp3.json", 6),
            ("expression:live2d_expression2.exp3.json", 7),
        ] {
            let binding = bindings
                .iter()
                .find(|binding| binding.behavior_id == behavior_id)
                .unwrap_or_else(|| panic!("{behavior_id} has no default binding"));
            assert_eq!(
                binding.shortcut,
                format!("{primary}+{slot}"),
                "{behavior_id}"
            );
        }

        // The chords are persisted, but the switch still gates whether the
        // platform adapters see them: a fresh v1 configuration leaves model
        // behaviour shortcuts off until the user opts in.
        assert!(!application.config().model.enable_behavior_shortcuts);
        let modifiers = behavior_shortcut_primary_modifiers();
        assert!(
            application
                .shortcut_table()
                .load()
                .resolve(modifiers, "1")
                .is_none()
        );
        application
            .set_behavior_shortcuts_enabled(true)
            .expect("enable behaviour shortcuts");
        assert!(
            application
                .shortcut_table()
                .load()
                .resolve(modifiers, "1")
                .is_some()
        );
        application.shutdown().expect("clean shutdown");
    }

    /// The command gate is a projection too: switching it off stops the
    /// recorded command chords from reaching the platform table without
    /// rewriting the configuration, so switching it back on restores them.
    /// The model behaviour gate next to it stays untouched.
    #[test]
    fn the_command_gate_keeps_the_recorded_bindings_and_only_leaves_the_table() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        // Control+Alt+0 sits outside the primary tier, so the expectation below
        // reads the same on macOS and Windows.
        application
            .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
                commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Alt+0".to_owned(),
                }],
                ..bongocat_ui_protocol::SettingsShortcuts::default()
            })
            .expect("persist user shortcuts");
        let modifiers = bongocat_config::ShortcutModifiers::from_bits(
            bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::ALT,
        )
        .expect("valid modifiers");
        let resolves = |application: &Application| {
            application
                .shortcut_table()
                .load()
                .resolve(modifiers, "0")
                .is_some()
        };
        assert!(resolves(&application), "a recorded command is registered");
        let behavior_gate = application.config().model.enable_behavior_shortcuts;

        application
            .set_command_shortcuts_enabled(false)
            .expect("disable command shortcuts");
        assert!(
            !resolves(&application),
            "the gate must empty the platform table"
        );
        assert_eq!(application.config().shortcuts.commands.len(), 1);
        assert!(!application.config().shortcuts.commands_enabled);
        assert_eq!(
            application.config().model.enable_behavior_shortcuts,
            behavior_gate,
            "the model behaviour gate is a separate switch"
        );
        assert!(
            std::fs::read_to_string(&layout.config)
                .expect("persisted config")
                .contains("\"commands_enabled\": false")
        );

        application
            .set_command_shortcuts_enabled(true)
            .expect("enable command shortcuts");
        assert!(
            resolves(&application),
            "re-enabling must not need the chord recorded again"
        );
        application.shutdown().expect("clean shutdown");
    }

    /// A default is only a starting point: the assignment never rewrites a
    /// binding the user recorded, and the behaviours they have not touched are
    /// still filled in around it.
    #[test]
    fn selecting_a_model_keeps_the_behavior_bindings_the_user_recorded() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        // Both chords sit outside the primary tier, so the expectation below
        // reads the same on macOS and Windows.
        application
            .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
                commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Alt+0".to_owned(),
                }],
                model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "motion:CAT_motion:0".to_owned(),
                    shortcut: "Control+Alt+9".to_owned(),
                }],
            })
            .expect("persist user shortcuts");
        let revision_before = application.config_revision();

        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select standard model");

        let bindings = application.config().shortcuts.model_behaviors.clone();
        assert_eq!(bindings.len(), 7);
        let chord = |behavior_id: &str| {
            bindings
                .iter()
                .find(|binding| binding.behavior_id == behavior_id)
                .map(|binding| binding.shortcut.clone())
        };
        assert_eq!(
            chord("motion:CAT_motion:0"),
            Some("Control+Alt+9".to_owned()),
            "the recorded binding must survive the auto-assignment"
        );
        let primary = behavior_shortcut_primary_name();
        assert_eq!(
            chord("motion:CAT_motion:1"),
            Some(format!("{primary}+1")),
            "the first free slot is the primary tier's first digit"
        );
        assert_eq!(
            chord("motion:CAT_motion_lock:0"),
            Some(format!("{primary}+2"))
        );
        assert_eq!(
            chord("expression:live2d_expression2.exp3.json"),
            Some(format!("{primary}+6"))
        );
        assert_eq!(
            application.config().shortcuts.commands.len(),
            1,
            "application commands are never rewritten"
        );
        // A config revision is a hash of the persisted document, not a counter,
        // so the only question it can answer is whether the document changed.
        assert!(
            application.config_revision() != revision_before,
            "the assignment commits with the selection"
        );
        application.shutdown().expect("clean shutdown");
    }

    /// "Clear all shortcuts" empties the configuration, but only the
    /// application command half is durable: the model behaviour half comes back
    /// on the next activation, because the auto-assignment fills every
    /// behaviour that has no binding and runs on every activation.
    ///
    /// This is the legacy behaviour — it re-assigned on every model load with
    /// no way to opt out — and it is why the model behaviour switch, not
    /// clearing, is how a user stops those chords from firing. The test exists
    /// so a future change to either half is a deliberate decision rather than a
    /// surprise.
    #[test]
    fn clearing_all_shortcuts_does_not_survive_the_next_activation() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select standard model");
        assert_eq!(application.config().shortcuts.model_behaviors.len(), 7);

        // What the page's "Clear all shortcuts" button sends.
        application
            .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts::default())
            .expect("clear all shortcuts");
        assert!(application.config().shortcuts.commands.is_empty());
        assert!(application.config().shortcuts.model_behaviors.is_empty());

        // Re-activating the model is what startup does on the next launch.
        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("re-activate standard model");
        assert!(application.config().shortcuts.commands.is_empty());
        assert_eq!(
            application.config().shortcuts.model_behaviors.len(),
            7,
            "the model behaviour defaults are re-assigned by activation"
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_compiles_committed_shortcuts_for_platform_adapters() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        // A model behavior binding belongs to a model, and only the model that
        // is live reaches the platform table, so this test has to activate one
        // before its half of the table means anything.
        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select standard model");
        // A fresh v1 configuration leaves model behaviour shortcuts off, so the
        // enabled half of this test has to opt in before recording the binding.
        application
            .set_behavior_shortcuts_enabled(true)
            .expect("enable behavior shortcuts");
        let shortcuts = bongocat_ui_protocol::SettingsShortcuts {
            commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "ctrl+shift+b".to_owned(),
            }],
            model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
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
            .select_model(ModelOrigin::Preset, "standard")
            .expect("re-select standard model");
        restarted
            .set_behavior_shortcuts_enabled(true)
            .expect("re-enable behavior shortcuts");
        let reenabled = restarted.shortcut_table().load();
        assert!(reenabled.resolve(modifiers, "B").is_some());
        assert!(reenabled.resolve(alt, "M").is_some());
        restarted.shutdown().expect("clean restarted shutdown");
    }

    /// The behavior half of the platform table belongs to one model at a time:
    /// the model being switched to answers its own shortcuts immediately, and
    /// the model being left stops answering its chords. Every model also counts
    /// its own defaults from the first digit, so the two models legitimately
    /// hold the same chords — which is only sound while the other half is not
    /// registered.
    #[test]
    fn switching_models_swaps_the_behavior_half_of_the_shortcut_table() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        application
            .set_behavior_shortcuts_enabled(true)
            .expect("enable behavior shortcuts");
        // Recorded before any model is active, so `standard` owns a chord no
        // other model will be handed: outside the primary tier, so the
        // expectation below reads the same on macOS and Windows.
        application
            .set_shortcuts(bongocat_ui_protocol::SettingsShortcuts {
                commands: Vec::new(),
                model_behaviors: vec![bongocat_ui_protocol::SettingsModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "motion:CAT_motion:0".to_owned(),
                    shortcut: "Control+Alt+9".to_owned(),
                }],
            })
            .expect("persist a recorded shortcut");

        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select standard model");
        application
            .select_model(ModelOrigin::Preset, "keyboard")
            .expect("select keyboard model");

        let primary = behavior_shortcut_primary_name();
        for model_id in ["standard", "keyboard"] {
            let chords = application
                .config()
                .shortcuts
                .model_behaviors
                .iter()
                .filter(|binding| binding.model_id == model_id)
                .map(|binding| binding.shortcut.as_str())
                .collect::<Vec<_>>();
            assert!(
                chords.contains(&format!("{primary}+1").as_str()),
                "{model_id} counts its defaults from the first digit: {chords:?}"
            );
        }

        let compiled = application.shortcut_table().load();
        let behavior_targets = compiled
            .iter()
            .filter_map(|shortcut| match shortcut.target() {
                bongocat_config::ShortcutTarget::ModelBehavior { model_id, .. } => {
                    Some(model_id.as_str())
                }
                bongocat_config::ShortcutTarget::Application(_) => None,
            })
            .collect::<Vec<_>>();
        assert!(
            !behavior_targets.is_empty(),
            "the live model's behaviors are registered"
        );
        assert!(
            behavior_targets
                .iter()
                .all(|model_id| *model_id == "keyboard"),
            "only the live model is registered, not the one being left: {behavior_targets:?}"
        );

        // The chord the model being left owned no longer reaches anything.
        let recorded = bongocat_config::ShortcutModifiers::from_bits(
            bongocat_config::ShortcutModifiers::CONTROL | bongocat_config::ShortcutModifiers::ALT,
        )
        .expect("valid modifiers");
        assert!(
            compiled.resolve(recorded, "9").is_none(),
            "the previous model's chords must stop working on the switch"
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn shortcut_capture_suspends_a_binding_without_persisting_and_restores_it_on_cancel() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let shortcuts = bongocat_ui_protocol::SettingsShortcuts {
            commands: vec![bongocat_ui_protocol::SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Meta+L".to_owned(),
            }],
            model_behaviors: Vec::new(),
        };
        application
            .set_shortcuts(shortcuts.clone())
            .expect("persist shortcut");
        let persisted_before_capture =
            std::fs::read(&layout.config).expect("read persisted config");
        let meta =
            bongocat_config::ShortcutModifiers::from_bits(bongocat_config::ShortcutModifiers::META)
                .expect("valid modifier");
        assert!(
            application
                .shortcut_table()
                .load()
                .resolve(meta, "L")
                .is_some()
        );

        application
            .suspend_shortcut_capture(bongocat_ui_protocol::SettingsShortcuts::default())
            .expect("suspend shortcut");
        assert!(
            application
                .shortcut_table()
                .load()
                .resolve(meta, "L")
                .is_none()
        );
        assert_eq!(
            std::fs::read(&layout.config).expect("config remains unchanged"),
            persisted_before_capture
        );

        application
            .resume_shortcut_capture()
            .expect("restore shortcut");
        assert!(
            application
                .shortcut_table()
                .load()
                .resolve(meta, "L")
                .is_some()
        );
        application.shutdown().expect("clean shutdown");
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
            corner_radius_percent: 25,
            hide_on_pointer_hover: true,
            hide_on_pointer_hover_delay_seconds: 1,
            keep_inside_screen: false,
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
        assert!(application.config().overlay.hide_on_pointer_hover);
        assert_eq!(
            application
                .config()
                .overlay
                .hide_on_pointer_hover_delay_seconds,
            1
        );
        assert!(!application.config().overlay.keep_inside_screen);

        application
            .set_appearance_theme(ConfigTheme::Dark)
            .expect("update appearance theme");
        assert_eq!(application.config().appearance.theme, ConfigTheme::Dark);

        let audio_snapshot = application
            .set_motion_audio_enabled(true)
            .expect("enable motion audio");
        assert!(audio_snapshot.motion_audio_enabled);
        assert!(application.config().model.play_motion_audio);

        let frame_rate_snapshot = application
            .set_maximum_fps(120)
            .expect("update maximum FPS");
        assert_eq!(frame_rate_snapshot.maximum_fps, 120);
        assert_eq!(application.config().model.maximum_fps, 120);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
        assert!(persisted.contains("\"play_motion_audio\": true"));
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
        assert!(restarted.config().overlay.visible);
        restarted.shutdown().expect("clean restart shutdown");
    }

    /// Hiding the model window is a per-session choice: the overlay always
    /// starts visible and a persisted hidden overlay never survives a restart.
    #[test]
    fn startup_forces_the_overlay_visible_like_the_legacy_window_state() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.visible = false;
        store.commit(&config).expect("hidden config commit");

        let application =
            Application::start_with_layout(layout.clone()).expect("start application");
        assert!(application.config().overlay.visible);
        assert!(application.runtime_client().snapshot().overlay_visible);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("\"visible\": true"));
        application.shutdown().expect("clean shutdown");

        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(restarted.config().overlay.visible);
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
        config.overlay.opacity_percent = 87;
        store.commit(&config).expect("older config commit");
        config.overlay.opacity_percent = 93;
        store.commit(&config).expect("newer config commit");
        std::fs::write(&layout.config, b"corrupt-current").expect("corrupt current config");

        let application = Application::start_with_layout(layout.clone()).expect("recover startup");
        assert_eq!(application.config().overlay.opacity_percent, 87);
        assert!(application.runtime_client().snapshot().overlay_visible);
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
    fn application_uses_defaults_when_current_and_backups_are_invalid() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        ConfigStore::new(layout.clone()).expect("config store");
        std::fs::create_dir_all(
            layout
                .config
                .parent()
                .expect("configuration parent directory"),
        )
        .expect("configuration directory");
        std::fs::write(&layout.config, b"invalid-current").expect("invalid current config");

        let application = Application::start_with_layout(layout.clone()).expect("default startup");
        assert_eq!(application.config(), &NativeConfig::default());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_starts_from_interrupted_config_without_exposing_storage_details() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut current = store.load_or_default().expect("default config").config;
        let interrupted_bytes = std::fs::read(&layout.config).expect("committed config bytes");
        current.overlay.opacity_percent = 87;
        store.commit(&current).expect("current config commit");
        std::fs::write(layout.config.with_extension("json.tmp"), interrupted_bytes)
            .expect("interrupted config temp");

        let application = Application::start_with_layout(layout.clone()).expect("recover startup");
        assert_eq!(application.config().overlay.opacity_percent, 87);
        assert!(!layout.config.with_extension("json.tmp").exists());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn development_and_production_applications_never_share_roots() {
        let base = tempdir().expect("temp directory");
        let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let development_root = development.root.clone();
        let production_root = production.root.clone();
        let development_logs = development.logs.clone();
        let production_logs = production.logs.clone();

        let mut development_app =
            Application::start_with_layout(development).expect("development application");
        let mut production_app =
            Application::start_with_layout(production).expect("production application");

        assert!(development_root.join("config.json").is_file());
        assert!(production_root.join("config.json").is_file());
        assert_ne!(development_root, production_root);

        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        import_one(&mut development_app, "same-id", &source);
        import_one(&mut production_app, "same-id", source);
        let development_ids = installed_catalog_ids(&development_app);
        let production_ids = installed_catalog_ids(&production_app);
        assert_eq!(development_ids.len(), 1);
        assert_eq!(production_ids.len(), 1);
        // Store keys are UUIDs generated inside each environment, so the same
        // import hint never produces the same identity across environments.
        assert_ne!(development_ids, production_ids);
        assert!(
            development_root
                .join("models")
                .join(&development_ids[0])
                .is_dir()
        );
        assert!(
            production_root
                .join("models")
                .join(&production_ids[0])
                .is_dir()
        );
        development_app.record_log(ApplicationLogEvent::shutdown_failed());
        production_app.record_log(ApplicationLogEvent::panicked());
        assert_ne!(development_logs, production_logs);
        let development_log = std::fs::read_to_string(
            std::fs::read_dir(&development_logs)
                .expect("development logs")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with("application-") && name.ends_with(".jsonl")
                    })
                })
                .expect("development application log"),
        )
        .expect("development log contents");
        let production_log = std::fs::read_to_string(
            std::fs::read_dir(&production_logs)
                .expect("production logs")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with("application-") && name.ends_with(".jsonl")
                    })
                })
                .expect("production application log"),
        )
        .expect("production log contents");
        assert!(development_log.contains("shutdown_failed"));
        assert!(!development_log.contains("panicked"));
        assert!(production_log.contains("panicked"));
        assert!(!production_log.contains("shutdown_failed"));
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

        let active_id = import_one(&mut application, "active", fixtures.join("非 ASCII 模型"))
            .id()
            .as_str()
            .to_owned();
        let broken_id = import_one(&mut application, "broken", fixtures.join("非 ASCII 模型"))
            .id()
            .as_str()
            .to_owned();

        let active = application
            .select_model(ModelOrigin::Installed, active_id.as_str())
            .expect("activate valid model");
        let active_revision = active.revision;
        assert_eq!(
            active
                .active_model
                .as_ref()
                .expect("active model")
                .id
                .as_str(),
            active_id
        );

        std::fs::remove_file(models_root.join(&broken_id).join("模型 数据.moc3"))
            .expect("corrupt installed model");

        let error = application
            .select_model(ModelOrigin::Installed, broken_id.as_str())
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

        let imported = import_one(&mut application, "unicode", source);
        assert_eq!(
            imported.root(),
            models_root
                .canonicalize()
                .expect("canonical models root")
                .join(imported.id().as_str())
        );
        assert!(imported.root().join("猫.model3.json").is_file());

        let catalog = application.model_catalog().expect("model catalog");
        assert!(catalog.iter().any(|entry| {
            entry.origin() == bongocat_model::ModelOrigin::Installed && entry.id() == imported.id()
        }));

        application
            .delete_model(ModelOrigin::Installed, imported.id().as_str())
            .expect("delete model");
        assert!(installed_catalog_ids(&application).is_empty());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn import_hints_become_titles_while_ids_stay_generated_uuids() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

        let first = import_one(&mut application, "我的猫", source.clone());
        let second = import_one(&mut application, "我的猫", source);
        assert_ne!(first.id(), second.id(), "ids are independent UUIDs");

        let installed = installed_catalog_ids(&application);
        assert_eq!(installed.len(), 2);
        assert_eq!(
            application.config().model.installed_models,
            vec![
                ModelMetadata {
                    id: first.id().as_str().to_owned(),
                    title: "我的猫".to_owned(),
                },
                ModelMetadata {
                    id: second.id().as_str().to_owned(),
                    title: "我的猫".to_owned(),
                },
            ]
        );

        application
            .delete_model(ModelOrigin::Installed, first.id().as_str())
            .expect("delete first model");
        assert_eq!(
            application.config().model.installed_models,
            vec![ModelMetadata {
                id: second.id().as_str().to_owned(),
                title: "我的猫".to_owned(),
            }]
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn missing_selected_model_falls_back_to_the_standard_preset_at_startup() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut configured = store.load_or_default().expect("default config").config;
        configured.model.selected_model_id = Some("ghost".to_owned());
        configured.model.selected_model_origin = Some(SelectedModelOrigin::Installed);
        configured.model.installed_models = vec![ModelMetadata {
            id: "ghost".to_owned(),
            title: "幽灵模型".to_owned(),
        }];
        store.commit(&configured).expect("seed selection");

        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        // Without a render consumer the fallback cannot finish activation, but
        // the corrected selection must already be persisted and logged.
        assert!(matches!(
            application.restore_startup_model(),
            Err(ApplicationError::RenderConsumerUnavailable)
        ));
        assert_eq!(
            application.config().model.selected_model_id,
            Some("standard".to_owned())
        );
        assert_eq!(
            application.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Preset)
        );
        assert_eq!(
            application
                .application_log_diagnostics()
                .events
                .model_selection_fallback,
            1
        );
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("\"standard\""));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn metadata_records_for_missing_model_directories_are_pruned_at_startup() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut configured = store.load_or_default().expect("default config").config;
        configured.model.installed_models = vec![
            ModelMetadata {
                id: "ghost".to_owned(),
                title: "被手动删除".to_owned(),
            },
            ModelMetadata {
                id: "still-there".to_owned(),
                title: "目录仍在".to_owned(),
            },
        ];
        store.commit(&configured).expect("seed metadata");
        std::fs::create_dir_all(layout.models.join("still-there"))
            .expect("model directory present");

        let application = Application::start_with_layout(layout).expect("start application");
        assert_eq!(
            application.config().model.installed_models,
            vec![ModelMetadata {
                id: "still-there".to_owned(),
                title: "目录仍在".to_owned(),
            }]
        );
        application.shutdown().expect("clean shutdown");
    }

    /// Regression for a model deleted by hand in the file manager: browsing the
    /// models root leaves `.DS_Store` behind, and that single foreign file used
    /// to fail the whole catalog scan, which turned the Models page into an
    /// unusable "catalog unavailable" state and also stopped stale metadata from
    /// being pruned.
    #[test]
    fn hand_deleted_model_beside_file_manager_metadata_keeps_the_catalog_available() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut configured = store.load_or_default().expect("default config").config;
        configured.model.installed_models = vec![ModelMetadata {
            id: "deleted-by-hand".to_owned(),
            title: "被手动删除".to_owned(),
        }];
        store.commit(&configured).expect("seed metadata");
        std::fs::create_dir_all(&layout.models).expect("models root");
        std::fs::write(layout.models.join(".DS_Store"), b"finder metadata")
            .expect("file manager metadata");

        let application = Application::start_with_layout(layout).expect("start application");
        let catalog = application.model_catalog().expect("merged catalog");
        assert!(catalog.iter().any(|entry| {
            entry.origin() == ModelOrigin::Preset && entry.id().as_str() == "standard"
        }));
        assert!(application.config().model.installed_models.is_empty());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn merged_model_catalog_retains_source_identity_for_duplicate_ids() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        seed_installed_model(&layout.models, "standard");
        let application = Application::start_with_layout(layout).expect("start application");

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
            entries[0].origin() == ModelOrigin::Preset
                || entries[1].origin() == ModelOrigin::Installed
        }));
        application.shutdown().expect("clean shutdown");
    }

    /// The Models page lists the build's presets first, in mode order, and the
    /// imported models after them in the order they were imported.
    ///
    /// Both halves used to be one list sorted by id, which put the presets in
    /// reverse — `gamepad` < `keyboard` < `standard` alphabetically — and let a
    /// new import land anywhere among them instead of at the end. The order is
    /// state rather than a per-run accident, so this pins it across a restart
    /// and across a deletion too.
    #[test]
    fn model_catalog_lists_presets_in_mode_order_then_installed_models_in_import_order() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

        assert_eq!(
            catalog_ids(&application),
            vec!["standard", "keyboard", "gamepad"],
            "the presets are the three modes, in mode order"
        );

        let first = import_one(&mut application, "第一只猫", source.clone());
        let second = import_one(&mut application, "第二只猫", source);
        let imported = vec![
            "standard".to_owned(),
            "keyboard".to_owned(),
            "gamepad".to_owned(),
            first.id().as_str().to_owned(),
            second.id().as_str().to_owned(),
        ];
        assert_eq!(
            catalog_ids(&application),
            imported,
            "each import joins the end of the page, after every preset"
        );
        application.shutdown().expect("clean shutdown");

        let mut restarted =
            Application::start_with_layout(layout).expect("restart the application");
        assert_eq!(
            catalog_ids(&restarted),
            imported,
            "the order is configuration, not the order one run happened to build"
        );

        restarted
            .delete_model(ModelOrigin::Installed, first.id().as_str())
            .expect("delete the first import");
        assert_eq!(
            catalog_ids(&restarted),
            vec![
                "standard".to_owned(),
                "keyboard".to_owned(),
                "gamepad".to_owned(),
                second.id().as_str().to_owned(),
            ],
            "removing an import leaves the rest in place"
        );
        restarted.shutdown().expect("clean shutdown");
    }

    /// A package copied into the store root by hand never went through an
    /// import, so it has no place in the import order. It still has to appear —
    /// the store scan is what makes a user's own directory visible — and the
    /// page must not reshuffle every time the scan walks the directory in a
    /// different order, so those entries are ordered by id instead.
    #[test]
    fn hand_copied_model_lands_after_the_imports_in_id_order() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        let imported = import_one(&mut application, "导入的猫", source);

        // Deliberately seeded in the opposite order to the ids, so passing this
        // cannot come from the order the scan walked the directory in.
        seed_installed_model(&layout.models, "zz-copied-by-hand");
        seed_installed_model(&layout.models, "aa-copied-by-hand");
        assert_eq!(
            catalog_ids(&application),
            vec![
                "standard".to_owned(),
                "keyboard".to_owned(),
                "gamepad".to_owned(),
                imported.id().as_str().to_owned(),
                "aa-copied-by-hand".to_owned(),
                "zz-copied-by-hand".to_owned(),
            ]
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_selection_persists_its_origin_across_restart() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        seed_installed_model(&layout.models, "standard");
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
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
    fn deleting_the_live_installed_model_falls_back_to_the_standard_preset() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        let active_id = import_one(&mut application, "active", source)
            .id()
            .as_str()
            .to_owned();
        application
            .select_model(ModelOrigin::Installed, active_id.as_str())
            .expect("activate model");

        application
            .delete_model(ModelOrigin::Installed, active_id.as_str())
            .expect("selected model deletion switches away first");
        assert!(installed_catalog_ids(&application).is_empty());
        assert_eq!(
            application.config().model.selected_model_id,
            Some("standard".to_owned())
        );
        assert_eq!(
            application.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Preset)
        );
        assert_eq!(application.active_model_origin(), Some(ModelOrigin::Preset));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_can_be_deleted_while_same_id_preset_is_selected() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        seed_installed_model(&layout.models, "standard");
        let mut application = Application::start_with_layout(layout).expect("start application");
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
    fn configured_installed_model_deletion_falls_back_before_restart_activation() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let selected_id = import_one(&mut application, "selected", source)
            .id()
            .as_str()
            .to_owned();
        application
            .select_model(ModelOrigin::Installed, selected_id.as_str())
            .expect("select installed model");
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(restarted.runtime_client().snapshot().active_model.is_none());
        // Nothing is live yet, so the recorded selection is the only fact that
        // names this model — and it is enough on its own to switch away first.
        restarted
            .delete_model(ModelOrigin::Installed, selected_id.as_str())
            .expect("configured model deletion switches away first");
        assert!(installed_catalog_ids(&restarted).is_empty());
        assert_eq!(
            restarted.config().model.selected_model_id,
            Some("standard".to_owned())
        );
        assert_eq!(
            restarted.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Preset)
        );
        restarted.shutdown().expect("clean restart shutdown");
    }

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

    /// Seed the environment model store with a package stored under an exact
    /// id. Imports always generate UUID ids, so a store entry whose id collides
    /// with a preset id can only be produced through direct seeding; the merged
    /// catalog must still keep both identities.
    fn seed_installed_model(models_root: &Path, id: &str) {
        let destination = models_root.join(id);
        std::fs::create_dir_all(&destination).expect("seeded model directory");
        let fixture = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        for entry in std::fs::read_dir(fixture).expect("fixture entries") {
            let entry = entry.expect("fixture entry");
            std::fs::copy(entry.path(), destination.join(entry.file_name()))
                .expect("seeded package file");
        }
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

    /// Every catalog id in the order the Models page shows them.
    fn catalog_ids(application: &Application) -> Vec<String> {
        application
            .model_catalog()
            .expect("model catalog")
            .into_iter()
            .map(|entry| entry.id().as_str().to_owned())
            .collect()
    }

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

    #[test]
    fn application_reads_only_anonymous_core_log_diagnostics() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        assert_eq!(application.core_log_diagnostics(), None);

        application.set_core_log_diagnostics_provider(|| CoreLogDiagnostics {
            written: 3,
            dropped: 1,
            rotated: 2,
            pruned: 4,
            bytes: 128,
            retained_files: 2,
            retained_bytes: 192,
        });

        assert_eq!(
            application.core_log_diagnostics(),
            Some(CoreLogDiagnostics {
                written: 3,
                dropped: 1,
                rotated: 2,
                pruned: 4,
                bytes: 128,
                retained_files: 2,
                retained_bytes: 192,
            })
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_reads_only_anonymous_update_diagnostics() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        assert_eq!(application.update_diagnostics(), None);

        application.set_update_diagnostics_provider(|| bongocat_update::UpdateDiagnostics {
            last_error_code: Some("private_update_detail"),
            checks_started: 1,
            ..bongocat_update::UpdateDiagnostics::default()
        });
        assert_eq!(
            application
                .update_diagnostics()
                .expect("sanitized update diagnostics")
                .last_error_code,
            None
        );
        assert_eq!(
            application
                .update_diagnostics()
                .expect("sanitized update diagnostics")
                .checks_started,
            1
        );

        application.set_update_diagnostics_provider(|| bongocat_update::UpdateDiagnostics {
            last_error_code: Some("update_download_transport_failed"),
            checks_started: 3,
            checks_succeeded: 2,
            checks_failed: 1,
            downloads_started: 2,
            downloads_succeeded: 1,
            downloads_failed: 1,
            installs_started: 1,
            installs_succeeded: 0,
            installs_failed: 1,
        });

        assert_eq!(
            application.update_diagnostics(),
            Some(bongocat_update::UpdateDiagnostics {
                last_error_code: Some("update_download_transport_failed"),
                checks_started: 3,
                checks_succeeded: 2,
                checks_failed: 1,
                downloads_started: 2,
                downloads_succeeded: 1,
                downloads_failed: 1,
                installs_started: 1,
                installs_succeeded: 0,
                installs_failed: 1,
            })
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_registers_shared_update_diagnostics_tracker() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let tracker = bongocat_update::UpdateDiagnosticsTracker::default();
        application.set_update_diagnostics_tracker(tracker.clone());

        tracker.record_check_started();
        tracker.record_check_succeeded();
        tracker.record_download_failed("update_download_transport_failed");

        assert_eq!(
            application.update_diagnostics(),
            Some(bongocat_update::UpdateDiagnostics {
                last_error_code: Some("update_download_transport_failed"),
                checks_started: 1,
                checks_succeeded: 1,
                checks_failed: 0,
                downloads_started: 0,
                downloads_succeeded: 0,
                downloads_failed: 1,
                installs_started: 0,
                installs_succeeded: 0,
                installs_failed: 0,
            })
        );
        application.shutdown().expect("clean shutdown");
    }
}
