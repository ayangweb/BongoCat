use crate::{
    OverlayError, OverlayPresentationState, OverlaySessionOptions, OverlayTickOutcome,
    OverlayWindowBounds, PreviewReport, ProductOverlayReport, default_overlay_window_dimensions,
    validate_model_generation_advance,
};
use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_platform::{
    MacInputService, PlatformInputDiagnostics, PlatformInputError, ShortcutDispatcher,
};
use bongocat_render::{
    BlendMode, DrawableId, KeyAssetId, KeyOverlay, ModelBounds, ModelCommitErrorCode,
    ModelCommitFeedback, ModelCommitOutcome, ModelCommitToken, RenderConsumer, RenderFrame,
    RenderResources, RenderSnapshot, TextureAsset, TextureId,
};
use bongocat_runtime::{
    CursorPosition, CursorProducer, CursorSample, CursorViewport, GamepadAxisProducer,
    GamepadButton, HandSide, InputBindings, InputControl, InputEdge, InputEvent, InputProducer,
    InputSource, MonotonicMillis, MouseButton, PhysicalKey, RuntimeClient, RuntimeCommand,
    RuntimeOwner, RuntimeRenderErrorCode, RuntimeState, frame_interval_for_maximum_fps,
    maximum_fps_is_valid,
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
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEvent,
    NSEventMask, NSMainMenuWindowLevel, NSNormalWindowLevel, NSPanel, NSScreen, NSView,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowLevel, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
use objc2_quartz_core::CAMetalLayer as ObjcMetalLayer;
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::{self, ManuallyDrop},
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
const RUNTIME_TIMEOUT: Duration = Duration::from_millis(250);
const METAL_COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
const SWITCH_WARMUP_FRAMES: u64 = 30;
const SWITCH_SETTLE_FRAMES: u64 = 30;
const PRESET_MODEL_IDS: [&str; 3] = ["standard", "keyboard", "gamepad"];
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
        float opacity;
        float3 padding;
    };

    struct RasterVertex {
        float4 position [[position]];
        float2 uv;
    };

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
        float alpha = texture_color.a * uniforms.opacity * mask;
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
    opacity: f32,
    padding: [f32; 3],
}

struct Mesh {
    id: DrawableId,
    render_order: i32,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
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
}

