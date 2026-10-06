//! Vulkan renderer for the Wayland surface and offscreen cover captures.

use super::*;

const COMPOSITION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const MODEL_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const MASK_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const WAYLAND_MAXIMUM_FRAME_LATENCY: u32 = 2;
const SHADER_SOURCE: &str = include_str!("shader.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scale_offset: [f32; 4],
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    mask_settings: [f32; 4],
    corner_radius: [f32; 4],
    opacity_padding: [f32; 4],
}

struct TextureResource {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct MaskTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Mesh {
    id: DrawableId,
    render_order: i32,
    vertex_buffer: wgpu::Buffer,
    vertex_count: usize,
    index_buffer: wgpu::Buffer,
    indices: Vec<u16>,
    texture_id: TextureId,
    opacity: f32,
    blend_mode: BlendMode,
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    masks: Vec<DrawableId>,
    visible: bool,
    double_sided: bool,
    inverted_mask: bool,
    mask_target: Option<MaskTarget>,
}

struct GpuModel {
    textures: BTreeMap<TextureId, TextureResource>,
    key_textures: BTreeMap<KeyAssetId, TextureResource>,
    background: Option<TextureResource>,
    background_vertex_buffer: wgpu::Buffer,
    background_index_buffer: wgpu::Buffer,
    meshes: Vec<Mesh>,
    empty_mask: TextureResource,
    bounds: ModelBounds,
    model_opacity: f32,
    mirror_horizontal: bool,
    active_keys: Vec<KeyOverlay>,
    masked_drawable_count: usize,
}

struct Pipelines {
    model: Vec<wgpu::RenderPipeline>,
    mask: Vec<wgpu::RenderPipeline>,
    present: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    uniform_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

struct PreparedDraw {
    geometry: Geometry,
    pipeline: usize,
    _uniform_buffer: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    texture_group: wgpu::BindGroup,
}

#[derive(Clone, Copy)]
enum Geometry {
    Background,
    Mesh(usize),
}

pub(crate) struct Renderer {
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    composition: TextureResource,
    pipelines: Pipelines,
    pub(crate) model_generation: u64,
    resources: Arc<RenderResources>,
    model: GpuModel,
    width: u32,
    height: u32,
    corner_radius_percent: u8,
    presentation_opacity: f32,
}

pub(crate) struct PreparedModel {
    model_generation: u64,
    resources: Arc<RenderResources>,
    model: GpuModel,
}

fn select_wayland_alpha_mode(
    available: &[wgpu::CompositeAlphaMode],
) -> Result<wgpu::CompositeAlphaMode, OverlayError> {
    [
        wgpu::CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::Inherit,
    ]
    .into_iter()
    .find(|mode| available.contains(mode))
    .ok_or_else(|| {
        OverlayError::new(format!(
            "Wayland surface exposes no transparent premultiplied alpha mode; available modes: {available:?}"
        ))
    })
}

impl Renderer {
    pub(crate) fn create<T>(
        target: T,
        width: u32,
        height: u32,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
    ) -> Result<Self, OverlayError>
    where
        T: HasDisplayHandle + HasWindowHandle + Send + Sync + 'static,
    {
        let width = width.max(1);
        let height = height.max(1);
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(instance_descriptor);
        let surface = instance
            .create_surface(target)
            .map_err(|error| gpu_error("create Wayland Vulkan surface", error))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|error| gpu_error("request Vulkan adapter", error))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("BongoCat Wayland device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| gpu_error("request Vulkan device", error))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| {
                matches!(
                    format,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .ok_or_else(|| OverlayError::new("Wayland surface exposes no encoded UNORM format"))?;
        let alpha_mode = select_wayland_alpha_mode(&capabilities.alpha_modes)?;
        if !capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            return Err(OverlayError::new(
                "Wayland surface does not support FIFO presentation",
            ));
        }
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: WAYLAND_MAXIMUM_FRAME_LATENCY,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &surface_config);
        let pipelines = create_pipelines(&device, format);
        let composition = create_target(&device, width, height, COMPOSITION_FORMAT, "composition");
        let model = GpuModel::prepare(
            &device,
            &queue,
            &frame.resources,
            &frame.snapshot,
            width,
            height,
        )?;
        Ok(Self {
            surface: Some(surface),
            device,
            queue,
            surface_config: Some(surface_config),
            composition,
            pipelines,
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model,
            width,
            height,
            corner_radius_percent: options.corner_radius_percent,
            presentation_opacity: f32::from(options.opacity_percent) / 100.0,
        })
    }

