//! Bringing the application up: storage, configuration, audio and the runtime.
//!
//! Every step that can fail records what it was doing before it gives up, because
//! the failures that reach the user — an unwritable store, a corrupt
//! configuration, a catalog that will not open — are otherwise indistinguishable
//! from each other in the log.

use super::Application;
use crate::app_log::{
    ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent, ApplicationLogHandle,
};
use crate::config_projection::{
    gamepad_axis_settings_from_config, model_settings_from_config, overlay_settings_from_config,
    random_behavior_settings_from_config, runtime_log_settings,
};
use crate::model_identity::model_origin_from_config;
use crate::shortcut_config::active_shortcuts;
use crate::{
    AUDIO_COMMAND_CAPACITY, ApplicationError, BUILD_ENVIRONMENT, COMMAND_CAPACITY, RUNTIME_TIMEOUT,
};
use bongocat_audio::MotionAudioService;
use bongocat_config::{
    ConfigError, ConfigStore, Language, ShortcutTable, StorageLayout, WindowStateLoadStatus,
    WindowStateStore, platform_layout,
};
use bongocat_log::LogSettings as RuntimeLogSettings;
use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_model_store::{ModelStore, PresetCoverStore};
use bongocat_runtime::{RuntimeCommand, RuntimeOwner};
use std::path::Path;

