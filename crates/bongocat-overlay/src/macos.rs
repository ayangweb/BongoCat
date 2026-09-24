use crate::{
    BlendFactor, FRAME_SMOKE_GRID_DIMENSION, FrameRetryBackoff, FrameTimingCollector,
    MAXIMUM_CORNER_RADIUS_PERCENT, OverlayContextMenuRequest, OverlayError,
    OverlayInteractionSinks, OverlayPresentationState, OverlayResizeOutcome, OverlayScreenBounds,
    OverlaySessionOptions, OverlayTickOutcome, OverlayWindowBounds, PreviewReport,
    ProductOverlayReport, blend_factors, corner_radius_uniform,
    cover::{
        COVER_CAPTURE_FRAMES, COVER_CAPTURE_SCALE_PERCENT, COVER_CAPTURE_TIMEOUT, CapturedFrame,
    },
    default_overlay_window_dimensions,
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
    MTLCommandBufferStatus, MTLIndexType, MTLLoadAction, MTLOrigin, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerMinMagFilter,
    MTLSize, MTLStorageMode, MTLStoreAction, MTLTextureType, MTLTextureUsage, MetalLayer,
    RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState, SamplerDescriptor,
    SamplerState, Texture, TextureDescriptor,
};
use objc2::{
    MainThreadMarker, MainThreadOnly,
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
    sync::{Arc, mpsc::SyncSender},
    thread,
    time::{Duration, Instant},
};

const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
const RUNTIME_TIMEOUT: Duration = Duration::from_millis(250);
const METAL_COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
const SWITCH_WARMUP_FRAMES: u64 = 30;
const SWITCH_SETTLE_FRAMES: u64 = 30;
const PRESET_MODEL_IDS: [&str; 3] = ["standard", "keyboard", "gamepad"];
// PNG RGBA payloads are encoded sRGB. Sampling and color blending therefore
// happen in linear space, while the drawable encodes its premultiplied result
// back to sRGB for the window compositor. Masks carry alpha only. The Windows
// backend reaches the same contract through its `_SRGB` render target view, so
// either side changing this format must keep the other in step.
const COLOR_ATTACHMENT_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm_sRGB;
const MODEL_TEXTURE_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA8Unorm_sRGB;
const MASK_TEXTURE_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;
const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);
const SHADER_SOURCE: &str = r#"
    #include <metal_stdlib>
    using namespace metal;

    struct Vertex {
        float2 position;
        float2 uv;
    };

    struct Uniforms {
        float4 scale_offset;
        float4 multiply_color;
        float4 screen_color;
        float4 mask_settings;
        float4 corner_radius;
        float opacity;
        float3 padding;
    };

    struct RasterVertex {
        float4 position [[position]];
        float2 uv;
    };

    // Legacy window rounding. `corner_radius.x` is the configured radius as a
    // fraction of the window box, and `corner_radius.yz` are the drawable
    // dimensions. The corner arcs stay elliptical on a non-square window,
    // exactly like a CSS percentage `border-radius`.
    float corner_coverage(float2 position, float4 corner_radius) {
        float radius = min(corner_radius.x, 0.5);
        if (radius <= 0.0) {
            return 1.0;
        }
        float2 uv = position / corner_radius.yz;
        float2 centered = abs(uv * 2.0 - 1.0);
        float extent = 2.0 * radius;
        float2 delta = centered - 1.0 + extent;
        float distance = length(max(delta, 0.0)) + min(max(delta.x, delta.y), 0.0) - extent;
        // Convert the signed distance to device pixels using the smaller
        // drawable dimension, so the antialiased band never narrows below one
        // pixel on the longer axis.
        float scale = 0.5 * min(corner_radius.y, corner_radius.z);
        return saturate(0.5 - distance * scale);
    }

    vertex RasterVertex cubism_vertex(
        const device Vertex* vertices [[buffer(0)]],
        constant Uniforms& uniforms [[buffer(1)]],
        uint vertex_id [[vertex_id]]
    ) {
        RasterVertex output;
        float2 clip = vertices[vertex_id].position * uniforms.scale_offset.xy
                    + uniforms.scale_offset.zw;
        output.position = float4(clip, 0.0, 1.0);
        output.uv = vertices[vertex_id].uv;
        output.uv.y = 1.0 - output.uv.y;
        return output;
    }

    fragment float4 cubism_fragment(
        RasterVertex input [[stage_in]],
        texture2d<float> model_texture [[texture(0)]],
        texture2d<float> mask_texture [[texture(1)]],
        sampler texture_sampler [[sampler(0)]],
        constant Uniforms& uniforms [[buffer(1)]]
    ) {
        float4 texture_color = model_texture.sample(texture_sampler, input.uv);
        float3 color = texture_color.rgb * uniforms.multiply_color.rgb;
        color = color + uniforms.screen_color.rgb - color * uniforms.screen_color.rgb;
        float mask = 1.0;
        if (uniforms.mask_settings.z > 0.5) {
            float2 mask_uv = input.position.xy / uniforms.mask_settings.xy;
            mask = mask_texture.sample(texture_sampler, mask_uv).a;
            if (uniforms.mask_settings.w > 0.5) {
                mask = 1.0 - mask;
            }
        }
        float alpha = texture_color.a * uniforms.opacity * mask
                    * corner_coverage(input.position.xy, uniforms.corner_radius);
        return float4(color * alpha, alpha);
    }

    fragment float4 cubism_mask_fragment(
        RasterVertex input [[stage_in]],
        texture2d<float> model_texture [[texture(0)]],
        sampler texture_sampler [[sampler(0)]]
    ) {
        float alpha = model_texture.sample(texture_sampler, input.uv).a;
        return float4(0.0, 0.0, 0.0, alpha);
    }
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms {
    scale_offset: [f32; 4],
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    mask_settings: [f32; 4],
    corner_radius: [f32; 4],
    opacity: f32,
    padding: [f32; 3],
}

struct Mesh {
    id: DrawableId,
    render_order: i32,
    vertex_buffer: Buffer,
    /// Byte length of the snapshot's own vertex array. It is what the buffer
    /// holds unless the drawable carries no vertices at all, in which case the
    /// buffer is a placeholder and this stays zero.
    vertex_bytes: usize,
    index_buffer: Buffer,
    indices: Vec<u16>,
    index_count: u64,
    texture_id: TextureId,
    opacity: f32,
    blend_mode: BlendMode,
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    masks: Vec<DrawableId>,
    visible: bool,
    inverted_mask: bool,
    mask_texture: Option<Texture>,
}

struct Pipelines {
    normal: RenderPipelineState,
    additive: RenderPipelineState,
    multiplicative: RenderPipelineState,
    mask: RenderPipelineState,
}

impl Pipelines {
    fn for_mode(&self, mode: BlendMode) -> &RenderPipelineState {
        match mode {
            BlendMode::Normal => &self.normal,
            BlendMode::Additive => &self.additive,
            BlendMode::Multiplicative => &self.multiplicative,
        }
    }
}

struct NativeOverlay {
    panel: ManuallyDrop<Retained<NSPanel>>,
    device: Device,
    layer: MetalLayer,
    queue: CommandQueue,
    pipelines: Pipelines,
    sampler: SamplerState,
    model_generation: u64,
    resources: Arc<RenderResources>,
    model: GpuModel,
    presentation: OverlayPresentationState,
    corner_radius_percent: u8,
    /// Window alpha currently applied to the panel, including the hover fade.
    /// It lives here rather than on the session so replacing the native window
    /// resets it together with the panel that carries it.
    applied_alpha: f64,
    applied_click_through: bool,
}

struct GpuModel {
    textures: BTreeMap<TextureId, Texture>,
    key_textures: BTreeMap<KeyAssetId, Texture>,
    background: Option<Texture>,
    background_vertex_buffer: Buffer,
    background_index_buffer: Buffer,
    meshes: Vec<Mesh>,
    empty_mask: Texture,
    bounds: ModelBounds,
    model_opacity: f32,
    mirror_horizontal: bool,
    active_keys: Vec<KeyOverlay>,
    masked_drawable_count: usize,
}

pub(super) struct ProductOverlaySession {
    application: Retained<NSApplication>,
    overlay: NativeOverlay,
    runtime_client: RuntimeClient,
    render_consumer: RenderConsumer,
    input_service: Option<MacInputService>,
    input_start_error: Option<PlatformInputError>,
    input_diagnostics: Option<PlatformInputDiagnostics>,
    input_stopped: bool,
    frames_presented: u64,
    dynamic_snapshots: u64,
    model_commit_rejections: u64,
    previous_snapshot: Arc<RenderSnapshot>,
    options: OverlaySessionOptions,
    last_frame: RenderFrame,
    pending_initial_model_commit: Option<ModelCommitToken>,
    pending_model_frame: Option<RenderFrame>,
    retry_backoff: FrameRetryBackoff,
    context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    right_button_monitor: Option<RightButtonMonitor>,
    hover: PointerHoverHide,
    placement: OverlayPlacementConstraint,
    /// Monotonic base for every time-based rule in this session. The hover fade
    /// and the placement settle delay both measure elapsed time from it, so the
    /// session needs exactly one wall-clock reading at start.
    session_started: Instant,
}

impl ProductOverlaySession {
    pub(super) fn start(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
        interaction_sinks: OverlayInteractionSinks,
    ) -> Result<Self, OverlayError> {
        let OverlayInteractionSinks {
            context_menu_sender,
            resize_sender,
        } = interaction_sinks;
        validate_product_options(options)?;
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime did not publish an initial render frame"))?;
        let token = initial_frame
            .model_commit
            .ok_or_else(|| OverlayError::new("initial render frame has no model commit token"))?;
        let Some(mtm) = MainThreadMarker::new() else {
            reject_model_commit(&runtime_client, &render_consumer, token)?;
            return Err(OverlayError::new(
                "macOS overlay must start on the main thread",
            ));
        };
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        application.finishLaunching();
        let mut overlay =
            match NativeOverlay::create(mtm, &initial_frame, options, options.window_bounds) {
                Ok(overlay) => overlay,
                Err(error) => {
                    reject_model_commit(&runtime_client, &render_consumer, token)?;
                    return Err(error);
                }
            };
        let mut frames_presented = 0;
        let mut retry_backoff = FrameRetryBackoff::default();
        let mut pending_initial_model_commit = None;
        if runtime_client.snapshot().overlay_visible {
            match overlay.draw(true) {
                Ok(()) => {
                    retry_backoff.record_success();
                    if let Err(error) = overlay.set_visible(true) {
                        reject_model_commit(&runtime_client, &render_consumer, token)?;
                        return Err(error);
                    }
                    frames_presented = 1;
                }
                Err(error) if error.is_temporary_presentation_unavailable() => {
                    pending_initial_model_commit = Some(token);
                    let _ = retry_backoff.register_temporary_failure();
                }
                Err(error) => {
                    reject_model_commit(&runtime_client, &render_consumer, token)?;
                    return Err(error);
                }
            }
        }
        if pending_initial_model_commit.is_none() {
            report_model_commit(
                &runtime_client,
                &render_consumer,
                token,
                ModelCommitOutcome::Prepared,
            )?;
        }
        let diagnostics_producer = runtime_client.platform_input_diagnostics_producer();
        let (input_service, input_start_error) =
            super::start_platform_input(&diagnostics_producer, || {
                MacInputService::start_with_diagnostics(
                    input_producer,
                    cursor_producer,
                    gamepad_axis_producer,
                    diagnostics_producer.clone(),
                )
            });
        let (base_width, base_height) =
            default_overlay_window_dimensions(initial_frame.snapshot.canvas);
        let context_menu_monitor = install_context_menu_monitor(
            mtm,
            Retained::clone(&overlay.panel),
            ResizeBase::new(f64::from(base_width), f64::from(base_height)),
            context_menu_sender.clone(),
            resize_sender.clone(),
        );
        Ok(Self {
            application,
            overlay,
            runtime_client,
            render_consumer,
            input_service,
            input_start_error,
            input_diagnostics: None,
            input_stopped: false,
            frames_presented,
            dynamic_snapshots: 0,
            model_commit_rejections: 0,
            previous_snapshot: Arc::clone(&initial_frame.snapshot),
            options,
            last_frame: initial_frame,
            pending_initial_model_commit,
            pending_model_frame: None,
            retry_backoff,
            context_menu_sender,
            resize_sender,
            right_button_monitor: context_menu_monitor,
            hover: PointerHoverHide::default(),
            placement: OverlayPlacementConstraint::default(),
            session_started: Instant::now(),
        })
    }