    pub(crate) fn create_headless(
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        width: u32,
        height: u32,
    ) -> Result<Self, OverlayError> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|error| gpu_error("request headless Vulkan adapter", error))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("BongoCat headless Vulkan device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| gpu_error("request headless Vulkan device", error))?;
        let pipelines = create_pipelines(&device, COMPOSITION_FORMAT);
        let composition = create_target(&device, width, height, COMPOSITION_FORMAT, "composition");
        let model = GpuModel::prepare(
            &device,
            &queue,
            &frame.resources,
            &frame.snapshot,
            width,
            height,
        )?;
        Ok(Self {
            surface: None,
            device,
            queue,
            surface_config: None,
            composition,
            pipelines,
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model,
            width,
            height,
            corner_radius_percent: options.corner_radius_percent,
            presentation_opacity: f32::from(options.opacity_percent) / 100.0,
        })
    }

    pub(crate) fn set_presentation_opacity(&mut self, opacity: f32) {
        self.presentation_opacity = opacity.clamp(0.0, 1.0);
    }

    pub(crate) fn set_corner_radius(&mut self, corner_radius_percent: u8) {
        self.corner_radius_percent = corner_radius_percent;
    }

    pub(crate) fn prepare_model(&self, frame: &RenderFrame) -> Result<PreparedModel, OverlayError> {
        validate_model_generation_advance(self.model_generation, frame.model_generation)?;
        Ok(PreparedModel {
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model: GpuModel::prepare(
                &self.device,
                &self.queue,
                &frame.resources,
                &frame.snapshot,
                self.width,
                self.height,
            )?,
        })
    }

    pub(crate) fn install_model(&mut self, prepared: PreparedModel) -> PreparedModel {
        PreparedModel {
            model_generation: std::mem::replace(
                &mut self.model_generation,
                prepared.model_generation,
            ),
            resources: std::mem::replace(&mut self.resources, prepared.resources),
            model: std::mem::replace(&mut self.model, prepared.model),
        }
    }

    pub(crate) fn drawable_count(&self) -> usize {
        self.model.meshes.len()
    }

    pub(crate) fn masked_drawable_count(&self) -> usize {
        self.model.masked_drawable_count
    }

