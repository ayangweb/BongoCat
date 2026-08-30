#![forbid(unsafe_code)]

use std::{
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DrawableId(usize);

impl DrawableId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for DrawableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextureId(usize);

impl TextureId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for TextureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInfo {
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub pixels_per_unit: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    Normal,
    Additive,
    Multiplicative,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawableSnapshot {
    pub id: DrawableId,
    pub render_order: i32,
    pub visible: bool,
    pub texture_id: TextureId,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub double_sided: bool,
    pub inverted_mask: bool,
    pub multiply_color: [f32; 4],
    pub screen_color: [f32; 4],
    pub masks: Vec<DrawableId>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSnapshot {
    pub canvas: CanvasInfo,
    pub model_opacity: f32,
    pub drawables: Vec<DrawableSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureAsset {
    pub id: TextureId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderResources {
    pub textures: Vec<TextureAsset>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModelCommitToken {
    pub command_sequence: u64,
    pub model_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCommitErrorCode {
    ResourcePreparationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCommitOutcome {
    Prepared,
    Rejected(ModelCommitErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelCommitFeedback {
    pub token: ModelCommitToken,
    pub outcome: ModelCommitOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    pub transport_sequence: u64,
    pub model_generation: u64,
    pub frame_number: u64,
    pub model_commit: Option<ModelCommitToken>,
    pub resources: Arc<RenderResources>,
    pub snapshot: Arc<RenderSnapshot>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderTransportDiagnostics {
    pub published: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub non_monotonic: u64,
    pub rejected_after_close: u64,
    pub pending: u64,
    pub feedback_reported: u64,
    pub feedback_consumed: u64,
    pub feedback_occupied: u64,
    pub feedback_rejected_after_close: u64,
    pub feedback_stale: u64,
    pub feedback_pending: u64,
}

#[derive(Debug)]
pub enum RenderPublishError {
    NonMonotonic(RenderFrame),
    Closed(RenderFrame),
}

impl RenderPublishError {
    pub fn into_frame(self) -> RenderFrame {
        match self {
            Self::NonMonotonic(frame) | Self::Closed(frame) => frame,
        }
    }
}

impl fmt::Display for RenderPublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonMonotonic(_) => "render frame sequence moved backwards",
            Self::Closed(_) => "render transport is closed",
        })
    }
}

impl std::error::Error for RenderPublishError {}

#[derive(Debug)]
pub enum ModelCommitFeedbackError {
    Occupied(ModelCommitFeedback),
    Closed(ModelCommitFeedback),
}

impl ModelCommitFeedbackError {
    pub fn into_feedback(self) -> ModelCommitFeedback {
        match self {
            Self::Occupied(feedback) | Self::Closed(feedback) => feedback,
        }
    }
}

impl fmt::Display for ModelCommitFeedbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Occupied(_) => "a model commit result is already pending",
            Self::Closed(_) => "render transport is closed",
        })
    }
}

impl std::error::Error for ModelCommitFeedbackError {}

#[derive(Default)]
struct LatestFrameState {
    pending: Option<RenderFrame>,
    last_transport_sequence: Option<u64>,
    feedback: Option<ModelCommitFeedback>,
    closed: bool,
    diagnostics: RenderTransportDiagnostics,
}

#[derive(Default)]
struct LatestFrameSlot {
    state: Mutex<LatestFrameState>,
}

pub struct RenderProducer {
    slot: Arc<LatestFrameSlot>,
}

impl RenderProducer {
    pub fn publish(&self, frame: RenderFrame) -> Result<(), RenderPublishError> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.diagnostics.rejected_after_close =
                state.diagnostics.rejected_after_close.saturating_add(1);
            return Err(RenderPublishError::Closed(frame));
        }
        if state
            .last_transport_sequence
            .is_some_and(|previous| frame.transport_sequence <= previous)
        {
            state.diagnostics.non_monotonic = state.diagnostics.non_monotonic.saturating_add(1);
            return Err(RenderPublishError::NonMonotonic(frame));
        }
        state.last_transport_sequence = Some(frame.transport_sequence);
        state.diagnostics.published = state.diagnostics.published.saturating_add(1);
        if state.pending.replace(frame).is_some() {
            state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
        }
        Ok(())
    }

    pub fn close(&self) {
        self.slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
    }

    pub fn diagnostics(&self) -> RenderTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn take_model_commit_feedback(&self) -> Option<ModelCommitFeedback> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let feedback = state.feedback.take();
        if feedback.is_some() {
            state.diagnostics.feedback_consumed =
                state.diagnostics.feedback_consumed.saturating_add(1);
        }
        feedback
    }

    pub fn record_stale_model_commit_feedback(&self) {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.diagnostics.feedback_stale = state.diagnostics.feedback_stale.saturating_add(1);
    }
}

pub struct RenderConsumer {
    slot: Arc<LatestFrameSlot>,
}

impl RenderConsumer {
    pub fn take_latest(&self) -> Option<RenderFrame> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = state.pending.take();
        if frame.is_some() {
            state.diagnostics.consumed = state.diagnostics.consumed.saturating_add(1);
        }
        frame
    }

    pub fn diagnostics(&self) -> RenderTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn report_model_commit(
        &self,
        feedback: ModelCommitFeedback,
    ) -> Result<(), ModelCommitFeedbackError> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.diagnostics.feedback_rejected_after_close = state
                .diagnostics
                .feedback_rejected_after_close
                .saturating_add(1);
            return Err(ModelCommitFeedbackError::Closed(feedback));
        }
        if state.feedback.is_some() {
            state.diagnostics.feedback_occupied =
                state.diagnostics.feedback_occupied.saturating_add(1);
            return Err(ModelCommitFeedbackError::Occupied(feedback));
        }
        state.feedback = Some(feedback);
        state.diagnostics.feedback_reported = state.diagnostics.feedback_reported.saturating_add(1);
        Ok(())
    }
}

