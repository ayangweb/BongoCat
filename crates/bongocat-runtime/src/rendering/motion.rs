//! Playing a motion, and knowing when it has finished.
//!
//! A motion settles rather than stops: once its natural fade is done the renderer
//! keeps the terminal parameters and reports that it is settled, and only an
//! explicit stop or a new motion clears them. A hidden motion settles without a
//! frame being evaluated at all, because a window nobody is looking at should
//! cost nothing.

use super::*;

impl RuntimeRenderer {
    pub(crate) fn start_motion(
        &mut self,
        motion: &MotionId,
        now: Duration,
        looping: bool,
    ) -> Result<(), RuntimeRenderErrorCode> {
        let active = self
            .active
            .as_mut()
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)?;
        let clip = active
            .model
            .motion_clip(motion.group(), motion.index())
            .cloned()
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)?;
        active.motion = Some(MotionPlayback {
            clip,
            looping,
            started_at: now,
            completed: false,
            fade_out_started_at: None,
            last_event_elapsed: None,
        });
        Ok(())
    }
}

impl RuntimeRenderer {
    /// Confirms that the active model has the already-prepared clip before an
    /// audio worker is allowed to make the motion externally observable.
    pub(crate) fn validate_motion(&self, motion: &MotionId) -> Result<(), RuntimeRenderErrorCode> {
        self.active
            .as_ref()
            .and_then(|active| active.model.motion_clip(motion.group(), motion.index()))
            .map(|_| ())
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)
    }
}

impl RuntimeRenderer {
    /// A completed one-shot keeps contributing its terminal sample, but it no
    /// longer reserves priority. Derive completion from the injected clock as
    /// well as the last delivered frame so a hidden or sleeping overlay cannot
    /// swallow a command sent after the clip duration. An explicit stop in
    /// progress is still stopping, not settled, until its fade duration has
    /// elapsed, even when no hidden frame was delivered to remove the layer.
    pub(crate) fn motion_is_settled(&self, now: Duration) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.motion.as_ref().is_some_and(|playback| {
                let completed = playback.completed
                    || (!playback.looping
                        && now.saturating_sub(playback.started_at) >= playback.clip.duration());
                let fade_finished = playback.fade_out_started_at.is_some_and(|started_at| {
                    now.saturating_sub(started_at) >= playback.clip.fade_out_duration()
                });
                (completed && playback.fade_out_started_at.is_none()) || fade_finished
            })
        })
    }
}

impl RuntimeRenderer {
    pub(crate) fn stop_motion(&mut self, now: Duration) -> MotionStopStatus {
        if let Some(active) = &mut self.active
            && let Some(playback) = &mut active.motion
        {
            if playback.clip.fade_out_duration().is_zero() {
                active.motion = None;
                return MotionStopStatus::Finished;
            }
            playback.fade_out_started_at.get_or_insert(now);
            return MotionStopStatus::Fading;
        }
        MotionStopStatus::Finished
    }
}
