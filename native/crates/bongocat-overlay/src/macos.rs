use crate::{OverlayError, PreviewReport};
use bongocat_live2d::{BlendMode, CanvasInfo, Live2dModel, RenderSnapshot, TextureAsset};
use bongocat_model::{ModelId, ModelPackageLimits, PreparedModel};
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
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask,
    NSFloatingWindowLevel, NSPanel, NSView, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
use std::{
    mem::{self, ManuallyDrop},
    path::Path,
    thread,
    time::{Duration, Instant},
};

const WINDOW_WIDTH: f64 = 640.0;
const WINDOW_HEIGHT: f64 = 560.0;
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
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
    source_index: usize,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u64,
    texture_index: usize,
    opacity: f32,
    blend_mode: BlendMode,
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    masks: Vec<usize>,
    visible: bool,
    inverted_mask: bool,
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
    layer: MetalLayer,
    queue: CommandQueue,
    pipelines: Pipelines,
    sampler: SamplerState,
    textures: Vec<Texture>,
    meshes: Vec<Mesh>,
    masks: Vec<Option<Texture>>,
    empty_mask: Texture,
    canvas: CanvasInfo,
    masked_drawable_count: usize,
}

pub(crate) fn run_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
) -> Result<PreviewReport, OverlayError> {
    let prepared = PreparedModel::prepare(
        ModelId::parse(model_id).map_err(|error| OverlayError::new(error.to_string()))?,
        model_root,
        ModelPackageLimits::default(),
    )
    .map_err(|error| OverlayError::new(error.to_string()))?;
    let mut model =
        Live2dModel::load(&prepared).map_err(|error| OverlayError::new(error.to_string()))?;
    let snapshot = model
        .update_and_snapshot()
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| OverlayError::new("macOS preview must run on the main thread"))?;
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    application.finishLaunching();
    let overlay = NativeOverlay::create(mtm, model.texture_assets(), &snapshot)?;
    overlay.panel.orderFrontRegardless();

    let started = Instant::now();
    let mut next_frame = started;
    let mut frames_presented = 0_u64;
    while overlay.panel.isVisible() && (duration.is_zero() || started.elapsed() < duration) {
        autoreleasepool(|_| {
            let deadline = NSDate::dateWithTimeIntervalSinceNow(0.0);
            // SAFETY: AppKit exports this immutable process-global run-loop
            // mode for use on the application main thread.
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
        overlay.draw(frames_presented == 0)?;
        frames_presented += 1;
        next_frame += FRAME_INTERVAL;
        if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
        } else {
            next_frame = Instant::now();
        }
    }

    Ok(PreviewReport {
        frames_presented,
        drawable_count: overlay.meshes.len(),
        masked_drawable_count: overlay.masked_drawable_count,
        texture_count: overlay.textures.len(),
    })
}