impl LatestFrameSlot {
    fn diagnostics(&self) -> RenderTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RenderTransportDiagnostics {
            pending: u64::from(state.pending.is_some()),
            feedback_pending: u64::from(state.feedback.is_some()),
            ..state.diagnostics
        }
    }
}

pub fn latest_render_channel() -> (RenderProducer, RenderConsumer) {
    let slot = Arc::new(LatestFrameSlot::default());
    (
        RenderProducer {
            slot: Arc::clone(&slot),
        },
        RenderConsumer { slot },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(number: u64) -> RenderFrame {
        RenderFrame {
            transport_sequence: number,
            model_generation: 3,
            frame_number: number,
            model_commit: None,
            resources: Arc::new(RenderResources { textures: vec![] }),
            snapshot: Arc::new(RenderSnapshot {
                canvas: CanvasInfo {
                    width: 1.0,
                    height: 1.0,
                    origin_x: 0.0,
                    origin_y: 0.0,
                    pixels_per_unit: 1.0,
                },
                model_opacity: 1.0,
                drawables: vec![],
            }),
        }
    }

    #[test]
    fn strong_resource_ids_preserve_source_identity() {
        assert_eq!(DrawableId::new(7).index(), 7);
        assert_eq!(TextureId::new(2).index(), 2);
        assert_eq!(DrawableId::new(7).to_string(), "7");
        assert_eq!(TextureId::new(2).to_string(), "2");
    }

    #[test]
    fn latest_frame_transport_coalesces_without_blocking_the_producer() {
        let (producer, consumer) = latest_render_channel();
        for number in 0..10_000 {
            producer.publish(frame(number)).expect("publish frame");
        }
        assert_eq!(
            consumer.take_latest().map(|frame| frame.frame_number),
            Some(9_999)
        );
        assert_eq!(
            consumer.diagnostics(),
            RenderTransportDiagnostics {
                published: 10_000,
                coalesced: 9_999,
                consumed: 1,
                ..RenderTransportDiagnostics::default()
            }
        );
    }

    #[test]
    fn close_rejects_new_frames_but_allows_pending_drain() {
        let (producer, consumer) = latest_render_channel();
        producer.publish(frame(1)).expect("publish frame");
        producer.close();
        let rejected = producer.publish(frame(2)).expect_err("closed channel");
        assert_eq!(rejected.into_frame().frame_number, 2);
        assert_eq!(
            consumer.take_latest().map(|frame| frame.frame_number),
            Some(1)
        );
        assert_eq!(
            producer.diagnostics(),
            RenderTransportDiagnostics {
                published: 1,
                consumed: 1,
                rejected_after_close: 1,
                ..RenderTransportDiagnostics::default()
            }
        );
    }

    #[test]
    fn frame_sequence_must_increase_within_and_across_model_generations() {
        let (producer, consumer) = latest_render_channel();
        producer.publish(frame(2)).expect("first frame");
        let rejected = producer.publish(frame(1)).expect_err("older frame");
        assert!(matches!(rejected, RenderPublishError::NonMonotonic(_)));
        let mut replacement = frame(3);
        replacement.model_generation = 4;
        replacement.frame_number = 0;
        producer
            .publish(replacement)
            .expect("new model generation may reset frame number");
        assert_eq!(
            consumer.take_latest().map(|frame| frame.model_generation),
            Some(4)
        );
        assert_eq!(consumer.diagnostics().non_monotonic, 1);
    }

    #[test]
    fn model_commit_feedback_is_reliable_and_never_overwrites() {
        let (producer, consumer) = latest_render_channel();
        let first = ModelCommitFeedback {
            token: ModelCommitToken {
                command_sequence: 7,
                model_generation: 3,
            },
            outcome: ModelCommitOutcome::Prepared,
        };
        let second = ModelCommitFeedback {
            token: ModelCommitToken {
                command_sequence: 8,
                model_generation: 4,
            },
            outcome: ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
        };
        consumer.report_model_commit(first).expect("first feedback");
        let occupied = consumer
            .report_model_commit(second)
            .expect_err("feedback cannot overwrite");
        assert_eq!(occupied.into_feedback(), second);
        assert_eq!(producer.take_model_commit_feedback(), Some(first));
        consumer
            .report_model_commit(second)
            .expect("second feedback after drain");
        assert_eq!(producer.take_model_commit_feedback(), Some(second));
        assert_eq!(
            producer.diagnostics(),
            RenderTransportDiagnostics {
                feedback_reported: 2,
                feedback_consumed: 2,
                feedback_occupied: 1,
                ..RenderTransportDiagnostics::default()
            }
        );
    }

    #[test]
    fn model_commit_feedback_is_rejected_after_close() {
        let (producer, consumer) = latest_render_channel();
        producer.close();
        let feedback = ModelCommitFeedback {
            token: ModelCommitToken {
                command_sequence: 1,
                model_generation: 0,
            },
            outcome: ModelCommitOutcome::Prepared,
        };
        let rejected = consumer
            .report_model_commit(feedback)
            .expect_err("closed feedback");
        assert_eq!(rejected.into_feedback(), feedback);
        assert_eq!(consumer.diagnostics().feedback_rejected_after_close, 1);
    }
}
