//! The macOS overlay adapter.
//!
//! The product is one borderless `NSPanel` over the user's desktop, drawn by
//! Metal through a `CAMetalLayer`. The layout mirrors the Windows adapter's on
//! purpose, so the two read the same way: `session` is what the product drives,
//! `renderer`, `pipelines` and `textures` own the frame, `resize_monitor` is
//! the only view of the desktop, and the rest each answer one question.

mod cover_capture;
mod geometry;
mod pipelines;
mod renderer;
mod resize_monitor;
mod session;
mod switch_preview;
#[cfg(test)]
mod tests;
mod textures;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the adapter's items are one vocabulary, and a list per
// module would be the same list eight times.
pub(crate) use cover_capture::*;
pub(crate) use geometry::*;
pub(crate) use pipelines::*;
pub(crate) use renderer::*;
pub(crate) use resize_monitor::*;
pub(crate) use session::*;
pub(crate) use switch_preview::*;
pub(crate) use textures::*;

use crate::{
    BlendFactor, DrawableCullMode, FRAME_SMOKE_GRID_DIMENSION, FrameRetryBackoff,
    FrameTimingCollector, MAXIMUM_CORNER_RADIUS_PERCENT, OverlayContextMenuRequest, OverlayError,
    OverlayInteractionSinks, OverlayPresentationState, OverlayResizeOutcome, OverlayScreenBounds,
    OverlaySessionOptions, OverlayTickOutcome, OverlayWindowBounds, PreviewReport,
    ProductOverlayReport, blend_factors, corner_radius_uniform,
    cover::{
        COVER_CAPTURE_FRAMES, COVER_CAPTURE_SCALE_PERCENT, COVER_CAPTURE_TIMEOUT, CapturedFrame,
    },
    default_overlay_window_dimensions, drawable_cull_mode,
    hover::{PointerHoverHide, PointerHoverObservation, pointer_inside_window},
    model_switch_window_bounds, model_window_dimensions,
    placement::{OverlayPlacementConstraint, bounds_inside_screens, correction_for_screens},
    resize_drag::{ResizeBase, ResizeDrag, ResizeOutcome},
    validate_frame_smoke, validate_model_generation_advance,
};
use block2::RcBlock;
use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_platform::{
    MacInputService, PlatformInputDiagnostics, PlatformInputError, PlatformInputServiceStatus,
};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableId, KeyAssetId, KeyOverlay, ModelBounds, ModelCommitErrorCode,
    ModelCommitFeedback, ModelCommitOutcome, ModelCommitToken, RenderConsumer, RenderFrame,
    RenderResources, RenderSnapshot, TextureAsset, TextureId, validate_render_snapshot,
};
use bongocat_runtime::{
    CursorPosition, CursorProducer, CursorSample, CursorViewport, GamepadAxisProducer,
    GamepadButton, HandSide, InputBindings, InputControl, InputEdge, InputEvent, InputProducer,
    InputSource, MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS, MonotonicMillis, MouseButton, PhysicalKey,
    RuntimeClient, RuntimeCommand, RuntimeOwner, RuntimeRenderErrorCode, RuntimeState,
    frame_interval_for_maximum_fps, maximum_fps_is_valid,
};
use image::ImageReader;
use metal::{
    Buffer, CommandQueue, CompileOptions, Device, MTLBlendFactor, MTLClearColor,
    MTLCommandBufferStatus, MTLCullMode, MTLIndexType, MTLLoadAction, MTLOrigin, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerMinMagFilter,
    MTLSize, MTLStorageMode, MTLStoreAction, MTLTextureType, MTLTextureUsage, MTLWinding,
    MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState,
    SamplerDescriptor, SamplerState, Texture, TextureDescriptor, foreign_types::ForeignTypeRef,
};
use objc2::{
    MainThreadMarker, MainThreadOnly, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::AnyObject,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEvent,
    NSEventMask, NSMainMenuWindowLevel, NSNormalWindowLevel, NSPanel, NSScreen, NSView,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowLevel, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
use objc2_quartz_core::CAMetalLayer as ObjcMetalLayer;
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, WindowHandle};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    mem::{self, ManuallyDrop},
    path::Path,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, mpsc::SyncSender},
    thread,
    time::{Duration, Instant},
};
