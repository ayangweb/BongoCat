//! The settings commands.
//!
//! Each one commits configuration under the current revision and, where the
//! runtime shows the value, sends the matching runtime command. Where the runtime
//! rejects a value the configuration is rolled back, so the configuration never
//! describes a state the runtime is not in.

use super::Application;
use crate::config_projection::{
    logging_config_from_settings, persistent_dead_zone, runtime_log_settings,
};
use crate::shortcut_config::{active_shortcuts, shortcut_config_from_settings};
use crate::{ApplicationError, RUNTIME_TIMEOUT};
use bongocat_config::{Language, Theme as ConfigTheme};
use bongocat_input::GamepadAxisSettings;
use bongocat_runtime::{
    ModelSettings, OverlaySettings, RandomBehaviorSettings, RuntimeCommand, RuntimeCommandFailure,
    RuntimeRenderErrorCode, RuntimeSnapshot, maximum_fps_is_valid,
    release_fallback_timeout_is_valid,
};

impl Application {
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
        next_config.system.show_status_icon = visible;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_taskbar_icon_visible(&mut self, visible: bool) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.system.show_taskbar_icon = visible;
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
        next_config.updates.check_automatically = enabled;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_check_for_updates_interval_hours(
        &mut self,
        interval_hours: u16,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.updates.check_interval_hours = interval_hours;
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

    /// Change the overlay's visibility for this session only.
    ///
    /// Visibility is runtime state, not a user preference: a fresh process
    /// always presents the overlay, while a shortcut or settings command can
    /// hide it until shutdown. The settings worker still supplies the current
    /// config revision as a stale-view guard, but this command never writes
    /// `config.json`.
    pub fn set_overlay_visible(
        &mut self,
        visible: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetOverlayVisible(visible))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if snapshot
            .last_command_failure
            .is_some_and(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(
                snapshot
                    .last_command_failure
                    .expect("checked command failure"),
            ));
        }
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
        next_config.overlay.maximum_fps = maximum_fps;
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
        next_config.input.keyboard.release_fallback_timeout_ms = timeout_ms;
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

    pub fn set_random_behavior_settings(
        &mut self,
        settings: RandomBehaviorSettings,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        if !settings.is_valid() {
            return Err(ApplicationError::RuntimeCommandFailed(
                RuntimeCommandFailure {
                    sequence: 0,
                    code: RuntimeRenderErrorCode::RandomBehaviorSettingsInvalid,
                },
            ));
        }
        let mut next_config = self.config.clone();
        next_config.model.random_behavior.enabled = settings.enabled;
        next_config.model.random_behavior.interval_seconds = settings.interval_seconds;
        next_config.validate()?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;

        let runtime_result = (|| {
            let client = self.runtime.client();
            let sequence = client
                .send(RuntimeCommand::SetRandomBehaviorSettings(settings))
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
        })();
        match runtime_result {
            Ok(snapshot) => {
                self.config = next_config;
                self.config_revision = Some(next_revision);
                Ok(snapshot)
            }
            Err(error) => {
                let rollback_revision = self
                    .config_store
                    .commit_if_revision(&self.config, next_revision)
                    .map_err(ApplicationError::ConfigRollback)?;
                self.config_revision = Some(rollback_revision);
                Err(error)
            }
        }
    }

    pub fn set_model_settings(
        &mut self,
        settings: ModelSettings,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.mirror = settings.mirror;
        next_config.model.mirror_pointer_tracking = settings.mirror_pointer_tracking;
        next_config.model.ignore_keyboard = settings.ignore_keyboard;
        next_config.model.ignore_gamepad = settings.ignore_gamepad;
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
        next_config.input.gamepad.stick_dead_zone = persistent_dead_zone(settings.stick_dead_zone);
        next_config.input.gamepad.trigger_dead_zone =
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

    pub fn set_logging_settings(
        &mut self,
        settings: bongocat_ui_protocol::SettingsLogging,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.logging = logging_config_from_settings(settings);
        next_config.validate()?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.application_log
            .replace_settings(runtime_log_settings(&next_config.logging));
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    pub fn set_shortcuts(
        &mut self,
        shortcuts: bongocat_ui_protocol::SettingsShortcuts,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        let commands_enabled = next_config.shortcuts.commands_enabled;
        let model_behaviors_enabled = next_config.shortcuts.model_behaviors_enabled;
        next_config.shortcuts =
            shortcut_config_from_settings(shortcuts, commands_enabled, model_behaviors_enabled);
        next_config.shortcuts = next_config.shortcuts.canonicalized()?;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_identity().as_ref())?;
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
        let model_behaviors_enabled = temporary.shortcuts.model_behaviors_enabled;
        temporary.shortcuts = shortcut_config_from_settings(
            shortcuts_without_capture_target,
            commands_enabled,
            model_behaviors_enabled,
        );
        temporary.shortcuts = temporary.shortcuts.canonicalized()?;
        temporary.validate()?;
        let compiled = active_shortcuts(&temporary, self.live_model_identity().as_ref())?;
        self.shortcut_table.replace(compiled);
        self.shortcut_capture_suspended = true;
        Ok(())
    }

    /// Restores the platform-facing table from the current committed config
    /// after shortcut recording is abandoned.
    pub fn resume_shortcut_capture(&mut self) -> Result<(), ApplicationError> {
        if self.shortcut_capture_suspended {
            let compiled = active_shortcuts(&self.config, self.live_model_identity().as_ref())?;
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
        next_config.shortcuts.model_behaviors_enabled = enabled;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_identity().as_ref())?;
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
    /// whether
    /// [`ShortcutConfig::command_bindings`](bongocat_config::ShortcutConfig::command_bindings)
    /// reaches the platform table, so turning it back on restores them without
    /// re-recording, exactly like the model behaviour gate next to it. The two
    /// gates are independent.
    pub fn set_command_shortcuts_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.shortcuts.commands_enabled = enabled;
        next_config.validate()?;
        let compiled = active_shortcuts(&next_config, self.live_model_identity().as_ref())?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        let snapshot = self.runtime.client().snapshot();
        self.config = next_config;
        self.shortcut_table.replace(compiled);
        self.config_revision = Some(next_revision);
        Ok(snapshot)
    }
}