    pub(crate) fn texture_count(&self) -> usize {
        self.model.textures.len()
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        if let (Some(surface), Some(config)) = (&self.surface, &mut self.surface_config) {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);
        }
        self.composition = create_target(
            &self.device,
            width,
            height,
            COMPOSITION_FORMAT,
            "composition",
        );
        self.model.resize_masks(&self.device, width, height);
    }

    pub(crate) fn sync_frame(&mut self, frame: &RenderFrame) -> Result<bool, OverlayError> {
        if frame.model_generation != self.model_generation {
            validate_model_generation_advance(self.model_generation, frame.model_generation)?;
            let candidate = GpuModel::prepare(
                &self.device,
                &self.queue,
                &frame.resources,
                &frame.snapshot,
                self.width,
                self.height,
            )?;
            self.model = candidate;
            self.resources = Arc::clone(&frame.resources);
            self.model_generation = frame.model_generation;
            return Ok(true);
        }
        if !Arc::ptr_eq(&self.resources, &frame.resources) {
            return Err(OverlayError::new(
                "render resources changed within one model generation",
            ));
        }
        self.model.sync_snapshot(&self.queue, &frame.snapshot)?;
        Ok(false)
    }

    pub(crate) fn draw(&mut self, verify: bool) -> Result<(), OverlayError> {
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| OverlayError::new("headless renderer has no presentation surface"))?;
        let surface_texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Err(OverlayError::temporary_presentation_unavailable(
                    "Wayland Vulkan surface is temporarily unavailable",
                ));
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                let config = self.surface_config.as_ref().ok_or_else(|| {
                    OverlayError::new("Wayland surface configuration is unavailable")
                })?;
                surface.configure(&self.device, config);
                return Err(OverlayError::temporary_presentation_unavailable(
                    "Wayland Vulkan surface was reconfigured",
                ));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err(OverlayError::presentation_surface_lost(
                    "Wayland Vulkan surface was lost",
                ));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(OverlayError::new(
                    "Wayland Vulkan surface acquisition failed validation",
                ));
            }
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BongoCat Wayland frame"),
            });
        self.encode_composition(&mut encoder)?;
        self.encode_presentation(&mut encoder, &surface_view);
        self.queue.submit(Some(encoder.finish()));
        if verify {
            self.verify_frame_smoke()?;
        }
        surface_texture.present();
        Ok(())
    }

    pub(crate) fn draw_capturing(&mut self, verify: bool) -> Result<CapturedFrame, OverlayError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BongoCat Wayland cover frame"),
            });
        self.encode_composition(&mut encoder)?;
        self.queue.submit(Some(encoder.finish()));
        let frame = self.read_composition()?;
        if verify {
            verify_pixels(frame.pixels(), self.width, self.height)?;
        }
        Ok(frame)
    }

    pub(crate) fn verify_composition(&mut self) -> Result<(), OverlayError> {
        self.draw_capturing(true).map(drop)
    }

    fn encode_composition(&self, encoder: &mut wgpu::CommandEncoder) -> Result<(), OverlayError> {
        self.encode_masks(encoder)?;
        let corner_radius = corner_radius_uniform(
            self.corner_radius_percent,
            self.width as f32,
            self.height as f32,
        );
        let scale_offset = model_transform(
            self.model.bounds,
            self.width as f32,
            self.height as f32,
            self.model.mirror_horizontal,
        );
        let mut draws = Vec::new();
        if let Some(background) = &self.model.background {
            draws.push(self.prepare_draw(
                Geometry::Background,
                background,
                &self.model.empty_mask.view,
                Uniforms {
                    scale_offset,
                    multiply_color: [1.0; 4],
                    screen_color: [0.0; 4],
                    mask_settings: [0.0; 4],
                    corner_radius,
                    opacity_padding: [1.0, 0.0, 0.0, 0.0],
                },
                pipeline_index(BlendMode::Normal, DrawableCullMode::None),
            ));
        }
        for (index, mesh) in self.model.meshes.iter().enumerate() {
            if !mesh.visible || mesh.opacity <= 0.0 || mesh.indices.is_empty() {
                continue;
            }
            let texture = self.model.textures.get(&mesh.texture_id).ok_or_else(|| {
                OverlayError::new(format!("drawable {} texture is unavailable", mesh.id))
            })?;
            let mask = mesh
                .mask_target
                .as_ref()
                .map_or(&self.model.empty_mask.view, |target| &target.view);
            draws.push(self.prepare_draw(
                Geometry::Mesh(index),
                texture,
                mask,
                Uniforms {
                    scale_offset,
                    multiply_color: mesh.multiply_color,
                    screen_color: mesh.screen_color,
                    mask_settings: [
                        self.width as f32,
                        self.height as f32,
                        f32::from(mesh.mask_target.is_some()),
                        f32::from(mesh.inverted_mask),
                    ],
                    corner_radius,
                    opacity_padding: [mesh.opacity * self.model.model_opacity, 0.0, 0.0, 0.0],
                },
                pipeline_index(
                    mesh.blend_mode,
                    drawable_cull_mode(mesh.double_sided, self.model.mirror_horizontal),
                ),
            ));
        }
        for key in &self.model.active_keys {
            if let Some(texture) = self.model.key_textures.get(&key.asset_id) {
                draws.push(self.prepare_draw(
                    Geometry::Background,
                    texture,
                    &self.model.empty_mask.view,
                    Uniforms {
                        scale_offset,
                        multiply_color: [1.0; 4],
                        screen_color: [0.0; 4],
                        mask_settings: [0.0; 4],
                        corner_radius,
                        opacity_padding: [1.0, 0.0, 0.0, 0.0],
                    },
                    pipeline_index(BlendMode::Normal, DrawableCullMode::None),
                ));
            }
        }
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &self.composition.view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("BongoCat composition"),
            color_attachments: &attachments,
            ..Default::default()
        });
        for draw in &draws {
            pass.set_pipeline(&self.pipelines.model[draw.pipeline]);
            pass.set_bind_group(0, &draw.texture_group, &[]);
            pass.set_bind_group(1, &draw.uniform_group, &[]);
            match draw.geometry {
                Geometry::Background => {
                    pass.set_vertex_buffer(0, self.model.background_vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        self.model.background_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.draw_indexed(0..6, 0, 0..1);
                }
                Geometry::Mesh(index) => {
                    let mesh = &self.model.meshes[index];
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                }
            }
        }
        Ok(())
    }

    fn encode_masks(&self, encoder: &mut wgpu::CommandEncoder) -> Result<(), OverlayError> {
        let scale_offset = model_transform(
            self.model.bounds,
            self.width as f32,
            self.height as f32,
            self.model.mirror_horizontal,
        );
        for mesh in &self.model.meshes {
            let Some(target) = &mesh.mask_target else {
                continue;
            };
            let mut draws = Vec::new();
            for source_id in &mesh.masks {
                let source_index = self
                    .model
                    .meshes
                    .iter()
                    .position(|source| source.id == *source_id)
                    .ok_or_else(|| {
                        OverlayError::new(format!("mask source {source_id} is unavailable"))
                    })?;
                let source = &self.model.meshes[source_index];
                let texture = self.model.textures.get(&source.texture_id).ok_or_else(|| {
                    OverlayError::new(format!("mask source {source_id} texture is unavailable"))
                })?;
                draws.push(self.prepare_draw(
                    Geometry::Mesh(source_index),
                    texture,
                    &self.model.empty_mask.view,
                    Uniforms {
                        scale_offset,
                        multiply_color: [1.0; 4],
                        screen_color: [0.0; 4],
                        mask_settings: [0.0; 4],
                        corner_radius: [0.0; 4],
                        opacity_padding: [1.0, 0.0, 0.0, 0.0],
                    },
                    cull_index(drawable_cull_mode(
                        source.double_sided,
                        self.model.mirror_horizontal,
                    )),
                ));
            }
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BongoCat clipping mask"),
                color_attachments: &attachments,
                ..Default::default()
            });
            for draw in &draws {
                pass.set_pipeline(&self.pipelines.mask[draw.pipeline]);
                pass.set_bind_group(0, &draw.texture_group, &[]);
                pass.set_bind_group(1, &draw.uniform_group, &[]);
                let Geometry::Mesh(index) = draw.geometry else {
                    continue;
                };
                let source = &self.model.meshes[index];
                pass.set_vertex_buffer(0, source.vertex_buffer.slice(..));
                pass.set_index_buffer(source.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..source.indices.len() as u32, 0, 0..1);
            }
        }
        Ok(())
    }

    fn encode_presentation(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let draw = self.prepare_draw(
            Geometry::Background,
            &self.composition,
            &self.model.empty_mask.view,
            Uniforms {
                scale_offset: [0.0; 4],
                multiply_color: [0.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius: [0.0; 4],
                opacity_padding: [self.presentation_opacity, 0.0, 0.0, 0.0],
            },
            0,
        );
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("BongoCat final presentation"),
            color_attachments: &attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipelines.present);
        pass.set_bind_group(0, &draw.texture_group, &[]);
        pass.set_bind_group(1, &draw.uniform_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn prepare_draw(
        &self,
        geometry: Geometry,
        texture: &TextureResource,
        mask: &wgpu::TextureView,
        uniforms: Uniforms,
        pipeline: usize,
    ) -> PreparedDraw {
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BongoCat draw uniforms"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let uniform_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BongoCat draw uniform group"),
            layout: &self.pipelines.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let texture_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BongoCat draw texture group"),
            layout: &self.pipelines.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.pipelines.sampler),
                },
            ],
        });
        PreparedDraw {
            geometry,
            pipeline,
            _uniform_buffer: uniform_buffer,
            uniform_group,
            texture_group,
        }
    }

    fn verify_frame_smoke(&self) -> Result<(), OverlayError> {
        let frame = self.read_composition()?;
        verify_pixels(frame.pixels(), self.width, self.height)
    }

    fn read_composition(&self) -> Result<CapturedFrame, OverlayError> {
        let unpadded = self.width as usize * 4;
        let row_pitch = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BongoCat Wayland readback"),
            size: (row_pitch * self.height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BongoCat Wayland readback copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.composition._texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_pitch as u32),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| gpu_error("wait for Vulkan readback", error))?;
        receiver
            .recv()
            .map_err(|_| OverlayError::new("Vulkan readback callback was dropped"))?
            .map_err(|error| gpu_error("map Vulkan readback", error))?;
        let mapped = buffer.slice(..).get_mapped_range();
        let frame =
            CapturedFrame::from_premultiplied_bgra(&mapped, row_pitch, self.width, self.height);
        drop(mapped);
        buffer.unmap();
        frame
    }
}

