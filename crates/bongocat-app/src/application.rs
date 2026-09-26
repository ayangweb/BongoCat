//! The application facade: the one owner of configuration, model and runtime state.
//!
//! The struct and the accessors every caller needs live here. The work is split
//! across this directory by responsibility — startup, observability, window
//! placement, the settings commands that commit configuration, the model catalog,
//! model activation, model import, the startup selection restore and shutdown.
//! One inherent impl spanning all of them is what made the crate root
//! unreadable, so each of those is now an ordinary `impl Application` block in its
//! own module. The public surface is exactly what it was.

use crate::ApplicationError;
use crate::app_log::{
    ApplicationLogHandle, ApplicationPanicHook, ApplicationRunMarker, CoreLogDiagnostics,
};
use crate::model_identity::config_source_from_model;
use crate::shortcut_config::active_shortcuts;
use bongocat_audio::MotionAudioService;
use bongocat_config::{
    CompiledShortcuts, ConfigRevision, ConfigStore, Language, ModelIdentity, ModelInputMode,
    NativeConfig, ShortcutTable, WindowState, WindowStateStore,
};
use bongocat_input::{CursorProducer, GamepadAxisProducer, InputProducer};
use bongocat_model::{ModelId, ModelOrigin, PresetModelCatalog};
use bongocat_model_store::{ModelStore, PresetCoverStore};
use bongocat_render::RenderConsumer;
use bongocat_runtime::{RuntimeClient, RuntimeOwner};
use bongocat_update::UpdateDiagnostics;
use std::sync::Arc;

mod diagnostics;
mod model_activation;
mod model_catalog;
mod model_import;
mod settings_commands;
mod shutdown;
mod startup;
mod startup_restore;
mod window_state;

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
    /// The last model activated from the gamepad-mode catalog, and the last one
    /// activated from any other mode. This is what `gamepad_auto_switch` means by
    /// "the last model used": session memory rather than configuration, because it
    /// is a fact about what happened, not a preference the user has to maintain.
    last_gamepad_model: Option<ModelIdentity>,
    last_other_model: Option<ModelIdentity>,
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
        active_shortcuts(&self.config, self.live_model_identity().as_ref())
            .map_err(ApplicationError::Config)
    }

    /// The model identity whose behavior bindings are live: the one the runtime
    /// is showing. This is tracked on the application instead of being read from
    /// the persisted selection, because startup may activate the default model
    /// before a selection has been written.
    pub(crate) fn live_model_identity(&self) -> Option<ModelIdentity> {
        Some(ModelIdentity {
            id: self.active_model_id.as_ref()?.as_str().to_owned(),
            source: self.active_model_origin.map(config_source_from_model)?,
        })
    }

    /// Record the model that has just become live for the input family it
    /// belongs to, which is what the gamepad auto switch's "last model used"
    /// targets read.
    ///
    /// A model the user picked by hand counts exactly like one an automatic
    /// switch chose: the memory is about what was on screen, not about who asked
    /// for it. A model whose mode cannot be resolved is not recorded, because
    /// there is no family to file it under.
    fn remember_live_model(&mut self) {
        let Some(identity) = self.live_model_identity() else {
            return;
        };
        let (Some(origin), Some(id)) = (
            self.active_model_origin,
            self.active_model_id.as_ref().map(ModelId::as_str),
        ) else {
            return;
        };
        match self.model_input_mode(origin, id) {
            Some(ModelInputMode::Gamepad) => self.last_gamepad_model = Some(identity),
            Some(_) => self.last_other_model = Some(identity),
            None => {}
        }
    }

    /// Rebuild the platform-facing shortcut table from the committed
    /// configuration, scoped to the model that is actually live.
    ///
    /// Best effort, like the assignments that feed it: a configuration that no
    /// longer compiles leaves the previous table in place and the platform
    /// keeps what it already registered.
    fn refresh_shortcut_table(&mut self) {
        let compiled = {
            let active_model = self.live_model_identity();
            active_shortcuts(&self.config, active_model.as_ref())
        };
        if let Ok(compiled) = compiled {
            self.shortcut_table.replace(compiled);
        }
    }

    pub fn shortcut_table(&self) -> ShortcutTable {
        self.shortcut_table.clone()
    }

    fn ready_config_revision(&self) -> Result<ConfigRevision, ApplicationError> {
        self.config_revision
            .ok_or(ApplicationError::RuntimeDidNotPublish)
    }
}