    pub(super) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while duration.is_zero() || started.elapsed() < duration {
            pump_application_events(&self.application);
            let outcome = self.tick()?;
            if outcome == OverlayTickOutcome::Hidden {
                break;
            }
            let interval = outcome.retry_after().unwrap_or_else(|| {
                frame_interval_for_maximum_fps(self.options.maximum_fps)
                    .expect("product overlay stores a validated maximum FPS")
            });
            if outcome.retry_after().is_some() {
                next_frame = Instant::now();
            }
            next_frame += interval;
            if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            } else {
                next_frame = Instant::now();
            }
        }
        Ok(())
    }

    pub(super) fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
        let runtime_snapshot = self.runtime_client.snapshot();
        if runtime_snapshot.state == RuntimeState::Stopped {
            return Err(OverlayError::new(
                "runtime stopped while the product overlay was active",
            ));
        }
        let next_options = self
            .options
            .with_runtime_settings(runtime_snapshot.overlay_settings);
        if next_options != self.options {
            if self.options.requires_window_recreation(next_options) {
                let bounds = self.window_bounds()?;
                let bounds = if next_options.scale_percent != self.options.scale_percent
                    && !self.bounds_match_scale(bounds, next_options.scale_percent)
                {
                    bounds.rescale(self.options.scale_percent, next_options.scale_percent)
                } else {
                    bounds
                };
                let mut replacement = self.create_overlay(
                    MainThreadMarker::new().ok_or_else(|| {
                        OverlayError::new("macOS overlay settings update lost the main thread")
                    })?,
                    &self.last_frame,
                    next_options,
                    Some(bounds),
                )?;
                if runtime_snapshot.overlay_visible {
                    match replacement.draw(self.frames_presented == 0) {
                        Ok(()) => self.retry_backoff.record_success(),
                        Err(error) if error.is_temporary_presentation_unavailable() => {
                            return Ok(self.defer_drawable_unavailable());
                        }
                        Err(error) => return Err(error),
                    }
                    replacement.set_visible(true)?;
                    self.frames_presented = self.frames_presented.saturating_add(1);
                }
                self.overlay = replacement;
                let mtm = MainThreadMarker::new().ok_or_else(|| {
                    OverlayError::new("macOS overlay settings update lost the main thread")
                })?;
                self.refresh_right_button_monitor(mtm);
            } else {
                if next_options.always_on_top != self.options.always_on_top {
                    self.overlay.set_always_on_top(next_options.always_on_top);
                }
            }
            self.options = next_options;
        }
        // A right-button resize drag changes the panel frame directly, so the
        // drawable and the mask targets follow here, before anything draws
        // against them.
        self.overlay.sync_window_size()?;
        if self.options.keep_inside_screen {
            let mtm = MainThreadMarker::new().ok_or_else(|| {
                OverlayError::new("macOS overlay placement check lost the main thread")
            })?;
            let bounds = self.window_bounds()?;
            // A box that is still outside the displays is only corrected once it
            // has been observed at rest for the settle delay, so a drag that is
            // still in progress is never interrupted.
            let correction = self
                .placement
                .observe(bounds, self.session_started.elapsed(), || {
                    screen_bounds_all(mtm).unwrap_or_default()
                });
            if let Some(correction) = correction {
                self.overlay.set_origin(correction);
            }
        }
        // Pointer routing and window alpha are applied every tick rather than
        // only when the settings change, because the hover hide changes both
        // while the session keeps running.
        self.update_hover_presentation(
            self.options,
            runtime_snapshot.cursor.sample,
            runtime_snapshot.platform_input.service_status == PlatformInputServiceStatus::Running,
        )?;
        self.options.maximum_fps = runtime_snapshot.maximum_fps;
        let overlay_visible = runtime_snapshot.overlay_visible;
        if !overlay_visible {
            self.overlay.set_visible(false)?;
        }

        if let Some(token) = self.pending_initial_model_commit {
            match self.overlay.draw(true) {
                Ok(()) => {
                    self.retry_backoff.record_success();
                    if overlay_visible {
                        self.overlay.set_visible(true)?;
                    }
                    report_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        token,
                        ModelCommitOutcome::Prepared,
                    )?;
                    self.pending_initial_model_commit = None;
                    self.frames_presented = self.frames_presented.saturating_add(1);
                    return Ok(if overlay_visible {
                        OverlayTickOutcome::Presented
                    } else {
                        OverlayTickOutcome::Hidden
                    });
                }
                Err(error) if error.is_temporary_presentation_unavailable() => {
                    return Ok(self.defer_drawable_unavailable());
                }
                Err(error) => {
                    reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                    return Err(error);
                }
            }
        }

        let next_frame = self.pending_model_frame.take().or_else(|| {
            if overlay_visible {
                self.render_consumer.take_latest()
            } else {
                self.render_consumer.take_model_commit()
            }
        });
        if let Some(frame) = next_frame {
            let model_changed = frame.model_generation != self.overlay.model_generation;
            if model_changed {
                let bounds =
                    model_switch_window_bounds(self.window_bounds()?, frame.snapshot.canvas);
                let replacement = self.create_overlay(
                    MainThreadMarker::new().ok_or_else(|| {
                        OverlayError::new("macOS overlay model update lost the main thread")
                    })?,
                    &frame,
                    self.options,
                    Some(bounds),
                );
                let mut replacement = match replacement {
                    Ok(replacement) => replacement,
                    Err(error) if frame.model_commit.is_some() => {
                        reject_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            frame.model_commit.expect("checked model commit token"),
                        )?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        if overlay_visible {
                            match self.overlay.draw(self.frames_presented == 0) {
                                Ok(()) => self.retry_backoff.record_success(),
                                Err(error) if error.is_temporary_presentation_unavailable() => {
                                    return Ok(self.defer_drawable_unavailable());
                                }
                                Err(error) => return Err(error),
                            }
                            self.frames_presented = self.frames_presented.saturating_add(1);
                            self.overlay.set_visible(true)?;
                            return Ok(OverlayTickOutcome::Presented);
                        }
                        return Ok(OverlayTickOutcome::Hidden);
                    }
                    Err(error) => return Err(error),
                };
                let candidate = match replacement.draw(true) {
                    Ok(()) => {
                        self.retry_backoff.record_success();
                        if overlay_visible {
                            replacement.set_visible(true)
                        } else {
                            Ok(())
                        }
                    }
                    Err(error) if error.is_temporary_presentation_unavailable() => {
                        self.pending_model_frame = Some(frame);
                        return Ok(self.defer_drawable_unavailable());
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = candidate {
                    if let Some(token) = frame.model_commit {
                        reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        if overlay_visible {
                            match self.overlay.draw(self.frames_presented == 0) {
                                Ok(()) => self.retry_backoff.record_success(),
                                Err(error) if error.is_temporary_presentation_unavailable() => {
                                    return Ok(self.defer_drawable_unavailable());
                                }
                                Err(error) => return Err(error),
                            }
                            self.frames_presented = self.frames_presented.saturating_add(1);
                            self.overlay.set_visible(true)?;
                            return Ok(OverlayTickOutcome::Presented);
                        }
                        return Ok(OverlayTickOutcome::Hidden);
                    }
                    return Err(error);
                }
                if let Some(token) = frame.model_commit {
                    report_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        token,
                        ModelCommitOutcome::Prepared,
                    )?;
                }
                if frame.snapshot.as_ref() != self.previous_snapshot.as_ref() {
                    self.dynamic_snapshots = self.dynamic_snapshots.saturating_add(1);
                }
                self.last_frame = frame.clone();
                self.previous_snapshot = frame.snapshot;
                self.overlay = replacement;
                let mtm = MainThreadMarker::new().ok_or_else(|| {
                    OverlayError::new("macOS overlay model update lost the main thread")
                })?;
                self.refresh_right_button_monitor(mtm);
                self.frames_presented = self.frames_presented.saturating_add(1);
                return Ok(if overlay_visible {
                    OverlayTickOutcome::Presented
                } else {
                    OverlayTickOutcome::Hidden
                });
            }
            match self.overlay.sync_frame(&frame) {
                Ok(switched) => {
                    if let Some(token) = frame.model_commit {
                        report_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            token,
                            ModelCommitOutcome::Prepared,
                        )?;
                    }
                    if frame.snapshot.as_ref() != self.previous_snapshot.as_ref() {
                        self.dynamic_snapshots = self.dynamic_snapshots.saturating_add(1);
                    }
                    debug_assert!(!switched);
                    self.last_frame = frame.clone();
                    self.previous_snapshot = frame.snapshot;
                }
                Err(error) if frame.model_commit.is_some() => {
                    reject_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        frame.model_commit.expect("checked model commit token"),
                    )?;
                    self.model_commit_rejections = self.model_commit_rejections.saturating_add(1);
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
        if !overlay_visible {
            return Ok(OverlayTickOutcome::Hidden);
        }
        match self.overlay.draw(self.frames_presented == 0) {
            Ok(()) => self.retry_backoff.record_success(),
            Err(error) if error.is_temporary_presentation_unavailable() => {
                return Ok(self.defer_drawable_unavailable());
            }
            Err(error) => return Err(error),
        }
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.overlay.set_visible(true)?;
        Ok(OverlayTickOutcome::Presented)
    }

    fn defer_drawable_unavailable(&mut self) -> OverlayTickOutcome {
        OverlayTickOutcome::Deferred(self.retry_backoff.register_temporary_failure())
    }

    /// Advance the hover hide and push the resulting window presentation.
    ///
    /// Hover hide needs a trustworthy pointer position. A missing sample (no
    /// pointer event has arrived yet) and a platform input service that is not
    /// running both count as "not inside", so a degraded pointer pipeline can
    /// never leave the overlay stuck invisible.
    fn update_hover_presentation(
        &mut self,
        options: OverlaySessionOptions,
        cursor: Option<CursorSample>,
        input_running: bool,
    ) -> Result<(), OverlayError> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("macOS overlay hover update lost the main thread"))?;
        let bounds = self.window_bounds()?;
        // A right-button resize drag keeps the overlay visible: the hover hide
        // fades the window out and starts passing pointer events through, which
        // would end the drag.
        let resizing = self
            .right_button_monitor
            .as_ref()
            .is_some_and(RightButtonMonitor::is_resize_dragging);
        let pointer_inside = !resizing
            && cursor
                .and_then(|sample| appkit_cursor_position(sample, mtm))
                .is_some_and(|position| pointer_inside_window(bounds, position.x, position.y));
        let fade = self.hover.observe(PointerHoverObservation {
            enabled: options.hide_on_pointer_hover && input_running,
            delay: Duration::from_millis(u64::from(options.hide_on_pointer_hover_delay_ms)),
            pointer_inside,
            now: self.session_started.elapsed(),
        });
        let alpha = f64::from(options.opacity_percent) / 100.0 * fade;
        self.overlay
            .apply_presentation(alpha, options.click_through || self.hover.hidden());
        Ok(())
    }

    /// Create a native window that already carries the current hover fade.
    ///
    /// A replacement window is created with the configured opacity, so a
    /// settings change or model change while the overlay is hover-hidden would
    /// otherwise show the new window at full opacity before the next tick could
    /// correct it.
    fn create_overlay(
        &self,
        mtm: MainThreadMarker,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
    ) -> Result<NativeOverlay, OverlayError> {
        let mut overlay = NativeOverlay::create(mtm, frame, options, bounds)?;
        let alpha = f64::from(options.opacity_percent) / 100.0 * self.hover.visible();
        overlay.apply_presentation(alpha, options.click_through || self.hover.hidden());
        Ok(overlay)
    }

    pub(super) fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        let frame = self.overlay.panel.frame();
        OverlayWindowBounds::new(
            rounded_i32(frame.origin.x)?,
            rounded_i32(frame.origin.y)?,
            rounded_u32(frame.size.width)?,
            rounded_u32(frame.size.height)?,
        )
        .validate()
    }

    pub(super) fn is_visible(&self) -> bool {
        self.overlay.panel.isVisible()
    }

    pub(super) fn model_generation(&self) -> u64 {
        self.overlay.model_generation
    }

    pub(super) fn stop_input(&mut self) -> Result<(), OverlayError> {
        if self.input_stopped {
            return Ok(());
        }
        self.input_stopped = true;
        if let Some(service) = self.input_service.take() {
            self.input_diagnostics = Some(
                service
                    .stop()
                    .map_err(|error| OverlayError::new(error.to_string()))?,
            );
        }
        Ok(())
    }

    pub(super) fn finish_after_runtime_shutdown(
        mut self,
    ) -> Result<ProductOverlayReport, OverlayError> {
        self.remove_right_button_monitor();
        if !self.input_stopped {
            return Err(OverlayError::new(
                "platform input must stop before the runtime",
            ));
        }
        if self.runtime_client.snapshot().state != RuntimeState::Stopped {
            return Err(OverlayError::new(
                "runtime must stop before releasing the product overlay",
            ));
        }
        while self.render_consumer.take_latest().is_some() {}
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("macOS overlay shutdown lost the main thread"))?;
        let bounds = self.window_bounds()?;
        let placement_fully_visible = !self.options.keep_inside_screen
            || bounds_inside_screens(&screen_bounds_all(mtm)?, bounds);
        Ok(ProductOverlayReport {
            frames_presented: self.frames_presented,
            placement_fully_visible,
            dynamic_snapshots: self.dynamic_snapshots,
            model_commit_rejections: self.model_commit_rejections,
            input_start_error: self.input_start_error,
            input_diagnostics: self.input_diagnostics,
            render_diagnostics: self.render_consumer.diagnostics(),
            model_generation: self.overlay.model_generation,
            drawable_count: self.overlay.model.meshes.len(),
            masked_drawable_count: self.overlay.model.masked_drawable_count,
            texture_count: self.overlay.model.textures.len(),
        })
    }
}

impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let view = self
            .overlay
            .panel
            .contentView()
            .ok_or(HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(NonNull::from(&*view).cast());
        // SAFETY: the panel retains its content view while this session and the
        // returned borrow of it remain alive.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl ProductOverlaySession {
    /// The window size a resize drag treats as `100%`.
    ///
    /// It is the same size the window would be created with for the current
    /// model, so a drag maps onto the scale the settings page shows, and a
    /// model switch with a different canvas aspect ratio keeps the mapping
    /// correct because the base is recomputed from the current frame.
    fn resize_base(&self) -> Option<ResizeBase> {
        let (width, height) = default_overlay_window_dimensions(self.last_frame.snapshot.canvas);
        ResizeBase::new(f64::from(width), f64::from(height))
    }

    /// Whether the live window box already matches a scale.
    ///
    /// A resize drag resizes the window before the scale reaches the
    /// configuration, so the rebuild that follows the write-back must not scale
    /// the box a second time. See [`crate::bounds_match_scale`].
    fn bounds_match_scale(&self, bounds: OverlayWindowBounds, scale_percent: u16) -> bool {
        self.resize_base()
            .is_some_and(|base| crate::bounds_match_scale(bounds, base, scale_percent))
    }

    /// Re-install the right-button monitor for the current panel.
    ///
    /// A replacement window is a different panel with a different window
    /// number, so the monitor that belonged to the old one is dropped first.
    fn refresh_right_button_monitor(&mut self, mtm: MainThreadMarker) {
        self.remove_right_button_monitor();
        self.right_button_monitor = install_context_menu_monitor(
            mtm,
            Retained::clone(&self.overlay.panel),
            self.resize_base(),
            self.context_menu_sender.clone(),
            self.resize_sender.clone(),
        );
    }

    fn remove_right_button_monitor(&mut self) {
        if let Some(monitor) = self.right_button_monitor.take() {
            // SAFETY: this monitor was created by NSEvent for this session and is
            // removed on the AppKit main thread before its callback state drops.
            unsafe { NSEvent::removeMonitor(&monitor.token) };
        }
    }
}

impl Drop for ProductOverlaySession {
    fn drop(&mut self) {
        self.remove_right_button_monitor();
    }
}

fn reject_model_commit(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    token: ModelCommitToken,
) -> Result<(), OverlayError> {
    report_model_commit(
        runtime_client,
        render_consumer,
        token,
        ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
    )
}

fn report_model_commit(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    token: ModelCommitToken,
    outcome: ModelCommitOutcome,
) -> Result<(), OverlayError> {
    render_consumer
        .report_model_commit(ModelCommitFeedback { token, outcome })
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let completed = runtime_client
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("runtime did not finish the model commit"))?;
    let failure = completed
        .last_command_failure
        .filter(|failure| failure.sequence == token.command_sequence);
    match (outcome, failure) {
        (ModelCommitOutcome::Prepared, None)
        | (
            ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
            Some(bongocat_runtime::RuntimeCommandFailure {
                code: RuntimeRenderErrorCode::GpuPreparationFailed,
                ..
            }),
        ) => Ok(()),
        (ModelCommitOutcome::Prepared, Some(failure)) => Err(OverlayError::new(format!(
            "runtime rejected prepared model generation: {:?}",
            failure.code
        ))),
        (ModelCommitOutcome::Rejected(_), None) => Err(OverlayError::new(
            "runtime committed a renderer-rejected model generation",
        )),
        (ModelCommitOutcome::Rejected(_), Some(failure)) => Err(OverlayError::new(format!(
            "runtime reported the wrong model rejection: {:?}",
            failure.code
        ))),
    }
}

fn validate_product_options(options: OverlaySessionOptions) -> Result<(), OverlayError> {
    if let Some(bounds) = options.window_bounds {
        bounds.validate()?;
    }
    if !(25..=400).contains(&options.scale_percent) {
        return Err(OverlayError::new(
            "overlay scale must be between 25 and 400 percent",
        ));
    }
    if !(1..=100).contains(&options.opacity_percent) {
        return Err(OverlayError::new(
            "overlay opacity must be between 1 and 100 percent",
        ));
    }
    if options.corner_radius_percent > MAXIMUM_CORNER_RADIUS_PERCENT {
        return Err(OverlayError::new(
            "overlay corner radius must be between 0 and 50 percent",
        ));
    }
    if options.hide_on_pointer_hover_delay_ms > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS {
        return Err(OverlayError::new(
            "overlay hover hide delay must be between 0 and 60000 milliseconds",
        ));
    }
    if !maximum_fps_is_valid(options.maximum_fps) {
        return Err(OverlayError::new("overlay FPS must be between 15 and 240"));
    }
    Ok(())
}

fn main_window_level(always_on_top: bool) -> NSWindowLevel {
    if always_on_top {
        NSMainMenuWindowLevel
    } else {
        NSNormalWindowLevel
    }
}

/// Convert a cursor sample into the AppKit screen space used by `NSScreen` and
/// by the overlay panel's frame.
///
/// CoreGraphics measures the primary display from its top-left corner while
/// AppKit measures it from its bottom-left corner, so the vertical axis has to
/// be mirrored about the primary display's height before a cursor position can
/// be compared with a window frame. Both spaces use the same unit, so no
/// scaling is involved. The primary display is the one whose AppKit frame
/// origin is `(0, 0)`, which is also the display whose CoreGraphics bounds
/// start at `(0, 0)`.
///
/// Returns `None` when the primary display cannot be identified, which makes
/// the caller treat the pointer as unknown instead of guessing a position.
fn appkit_cursor_position(sample: CursorSample, mtm: MainThreadMarker) -> Option<CursorPosition> {
    let primary_height = NSScreen::screens(mtm).iter().find_map(|screen| {
        let frame = screen.frame();
        (frame.origin.x == 0.0 && frame.origin.y == 0.0).then_some(frame.size.height)
    })?;
    Some(CursorPosition {
        x: sample.position.x,
        y: primary_height - sample.position.y,
    })
}

#[cfg(test)]
mod product_options_tests {
    use super::*;

    #[test]
    fn product_options_accept_config_boundaries() {
        for options in [
            OverlaySessionOptions {
                scale_percent: 25,
                opacity_percent: 1,
                corner_radius_percent: 0,
                maximum_fps: 15,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                scale_percent: 400,
                opacity_percent: 100,
                corner_radius_percent: 50,
                hide_on_pointer_hover: true,
                hide_on_pointer_hover_delay_ms: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS,
                maximum_fps: 240,
                ..OverlaySessionOptions::default()
            },
        ] {
            validate_product_options(options).expect("valid product options");
        }
    }

    #[test]
    fn product_options_reject_values_outside_config_boundaries() {
        for options in [
            OverlaySessionOptions {
                scale_percent: 24,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                scale_percent: 401,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                opacity_percent: 0,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                corner_radius_percent: 51,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                hide_on_pointer_hover_delay_ms: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS + 1,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                maximum_fps: 14,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                maximum_fps: 241,
                ..OverlaySessionOptions::default()
            },
        ] {
            assert!(validate_product_options(options).is_err());
        }
    }

    #[test]
    fn main_window_level_tracks_always_on_top() {
        assert_eq!(main_window_level(true), NSMainMenuWindowLevel);
        assert_eq!(main_window_level(false), NSNormalWindowLevel);
    }
}

/// State shared by the right-button event monitor.
///
/// The monitor sees every right-button event in the process, so it holds the
/// panel it belongs to, the base size a drag scales from, and the sinks that
/// carry the resulting request back to the application. `Cell` is what lets a
/// plain `Fn` callback own the drag: AppKit's local monitor API takes no
/// mutable context, and every callback runs on the same main thread.
struct ResizeMonitorState {
    panel: Retained<NSPanel>,
    /// `None` when the panel's base size could not be derived, which leaves the
    /// right button a plain context-menu click.
    base: Option<ResizeBase>,
    drag: Cell<Option<ResizeDrag>>,
    context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
}

/// The right-button event monitor for one panel, together with the state its
/// callbacks read.
struct RightButtonMonitor {
    /// The token `NSEvent::removeMonitor` needs at shutdown.
    token: Retained<AnyObject>,
    /// Shared with the monitor callbacks, which run on the same main thread.
    state: Rc<ResizeMonitorState>,
}

impl RightButtonMonitor {
    /// Whether a right-button resize drag is in progress.
    ///
    /// The hover hide has to stand down while it is: the overlay fades out and
    /// starts passing pointer events through, which would end the drag.
    fn is_resize_dragging(&self) -> bool {
        self.state.drag.get().is_some()
    }
}

/// Install the right-button monitor for one panel.
///
/// A right button that moves past the drag threshold resizes the window; one
/// that does not is still the context-menu request it always was. The monitor
/// is installed whenever either handoff exists, because the two share the same
/// button and the same event.
fn install_context_menu_monitor(
    _: MainThreadMarker,
    panel: Retained<NSPanel>,
    base: Option<ResizeBase>,
    context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
) -> Option<RightButtonMonitor> {
    if context_menu_sender.is_none() && resize_sender.is_none() {
        return None;
    }
    let window_number = panel.windowNumber();
    let state = Rc::new(ResizeMonitorState {
        panel,
        base,
        drag: Cell::new(None),
        context_menu_sender,
        resize_sender,
    });
    let shared = Rc::clone(&state);
    let handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
        RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit supplies a valid NSEvent pointer for the duration of
            // this local event-monitor callback, and returns it to continue normal dispatch.
            let event_ref = unsafe { event.as_ref() };
            if event_ref.windowNumber() == window_number {
                match event_ref.r#type() {
                    objc2_app_kit::NSEventType::RightMouseDown => begin_resize(&state),
                    objc2_app_kit::NSEventType::RightMouseDragged => drag_resize(&state),
                    objc2_app_kit::NSEventType::RightMouseUp => finish_resize(&state),
                    _ => {}
                }
            }
            event.as_ptr()
        });
    // SAFETY: the handler returns AppKit's original valid event pointer and is
    // retained by the returned monitor token until session shutdown.
    let token = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::RightMouseDown
                | NSEventMask::RightMouseDragged
                | NSEventMask::RightMouseUp,
            &handler,
        )
    }?;
    Some(RightButtonMonitor {
        token,
        state: shared,
    })
}

/// The pointer position the resize math measures against.
///
/// AppKit's screen coordinates grow upwards while the drag math assumes a
/// downwards-positive axis (the legacy webview and the Windows adapter both use
/// one), so the vertical component is negated once, here. The window's own
/// coordinate space is deliberately not used: the drag changes the frame while
/// it runs, which would make the same screen position read differently from one
/// event to the next.
fn resize_pointer() -> (f64, f64) {
    let location = NSEvent::mouseLocation();
    (location.x, -location.y)
}

fn begin_resize(state: &ResizeMonitorState) {
    let Some(base) = state.base else {
        return;
    };
    // The drag starts from the width the panel actually has, so a box that
    // drifted from the stored scale does not jump on the first pointer move.
    let width = state.panel.frame().size.width.max(0.0).round() as u32;
    state.drag.set(Some(ResizeDrag::begin(
        resize_pointer(),
        base,
        base.scale_percent_for_width(width),
    )));
}