impl NativeOverlay {
    fn create(
        mtm: MainThreadMarker,
        texture_assets: &[TextureAsset],
        snapshot: &RenderSnapshot,
    ) -> Result<Self, OverlayError> {
        let frame = NSRect::new(
            NSPoint::new(80.0, 80.0),
            NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setHasShadow(false);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setLevel(NSFloatingWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setMovableByWindowBackground(false);
        panel.setIgnoresMouseEvents(true);

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
            WINDOW_WIDTH * scale,
            WINDOW_HEIGHT * scale,
        ));
        // SAFETY: metal::MetalLayerRef and objc2 QuartzCore both wrap the
        // same Objective-C CAMetalLayer instance, which NSView retains.
        let layer_ref = unsafe {
            mem::transmute::<&metal::MetalLayerRef, &objc2_quartz_core::CALayer>(layer.as_ref())
        };
        view.setLayer(Some(layer_ref));
        panel.setContentView(Some(&view));

        let pipelines = create_pipelines(&device)?;
        let sampler_descriptor = SamplerDescriptor::new();
        sampler_descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
        sampler_descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
        let sampler = device.new_sampler(&sampler_descriptor);
        let textures = texture_assets
            .iter()
            .map(|asset| load_texture(&device, asset))
            .collect::<Result<Vec<_>, _>>()?;
        let meshes = snapshot
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
                    source_index: drawable.source_index,
                    vertex_buffer,
                    index_buffer,
                    index_count: drawable.indices.len() as u64,
                    texture_index: drawable.texture_index,
                    opacity: drawable.opacity,
                    blend_mode: drawable.blend_mode,
                    multiply_color: drawable.multiply_color,
                    screen_color: drawable.screen_color,
                    masks: drawable.masks.clone(),
                    visible: drawable.visible,
                    inverted_mask: drawable.inverted_mask,
                }
            })
            .collect::<Vec<_>>();
        let drawable_width = layer.drawable_size().width.round() as u64;
        let drawable_height = layer.drawable_size().height.round() as u64;
        let masks = meshes
            .iter()
            .map(|mesh| {
                (!mesh.masks.is_empty())
                    .then(|| create_mask_texture(&device, drawable_width, drawable_height))
            })
            .collect::<Vec<_>>();
        let empty_mask = create_solid_mask_texture(&device);
        Ok(Self {
            panel: ManuallyDrop::new(panel),
            layer,
            queue: device.new_command_queue(),
            pipelines,
            sampler,
            textures,
            meshes,
            masks,
            empty_mask,
            canvas: snapshot.canvas,
            masked_drawable_count: snapshot
                .drawables
                .iter()
                .filter(|drawable| !drawable.masks.is_empty())
                .count(),
        })
    }

    fn draw(&self, verify_frame: bool) -> Result<(), OverlayError> {
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
            self.canvas,
            drawable.texture().width() as f32,
            drawable.texture().height() as f32,
        );
        for (mesh, mask_texture) in self.meshes.iter().zip(&self.masks) {
            let Some(mask_texture) = mask_texture else {
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
            for source_index in &mesh.masks {
                let source = self
                    .meshes
                    .iter()
                    .find(|source| source.source_index == *source_index)
                    .ok_or_else(|| {
                        OverlayError::new(format!("mask source {source_index} is unavailable"))
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
                mask_encoder.set_fragment_texture(0, Some(&self.textures[source.texture_index]));
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
        for (mesh, mask_texture) in self.meshes.iter().zip(&self.masks) {
            if !mesh.visible || mesh.opacity <= 0.0 {
                continue;
            }
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
                opacity: mesh.opacity,
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
            encoder.set_fragment_texture(0, Some(&self.textures[mesh.texture_index]));
            encoder
                .set_fragment_texture(1, Some(mask_texture.as_ref().unwrap_or(&self.empty_mask)));
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                mesh.index_count,
                MTLIndexType::UInt16,
                &mesh.index_buffer,
                0,
            );
        }
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();
        if verify_frame {
            command_buffer.wait_until_completed();
            if command_buffer.status() != MTLCommandBufferStatus::Completed {
                return Err(OverlayError::new(format!(
                    "Metal command buffer ended with {:?}",
                    command_buffer.status()
                )));
            }
            verify_non_empty_frame(drawable.texture())?;
        }
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

fn model_transform(canvas: CanvasInfo, width: f32, height: f32) -> [f32; 4] {
    let model_width = canvas.width / canvas.pixels_per_unit;
    let model_height = canvas.height / canvas.pixels_per_unit;
    let center_x = (canvas.width * 0.5 - canvas.origin_x) / canvas.pixels_per_unit;
    let center_y = (canvas.origin_y - canvas.height * 0.5) / canvas.pixels_per_unit;
    let model_aspect = model_width / model_height;
    let viewport_aspect = width / height;
    let (mut scale_x, mut scale_y) = (2.0 / model_width, 2.0 / model_height);
    if viewport_aspect > model_aspect {
        scale_x *= model_aspect / viewport_aspect;
    } else {
        scale_y *= viewport_aspect / model_aspect;
    }
    [scale_x, scale_y, -center_x * scale_x, -center_y * scale_y]
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
            model_transform(canvas, 800.0, 800.0),
            [1.0, 1.0, -0.0, -0.0]
        );
        assert_eq!(
            model_transform(canvas, 1600.0, 800.0),
            [0.5, 1.0, -0.0, -0.0]
        );
    }

    #[test]
    fn gpu_structs_match_metal_layout() {
        assert_eq!(size_of::<bongocat_live2d::Vertex>(), 16);
        assert_eq!(size_of::<Uniforms>(), 80);
    }
}