#[cfg(test)]
use crate::tests::repository_preset_root;

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
    pub(crate) fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            false,
            Language::EnglishUnitedStates,
        )
    }

    pub(crate) fn start_with_layout_internal(
        layout: StorageLayout,
        preset_root: &Path,
        enable_rendering: bool,
        system_language: Language,
    ) -> Result<Self, ApplicationError> {
        // Install the app-owned sink before opening user-data stores so their
        // bounded failures are observable. Historical cleanup stays deferred
        // until the persisted logging policy has been loaded.
        let application_log = ApplicationLogHandle::install_with_settings(
            &layout.logs,
            RuntimeLogSettings::default(),
            true,
        )?;
        let (run_marker, previous_run) = match application_log.begin_run() {
            Ok(result) => result,
            Err(error) => {
                application_log.record(
                    ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                        .with_context(ApplicationLogContext::Phase("run_marker"))
                        .with_context(ApplicationLogContext::Reason("run_marker_failed")),
                );
                return Err(error.into());
            }
        };
        let preset_models =
            match PresetModelCatalog::open(preset_root, ModelPackageLimits::default()) {
                Ok(catalog) => catalog,
                Err(error) => {
                    application_log.record(
                        ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                            .with_context(ApplicationLogContext::Phase("preset_catalog"))
                            .with_context(ApplicationLogContext::Reason(
                                "preset_catalog_open_failed",
                            )),
                    );
                    return Err(error.into());
                }
            };
        let model_store = match ModelStore::new(
            &layout.models,
            layout.locks.join("models.writer.lock"),
            ModelPackageLimits::default(),
        ) {
            Ok(store) => store,
            Err(error) => {
                application_log.record(
                    ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                        .with_context(ApplicationLogContext::Phase("model_store"))
                        .with_context(ApplicationLogContext::Reason(
                            "model_store_initialization_failed",
                        )),
                );
                return Err(error.into());
            }
        };
        let preset_covers = match PresetCoverStore::open(layout.model_overrides.clone()) {
            Ok(store) => store,
            Err(error) => {
                application_log.record(
                    ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                        .with_context(ApplicationLogContext::Phase("preset_cover_store"))
                        .with_context(ApplicationLogContext::Reason(
                            "preset_cover_store_initialization_failed",
                        )),
                );
                return Err(error.into());
            }
        };
        let config_store = match ConfigStore::new(layout.clone()) {
            Ok(store) => store,
            Err(error) => {
                application_log.record(
                    ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                        .with_context(ApplicationLogContext::Phase("config_store"))
                        .with_context(ApplicationLogContext::Reason(
                            "config_store_initialization_failed",
                        )),
                );
                return Err(error.into());
            }
        };
        let window_state_store = WindowStateStore::new(layout);
        let window_state_outcome = window_state_store.load_or_default();
        let recovered_window_source = match window_state_outcome.status {
            WindowStateLoadStatus::Loaded | WindowStateLoadStatus::Missing => None,
            WindowStateLoadStatus::IgnoredInvalid => Some("invalid"),
            WindowStateLoadStatus::IgnoredUnsupportedSchema(_) => Some("unsupported_schema"),
            WindowStateLoadStatus::IgnoredIo => Some("io"),
        };
        if let Some(source) = recovered_window_source {
            application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::StateRecovered)
                    .with_context(ApplicationLogContext::State("window_state"))
                    .with_context(ApplicationLogContext::Source(source)),
            );
        }
        if matches!(
            window_state_outcome.status,
            WindowStateLoadStatus::IgnoredInvalid
                | WindowStateLoadStatus::IgnoredUnsupportedSchema(_)
        ) {
            application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::ParsingFailed)
                    .with_context(ApplicationLogContext::Operation("window_state_load"))
                    .with_context(ApplicationLogContext::Reason("invalid_state")),
            );
        } else if matches!(
            window_state_outcome.status,
            WindowStateLoadStatus::IgnoredIo
        ) {
            application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::FilesystemOperationFailed)
                    .with_context(ApplicationLogContext::Operation("window_state_load"))
                    .with_context(ApplicationLogContext::Reason("io")),
            );
        }
        let window_state = window_state_outcome.state;
        let loaded = match config_store.load_or_default() {
            Ok(loaded) => loaded,
            Err(error) => {
                let event = match &error {
                    ConfigError::Io(_) => {
                        ApplicationLogEvent::new(ApplicationLogCode::FilesystemOperationFailed)
                            .with_context(ApplicationLogContext::Operation("config_load"))
                            .with_context(ApplicationLogContext::Reason("io"))
                    }
                    ConfigError::Json(_)
                    | ConfigError::InvalidValue(_)
                    | ConfigError::UnsupportedSchema(_) => {
                        ApplicationLogEvent::new(ApplicationLogCode::ParsingFailed)
                            .with_context(ApplicationLogContext::Operation("config_load"))
                            .with_context(ApplicationLogContext::Reason("invalid_config"))
                    }
                    _ => ApplicationLogEvent::new(ApplicationLogCode::StartupFailed)
                        .with_context(ApplicationLogContext::Phase("config_load"))
                        .with_context(ApplicationLogContext::Reason("config_load_failed")),
                };
                application_log.record(event);
                return Err(error.into());
            }
        };
        if let Some(recovery) = loaded.recovery {
            application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::StateRecovered)
                    .with_context(ApplicationLogContext::State("config"))
                    .with_context(ApplicationLogContext::Source("backup"))
                    .with_context(ApplicationLogContext::Count(u64::from(
                        recovery.skipped_newer_backups(),
                    ))),
            );
        } else if loaded.interrupted_recovery.is_some() {
            application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::StateRecovered)
                    .with_context(ApplicationLogContext::State("config"))
                    .with_context(ApplicationLogContext::Source("interrupted_temp")),
            );
        }
        application_log.replace_settings(runtime_log_settings(&loaded.config.logging));
        let config = loaded.config;
        let config_revision = Some(loaded.revision);
        let configured_model = config.model.selected_model.as_ref();
        let shortcut_table = ShortcutTable::new(active_shortcuts(&config, configured_model)?);
        let (motion_audio, motion_audio_client) =
            match MotionAudioService::start(AUDIO_COMMAND_CAPACITY) {
                Ok(service) => {
                    let client = service.client();
                    (Some(service), client)
                }
                Err(_) => {
                    application_log.record(
                        ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                            .with_context(ApplicationLogContext::Service("audio"))
                            .with_context(ApplicationLogContext::Reason("audio_start_failed")),
                    );
                    (None, bongocat_audio::MotionAudioClient::unavailable())
                }
            };
        let runtime_overlay_visible = true;
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
            .send(RuntimeCommand::SetMaximumFps(config.overlay.maximum_fps))
            .map_err(ApplicationError::RuntimeCommand)?;
        client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        let sequence = client
            .send(RuntimeCommand::SetReleaseFallbackTimeout(
                config.input.keyboard.release_fallback_timeout_ms,
            ))
            .map_err(ApplicationError::RuntimeCommand)?;
        client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        let sequence = client
            .send(RuntimeCommand::SetRandomBehaviorSettings(
                random_behavior_settings_from_config(&config),
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
        let active_model_origin =
            configured_model.map(|selected| model_origin_from_config(selected.source));
        let active_model_id =
            configured_model.and_then(|selected| ModelId::parse(&selected.id).ok());
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
            last_gamepad_model: None,
            last_other_model: None,
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
}

fn system_language() -> Language {
    bongocat_platform::system_language()
}