fn drag_resize(state: &ResizeMonitorState) {
    let Some(mut drag) = state.drag.take() else {
        return;
    };
    let outcome = drag.observe(resize_pointer());
    state.drag.set(Some(drag));
    if let Some(outcome) = outcome {
        apply_resize(state, outcome);
    }
}

fn finish_resize(state: &ResizeMonitorState) {
    let Some(drag) = state.drag.take() else {
        // A right button that was never observed going down is still a click:
        // the monitor can miss the press when the panel was replaced mid-click.
        request_context_menu(state);
        return;
    };
    if !drag.dragging() {
        request_context_menu(state);
        return;
    }
    let Some(scale_percent) = drag.finish() else {
        return;
    };
    if let Some(sender) = &state.resize_sender {
        let _ = sender.try_send(OverlayResizeOutcome { scale_percent });
    }
}

fn request_context_menu(state: &ResizeMonitorState) {
    if let Some(sender) = &state.context_menu_sender {
        let _ = sender.try_send(OverlayContextMenuRequest);
    }
}

/// Apply one drag step to the panel, keeping the window's top edge where it is.
///
/// AppKit window frames are anchored at their bottom-left corner, so growing a
/// window from a fixed origin would move its top edge up the screen. The origin
/// is corrected instead, which is what makes a right-button drag feel like the
/// legacy `setSize` call: the top-left corner the user grabbed stays put.
fn apply_resize(state: &ResizeMonitorState, outcome: ResizeOutcome) {
    let frame = state.panel.frame();
    let top = frame.origin.y + frame.size.height;
    let size = NSSize::new(f64::from(outcome.width), f64::from(outcome.height));
    state.panel.setFrame_display(
        NSRect::new(NSPoint::new(frame.origin.x, top - size.height), size),
        true,
    );
}

fn pump_application_events(application: &NSApplication) {
    autoreleasepool(|_| {
        let deadline = NSDate::dateWithTimeIntervalSinceNow(0.0);
        // SAFETY: AppKit exports this immutable process-global run-loop mode
        // for use on the application main thread.
        let run_loop_mode = unsafe { NSDefaultRunLoopMode };
        if let Some(event) = application.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&deadline),
            run_loop_mode,
            true,
        ) {
            application.sendEvent(&event);
        }
        application.updateWindows();
    });
}