impl GpuModel {
    fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resources: &RenderResources,
        snapshot: &RenderSnapshot,
        width: u32,
        height: u32,
    ) -> Result<Self, OverlayError> {
        validate_render_snapshot(resources, snapshot)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        let textures = resources
            .textures
            .iter()
            .map(|asset| load_texture(device, queue, asset).map(|texture| (asset.id, texture)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let key_textures = resources
            .key_assets
            .iter()
            .map(|asset| {
                load_texture(
                    device,
                    queue,
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
                    queue,
                    &TextureAsset {
                        id: TextureId::new(usize::MAX),
                        path: asset.path.clone(),
                        width: asset.width,
                        height: asset.height,
                    },
                )
            })
            .transpose()?;
        let bounds = ModelBounds::from_canvas(snapshot.canvas);
        let background_vertices = [
            GpuVertex {
                position: [bounds.min_x, bounds.min_y],
                uv: [0.0, 0.0],
            },
            GpuVertex {
                position: [bounds.max_x, bounds.min_y],
                uv: [1.0, 0.0],
            },
            GpuVertex {
                position: [bounds.max_x, bounds.max_y],
                uv: [1.0, 1.0],
            },
            GpuVertex {
                position: [bounds.min_x, bounds.max_y],
                uv: [0.0, 1.0],
            },
        ];
        let background_indices = [0_u16, 1, 2, 0, 2, 3];
        let mut meshes = Vec::with_capacity(snapshot.drawables.len());
        for drawable in &snapshot.drawables {
            let vertices = drawable
                .vertices
                .iter()
                .map(|vertex| GpuVertex {
                    position: vertex.position,
                    uv: vertex.uv,
                })
                .collect::<Vec<_>>();
            let vertex_data = if vertices.is_empty() {
                bytemuck::bytes_of(&GpuVertex {
                    position: [0.0; 2],
                    uv: [0.0; 2],
                })
            } else {
                bytemuck::cast_slice(&vertices)
            };
            let placeholder_index = [0_u16];
            let index_data = if drawable.indices.is_empty() {
                placeholder_index.as_slice()
            } else {
                drawable.indices.as_slice()
            };
            meshes.push(Mesh {
                id: drawable.id,
                render_order: drawable.render_order,
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BongoCat drawable vertices"),
                    contents: vertex_data,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
                vertex_count: drawable.vertices.len(),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BongoCat drawable indices"),
                    contents: bytemuck::cast_slice(index_data),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                indices: drawable.indices.clone(),
                texture_id: drawable.texture_id,
                opacity: drawable.opacity,
                blend_mode: drawable.blend_mode,
                multiply_color: drawable.multiply_color,
                screen_color: drawable.screen_color,
                masks: drawable.masks.clone(),
                visible: drawable.visible,
                double_sided: drawable.double_sided,
                inverted_mask: drawable.inverted_mask,
                mask_target: (!drawable.masks.is_empty())
                    .then(|| create_mask_target(device, width, height)),
            });
        }
        meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        Ok(Self {
            textures,
            key_textures,
            background,
            background_vertex_buffer: device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("BongoCat background vertices"),
                    contents: bytemuck::cast_slice(&background_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            ),
            background_index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BongoCat background indices"),
                contents: bytemuck::cast_slice(&background_indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            meshes,
            empty_mask: create_empty_mask(device, queue),
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

    fn sync_snapshot(
        &mut self,
        queue: &wgpu::Queue,
        snapshot: &RenderSnapshot,
    ) -> Result<(), OverlayError> {
        if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
            return Err(OverlayError::new("model opacity is outside [0, 1]"));
        }
        if snapshot.drawables.len() != self.meshes.len() {
            return Err(OverlayError::new(
                "drawable count changed within a generation",
            ));
        }
        for drawable in &snapshot.drawables {
            let mesh = self
                .meshes
                .iter_mut()
                .find(|mesh| mesh.id == drawable.id)
                .ok_or_else(|| {
                    OverlayError::new(format!("drawable {} is unavailable", drawable.id))
                })?;
            if mesh.vertex_count != drawable.vertices.len() {
                return Err(OverlayError::new(format!(
                    "drawable {} changed immutable vertex count",
                    drawable.id
                )));
            }
            if mesh.indices != drawable.indices || mesh.masks != drawable.masks {
                return Err(OverlayError::new(format!(
                    "drawable {} changed immutable topology",
                    drawable.id
                )));
            }
            if drawable.dynamic_flags.vertex_positions_changed && !drawable.vertices.is_empty() {
                let vertices = drawable
                    .vertices
                    .iter()
                    .map(|vertex| GpuVertex {
                        position: vertex.position,
                        uv: vertex.uv,
                    })
                    .collect::<Vec<_>>();
                queue.write_buffer(&mesh.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
            }
            mesh.render_order = drawable.render_order;
            mesh.texture_id = drawable.texture_id;
            mesh.opacity = drawable.opacity;
            mesh.blend_mode = drawable.blend_mode;
            mesh.multiply_color = drawable.multiply_color;
            mesh.screen_color = drawable.screen_color;
            mesh.visible = drawable.visible;
            mesh.double_sided = drawable.double_sided;
            mesh.inverted_mask = drawable.inverted_mask;
        }
        self.meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        self.bounds = snapshot.bounds;
        self.model_opacity = snapshot.model_opacity;
        self.mirror_horizontal = snapshot.mirror_horizontal;
        self.active_keys.clone_from(&snapshot.active_keys);
        Ok(())
    }

    fn resize_masks(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        for mesh in &mut self.meshes {
            if mesh.mask_target.is_some() {
                mesh.mask_target = Some(create_mask_target(device, width, height));
            }
        }
    }
}

fn create_pipelines(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Pipelines {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("BongoCat Wayland WGSL"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("BongoCat textures"),
        entries: &[
            texture_layout_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_layout_entry(1, wgpu::ShaderStages::FRAGMENT),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("BongoCat uniforms"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("BongoCat Wayland pipeline layout"),
        bind_group_layouts: &[Some(&texture_layout), Some(&uniform_layout)],
        immediate_size: 0,
    });
    let mut model = Vec::with_capacity(9);
    for mode in [
        BlendMode::Normal,
        BlendMode::Additive,
        BlendMode::Multiplicative,
    ] {
        for cull in [
            DrawableCullMode::None,
            DrawableCullMode::Front,
            DrawableCullMode::Back,
        ] {
            model.push(create_pipeline(
                device,
                &shader,
                &layout,
                "model_vertex",
                "model_fragment",
                COMPOSITION_FORMAT,
                Some(blend_state(mode)),
                cull,
                true,
            ));
        }
    }
    let mask = [
        DrawableCullMode::None,
        DrawableCullMode::Front,
        DrawableCullMode::Back,
    ]
    .into_iter()
    .map(|cull| {
        create_pipeline(
            device,
            &shader,
            &layout,
            "model_vertex",
            "mask_fragment",
            MASK_TEXTURE_FORMAT,
            Some(blend_state(BlendMode::Normal)),
            cull,
            true,
        )
    })
    .collect();
    let present = create_pipeline(
        device,
        &shader,
        &layout,
        "fullscreen_vertex",
        "present_fragment",
        surface_format,
        None,
        DrawableCullMode::None,
        false,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("BongoCat linear clamp sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    Pipelines {
        model,
        mask,
        present,
        texture_layout,
        uniform_layout,
        sampler,
    }
}

#[allow(clippy::too_many_arguments)]
fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    vertex_entry: &str,
    fragment_entry: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    cull: DrawableCullMode,
    uses_vertices: bool,
) -> wgpu::RenderPipeline {
    let vertex_layout = [wgpu::VertexBufferLayout {
        array_stride: size_of::<GpuVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
        ],
    }];
    let buffers = if uses_vertices {
        vertex_layout.as_slice()
    } else {
        &[]
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("BongoCat Wayland render pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: match cull {
                DrawableCullMode::None => None,
                DrawableCullMode::Front => Some(wgpu::Face::Front),
                DrawableCullMode::Back => Some(wgpu::Face::Back),
            },
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn texture_layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn blend_state(mode: BlendMode) -> wgpu::BlendState {
    let factors = blend_factors(mode);
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu_blend_factor(factors.source_rgb),
            dst_factor: wgpu_blend_factor(factors.destination_rgb),
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu_blend_factor(factors.source_alpha),
            dst_factor: wgpu_blend_factor(factors.destination_alpha),
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn wgpu_blend_factor(factor: BlendFactor) -> wgpu::BlendFactor {
    match factor {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::OneMinusSourceAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::DestinationColor => wgpu::BlendFactor::Dst,
    }
}

fn pipeline_index(mode: BlendMode, cull: DrawableCullMode) -> usize {
    let blend = match mode {
        BlendMode::Normal => 0,
        BlendMode::Additive => 1,
        BlendMode::Multiplicative => 2,
    };
    blend * 3 + cull_index(cull)
}

fn cull_index(cull: DrawableCullMode) -> usize {
    match cull {
        DrawableCullMode::None => 0,
        DrawableCullMode::Front => 1,
        DrawableCullMode::Back => 2,
    }
}

fn create_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> TextureResource {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    TextureResource {
        _texture: texture,
        view,
    }
}

fn create_mask_target(device: &wgpu::Device, width: u32, height: u32) -> MaskTarget {
    let target = create_target(device, width, height, MASK_TEXTURE_FORMAT, "BongoCat mask");
    MaskTarget {
        _texture: target._texture,
        view: target.view,
    }
}

fn create_empty_mask(device: &wgpu::Device, queue: &wgpu::Queue) -> TextureResource {
    create_rgba_texture(
        device,
        queue,
        1,
        1,
        &[0, 0, 0, 0],
        MASK_TEXTURE_FORMAT,
        "empty mask",
    )
}

fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    asset: &TextureAsset,
) -> Result<TextureResource, OverlayError> {
    let image = image::ImageReader::open(&asset.path)
        .map_err(|error| gpu_error("open model texture", error))?
        .decode()
        .map_err(|error| gpu_error("decode model texture", error))?
        .into_rgba8();
    if image.width() != asset.width || image.height() != asset.height {
        return Err(OverlayError::new(format!(
            "texture dimensions changed for {}",
            asset.path.display()
        )));
    }
    Ok(create_rgba_texture(
        device,
        queue,
        asset.width,
        asset.height,
        image.as_raw(),
        MODEL_TEXTURE_FORMAT,
        "model texture",
    ))
}

fn create_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    bytes: &[u8],
    format: wgpu::TextureFormat,
    label: &'static str,
) -> TextureResource {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytes,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    TextureResource {
        _texture: texture,
        view,
    }
}

fn verify_pixels(bytes: &[u8], width: u32, height: u32) -> Result<(), OverlayError> {
    let mut pixels =
        Vec::with_capacity((FRAME_SMOKE_GRID_DIMENSION * FRAME_SMOKE_GRID_DIMENSION) as usize);
    for y in 0..FRAME_SMOKE_GRID_DIMENSION as usize {
        for x in 0..FRAME_SMOKE_GRID_DIMENSION as usize {
            let px =
                width.saturating_sub(1) as usize * x / (FRAME_SMOKE_GRID_DIMENSION as usize - 1);
            let py =
                height.saturating_sub(1) as usize * y / (FRAME_SMOKE_GRID_DIMENSION as usize - 1);
            let offset = (py * width as usize + px) * 4;
            pixels.push([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
        }
    }
    validate_frame_smoke(pixels)
        .map(|_| ())
        .map_err(|error| OverlayError::new(format!("Vulkan {error}")))
}

fn model_transform(
    bounds: ModelBounds,
    width: f32,
    height: f32,
    mirror_horizontal: bool,
) -> [f32; 4] {
    let model_width = bounds.width();
    let model_height = bounds.height();
    let [center_x, center_y] = bounds.center();
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

#[cfg(test)]
mod alpha_mode_tests {
    use super::{WAYLAND_MAXIMUM_FRAME_LATENCY, select_wayland_alpha_mode};
    use wgpu::CompositeAlphaMode::{Inherit, Opaque, PostMultiplied, PreMultiplied};

    #[test]
    fn inherit_keeps_a_wayland_surface_transparent_when_opaque_is_listed_first() {
        assert_eq!(
            select_wayland_alpha_mode(&[Opaque, Inherit]).unwrap(),
            Inherit
        );
    }

    #[test]
    fn premultiplied_is_preferred_independently_of_driver_order() {
        assert_eq!(
            select_wayland_alpha_mode(&[Inherit, Opaque, PreMultiplied]).unwrap(),
            PreMultiplied
        );
    }

    #[test]
    fn unsupported_alpha_encodings_fail_instead_of_showing_an_opaque_window() {
        let error = select_wayland_alpha_mode(&[Opaque, PostMultiplied]).unwrap_err();
        assert!(error.to_string().contains("Opaque"));
        assert!(error.to_string().contains("PostMultiplied"));
    }

    #[test]
    fn wayland_presentation_keeps_a_bounded_fifo_queue_depth() {
        assert_eq!(WAYLAND_MAXIMUM_FRAME_LATENCY, 2);
    }
}
