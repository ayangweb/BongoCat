//! Setting an expression.
//!
//! The latest expression is the one in effect, and it holds full weight until
//! something replaces it — including a clock that has gone backwards, which must
//! not make an expression fade out on its own.

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
}