pub(crate) fn run_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
    interactive: bool,
    switch_cycles: Option<u32>,
) -> Result<PreviewReport, OverlayError> {
    if interactive && switch_cycles.is_some() {
        return Err(OverlayError::new(
            "interactive input and model-switch probing cannot run together",
        ));
    }
    if switch_cycles == Some(0) {
        return Err(OverlayError::new(
            "model-switch cycle count must be greater than zero",
        ));
    }
    let model_id =
        ModelId::parse(model_id).map_err(|error| OverlayError::new(error.to_string()))?;
    let preset_root = model_root
        .parent()
        .ok_or_else(|| OverlayError::new("preset model root has no catalog parent"))?;
    let catalog = PresetModelCatalog::open(preset_root, ModelPackageLimits::default())
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let committed = Arc::new(
        catalog
            .load(&model_id)
            .map_err(|error| OverlayError::new(error.to_string()))?,
    );
    let switch_models = switch_cycles
        .map(|_| {
            PRESET_MODEL_IDS
                .iter()
                .map(|id| {
                    let id = ModelId::parse(*id)
                        .map_err(|error| OverlayError::new(error.to_string()))?;
                    catalog
                        .load(&id)
                        .map(Arc::new)
                        .map_err(|error| OverlayError::new(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
    let runtime_client = runtime.client();
    runtime_client
        .wait_for_revision(1, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview runtime did not become ready"))?;
    let binding_sequence = runtime_client
        .send(RuntimeCommand::SetInputBindings(std::sync::Arc::new(
            preview_input_bindings(model_id.as_str()),
        )))
        .map_err(|error| OverlayError::new(error.to_string()))?;
    runtime_client
        .wait_for_command(binding_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview input bindings were not applied"))?;
    let activation_sequence = runtime_client
        .send(RuntimeCommand::ActivateModel(Arc::clone(&committed)))
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let prepared = runtime_client
        .wait_for_model_preparation(activation_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview model activation was not prepared"))?;
    if let Some(failure) = prepared
        .last_command_failure
        .filter(|failure| failure.sequence == activation_sequence)
    {
        return Err(OverlayError::new(format!(
            "preview model activation failed: {:?}",
            failure.code
        )));
    }
    let initial_frame = render_consumer
        .take_latest()
        .ok_or_else(|| OverlayError::new("runtime did not publish the initial render frame"))?;
    let initial_token = initial_frame
        .model_commit
        .filter(|token| token.command_sequence == activation_sequence)
        .ok_or_else(|| OverlayError::new("initial frame has the wrong model commit token"))?;
    let mut previous_snapshot = std::sync::Arc::clone(&initial_frame.snapshot);
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| OverlayError::new("macOS preview must run on the main thread"))?;
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    application.finishLaunching();
    let mut overlay =
        match NativeOverlay::create(mtm, &initial_frame, OverlaySessionOptions::default(), None) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
                return Err(error);
            }
        };
    report_model_commit(
        &runtime_client,
        &render_consumer,
        initial_token,
        ModelCommitOutcome::Prepared,
    )?;
    let failed_gpu_prepare_preserved = if switch_cycles.is_some() {
        let mut invalid_resources = initial_frame.resources.as_ref().clone();
        let Some(first_texture) = invalid_resources.textures.first_mut() else {
            return Err(OverlayError::new(
                "model-switch probe requires at least one texture",
            ));
        };
        first_texture.path = model_root.join(".missing-gpu-prepare-texture.png");
        let probe_frame = RenderFrame {
            transport_sequence: initial_frame.transport_sequence.saturating_add(1),
            model_generation: initial_frame.model_generation.saturating_add(1),
            frame_number: 0,
            model_commit: None,
            resources: Arc::new(invalid_resources),
            snapshot: Arc::clone(&initial_frame.snapshot),
        };
        let generation_before = overlay.model_generation;
        if overlay.sync_frame(&probe_frame).is_ok() {
            return Err(OverlayError::new(
                "invalid GPU model preparation unexpectedly succeeded",
            ));
        }
        if overlay.model_generation != generation_before {
            return Err(OverlayError::new(
                "failed GPU model preparation replaced the active generation",
            ));
        }
        true
    } else {
        false
    };
    let mut frame_timing = FrameTimingCollector::new();
    let draw_started = Instant::now();
    overlay.draw(true)?;
    frame_timing.record_draw(draw_started.elapsed());
    overlay.set_visible(true)?;

    let input_producer = runtime.input_producer();
    let cursor_producer = runtime.cursor_producer();
    let gamepad_axis_producer = runtime.gamepad_axis_producer();
    let mut input_driver = PreviewInputDriver::default();
    let input_service = interactive
        .then(|| {
            MacInputService::start(
                input_producer.clone(),
                cursor_producer.clone(),
                gamepad_axis_producer.clone(),
            )
        })
        .transpose()
        .map_err(|error| OverlayError::new(error.to_string()))?;

    let started = Instant::now();
    let mut next_frame = started;
    let mut frames_presented = 1_u64;
    let mut dynamic_snapshots = 0_u64;
    let target_model_switches =
        switch_cycles.map(|cycles| u64::from(cycles).saturating_mul(PRESET_MODEL_IDS.len() as u64));
    let mut model_switch_commands = 0_u64;
    let mut model_switches = 0_u64;
    let mut current_model_index = if switch_cycles.is_some() {
        PRESET_MODEL_IDS
            .iter()
            .position(|id| *id == model_id.as_str())
            .ok_or_else(|| OverlayError::new("initial preset is not in the switch sequence"))?
    } else {
        0
    };
    let mut settle_frames = 0_u64;
    let mut metal_bytes_before = None;
    while overlay.panel.isVisible()
        && target_model_switches.map_or_else(
            || duration.is_zero() || started.elapsed() < duration,
            |target| model_switches < target || settle_frames < SWITCH_SETTLE_FRAMES,
        )
    {
        pump_application_events(&application);
        let elapsed = started.elapsed();
        if !interactive
            && let Some(sequence) = input_driver.update(
                model_id.as_str(),
                elapsed,
                &input_producer,
                &cursor_producer,
            )?
        {
            runtime_client
                .wait_for_input_sequence(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("preview input did not reach the runtime"))?;
        }
        if let (Some(target), Some(models)) = (target_model_switches, switch_models.as_ref())
            && frames_presented >= SWITCH_WARMUP_FRAMES
            && model_switch_commands == model_switches
            && model_switch_commands < target
        {
            metal_bytes_before.get_or_insert_with(|| overlay.current_allocated_size());
            current_model_index = (current_model_index + 1) % models.len();
            let sequence = runtime_client
                .send(RuntimeCommand::ActivateModel(Arc::clone(
                    &models[current_model_index],
                )))
                .map_err(|error| OverlayError::new(error.to_string()))?;
            let prepared = runtime_client
                .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("model switch was not prepared"))?;
            if let Some(failure) = prepared
                .last_command_failure
                .filter(|failure| failure.sequence == sequence)
            {
                return Err(OverlayError::new(format!(
                    "model switch failed: {:?}",
                    failure.code
                )));
            }
            model_switch_commands = model_switch_commands.saturating_add(1);
        }
        let mut gpu_model_switched = false;
        if let Some(frame) = render_consumer.take_latest() {
            if frame.snapshot.as_ref() != previous_snapshot.as_ref() {
                dynamic_snapshots = dynamic_snapshots.saturating_add(1);
            }
            let previous_generation = overlay.model_generation;
            gpu_model_switched = match overlay.sync_frame(&frame) {
                Ok(switched) => switched,
                Err(error) if frame.model_commit.is_some() => {
                    reject_model_commit(
                        &runtime_client,
                        &render_consumer,
                        frame.model_commit.expect("checked model commit token"),
                    )?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            if gpu_model_switched {
                overlay.resize_for_model(frame.snapshot.canvas)?;
                let token = frame
                    .model_commit
                    .ok_or_else(|| OverlayError::new("model switch frame has no commit token"))?;
                report_model_commit(
                    &runtime_client,
                    &render_consumer,
                    token,
                    ModelCommitOutcome::Prepared,
                )?;
                debug_assert_eq!(
                    frame.model_generation,
                    previous_generation.saturating_add(1)
                );
                model_switches = model_switches.saturating_add(1);
            }
            previous_snapshot = frame.snapshot;
        }
        let draw_started = Instant::now();
        overlay.draw(gpu_model_switched)?;
        frame_timing.record_draw(draw_started.elapsed());
        frames_presented += 1;
        if target_model_switches.is_some_and(|target| model_switches == target) {
            settle_frames = settle_frames.saturating_add(1);
        }
        next_frame += FRAME_INTERVAL;
        if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
        } else {
            frame_timing.record_missed_deadline();
            next_frame = Instant::now();
        }
    }

    if let Some(target) = target_model_switches
        && (model_switch_commands != target || model_switches != target)
    {
        return Err(OverlayError::new(format!(
            "model-switch preview stopped after {model_switches}/{target} committed GPU switches"
        )));
    }
    let metal_bytes_before = metal_bytes_before.unwrap_or_else(|| overlay.current_allocated_size());
    let metal_bytes_after = overlay.current_allocated_size();
    if target_model_switches.is_some() && metal_bytes_after > metal_bytes_before {
        return Err(OverlayError::new(format!(
            "Metal allocation grew from {metal_bytes_before} to {metal_bytes_after} bytes during model switching"
        )));
    }

    let (platform_input_edges, platform_cursor_samples) = if let Some(input_service) = input_service
    {
        let diagnostics = input_service
            .stop()
            .map_err(|error| OverlayError::new(error.to_string()))?;
        (diagnostics.consumed_edges, diagnostics.cursor_consumed)
    } else {
        if let Some(sequence) = input_driver.release_all(started.elapsed(), &input_producer)? {
            runtime_client
                .wait_for_input_sequence(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("preview releases did not reach the runtime"))?;
        }
        (0, 0)
    };
    let stopped = runtime
        .shutdown(RUNTIME_TIMEOUT)
        .map_err(|error| OverlayError::new(error.to_string()))?;
    while render_consumer.take_latest().is_some() {}
    let render_diagnostics = render_consumer.diagnostics();

    Ok(PreviewReport {
        frames_presented,
        dynamic_snapshots,
        runtime_input_events: stopped.input.transport.enqueued,
        platform_input_edges,
        runtime_cursor_published: stopped.cursor.transport.published,
        runtime_cursor_coalesced: stopped.cursor.transport.coalesced,
        runtime_cursor_consumed: stopped.cursor.transport.consumed,
        platform_cursor_samples,
        render_frames_published: render_diagnostics.published,
        render_frames_coalesced: render_diagnostics.coalesced,
        render_frames_consumed: render_diagnostics.consumed,
        model_switches,
        failed_gpu_prepare_preserved,
        gpu_bytes_before: metal_bytes_before,
        gpu_bytes_after: metal_bytes_after,
        drawable_count: overlay.model.meshes.len(),
        masked_drawable_count: overlay.model.masked_drawable_count,
        texture_count: overlay.model.textures.len(),
        warmup_thread_high_water: None,
        threads_after: None,
        frame_timing: Some(frame_timing.summary()),
    })
}

/// One model cover capture, drawn a frame at a time.
///
/// This is the model preview's own setup — a private runtime, the model activated
/// into it, and a native window sized from the model's canvas — with two
/// differences that make it a capture rather than a preview: the window is never
/// ordered front, so nothing the user did not ask for appears on screen, and the
/// frame is read out of the drawable instead of only sampled.
///
/// It must run on the main thread, like every other AppKit window owner. The
/// session exists so the owner does not have to stay busy for the whole capture:
/// `start` sets everything up and draws the first, verified frame, every `step`
/// draws one more frame, and `finish` tears the runtime down. The caller waits
/// between steps with [`Self::frame_interval`], which is what keeps the product's
/// windows redrawing while the capture's model settles (ADR-0055).
pub(crate) struct CoverCaptureSession {
    mtm: MainThreadMarker,
    overlay: NativeOverlay,
    runtime: RuntimeOwner,
    render_consumer: RenderConsumer,
    captured: Option<CapturedFrame>,
    deadline: Instant,
    frames_drawn: u32,
    frame_interval: Duration,
}

impl CoverCaptureSession {
    pub(crate) fn start(model: Arc<CommittedModel>) -> Result<Self, OverlayError> {
        let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
        let runtime_client = runtime.client();
        runtime_client
            .wait_for_revision(1, RUNTIME_TIMEOUT)
            .ok_or_else(|| OverlayError::new("cover capture runtime did not become ready"))?;
        let activation = runtime_client
            .send(RuntimeCommand::ActivateModel(model))
            .map_err(|error| OverlayError::new(error.to_string()))?;
        let prepared = runtime_client
            .wait_for_model_preparation(activation, RUNTIME_TIMEOUT)
            .ok_or_else(|| OverlayError::new("cover capture model activation was not prepared"))?;
        if let Some(failure) = prepared
            .last_command_failure
            .filter(|failure| failure.sequence == activation)
        {
            return Err(OverlayError::new(format!(
                "cover capture model activation failed: {:?}",
                failure.code
            )));
        }
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("cover capture runtime published no render frame"))?;
        let initial_token = initial_frame
            .model_commit
            .filter(|token| token.command_sequence == activation)
            .ok_or_else(|| {
                OverlayError::new("cover capture frame has the wrong model commit token")
            })?;
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("cover capture must run on the main thread"))?;
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        application.finishLaunching();
        let options = OverlaySessionOptions {
            scale_percent: COVER_CAPTURE_SCALE_PERCENT,
            keep_inside_screen: false,
            ..OverlaySessionOptions::default()
        };
        let mut overlay = match NativeOverlay::create(mtm, &initial_frame, options, None) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
                return Err(error);
            }
        };
        report_model_commit(
            &runtime_client,
            &render_consumer,
            initial_token,
            ModelCommitOutcome::Prepared,
        )?;

        // The first frame is verified like a committed model is: a capture that
        // renders nothing must fail before it replaces a cover with a blank image.
        let captured = Some(overlay.draw_capturing(true)?);
        Ok(Self {
            mtm,
            overlay,
            runtime,
            render_consumer,
            captured,
            deadline: Instant::now() + COVER_CAPTURE_TIMEOUT,
            frames_drawn: 1,
            frame_interval: FRAME_INTERVAL,
        })
    }

    /// The wait the caller should observe between two `step` calls so the
    /// capture's model animates at the pace the overlay renders at.
    pub(crate) fn frame_interval(&self) -> Duration {
        self.frame_interval
    }

    /// Draw one more capture frame. `Ok(false)` means every frame is drawn or
    /// the deadline has passed, and `finish` is the next call.
    pub(crate) fn step(&mut self) -> Result<bool, OverlayError> {
        if self.frames_drawn >= COVER_CAPTURE_FRAMES || Instant::now() >= self.deadline {
            return Ok(false);
        }
        pump_application_events(&NSApplication::sharedApplication(self.mtm));
        if let Some(frame) = self.render_consumer.take_latest() {
            self.overlay.sync_frame(&frame)?;
        }
        self.captured = Some(self.overlay.draw_capturing(false)?);
        self.frames_drawn += 1;
        Ok(true)
    }

    /// Stop the capture runtime and return the most recent captured frame.
    ///
    /// Dropping the session without `finish` (a failed `step`, an abandoned
    /// capture) tears the runtime down through its own `Drop`, exactly like the
    /// early returns of a blocking capture did.
    pub(crate) fn finish(self) -> Result<CapturedFrame, OverlayError> {
        self.runtime
            .shutdown(RUNTIME_TIMEOUT)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        self.captured
            .ok_or_else(|| OverlayError::new("cover capture drew no frame"))
    }
}

impl NativeOverlay {
    fn create(
        mtm: MainThreadMarker,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
    ) -> Result<Self, OverlayError> {
        // A saved box that no longer touches any display is only restorable when
        // the placement constraint can pull it back onto one. With the
        // constraint enabled the correction below is what makes such a box
        // usable, so the box is kept as the candidate and clamped; without it
        // there is nothing to recover the window with, and restoring a box that
        // no longer intersects a display would leave an unreachable window, so
        // the window falls back to the cursor's display instead.
        let bounds = bounds
            .filter(|bounds| options.keep_inside_screen || overlay_bounds_visible(mtm, *bounds));
        let (default_width, default_height) =
            model_window_dimensions(frame.snapshot.canvas, options.scale_percent);
        let window_width =
            bounds.map_or(f64::from(default_width), |bounds| f64::from(bounds.width));
        let window_height =
            bounds.map_or(f64::from(default_height), |bounds| f64::from(bounds.height));
        let origin = bounds.map_or_else(
            || centered_origin(mtm, window_width, window_height),
            |bounds| NSPoint::new(f64::from(bounds.x), f64::from(bounds.y)),
        );
        let candidate_bounds = OverlayWindowBounds::new(
            rounded_i32(origin.x)?,
            rounded_i32(origin.y)?,
            rounded_u32(window_width)?,
            rounded_u32(window_height)?,
        );
        // A brand new window has no drag to interrupt, so an unusable placement
        // is corrected immediately: this is the restore path for a saved box and
        // the centering path for a window that has never been placed.
        let origin = if options.keep_inside_screen {
            let screens = screen_bounds_all(mtm)?;
            if bounds_inside_screens(&screens, candidate_bounds) {
                origin
            } else {
                correction_for_screens(&screens, candidate_bounds).map_or(origin, |corrected| {
                    NSPoint::new(f64::from(corrected.x), f64::from(corrected.y))
                })
            }
        } else {
            origin
        };
        let window_frame = NSRect::new(origin, NSSize::new(window_width, window_height));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            window_frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setHasShadow(false);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setAlphaValue(f64::from(options.opacity_percent) / 100.0);
        panel.setLevel(main_window_level(options.always_on_top));
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setMovableByWindowBackground(true);
        panel.setIgnoresMouseEvents(options.click_through);

        let view = NSView::new(mtm);
        view.setWantsLayer(true);
        let device = Device::system_default()
            .ok_or_else(|| OverlayError::new("Metal device is unavailable"))?;
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(COLOR_ATTACHMENT_FORMAT);
        layer.set_opaque(false);
        layer.set_presents_with_transaction(false);
        layer.set_framebuffer_only(false);
        let scale = panel.backingScaleFactor();
        layer.set_drawable_size(core_graphics_types::geometry::CGSize::new(
            window_width * scale,
            window_height * scale,
        ));
        // SAFETY: metal::MetalLayerRef and objc2 QuartzCore both wrap the
        // same Objective-C CAMetalLayer instance, which NSView retains.
        let layer_ref =
            unsafe { mem::transmute::<&metal::MetalLayerRef, &ObjcMetalLayer>(layer.as_ref()) };
        // A headless or temporarily occluded compositor must not leave
        // nextDrawable waiting forever; return None so the caller can report
        // a recoverable renderer failure instead.
        layer_ref.setAllowsNextDrawableTimeout(true);
        layer_ref.setMaximumDrawableCount(3);
        view.setLayer(Some(layer_ref));
        panel.setContentView(Some(&view));

        let pipelines = create_pipelines(&device)?;
        let sampler_descriptor = SamplerDescriptor::new();
        sampler_descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
        sampler_descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
        let sampler = device.new_sampler(&sampler_descriptor);
        let drawable_width = layer.drawable_size().width.round() as u64;
        let drawable_height = layer.drawable_size().height.round() as u64;
        let model = GpuModel::prepare(
            &device,
            &frame.resources,
            &frame.snapshot,
            drawable_width,
            drawable_height,
        )?;
        Ok(Self {
            panel: ManuallyDrop::new(panel),
            device: device.clone(),
            layer,
            queue: device.new_command_queue(),
            pipelines,
            sampler,
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model,
            presentation: OverlayPresentationState::default(),
            corner_radius_percent: options.corner_radius_percent,
            applied_alpha: f64::from(options.opacity_percent) / 100.0,
            applied_click_through: options.click_through,
        })
    }

    /// Apply the per-frame presentation state without replacing the window.
    ///
    /// `alpha` is the configured window opacity multiplied by the hover fade,
    /// and `click_through` is the effective pointer routing. The hover hide
    /// forces pass-through on so an invisible overlay cannot swallow a click
    /// meant for whatever is underneath it.
    fn apply_presentation(&mut self, alpha: f64, click_through: bool) {
        if alpha != self.applied_alpha {
            self.panel.setAlphaValue(alpha);
            self.applied_alpha = alpha;
        }
        if click_through != self.applied_click_through {
            self.set_click_through(click_through);
            self.applied_click_through = click_through;
        }
    }

    /// Move the window to a corrected box without touching its size, z-order or
    /// activation. The caller has already compared the box against the live
    /// frame, so an unchanged origin is a no-op.
    fn set_origin(&self, bounds: OverlayWindowBounds) {
        let origin = NSPoint::new(f64::from(bounds.x), f64::from(bounds.y));
        if self.panel.frame().origin != origin {
            self.panel.setFrameOrigin(origin);
        }
    }

    /// Adapt the panel to a newly prepared model while keeping the live width.
    ///
    /// The model-switch probe updates the GPU model in place instead of
    /// replacing the panel, so it has to apply the same canvas-aspect rule as
    /// the product session explicitly. The resize happens after GPU
    /// preparation succeeds; a failed preparation therefore leaves the old
    /// window geometry untouched.
    fn resize_for_model(&mut self, canvas: CanvasInfo) -> Result<(), OverlayError> {
        let frame = self.panel.frame();
        let current = OverlayWindowBounds::new(
            rounded_i32(frame.origin.x)?,
            rounded_i32(frame.origin.y)?,
            rounded_u32(frame.size.width)?,
            rounded_u32(frame.size.height)?,
        );
        let target = model_switch_window_bounds(current, canvas);
        let target_frame = NSRect::new(
            NSPoint::new(f64::from(target.x), f64::from(target.y)),
            NSSize::new(f64::from(target.width), f64::from(target.height)),
        );
        if frame.origin.x != target_frame.origin.x
            || frame.origin.y != target_frame.origin.y
            || frame.size.width != target_frame.size.width
            || frame.size.height != target_frame.size.height
        {
            self.panel.setFrame_display(target_frame, true);
        }
        self.sync_window_size()?;
        Ok(())
    }

    fn sync_frame(&mut self, frame: &RenderFrame) -> Result<bool, OverlayError> {
        if frame.model_generation != self.model_generation {
            validate_model_generation_advance(self.model_generation, frame.model_generation)?;
            let drawable_width = self.layer.drawable_size().width.round() as u64;
            let drawable_height = self.layer.drawable_size().height.round() as u64;
            let prepared = GpuModel::prepare(
                &self.device,
                &frame.resources,
                &frame.snapshot,
                drawable_width,
                drawable_height,
            )?;
            self.model = prepared;
            self.resources = Arc::clone(&frame.resources);
            self.model_generation = frame.model_generation;
            return Ok(true);
        }
        if !Arc::ptr_eq(&self.resources, &frame.resources) {
            return Err(OverlayError::new(
                "render resources changed within one model generation",
            ));
        }
        self.model.sync_snapshot(&frame.snapshot)?;
        Ok(false)
    }

    fn draw(&mut self, verify_frame: bool) -> Result<(), OverlayError> {
        autoreleasepool(|_| self.draw_in_autorelease_pool(verify_frame, false))?;
        self.presentation.record_presented_frame();
        Ok(())
    }

    /// Draw one frame and read it back as cover pixels.
    ///
    /// The window is never ordered front for this: the frame is drawn into the
    /// layer's drawable and read from it before anything reaches the screen, so
    /// the capture cannot flash a window the user did not ask for.
    fn draw_capturing(&mut self, verify_frame: bool) -> Result<CapturedFrame, OverlayError> {
        let captured = autoreleasepool(|_| self.draw_in_autorelease_pool(verify_frame, true))?;
        self.presentation.record_presented_frame();
        captured.ok_or_else(|| OverlayError::new("captured frame was not read back"))
    }

    fn set_visible(&self, visible: bool) -> Result<(), OverlayError> {
        if visible {
            self.presentation.require_presented_frame()?;
            // The frame loop calls this method after every successful draw.
            // Ordering an already visible panel frontmost would continually
            // raise it above other windows, even when always-on-top is off.
            if !self.panel.isVisible() {
                self.panel.orderFrontRegardless();
            }
            if !self.panel.isVisible() {
                return Err(OverlayError::new("macOS overlay did not become visible"));
            }
        } else if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
        Ok(())
    }

    fn set_always_on_top(&self, always_on_top: bool) {
        self.panel.setLevel(main_window_level(always_on_top));
    }

    fn set_click_through(&self, click_through: bool) {
        self.panel.setIgnoresMouseEvents(click_through);
    }

    /// Align the Metal drawable and the mask targets with the panel's frame.
    ///
    /// A right-button resize drag changes the panel frame directly, so the
    /// layer and the mask textures keep the size they were created with until
    /// this runs. Everything else in the renderer reads the drawable size per
    /// frame — the model transform, the corner radius, the mask uniforms — so
    /// re-sizing the drawable is what makes the next frame match the window.
    ///
    /// Returns whether the size changed.
    fn sync_window_size(&mut self) -> Result<bool, OverlayError> {
        let frame = self.panel.frame();
        let backing = self.panel.backingScaleFactor();
        let width = (frame.size.width * backing).round().max(1.0) as u64;
        let height = (frame.size.height * backing).round().max(1.0) as u64;
        let current = self.layer.drawable_size();
        if current.width.round().max(1.0) as u64 == width
            && current.height.round().max(1.0) as u64 == height
        {
            return Ok(false);
        }
        self.layer
            .set_drawable_size(core_graphics_types::geometry::CGSize::new(
                width as f64,
                height as f64,
            ));
        self.model.resize_masks(&self.device, width, height);
        Ok(true)
    }

    fn draw_in_autorelease_pool(
        &self,
        verify_frame: bool,
        capture_frame: bool,
    ) -> Result<Option<CapturedFrame>, OverlayError> {
        let drawable = self.layer.next_drawable().ok_or_else(|| {
            OverlayError::temporary_presentation_unavailable("CAMetalLayer returned no drawable")
        })?;
        let pass = RenderPassDescriptor::new();
        let attachment = pass
            .color_attachments()
            .object_at(0)
            .ok_or_else(|| OverlayError::new("Metal color attachment is unavailable"))?;
        attachment.set_texture(Some(drawable.texture()));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_store_action(MTLStoreAction::Store);
        attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        let command_buffer = self.queue.new_command_buffer();
        let scale_offset = model_transform(
            self.model.bounds,
            drawable.texture().width() as f32,
            drawable.texture().height() as f32,
            self.model.mirror_horizontal,
        );
        let corner_radius = corner_radius_uniform(
            self.corner_radius_percent,
            drawable.texture().width() as f32,
            drawable.texture().height() as f32,
        );
        for mesh in &self.model.meshes {
            let Some(mask_texture) = &mesh.mask_texture else {
                continue;
            };
            let mask_pass = RenderPassDescriptor::new();
            let mask_attachment = mask_pass
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| OverlayError::new("Metal mask attachment is unavailable"))?;
            mask_attachment.set_texture(Some(mask_texture));
            mask_attachment.set_load_action(MTLLoadAction::Clear);
            mask_attachment.set_store_action(MTLStoreAction::Store);
            mask_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            let mask_encoder = command_buffer.new_render_command_encoder(mask_pass);
            mask_encoder.set_render_pipeline_state(&self.pipelines.mask);
            for source_id in &mesh.masks {
                let source = self
                    .model
                    .meshes
                    .iter()
                    .find(|source| source.id == *source_id)
                    .ok_or_else(|| {
                        OverlayError::new(format!("mask source {source_id} is unavailable"))
                    })?;
                let uniforms = Uniforms {
                    scale_offset,
                    multiply_color: [1.0; 4],
                    screen_color: [0.0; 4],
                    mask_settings: [0.0; 4],
                    corner_radius: [0.0; 4],
                    opacity: 1.0,
                    padding: [0.0; 3],
                };
                mask_encoder.set_vertex_buffer(0, Some(&source.vertex_buffer), 0);
                mask_encoder.set_vertex_bytes(
                    1,
                    size_of::<Uniforms>() as u64,
                    std::ptr::from_ref(&uniforms).cast(),
                );
                let texture = self.model.textures.get(&source.texture_id).ok_or_else(|| {
                    OverlayError::new(format!("texture {} is unavailable", source.texture_id))
                })?;
                mask_encoder.set_fragment_texture(0, Some(texture));
                mask_encoder.set_fragment_sampler_state(0, Some(&self.sampler));
                mask_encoder.draw_indexed_primitives(
                    MTLPrimitiveType::Triangle,
                    source.index_count,
                    MTLIndexType::UInt16,
                    &source.index_buffer,
                    0,
                );
            }
            mask_encoder.end_encoding();
        }

        let encoder = command_buffer.new_render_command_encoder(pass);
        if let Some(background) = &self.model.background {
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: [1.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(&self.pipelines.normal);
            encoder.set_vertex_buffer(0, Some(&self.model.background_vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_texture(0, Some(background));
            encoder.set_fragment_texture(1, Some(&self.model.empty_mask));
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                6,
                MTLIndexType::UInt16,
                &self.model.background_index_buffer,
                0,
            );
        }
        for mesh in &self.model.meshes {
            if !mesh.visible || mesh.opacity <= 0.0 {
                continue;
            }
            let mask_texture = &mesh.mask_texture;
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: mesh.multiply_color,
                screen_color: mesh.screen_color,
                mask_settings: [
                    drawable.texture().width() as f32,
                    drawable.texture().height() as f32,
                    f32::from(mask_texture.is_some()),
                    f32::from(mesh.inverted_mask),
                ],
                corner_radius,
                opacity: mesh.opacity * self.model.model_opacity,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(self.pipelines.for_mode(mesh.blend_mode));
            encoder.set_vertex_buffer(0, Some(&mesh.vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            let texture = self.model.textures.get(&mesh.texture_id).ok_or_else(|| {
                OverlayError::new(format!("texture {} is unavailable", mesh.texture_id))
            })?;
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_texture(
                1,
                Some(mask_texture.as_ref().unwrap_or(&self.model.empty_mask)),
            );
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                mesh.index_count,
                MTLIndexType::UInt16,
                &mesh.index_buffer,
                0,
            );
        }
        // Key overlays are the topmost layer so pressed-key imagery remains
        // visible above both the background and Live2D model drawables.
        for overlay in &self.model.active_keys {
            let Some(texture) = self.model.key_textures.get(&overlay.asset_id) else {
                continue;
            };
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: [1.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(&self.pipelines.normal);
            encoder.set_vertex_buffer(0, Some(&self.model.background_vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_texture(1, Some(&self.model.empty_mask));
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                6,
                MTLIndexType::UInt16,
                &self.model.background_index_buffer,
                0,
            );
        }
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();
        // Shared per-drawable buffers cannot be rewritten until this frame
        // retires. A later renderer revision will replace this correctness
        // fence with multiple in-flight frame resources.
        let completion_deadline = Instant::now() + METAL_COMPLETION_TIMEOUT;
        loop {
            match command_buffer.status() {
                MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error => break,
                _ if Instant::now() >= completion_deadline => break,
                _ => thread::sleep(Duration::from_millis(1)),
            }
        }
        if command_buffer.status() != MTLCommandBufferStatus::Completed {
            return Err(OverlayError::new(format!(
                "Metal command buffer ended with {:?}",
                command_buffer.status()
            )));
        }
        if verify_frame {
            verify_frame_smoke(drawable.texture())?;
        }
        if capture_frame {
            return read_drawable_frame(drawable.texture()).map(Some);
        }
        Ok(None)
    }

    fn current_allocated_size(&self) -> u64 {
        self.device.current_allocated_size()
    }
}

fn centered_origin(mtm: MainThreadMarker, width: f64, height: f64) -> NSPoint {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            mouse.x >= frame.origin.x
                && mouse.x < frame.origin.x + frame.size.width
                && mouse.y >= frame.origin.y
                && mouse.y < frame.origin.y + frame.size.height
        })
        .map(|screen| screen.frame())
        .or_else(|| NSScreen::mainScreen(mtm).map(|screen| screen.frame()));
    screen.map_or(NSPoint::new(80.0, 80.0), |screen| {
        NSPoint::new(
            screen.origin.x + (screen.size.width - width) / 2.0,
            screen.origin.y + (screen.size.height - height) / 2.0,
        )
    })
}

/// Every display's full frame, including the strips the menu bar and the Dock
/// occupy, so the placement constraint allows the overlay over desktop chrome
/// while still keeping it on a screen.
fn screen_bounds_all(mtm: MainThreadMarker) -> Result<Vec<OverlayScreenBounds>, OverlayError> {
    let mut bounds = Vec::new();
    for screen in NSScreen::screens(mtm) {
        let frame = screen.frame();
        bounds.push(OverlayScreenBounds {
            x: rounded_i32(frame.origin.x)?,
            y: rounded_i32(frame.origin.y)?,
            width: rounded_u32(frame.size.width)?,
            height: rounded_u32(frame.size.height)?,
        });
    }
    Ok(bounds)
}

fn overlay_bounds_visible(mtm: MainThreadMarker, bounds: OverlayWindowBounds) -> bool {
    let left = f64::from(bounds.x);
    let bottom = f64::from(bounds.y);
    let right = left + f64::from(bounds.width);
    let top = bottom + f64::from(bounds.height);
    NSScreen::screens(mtm).iter().any(|screen| {
        let frame = screen.frame();
        left < frame.origin.x + frame.size.width
            && right > frame.origin.x
            && bottom < frame.origin.y + frame.size.height
            && top > frame.origin.y
    })
}

fn rounded_i32(value: f64) -> Result<i32, OverlayError> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(OverlayError::new("overlay window coordinate is invalid"));
    }
    Ok(value.round() as i32)
}

fn rounded_u32(value: f64) -> Result<u32, OverlayError> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(OverlayError::new("overlay window dimension is invalid"));
    }
    Ok(value.round() as u32)
}

