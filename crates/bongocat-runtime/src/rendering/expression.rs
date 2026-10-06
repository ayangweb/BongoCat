//! Setting and clearing an expression.
//!
//! The latest expression is the one in effect, and it holds full weight until
//! something replaces it — including a clock that has gone backwards, which must
//! not make an expression fade out on its own.
//!
//! Clearing is not a separate model state: it is the same fade every replacement
//! starts with, minus the new layer. Because every frame restores the parameter
//! and part-opacity defaults before applying the surviving layers, a stack that
//! has finished fading leaves the model in its own default face.

use super::*;

impl RuntimeRenderer {
    pub(crate) fn set_expression(
        &mut self,
        expression: &crate::ExpressionId,
        now: Duration,
    ) -> Result<(), RuntimeRenderErrorCode> {
        let active = self
            .active
            .as_mut()
            .ok_or(RuntimeRenderErrorCode::ExpressionLoadFailed)?;
        let clip = active
            .model
            .expression_clip(expression.name())
            .cloned()
            .ok_or(RuntimeRenderErrorCode::ExpressionLoadFailed)?;
        if active.expressions.len() > 1 {
            let previous = active
                .expressions
                .pop()
                .expect("expression stack has a newest layer");
            active.expressions.clear();
            active.expressions.push(previous);
        }
        for playback in &mut active.expressions {
            playback.fade_out_started_at = Some(now);
        }
        active.expressions.push(ExpressionPlayback {
            clip,
            started_at: now,
            fade_in_completed: false,
            fade_out_started_at: None,
        });
        debug_assert!(active.expressions.len() <= 2);
        Ok(())
    }

    /// Start fading every expression layer out and add nothing in its place.
    ///
    /// The layers are dropped by the next evaluation once their fade has run, so
    /// this reports whether there was anything to fade rather than whether the
    /// model is already back to its default face — the face only settles over
    /// the fade the clip itself declares.
    pub(crate) fn clear_expression(&mut self, now: Duration) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let had_an_expression = !active.expressions.is_empty();
        for playback in &mut active.expressions {
            playback.fade_out_started_at.get_or_insert(now);
        }
        had_an_expression
    }
}
