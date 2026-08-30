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

#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    pub model_generation: u64,
    pub frame_number: u64,
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

#[derive(Default)]
struct LatestFrameState {
    pending: Option<RenderFrame>,
    last_published: Option<(u64, u64)>,
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
        let sequence = (frame.model_generation, frame.frame_number);
        if state
            .last_published
            .is_some_and(|previous| sequence <= previous)
        {
            state.diagnostics.non_monotonic = state.diagnostics.non_monotonic.saturating_add(1);
            return Err(RenderPublishError::NonMonotonic(frame));
        }
        state.last_published = Some(sequence);
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
}

impl LatestFrameSlot {
    fn diagnostics(&self) -> RenderTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RenderTransportDiagnostics {
            pending: u64::from(state.pending.is_some()),
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
            model_generation: 3,
            frame_number: number,
            resources: Arc::new(RenderResources { textures: vec![] }),
            snapshot: Arc::new(RenderSnapshot {
                canvas: CanvasInfo {
                    width: 1.0,
                    height: 1.0,
                    origin_x: 0.0,
                    origin_y: 0.0,
                    pixels_per_unit: 1.0,
                },
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
    fn latest_frame_transport_coalesces_without_renderer_acknowledgement() {
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
        let mut replacement = frame(0);
        replacement.model_generation = 4;
        producer
            .publish(replacement)
            .expect("new model generation may reset frame number");
        assert_eq!(
            consumer.take_latest().map(|frame| frame.model_generation),
            Some(4)
        );
        assert_eq!(consumer.diagnostics().non_monotonic, 1);
    }
}