impl GpuModel {
    fn prepare(
        device: &Device,
        resources: &RenderResources,
        snapshot: &RenderSnapshot,
        drawable_width: u64,
        drawable_height: u64,
    ) -> Result<Self, OverlayError> {
        validate_render_snapshot(resources, snapshot)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        let textures = resources
            .textures
            .iter()
            .map(|asset| load_texture(device, asset).map(|texture| (asset.id, texture)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let key_textures = resources
            .key_assets
            .iter()
            .map(|asset| {
                load_texture(
                    device,
                    &TextureAsset {
                        id: TextureId::new(asset.id.index()),
                        path: asset.path.clone(),
                        width: asset.width,
                        height: asset.height,
                    },
                )
                .map(|texture| (asset.id, texture))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let background = resources
            .background
            .as_ref()
            .map(|asset| {
                load_texture(
                    device,
                    &TextureAsset {
                        id: TextureId::new(usize::MAX),
                        path: asset.path.clone(),
                        width: asset.width,
                        height: asset.height,
                    },
                )
            })
            .transpose()?;
        let canvas_bounds = ModelBounds::from_canvas(snapshot.canvas);
        let background_vertices = [
            bongocat_render::Vertex {
                position: [canvas_bounds.min_x, canvas_bounds.min_y],
                uv: [0.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.max_x, canvas_bounds.min_y],
                uv: [1.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.max_x, canvas_bounds.max_y],
                uv: [1.0, 1.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.min_x, canvas_bounds.max_y],
                uv: [0.0, 1.0],
            },
        ];
        let background_indices = [0_u16, 1, 2, 0, 2, 3];
        let mut meshes = snapshot
            .drawables
            .iter()
            .map(|drawable| {
                // Metal rejects a zero-length buffer, and a drawable the Core
                // reports without triangles has nothing to upload. Both arrays
                // therefore fall back to a single placeholder element, while
                // `vertex_bytes` keeps the length of the snapshot's own array so
                // `sync_snapshot` still notices a changed vertex count. Drawing
                // zero triangles is then a no-op, which is what leaves the mask
                // buffer such a mesh fills at the value it was cleared to.
                let vertex_bytes = std::mem::size_of_val(drawable.vertices.as_slice());
                let placeholder_vertex = [bongocat_render::Vertex {
                    position: [0.0, 0.0],
                    uv: [0.0, 0.0],
                }];
                let placeholder_index = [0_u16];
                let vertices: &[bongocat_render::Vertex] = if drawable.vertices.is_empty() {
                    &placeholder_vertex
                } else {
                    &drawable.vertices
                };
                let indices: &[u16] = if drawable.indices.is_empty() {
                    &placeholder_index
                } else {
                    &drawable.indices
                };
                let vertex_buffer = device.new_buffer_with_data(
                    vertices.as_ptr().cast(),
                    std::mem::size_of_val(vertices) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                let index_buffer = device.new_buffer_with_data(
                    indices.as_ptr().cast(),
                    std::mem::size_of_val(indices) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                Mesh {
                    id: drawable.id,
                    render_order: drawable.render_order,
                    vertex_buffer,
                    vertex_bytes,
                    index_buffer,
                    indices: drawable.indices.clone(),
                    index_count: drawable.indices.len() as u64,
                    texture_id: drawable.texture_id,
                    opacity: drawable.opacity,
                    blend_mode: drawable.blend_mode,
                    multiply_color: drawable.multiply_color,
                    screen_color: drawable.screen_color,
                    masks: drawable.masks.clone(),
                    visible: drawable.visible,
                    inverted_mask: drawable.inverted_mask,
                    mask_texture: (!drawable.masks.is_empty())
                        .then(|| create_mask_texture(device, drawable_width, drawable_height)),
                }
            })
            .collect::<Vec<_>>();
        meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        Ok(Self {
            textures,
            key_textures,
            background,
            background_vertex_buffer: device.new_buffer_with_data(
                background_vertices.as_ptr().cast(),
                std::mem::size_of_val(&background_vertices) as u64,
                MTLResourceOptions::StorageModeShared,
            ),
            background_index_buffer: device.new_buffer_with_data(
                background_indices.as_ptr().cast(),
                std::mem::size_of_val(&background_indices) as u64,
                MTLResourceOptions::StorageModeShared,
            ),
            meshes,
            empty_mask: create_solid_mask_texture(device),
            bounds: snapshot.bounds,
            model_opacity: snapshot.model_opacity,
            mirror_horizontal: snapshot.mirror_horizontal,
            active_keys: snapshot.active_keys.clone(),
            masked_drawable_count: snapshot
                .drawables
                .iter()
                .filter(|drawable| !drawable.masks.is_empty())
                .count(),
        })
    }

    fn sync_snapshot(&mut self, snapshot: &RenderSnapshot) -> Result<(), OverlayError> {
        if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
            return Err(OverlayError::new("model opacity is outside [0, 1]"));
        }
        if snapshot.drawables.len() != self.meshes.len() {
            return Err(OverlayError::new(format!(
                "drawable count changed from {} to {}",
                self.meshes.len(),
                snapshot.drawables.len()
            )));
        }
        for drawable in &snapshot.drawables {
            let mesh = self
                .meshes
                .iter_mut()
                .find(|mesh| mesh.id == drawable.id)
                .ok_or_else(|| {
                    OverlayError::new(format!("drawable source {} is unavailable", drawable.id))
                })?;
            if mesh.mask_texture.is_some() != !drawable.masks.is_empty() {
                return Err(OverlayError::new(format!(
                    "drawable {} changed clipping topology",
                    drawable.id
                )));
            }
            if mesh.indices != drawable.indices {
                return Err(OverlayError::new(format!(
                    "drawable {} changed immutable triangle indices",
                    drawable.id
                )));
            }
            if mesh.vertex_bytes != std::mem::size_of_val(drawable.vertices.as_slice()) {
                return Err(OverlayError::new(format!(
                    "drawable {} changed vertex buffer size",
                    drawable.id
                )));
            }
            if drawable.dynamic_flags.vertex_positions_changed && !drawable.vertices.is_empty() {
                upload_slice(&mesh.vertex_buffer, &drawable.vertices, "vertices")?;
            }
            mesh.render_order = drawable.render_order;
            mesh.texture_id = drawable.texture_id;
            mesh.opacity = drawable.opacity;
            mesh.blend_mode = drawable.blend_mode;
            mesh.multiply_color = drawable.multiply_color;
            mesh.screen_color = drawable.screen_color;
            mesh.masks.clone_from(&drawable.masks);
            mesh.visible = drawable.visible;
            mesh.inverted_mask = drawable.inverted_mask;
        }
        self.meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        self.bounds = snapshot.bounds;
        self.active_keys.clone_from(&snapshot.active_keys);
        self.model_opacity = snapshot.model_opacity;
        self.mirror_horizontal = snapshot.mirror_horizontal;
        Ok(())
    }

    /// Re-create every mask target at a new drawable size.
    ///
    /// A mask pass renders at the drawable size, so a window resize invalidates
    /// those textures even though the meshes they clip do not change. Textures,
    /// vertex and index buffers, bounds and the solid mask are all
    /// size-independent and stay as they are, which is what keeps a resize from
    /// reloading every model asset.
    fn resize_masks(&mut self, device: &Device, width: u64, height: u64) {
        for mesh in &mut self.meshes {
            if mesh.mask_texture.is_some() {
                mesh.mask_texture = Some(create_mask_texture(device, width, height));
            }
        }
    }
}

impl Drop for NativeOverlay {
    fn drop(&mut self) {
        self.panel.setContentView(None);
        // SAFETY: releasedWhenClosed transfers the panel retain to AppKit's
        // close path. The ManuallyDrop field is not touched afterwards.
        unsafe { self.panel.setReleasedWhenClosed(true) };
        self.panel.close();
    }
}

#[derive(Default)]
struct PreviewInputDriver {
    pressed: BTreeSet<InputControl>,
}

impl PreviewInputDriver {
    fn update(
        &mut self,
        model_id: &str,
        elapsed: Duration,
        producer: &InputProducer,
        cursor: &CursorProducer,
    ) -> Result<Option<u64>, OverlayError> {
        self.publish_cursor(elapsed, cursor)?;
        let step = (elapsed.as_millis() / 600) % 4;
        let mut desired = BTreeSet::new();
        match model_id {
            "standard" => {
                if step < 2 {
                    desired.insert(InputControl::Key(PhysicalKey::KEY_A));
                }
                if step == 0 {
                    desired.insert(InputControl::Mouse(MouseButton::Left));
                } else if step == 1 {
                    desired.insert(InputControl::Mouse(MouseButton::Right));
                }
            }
            "keyboard" | "gamepad" => {
                desired.insert(InputControl::Key(if step < 2 {
                    PhysicalKey::KEY_A
                } else {
                    RIGHT_ARROW
                }));
            }
            _ => {}
        }
        self.apply(desired, elapsed, producer)
    }

    fn release_all(
        &mut self,
        elapsed: Duration,
        producer: &InputProducer,
    ) -> Result<Option<u64>, OverlayError> {
        self.apply(BTreeSet::new(), elapsed, producer)
    }

    fn apply(
        &mut self,
        desired: BTreeSet<InputControl>,
        elapsed: Duration,
        producer: &InputProducer,
    ) -> Result<Option<u64>, OverlayError> {
        let at = MonotonicMillis::new(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
        let releases = self
            .pressed
            .difference(&desired)
            .copied()
            .collect::<Vec<_>>();
        let presses = desired
            .difference(&self.pressed)
            .copied()
            .collect::<Vec<_>>();
        let mut last_sequence = None;
        for (control, edge) in releases
            .into_iter()
            .map(|control| (control, InputEdge::Up))
            .chain(
                presses
                    .into_iter()
                    .map(|control| (control, InputEdge::Down)),
            )
        {
            last_sequence = Some(
                producer
                    .publish(InputEvent::Edge {
                        control,
                        edge,
                        source: InputSource::Capture,
                        at,
                    })
                    .map_err(|error| OverlayError::new(error.to_string()))?,
            );
        }
        self.pressed = desired;
        Ok(last_sequence)
    }

    fn publish_cursor(
        &self,
        elapsed: Duration,
        producer: &CursorProducer,
    ) -> Result<(), OverlayError> {
        let seconds = elapsed.as_secs_f64();
        let x = (seconds * std::f64::consts::TAU / 4.0).sin();
        let y = (seconds * std::f64::consts::TAU / 5.0).cos();
        let sample = CursorSample::new(
            CursorPosition {
                x: 1.0 - x,
                y: 1.0 - y,
            },
            CursorViewport {
                origin: CursorPosition { x: 0.0, y: 0.0 },
                width: 2.0,
                height: 2.0,
            },
            MonotonicMillis::new(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        )
        .map_err(|error| OverlayError::new(format!("invalid preview cursor sample: {error:?}")))?;
        producer
            .publish(sample)
            .map_err(|error| OverlayError::new(error.to_string()))
    }
}

fn preview_input_bindings(model_id: &str) -> InputBindings {
    let mut key_hands = BTreeMap::new();
    if matches!(model_id, "standard" | "keyboard") {
        for usage in 0x04..=0x27 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
        for usage in [
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x35, 0x38, 0x39, 0x4c, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4,
            0xe5, 0xe6, 0xe7,
        ] {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
    } else {
        key_hands.insert(PhysicalKey::KEY_A, HandSide::Left);
    }
    if matches!(model_id, "keyboard" | "gamepad") {
        for usage in 0x4f..=0x52 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Right);
        }
    }
    let gamepad_hands = if model_id == "gamepad" {
        BTreeMap::from([
            (GamepadButton::South, HandSide::Left),
            (GamepadButton::East, HandSide::Right),
        ])
    } else {
        BTreeMap::new()
    };
    InputBindings::with_gamepad_hands(key_hands, gamepad_hands)
}

fn upload_slice<T>(buffer: &Buffer, values: &[T], name: &str) -> Result<(), OverlayError> {
    let bytes = std::mem::size_of_val(values) as u64;
    if buffer.length() != bytes {
        return Err(OverlayError::new(format!(
            "{name} buffer size changed from {} to {bytes}",
            buffer.length()
        )));
    }
    if bytes == 0 {
        return Ok(());
    }
    // SAFETY: StorageModeShared exposes a writable CPU mapping for the full
    // fixed-size buffer, and source and destination cannot overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(
            values.as_ptr().cast::<u8>(),
            buffer.contents().cast::<u8>(),
            bytes as usize,
        )
    };
    Ok(())
}

fn create_pipelines(device: &Device) -> Result<Pipelines, OverlayError> {
    let library = device
        .new_library_with_source(SHADER_SOURCE, &CompileOptions::new())
        .map_err(|error| OverlayError::new(format!("compile Metal shaders: {error}")))?;
    let vertex = library
        .get_function("cubism_vertex", None)
        .map_err(|error| OverlayError::new(format!("load vertex shader: {error}")))?;
    let fragment = library
        .get_function("cubism_fragment", None)
        .map_err(|error| OverlayError::new(format!("load fragment shader: {error}")))?;
    let mask_fragment = library
        .get_function("cubism_mask_fragment", None)
        .map_err(|error| OverlayError::new(format!("load mask fragment shader: {error}")))?;
    Ok(Pipelines {
        normal: create_pipeline(device, &vertex, &fragment, BlendMode::Normal)?,
        additive: create_pipeline(device, &vertex, &fragment, BlendMode::Additive)?,
        multiplicative: create_pipeline(device, &vertex, &fragment, BlendMode::Multiplicative)?,
        mask: create_mask_pipeline(device, &vertex, &mask_fragment)?,
    })
}

fn create_pipeline(
    device: &Device,
    vertex: &metal::FunctionRef,
    fragment: &metal::FunctionRef,
    mode: BlendMode,
) -> Result<RenderPipelineState, OverlayError> {
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(vertex));
    descriptor.set_fragment_function(Some(fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| OverlayError::new("Metal pipeline color attachment is unavailable"))?;
    attachment.set_pixel_format(COLOR_ATTACHMENT_FORMAT);
    attachment.set_blending_enabled(true);
    let factors = blend_factors(mode);
    attachment.set_source_rgb_blend_factor(metal_blend_factor(factors.source_rgb));
    attachment.set_destination_rgb_blend_factor(metal_blend_factor(factors.destination_rgb));
    attachment.set_source_alpha_blend_factor(metal_blend_factor(factors.source_alpha));
    attachment.set_destination_alpha_blend_factor(metal_blend_factor(factors.destination_alpha));
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| OverlayError::new(format!("create Metal pipeline: {error}")))
}

fn create_mask_pipeline(
    device: &Device,
    vertex: &metal::FunctionRef,
    fragment: &metal::FunctionRef,
) -> Result<RenderPipelineState, OverlayError> {
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(vertex));
    descriptor.set_fragment_function(Some(fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| OverlayError::new("Metal mask pipeline attachment is unavailable"))?;
    attachment.set_pixel_format(MASK_TEXTURE_FORMAT);
    attachment.set_blending_enabled(true);
    let factors = blend_factors(BlendMode::Normal);
    attachment.set_source_rgb_blend_factor(metal_blend_factor(factors.source_rgb));
    attachment.set_destination_rgb_blend_factor(metal_blend_factor(factors.destination_rgb));
    attachment.set_source_alpha_blend_factor(metal_blend_factor(factors.source_alpha));
    attachment.set_destination_alpha_blend_factor(metal_blend_factor(factors.destination_alpha));
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| OverlayError::new(format!("create Metal mask pipeline: {error}")))
}

const fn metal_blend_factor(factor: BlendFactor) -> MTLBlendFactor {
    match factor {
        BlendFactor::Zero => MTLBlendFactor::Zero,
        BlendFactor::One => MTLBlendFactor::One,
        BlendFactor::OneMinusSourceAlpha => MTLBlendFactor::OneMinusSourceAlpha,
        BlendFactor::DestinationColor => MTLBlendFactor::DestinationColor,
    }
}

fn create_mask_texture(device: &Device, width: u64, height: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MASK_TEXTURE_FORMAT);
    descriptor.set_width(width);
    descriptor.set_height(height);
    descriptor.set_storage_mode(MTLStorageMode::Private);
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    device.new_texture(&descriptor)
}

fn create_solid_mask_texture(device: &Device) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MASK_TEXTURE_FORMAT);
    descriptor.set_width(1);
    descriptor.set_height(1);
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    let pixel = [0_u8; 4];
    texture.replace_region(
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: 1,
                height: 1,
                depth: 1,
            },
        },
        0,
        pixel.as_ptr().cast(),
        4,
    );
    texture
}

