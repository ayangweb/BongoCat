//! Loading a model, and the commit handshake that puts it on screen.
//!
//! A model is prepared before it is committed, and a commit that arrives for a
//! model that has since been replaced is rejected rather than applied. Feedback
//! about a rejected commit is still recorded, because a model that silently never
//! appears is the failure this whole path exists to prevent.

use super::*;

impl RuntimeRenderer {
    pub(crate) fn set_model_settings(&mut self, settings: ModelSettings) {
        self.model_settings = settings;
    }
}

impl RuntimeRenderer {
    pub(crate) fn prepare(
        &mut self,
        command_sequence: u64,
        committed: &CommittedModel,
        input: ModelInputSnapshot,
    ) -> Result<ModelCommitToken, RuntimeRenderErrorCode> {
        debug_assert!(self.pending.is_none());
        let mut model = Live2dModel::load(committed)
            .map_err(|error| map_live2d_error(error, RuntimeRenderErrorCode::ModelLoadFailed))?;
        let resources = model.render_resources();
        apply_model_input(&mut model, input, self.model_settings)?;
        let mut snapshot = model.update_and_snapshot().map_err(|error| {
            map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
        })?;
        snapshot.active_keys = resolve_key_overlays(&resources, input.key_presses);
        snapshot.mirror_horizontal = self.model_settings.mirror;
        let model_generation = self.next_model_generation;
        let token = ModelCommitToken {
            command_sequence,
            model_generation,
        };
        let frame = RenderFrame {
            transport_sequence: self.next_transport_sequence,
            model_generation,
            frame_number: 0,
            model_commit: Some(token),
            resources: Arc::clone(&resources),
            snapshot: Arc::new(snapshot),
        };
        self.producer
            .publish(frame)
            .map_err(|_| RuntimeRenderErrorCode::TransportClosed)?;
        self.next_model_generation = self.next_model_generation.wrapping_add(1);
        self.next_transport_sequence = self.next_transport_sequence.wrapping_add(1);
        self.pending = Some(ActiveRenderModel {
            model,
            resources,
            model_generation,
            next_frame_number: 1,
            last_evaluated_at: None,
            motion: None,
            expressions: Vec::new(),
        });
        Ok(token)
    }
}

impl RuntimeRenderer {
    pub(crate) fn commit(&mut self, token: ModelCommitToken) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        if pending.model_generation != token.model_generation {
            self.pending = Some(pending);
            return false;
        }
        self.active = Some(pending);
        true
    }
}

impl RuntimeRenderer {
    pub(crate) fn reject(&mut self, token: ModelCommitToken) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.model_generation == token.model_generation)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }
}

impl RuntimeRenderer {
    pub(crate) fn take_model_commit_feedback(&self) -> Option<ModelCommitFeedback> {
        self.producer.take_model_commit_feedback()
    }
}

impl RuntimeRenderer {
    pub(crate) fn record_stale_model_commit_feedback(&self) {
        self.producer.record_stale_model_commit_feedback();
    }
}
