//! Linux Wayland overlay adapter backed by SCTK and Vulkan through wgpu.

mod renderer;

pub(crate) use renderer::*;
use crate::{
    BlendFactor, DrawableCullMode, FRAME_SMOKE_GRID_DIMENSION, FrameRetryBackoff,
    MAXIMUM_CORNER_RADIUS_PERCENT, OverlayContextMenuRequest, OverlayError,
    OverlayInteractionSinks, OverlayPresentationState, OverlayResizeOutcome, OverlaySessionOptions,
    OverlayTickOutcome, OverlayWindowBounds, ProductOverlayReport, ResizeBase, ResizeDrag,
    blend_factors, bounds_match_scale, corner_radius_uniform,
    cover::{
        COVER_CAPTURE_FRAMES, COVER_CAPTURE_SCALE_PERCENT, COVER_CAPTURE_TIMEOUT, CapturedFrame,
    },
    default_overlay_window_dimensions, drawable_cull_mode, model_switch_window_bounds,
    model_window_dimensions, validate_frame_smoke, validate_model_generation_advance,
};
use bongocat_model::CommittedModel;
use bongocat_platform::{
    LinuxInputService, PlatformInputDiagnostics, PlatformInputError, PlatformInputServiceStatus,
};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableId, KeyAssetId, KeyOverlay, ModelBounds, ModelCommitErrorCode,
    ModelCommitFeedback, ModelCommitOutcome, ModelCommitToken, RenderConsumer, RenderFrame,
    RenderResources, RenderSnapshot, TextureAsset, TextureId, validate_render_snapshot,
};
use bongocat_runtime::{
    CursorProducer, GamepadAxisProducer, InputProducer, RuntimeClient, RuntimeCommand,
    RuntimeOwner, RuntimeRenderErrorCode, RuntimeState, frame_interval_for_maximum_fps,
    maximum_fps_is_valid,
};
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle};
use std::{
    collections::BTreeMap,
    mem::size_of,
    sync::{Arc, mpsc::SyncSender},
    thread,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;

pub(crate) const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);
const WAYLAND_APPLICATION_ID: &str = "com.ayangweb.bongo-cat";

fn gpu_error(context: &'static str, error: impl std::fmt::Display) -> OverlayError {
    OverlayError::new(format!("{context}: {error}"))
}