fn load_texture(device: &Device, asset: &TextureAsset) -> Result<Texture, OverlayError> {
    let image = ImageReader::open(&asset.path)
        .map_err(|error| OverlayError::new(format!("open {}: {error}", asset.path.display())))?
        .decode()
        .map_err(|error| OverlayError::new(format!("decode {}: {error}", asset.path.display())))?
        .into_rgba8();
    if image.width() != asset.width || image.height() != asset.height {
        return Err(OverlayError::new(format!(
            "texture dimensions changed for {}",
            asset.path.display()
        )));
    }
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MODEL_TEXTURE_FORMAT);
    descriptor.set_width(u64::from(asset.width));
    descriptor.set_height(u64::from(asset.height));
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    texture.replace_region(
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(asset.width),
                height: u64::from(asset.height),
                depth: 1,
            },
        },
        0,
        image.as_ptr().cast(),
        u64::from(asset.width) * 4,
    );
    Ok(texture)
}

fn model_transform(
    bounds: ModelBounds,
    width: f32,
    height: f32,
    mirror_horizontal: bool,
) -> [f32; 4] {
    let model_width = bounds.width();
    let model_height = bounds.height();
    let center = bounds.center();
    let center_x = center[0];
    let center_y = center[1];
    let model_aspect = model_width / model_height;
    let viewport_aspect = width / height;
    let (mut scale_x, mut scale_y) = (2.0 / model_width, 2.0 / model_height);
    if viewport_aspect > model_aspect {
        scale_x *= model_aspect / viewport_aspect;
    } else {
        scale_y *= viewport_aspect / model_aspect;
    }
    let mut offset_x = -center_x * scale_x;
    if mirror_horizontal {
        scale_x = -scale_x;
        offset_x = -offset_x;
    }
    [scale_x, scale_y, offset_x, -center_y * scale_y]
}

