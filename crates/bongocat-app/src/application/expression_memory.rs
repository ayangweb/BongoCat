//! Remembering which expression each model was last showing, and putting it back.
//!
//! An expression is a per-model asset, so what "the expression the user last
//! chose" means is only ever a question about one model. Two rules follow from
//! that and are the whole of this module:
//!
//! - A choice is recorded against the model that was live when it was made, so
//!   switching away does not lose it and switching back returns to it.
//! - A model with nothing recorded for it shows its own default face, which is
//!   what a model that was only ever activated and never asked for an expression
//!   already does.
//!
//! Recording is driven by the runtime rather than by the two callers that can ask
//! for an expression, because those callers are not the same two things: the
//! settings window sends a command through the application, while a shortcut goes
//! straight to the runtime. Only the runtime sees both, and it already keeps them
//! apart — the idle scheduler plays expressions through the renderer rather than
//! through a command, so an automatic pick never becomes a remembered choice.
//!
//! The runtime holds one record, the newest choice it has seen, and the
//! configuration holds the accumulated per-model set. The application copies
//! between them, which is why the record has to survive the model switch that
//! clears the expression actually on screen.

use super::Application;
use crate::ApplicationError;
use crate::app_log::{ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent};
use crate::model_identity::config_source_from_model;
use bongocat_config::{
    ModelBehaviorName, ModelExpressionMemory, ModelIdentity, ModelSource, NativeConfig,
};
use bongocat_model::ModelId;
use bongocat_runtime::{ExpressionId, RuntimeCommand};

impl Application {
    /// Write the newest expression the user has chosen into the configuration,
    /// unless it is already recorded.
    ///
    /// Best effort, and deliberately silent about the outcome: this runs on a
    /// timer rather than in response to anything the user did, so there is no
    /// failure to report to anyone. A record that cannot be written — a
    /// configuration that no longer validates, or a store that refuses the
    /// commit — is dropped rather than retried, because retrying on the next tick
    /// would turn one unwritable record into an endless retry loop.
    ///
    /// Recording happens whether or not `remember_last_expression` is on, so
    /// turning the restore off and back on restores what the user had chosen
    /// rather than nothing.
    pub(crate) fn persist_user_expression_memory(&mut self) {
        let Some(memory) = self
            .runtime
            .client()
            .unrecorded_user_expression(self.recorded_expression_sequence)
        else {
            return;
        };
        // Marked before the write is attempted: the record either lands or it does
        // not, and either way this sequence has been dealt with.
        self.recorded_expression_sequence = Some(memory.command_sequence);
        let model = ModelIdentity {
            id: memory.model.as_str().to_owned(),
            source: config_source_from_model(memory.model_origin),
        };
        let mut next_config = self.config.clone();
        let remembered = &mut next_config.model.last_expressions;
        remembered.retain(|record| record.model != model);
        remembered.push(ModelExpressionMemory {
            model,
            expression: memory.expression.name().to_owned(),
        });
        if next_config == self.config {
            return;
        }
        if let Err(error) = self.commit_expression_memory(next_config) {
            self.application_log.record_once(
                ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                    .with_context(ApplicationLogContext::State("expression_memory"))
                    .with_context(ApplicationLogContext::Reason(error.stable_code())),
            );
        }
    }

    /// Play the expression remembered for the model that has just become live.
    ///
    /// Called once a model is on its way to being shown, so the restored face and
    /// the model it belongs to are always the same pair. Startup prepares its
    /// model without waiting for the commit, which is why the request is not
    /// waited on either: the runtime holds the command until the model it belongs
    /// to is committed, and the user cannot have moved the model on in between.
    ///
    /// Nothing is reported when this fails. A model whose package no longer ships
    /// the remembered expression simply keeps its default face, and a user who
    /// never asked for this restore should not be shown an error for it.
    pub(crate) fn restore_remembered_expression(&self) {
        if !self.config.model.remember_last_expression {
            return;
        }
        let Some(model) = self.live_model_identity() else {
            return;
        };
        let Some(remembered) = self
            .config
            .model
            .last_expressions
            .iter()
            .find(|record| record.model == model)
        else {
            return;
        };
        let Ok(expression) = ExpressionId::new(remembered.expression.clone()) else {
            return;
        };
        let _ = self
            .runtime
            .client()
            .send(RuntimeCommand::SetExpression(expression));
    }

    /// Drop the remembered expression of a model that no longer exists.
    ///
    /// Only an imported model can be removed, so a build-shipped record that
    /// happens to share the id names a different model and is kept. The restore
    /// ignores a record it cannot use anyway; this keeps the list from describing
    /// models the store no longer holds.
    pub(crate) fn without_removed_model_memories(
        &self,
        removed: &ModelId,
    ) -> Vec<ModelExpressionMemory> {
        self.config
            .model
            .last_expressions
            .iter()
            .filter(|record| {
                !(record.model.source == ModelSource::Imported
                    && record.model.id == removed.as_str())
            })
            .cloned()
            .collect()
    }

    /// The names a removed model leaves behind.
    ///
    /// A name is display text attached to one model, so a removed model takes its
    /// names with it. Only an imported model can be removed, so a build-shipped row
    /// that happens to share the id names a different model and is kept — the same
    /// reasoning as the remembered expressions above.
    pub(crate) fn without_removed_model_behavior_names(
        &self,
        removed: &ModelId,
    ) -> Vec<ModelBehaviorName> {
        self.config
            .model
            .behavior_names
            .iter()
            .filter(|row| {
                !(row.model.source == ModelSource::Imported && row.model.id == removed.as_str())
            })
            .cloned()
            .collect()
    }

    fn commit_expression_memory(
        &mut self,
        next_config: NativeConfig,
    ) -> Result<(), ApplicationError> {
        next_config.validate()?;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.ready_config_revision()?)?;
        self.config = next_config;
        self.config_revision = Some(next_revision);
        Ok(())
    }
}