impl ProductOverlaySession {
    pub(super) fn start(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
        shortcut_dispatcher: Option<ShortcutDispatcher>,
    ) -> Result<Self, OverlayError> {
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
        if runtime_client.snapshot().overlay_visible {
            if let Err(error) = overlay.draw(true).and_then(|()| overlay.set_visible(true)) {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
            frames_presented = 1;
        }
        report_model_commit(
            &runtime_client,
            &render_consumer,
            token,
            ModelCommitOutcome::Prepared,
        )?;
        let diagnostics_producer = runtime_client.platform_input_diagnostics_producer();
        let (input_service, input_start_error) =
            super::start_platform_input(&diagnostics_producer, || {
                MacInputService::start_with_diagnostics_and_shortcuts(
                    input_producer,
                    cursor_producer,
                    gamepad_axis_producer,
                    diagnostics_producer.clone(),
                    shortcut_dispatcher,
                )
            });
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
        })
    }

    pub(super) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while duration.is_zero() || started.elapsed() < duration {
            pump_application_events(&self.application);
            if self.tick()? == OverlayTickOutcome::Hidden {
                break;
            }
            next_frame += frame_interval_for_maximum_fps(self.options.maximum_fps)
                .expect("product overlay stores a validated maximum FPS");
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
        if self
            .options
            .with_runtime_settings(runtime_snapshot.overlay_settings)
            != self.options
        {
            let next_options = self
                .options
                .with_runtime_settings(runtime_snapshot.overlay_settings);
            let bounds = self.window_bounds()?;
            let bounds = if next_options.scale_percent != self.options.scale_percent {
                bounds.rescale(self.options.scale_percent, next_options.scale_percent)
            } else {
                bounds
            };
            let mut replacement = NativeOverlay::create(
                MainThreadMarker::new().ok_or_else(|| {
                    OverlayError::new("macOS overlay settings update lost the main thread")
                })?,
                &self.last_frame,
                next_options,
                Some(bounds),
            )?;
            if runtime_snapshot.overlay_visible {
                replacement.draw(self.frames_presented == 0)?;
                replacement.set_visible(true)?;
                self.frames_presented = self.frames_presented.saturating_add(1);
            }
            self.overlay = replacement;
            self.options = next_options;
        }
        self.options.maximum_fps = runtime_snapshot.maximum_fps;
        if !runtime_snapshot.overlay_visible {
            self.overlay.set_visible(false)?;
            return Ok(OverlayTickOutcome::Hidden);
        }

        if let Some(frame) = self.render_consumer.take_latest() {
            let model_changed = frame.model_generation != self.overlay.model_generation;
            if model_changed {
                let bounds = self.window_bounds()?;
                let replacement = NativeOverlay::create(
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
                        self.overlay.draw(self.frames_presented == 0)?;
                        self.frames_presented = self.frames_presented.saturating_add(1);
                        self.overlay.set_visible(true)?;
                        return Ok(OverlayTickOutcome::Presented);
                    }
                    Err(error) => return Err(error),
                };
                if let Err(error) = replacement
                    .draw(true)
                    .and_then(|()| replacement.set_visible(true))
                {
                    if let Some(token) = frame.model_commit {
                        reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        self.overlay.draw(self.frames_presented == 0)?;
                        self.frames_presented = self.frames_presented.saturating_add(1);
                        self.overlay.set_visible(true)?;
                        return Ok(OverlayTickOutcome::Presented);
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
                self.frames_presented = self.frames_presented.saturating_add(1);
                return Ok(OverlayTickOutcome::Presented);
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
        self.overlay.draw(self.frames_presented == 0)?;
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.overlay.set_visible(true)?;
        Ok(OverlayTickOutcome::Presented)
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
        self,
    ) -> Result<ProductOverlayReport, OverlayError> {
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
        Ok(ProductOverlayReport {
            frames_presented: self.frames_presented,
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

#[cfg(test)]
mod product_options_tests {
    use super::*;

    #[test]
    fn product_options_accept_config_boundaries() {
        for options in [
            OverlaySessionOptions {
                scale_percent: 25,
                opacity_percent: 1,
                maximum_fps: 15,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                scale_percent: 400,
                opacity_percent: 100,
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
    overlay.draw(true)?;
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
        overlay.draw(gpu_model_switched)?;
        frames_presented += 1;
        if target_model_switches.is_some_and(|target| model_switches == target) {
            settle_frames = settle_frames.saturating_add(1);
        }
        next_frame += FRAME_INTERVAL;
        if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
        } else {
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
    })
}

impl NativeOverlay {
    fn create(
        mtm: MainThreadMarker,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
    ) -> Result<Self, OverlayError> {
        let bounds = bounds.filter(|bounds| overlay_bounds_visible(mtm, *bounds));
        let window_scale = f64::from(options.scale_percent) / 100.0;
        let (base_width, base_height) = default_overlay_window_dimensions(frame.snapshot.canvas);
        let window_width = bounds.map_or(f64::from(base_width) * window_scale, |bounds| {
            f64::from(bounds.width)
        });
        let window_height = bounds.map_or(f64::from(base_height) * window_scale, |bounds| {
            f64::from(bounds.height)
        });
        let origin = bounds.map_or_else(
            || centered_origin(mtm, window_width, window_height),
            |bounds| NSPoint::new(f64::from(bounds.x), f64::from(bounds.y)),
        );
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
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
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
        })
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
        autoreleasepool(|_| self.draw_in_autorelease_pool(verify_frame))?;
        self.presentation.record_presented_frame();
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> Result<(), OverlayError> {
        if visible {
            self.presentation.require_presented_frame()?;
            self.panel.orderFrontRegardless();
            if !self.panel.isVisible() {
                return Err(OverlayError::new("macOS overlay did not become visible"));
            }
        } else if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
        Ok(())
    }

    fn draw_in_autorelease_pool(&self, verify_frame: bool) -> Result<(), OverlayError> {
        let drawable = self
            .layer
            .next_drawable()
            .ok_or_else(|| OverlayError::new("CAMetalLayer returned no drawable"))?;
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
            verify_non_empty_frame(drawable.texture())?;
        }
        Ok(())
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
        if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
            return Err(OverlayError::new("model opacity is outside [0, 1]"));
        }
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
        if textures.len() != resources.textures.len() {
            return Err(OverlayError::new("texture resource ids are not unique"));
        }
        let drawable_ids = snapshot
            .drawables
            .iter()
            .map(|drawable| drawable.id)
            .collect::<BTreeSet<_>>();
        if drawable_ids.len() != snapshot.drawables.len() {
            return Err(OverlayError::new("drawable resource ids are not unique"));
        }
        for drawable in &snapshot.drawables {
            if !textures.contains_key(&drawable.texture_id) {
                return Err(OverlayError::new(format!(
                    "drawable {} references missing texture {}",
                    drawable.id, drawable.texture_id
                )));
            }
            if let Some(mask) = drawable
                .masks
                .iter()
                .find(|mask| !drawable_ids.contains(mask))
            {
                return Err(OverlayError::new(format!(
                    "drawable {} references missing mask source {mask}",
                    drawable.id
                )));
            }
        }
        let mut meshes = snapshot
            .drawables
            .iter()
            .map(|drawable| {
                let vertex_buffer = device.new_buffer_with_data(
                    drawable.vertices.as_ptr().cast(),
                    std::mem::size_of_val(drawable.vertices.as_slice()) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                let index_buffer = device.new_buffer_with_data(
                    drawable.indices.as_ptr().cast(),
                    std::mem::size_of_val(drawable.indices.as_slice()) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                Mesh {
                    id: drawable.id,
                    render_order: drawable.render_order,
                    vertex_buffer,
                    index_buffer,
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
            upload_slice(&mesh.vertex_buffer, &drawable.vertices, "vertices")?;
            upload_slice(&mesh.index_buffer, &drawable.indices, "indices")?;
            mesh.render_order = drawable.render_order;
            mesh.index_count = drawable.indices.len() as u64;
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
    attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    attachment.set_blending_enabled(true);
    match mode {
        BlendMode::Normal => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        BlendMode::Additive => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::Zero);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::One);
        }
        BlendMode::Multiplicative => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::DestinationColor);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::Zero);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::One);
        }
    }
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
    attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    attachment.set_blending_enabled(true);
    attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
    attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
    attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
    attachment.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| OverlayError::new(format!("create Metal mask pipeline: {error}")))
}

fn create_mask_texture(device: &Device, width: u64, height: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_width(width);
    descriptor.set_height(height);
    descriptor.set_storage_mode(MTLStorageMode::Private);
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    device.new_texture(&descriptor)
}

fn create_solid_mask_texture(device: &Device) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
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
    descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
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

fn verify_non_empty_frame(texture: &metal::TextureRef) -> Result<(), OverlayError> {
    let width = texture.width();
    let height = texture.height();
    let mut pixel = [0_u8; 4];
    for y in 1..16 {
        for x in 1..16 {
            texture.get_bytes(
                pixel.as_mut_ptr().cast(),
                4,
                MTLRegion {
                    origin: MTLOrigin {
                        x: width * x / 16,
                        y: height * y / 16,
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
            if pixel[3] != 0 {
                return Ok(());
            }
        }
    }
    Err(OverlayError::new(
        "Metal readback found no non-transparent model pixels",
    ))
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
        assert_eq!(size_of::<Uniforms>(), 80);
    }
}