/// Read one frame's pixels out of a drawable texture.
///
/// This is the full-frame form of [`verify_frame_smoke`]: same readback, every
/// pixel of it. The drawable is BGRA, and the layer's sRGB pixel format means the
/// bytes are the values the compositor received rather than a linear-space copy,
/// so the cover is encoded from exactly what the overlay shows.
fn read_drawable_frame(texture: &metal::TextureRef) -> Result<CapturedFrame, OverlayError> {
    let width = u32::try_from(texture.width())
        .map_err(|_| OverlayError::new("Metal drawable width is out of range"))?;
    let height = u32::try_from(texture.height())
        .map_err(|_| OverlayError::new("Metal drawable height is out of range"))?;
    let row_bytes = width as usize * 4;
    let mut bytes = vec![0_u8; row_bytes * height as usize];
    texture.get_bytes(
        bytes.as_mut_ptr().cast(),
        row_bytes as u64,
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: texture.width(),
                height: texture.height(),
                depth: 1,
            },
        },
        0,
    );
    CapturedFrame::from_premultiplied_bgra(&bytes, row_bytes, width, height)
}

fn verify_frame_smoke(texture: &metal::TextureRef) -> Result<(), OverlayError> {
    let width = texture.width();
    let height = texture.height();
    let mut pixels =
        Vec::with_capacity((FRAME_SMOKE_GRID_DIMENSION * FRAME_SMOKE_GRID_DIMENSION) as usize);
    for y in 0..FRAME_SMOKE_GRID_DIMENSION {
        for x in 0..FRAME_SMOKE_GRID_DIMENSION {
            let mut pixel = [0_u8; 4];
            texture.get_bytes(
                pixel.as_mut_ptr().cast(),
                4,
                MTLRegion {
                    origin: MTLOrigin {
                        x: width.saturating_sub(1) * x / (FRAME_SMOKE_GRID_DIMENSION - 1),
                        y: height.saturating_sub(1) * y / (FRAME_SMOKE_GRID_DIMENSION - 1),
                        z: 0,
                    },
                    size: MTLSize {
                        width: 1,
                        height: 1,
                        depth: 1,
                    },
                },
                0,
            );
            pixels.push(pixel);
        }
    }
    validate_frame_smoke(pixels).map_err(|error| OverlayError::new(format!("Metal {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_render::CanvasInfo;

    #[test]
    fn model_transform_preserves_aspect_ratio() {
        let canvas = CanvasInfo {
            width: 2048.0,
            height: 2048.0,
            origin_x: 1024.0,
            origin_y: 1024.0,
            pixels_per_unit: 1024.0,
        };
        assert_eq!(
            model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, false),
            [1.0, 1.0, -0.0, -0.0]
        );
        assert_eq!(
            model_transform(ModelBounds::from_canvas(canvas), 1600.0, 800.0, false),
            [0.5, 1.0, -0.0, -0.0]
        );
        assert_eq!(
            model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, true),
            [-1.0, 1.0, 0.0, -0.0]
        );
    }

    #[test]
    fn gpu_structs_match_metal_layout() {
        assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
        assert_eq!(size_of::<Uniforms>(), 96);
    }

    #[test]
    fn color_formats_decode_assets_and_encode_the_composited_frame_as_srgb() {
        assert_eq!(MODEL_TEXTURE_FORMAT, MTLPixelFormat::RGBA8Unorm_sRGB);
        assert_eq!(COLOR_ATTACHMENT_FORMAT, MTLPixelFormat::BGRA8Unorm_sRGB);
        assert_eq!(MASK_TEXTURE_FORMAT, MTLPixelFormat::BGRA8Unorm);
    }

    /// A drawable the Core reports with vertices but no triangles must not stop
    /// the GPU model from being prepared.
    ///
    /// Cubism allows such a drawable — an authoring tool that deletes every
    /// triangle of a part without deleting the part leaves one behind — and a
    /// real community model ships one, so treating it as a fatal error made an
    /// otherwise drawable model fail both its cover capture and its activation.
    ///
    /// The mesh is still built, because another drawable may name it as a mask
    /// source and the mask buffer it fills has to keep the value it was cleared
    /// to; it simply draws zero triangles. Metal rejects a zero-length buffer,
    /// so both arrays are carried by a single placeholder element while the mesh
    /// keeps the snapshot's own vertex count for the cross-frame check.
    #[test]
    fn a_drawable_without_triangles_still_prepares_the_gpu_model() {
        use bongocat_render::{DrawableDynamicFlags, DrawableSnapshot};

        let Some(device) = Device::system_default() else {
            // No Metal device on this machine; the contract under test is a GPU
            // one, and the platform-neutral half is covered in `bongocat-render`.
            return;
        };
        let directory =
            std::env::temp_dir().join(format!("bongocat-empty-drawable-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temporary directory");
        let texture_path = directory.join("texture.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]))
            .save(&texture_path)
            .expect("write texture");

        let canvas = CanvasInfo {
            width: 2.0,
            height: 2.0,
            origin_x: 1.0,
            origin_y: 1.0,
            pixels_per_unit: 1.0,
        };
        let resources = RenderResources {
            textures: vec![TextureAsset {
                id: TextureId::new(0),
                path: texture_path.clone(),
                width: 2,
                height: 2,
            }],
            key_assets: Vec::new(),
            background: None,
        };
        let drawable = |id: usize, indices: Vec<u16>| DrawableSnapshot {
            id: DrawableId::new(id),
            dynamic_flags: DrawableDynamicFlags::default(),
            render_order: id as i32,
            visible: true,
            texture_id: TextureId::new(0),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            double_sided: false,
            inverted_mask: false,
            multiply_color: [1.0; 4],
            screen_color: [0.0; 4],
            masks: Vec::new(),
            vertices: vec![
                bongocat_render::Vertex {
                    position: [-0.5, -0.5],
                    uv: [0.0, 0.0],
                },
                bongocat_render::Vertex {
                    position: [0.5, -0.5],
                    uv: [1.0, 0.0],
                },
                bongocat_render::Vertex {
                    position: [0.0, 0.5],
                    uv: [0.5, 1.0],
                },
            ],
            indices,
        };
        let snapshot = RenderSnapshot {
            canvas,
            bounds: ModelBounds::from_canvas(canvas),
            active_keys: Vec::new(),
            model_opacity: 1.0,
            mirror_horizontal: false,
            drawables: vec![drawable(0, vec![0, 1, 2]), drawable(1, Vec::new())],
        };

        let model = GpuModel::prepare(&device, &resources, &snapshot, 2, 2)
            .expect("a model with an empty drawable must still prepare");
        let empty = model
            .meshes
            .iter()
            .find(|mesh| mesh.id == DrawableId::new(1))
            .expect("the empty drawable keeps its mesh so a mask can still name it");
        assert_eq!(
            empty.index_count, 0,
            "an empty drawable must draw no triangles, which is what leaves the mask buffer it \
             fills at its cleared value"
        );
        assert_eq!(
            empty.vertex_bytes,
            std::mem::size_of::<bongocat_render::Vertex>() * 3,
            "the mesh must keep the snapshot's own vertex count for the cross-frame check"
        );
        assert!(
            empty.vertex_buffer.length() > 0 && empty.index_buffer.length() > 0,
            "Metal cannot allocate a zero-length buffer, so both arrays need a placeholder"
        );

        let _ = std::fs::remove_file(&texture_path);
    }
}
