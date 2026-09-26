//! Activating a model: motions, expressions, the model itself, and the automatic
//! switch that follows gamepad connectivity.

use super::Application;
use crate::app_log::{ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent};
use crate::model_identity::{
    config_identity_from_settings, config_source_from_model, model_origin_from_config,
};
use crate::model_input::input_bindings_for_committed_model;
use crate::shortcut_config::assign_default_behavior_shortcuts;
use crate::{ApplicationError, RUNTIME_TIMEOUT};
use bongocat_config::{GamepadAutoSwitchConfig, ModelIdentity};
use bongocat_model::{CommittedModel, ModelId, ModelOrigin};
use bongocat_render::ModelCommitToken;
use bongocat_runtime::{ExpressionId, MotionId, MotionPriority, RuntimeCommand, RuntimeSnapshot};
use std::sync::Arc;

impl Application {
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
        self.application_log.record(
            ApplicationLogEvent::new(ApplicationLogCode::ModelPrepareStarted)
                .with_context(ApplicationLogContext::Operation("model_activation")),
        );
        let result = (|| {
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
            self.remember_live_model();
            // The model that just became live owns the behavior half of the
            // platform table. Rebuilding here swaps the previous model's chords
            // out and registers the incoming model's own chords.
            self.refresh_shortcut_table();
            Ok(token)
        })();
        if let Err(error) = &result {
            self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::ModelActivationFailed)
                    .with_context(ApplicationLogContext::Phase("prepare"))
                    .with_context(ApplicationLogContext::Reason(error.stable_code())),
            );
        }
        result
    }

    pub fn select_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.application_log.record(
            ApplicationLogEvent::new(ApplicationLogCode::ModelPrepareStarted)
                .with_context(ApplicationLogContext::Operation("model_selection")),
        );
        let result = (|| {
            let id = ModelId::parse(id)?;
            let committed = self.load_model(origin, &id)?;
            let mut next_config = self.config.clone();
            next_config.model.selected_model = Some(ModelIdentity {
                id: id.as_str().to_owned(),
                source: config_source_from_model(origin),
            });
            // Switching models is also when the new model's motions and
            // expressions receive the legacy default chords, so the assignment
            // rides on the same commit as the selection itself.
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
                    self.remember_live_model();
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
        })();
        if let Err(error) = &result {
            self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::ModelActivationFailed)
                    .with_context(ApplicationLogContext::Phase("selection"))
                    .with_context(ApplicationLogContext::Reason(error.stable_code())),
            );
        } else {
            self.application_log.record(
                ApplicationLogEvent::new(ApplicationLogCode::ModelOperationCompleted)
                    .with_context(ApplicationLogContext::Operation("selection")),
            );
        }
        result
    }

    /// Persist the gamepad-connection model switch as one atomic change.
    ///
    /// This is a preference, not runtime state: the product acts on it from the
    /// settings worker whenever gamepad connectivity changes, so the gate and
    /// both targets move together and no runtime command is involved.
    pub fn set_gamepad_auto_switch(
        &mut self,
        settings: bongocat_ui_protocol::SettingsGamepadAutoSwitch,
    ) -> Result<(), ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.gamepad_auto_switch = GamepadAutoSwitchConfig {
            enabled: settings.enabled,
            connected_model: settings
                .connected_model
                .as_ref()
                .map(config_identity_from_settings),
            disconnected_model: settings
                .disconnected_model
                .as_ref()
                .map(config_identity_from_settings),
        };
        next_config.validate()?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }

    /// Bring the shown model in line with gamepad connectivity, if the user
    /// asked for that.
    ///
    /// The runtime owns the connected set, so this reads the runtime's own
    /// answer instead of an observation carried by the caller. Reconciling
    /// rather than switching on a remembered transition is what makes repeated
    /// notices, a notice that arrives late and a notice the caller could not
    /// deliver all land on the same result.
    ///
    /// A `None` target is the default and means "the last model activated for
    /// this input family", which is a session fact rather than configuration.
    /// Until the user has activated a model of that family there is nothing to
    /// switch to and the current model stays.
    ///
    /// Nothing happens when the switch is off, when the direction resolves to no
    /// model, or when the resolved model is already live. A failure leaves the
    /// current model on screen, and the caller records it with the same anonymous
    /// codes a user-driven selection uses.
    pub fn apply_gamepad_auto_switch(&mut self) -> Result<Option<ModelIdentity>, ApplicationError> {
        let switch = self.config.model.gamepad_auto_switch.clone();
        if !switch.enabled {
            return Ok(None);
        }
        let connected = self
            .runtime
            .client()
            .snapshot()
            .input
            .connected_gamepad_count
            > 0;
        let target = if connected {
            match &switch.connected_model {
                Some(configured) => Some(configured.clone()),
                None => self.last_gamepad_model.clone(),
            }
        } else {
            match &switch.disconnected_model {
                Some(configured) => Some(configured.clone()),
                None => self.last_other_model.clone(),
            }
        };
        let Some(target) = target else {
            return Ok(None);
        };
        if self.live_model_identity().as_ref() == Some(&target) {
            return Ok(None);
        }
        self.application_log.record(
            ApplicationLogEvent::new(ApplicationLogCode::ModelPrepareStarted)
                .with_context(ApplicationLogContext::Operation("gamepad_auto_switch"))
                .with_context(ApplicationLogContext::State(if connected {
                    "gamepad_connected"
                } else {
                    "gamepad_disconnected"
                })),
        );
        self.select_model(model_origin_from_config(target.source), target.id.as_str())?;
        Ok(Some(target))
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
}
