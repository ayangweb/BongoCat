use super::*;
use bongocat_render::{DrawableSnapshot, RenderFrame, RenderResources, Vertex};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

pub struct Renderer {
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: Vec<wgpu::RenderPipeline>,
    mask_pipeline: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    canvas: wgpu::Texture,
    mask: wgpu::Texture,
    white: wgpu::Texture,
    textures: Vec<wgpu::Texture>,
    keys: Vec<wgpu::Texture>,
    background: Option<wgpu::Texture>,
    resources: Arc<RenderResources>,
}
fn error(e: impl std::fmt::Display) -> OverlayError {
    OverlayError::new(e.to_string())
}
fn bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}
impl Renderer {
    pub fn new(
        window: Option<Arc<winit::window::Window>>,
        frame: &RenderFrame,
        width: u32,
        height: u32,
    ) -> Result<Self, OverlayError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = window
            .map(|window| instance.create_surface(window))
            .transpose()
            .map_err(error)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface.as_ref(),
            ..Default::default()
        }))
        .map_err(error)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("BongoCat Linux"),
            ..Default::default()
        }))
        .map_err(error)?;
        let format = if let Some(surface) = &surface {
            let caps = surface.get_capabilities(&adapter);
            if !caps
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
            {
                return Err(error("Wayland surface lacks premultiplied transparency"));
            }
            caps.formats
                .iter()
                .copied()
                .find(|f| !f.is_srgb())
                .ok_or_else(|| error("surface has no encoded-color format"))?
        } else {
            FORMAT
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        if let Some(surface) = &surface {
            surface.configure(&device, &config);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Cubism Linux"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry, blend, cull, format| {
            pipeline(
                &device,
                &pipeline_layout,
                &shader,
                entry,
                blend,
                cull,
                format,
            )
        };
        let mut pipelines = Vec::new();
        for mode in [
            bongocat_render::BlendMode::Normal,
            bongocat_render::BlendMode::Additive,
            bongocat_render::BlendMode::Multiplicative,
        ] {
            let f = crate::blend_factors(mode);
            let blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: factor(f.source_rgb),
                    dst_factor: factor(f.destination_rgb),
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: factor(f.source_alpha),
                    dst_factor: factor(f.destination_alpha),
                    operation: wgpu::BlendOperation::Add,
                },
            };
            for cull in [None, Some(wgpu::Face::Back), Some(wgpu::Face::Front)] {
                pipelines.push(make("fragment", Some(blend), cull, FORMAT));
            }
        }
        let mask_pipeline = make(
            "mask_fragment",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            None,
            FORMAT,
        );
        let present = make("present_fragment", None, None, format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let canvas = target(&device, config.width, config.height);
        let mask = target(&device, config.width, config.height);
        let white = upload(&device, &queue, 1, 1, &[255, 255, 255, 255]);
        let textures = load_textures(&device, &queue, &frame.resources)?;
        let keys = frame
            .resources
            .key_assets
            .iter()
            .map(|a| load_image(&device, &queue, &a.path))
            .collect::<Result<Vec<_>, _>>()?;
        let background = frame
            .resources
            .background
            .as_ref()
            .map(|a| load_image(&device, &queue, &a.path))
            .transpose()?;
        Ok(Self {
            surface,
            device,
            queue,
            config,
            layout,
            sampler,
            pipelines,
            mask_pipeline,
            present,
            canvas,
            mask,
            white,
            textures,
            keys,
            background,
            resources: frame.resources.clone(),
        })
    }
    pub fn prepare(&mut self, frame: &RenderFrame) -> Result<(), OverlayError> {
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let prepared = (|| {
            let textures = load_textures(&self.device, &self.queue, &frame.resources)?;
            let keys = frame
                .resources
                .key_assets
                .iter()
                .map(|a| load_image(&self.device, &self.queue, &a.path))
                .collect::<Result<Vec<_>, _>>()?;
            let background = frame
                .resources
                .background
                .as_ref()
                .map(|a| load_image(&self.device, &self.queue, &a.path))
                .transpose()?;
            Ok::<_, OverlayError>((textures, keys, background))
        })();
        let validation = pollster::block_on(validation.pop());
        let oom = pollster::block_on(oom.pop());
        if let Some(failure) = validation.or(oom) {
            return Err(error(failure));
        }
        let (textures, keys, background) = prepared?;
        self.textures = textures;
        self.keys = keys;
        self.background = background;
        self.resources = frame.resources.clone();
        Ok(())
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.config.width && height == self.config.height)
        {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
        self.canvas = target(&self.device, width, height);
        self.mask = target(&self.device, width, height);
    }
    fn group(
        &self,
        texture: &wgpu::Texture,
        mask: &wgpu::Texture,
        values: &[f32],
    ) -> wgpu::BindGroup {
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes(values.iter().copied()),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &mask.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
    fn draw_mesh(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        mesh: (&[Vertex], &[u16]),
        clear: bool,
    ) {
        let (vertices, indices) = mesh;
        let v = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes(
                    vertices
                        .iter()
                        .flat_map(|v| v.position.into_iter().chain(v.uv)),
                ),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_bytes: Vec<_> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let i = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &index_bytes,
                usage: wgpu::BufferUsages::INDEX,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_vertex_buffer(0, v.slice(..));
        pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }
    pub fn draw(
        &mut self,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        visible: bool,
        opacity_multiplier: f32,
    ) -> Result<OverlayTickOutcome, OverlayError> {
        let output = if let Some(surface) = &self.surface {
            Some(match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok(OverlayTickOutcome::Deferred(Duration::from_millis(100)));
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    surface.configure(&self.device, &self.config);
                    return Ok(OverlayTickOutcome::Deferred(Duration::from_millis(100)));
                }
                _ => return Err(error("Wayland surface acquisition failed")),
            })
        } else {
            None
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let canvas = self.canvas.create_view(&Default::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &canvas,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        let snapshot = &frame.snapshot;
        let bounds = bongocat_render::ModelBounds::from_canvas(snapshot.canvas);
        let quad = [
            Vertex {
                position: [bounds.min_x, bounds.min_y],
                uv: [0., 0.],
            },
            Vertex {
                position: [bounds.max_x, bounds.min_y],
                uv: [1., 0.],
            },
            Vertex {
                position: [bounds.max_x, bounds.max_y],
                uv: [1., 1.],
            },
            Vertex {
                position: [bounds.min_x, bounds.max_y],
                uv: [0., 1.],
            },
        ];
        let mut layer_params = [0.; 20];
        layer_params[..4].copy_from_slice(&self.transform(frame));
        layer_params[4..8].fill(1.);
        layer_params[12] = self.config.width as f32;
        layer_params[13] = self.config.height as f32;
        layer_params[16] = 1.;
        if let Some(background) = &self.background {
            let group = self.group(background, &self.white, &layer_params);
            self.draw_mesh(
                &mut encoder,
                &canvas,
                &self.pipelines[0],
                &group,
                (&quad, &[0, 1, 2, 0, 2, 3]),
                false,
            );
        }
        let by_id: BTreeMap<_, _> = snapshot.drawables.iter().map(|d| (d.id, d)).collect();
        let mut drawables: Vec<_> = snapshot
            .drawables
            .iter()
            .filter(|d| d.visible && !d.indices.is_empty())
            .collect();
        drawables.sort_by_key(|d| d.render_order);
        for drawable in drawables {
            let masked = !drawable.masks.is_empty();
            if masked {
                let view = self.mask.create_view(&Default::default());
                {
                    let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                }
                for id in &drawable.masks {
                    let mask = by_id
                        .get(id)
                        .ok_or_else(|| error("missing mask drawable"))?;
                    if mask.indices.is_empty() {
                        continue;
                    }
                    let group = self.group(
                        &self.textures[mask.texture_id.index()],
                        &self.white,
                        &self.params(frame, mask, false),
                    );
                    self.draw_mesh(
                        &mut encoder,
                        &view,
                        &self.mask_pipeline,
                        &group,
                        (&mask.vertices, &mask.indices),
                        false,
                    );
                }
            }
            let group = self.group(
                &self.textures[drawable.texture_id.index()],
                if masked { &self.mask } else { &self.white },
                &self.params(frame, drawable, masked),
            );
            let mode = match drawable.blend_mode {
                bongocat_render::BlendMode::Normal => 0,
                bongocat_render::BlendMode::Additive => 1,
                bongocat_render::BlendMode::Multiplicative => 2,
            };
            let cull = match crate::drawable_cull_mode(
                drawable.double_sided,
                snapshot.mirror_horizontal,
            ) {
                crate::DrawableCullMode::None => 0,
                crate::DrawableCullMode::Back => 1,
                crate::DrawableCullMode::Front => 2,
            };
            self.draw_mesh(
                &mut encoder,
                &canvas,
                &self.pipelines[mode * 3 + cull],
                &group,
                (&drawable.vertices, &drawable.indices),
                false,
            );
        }
        for key in &snapshot.active_keys {
            let group = self.group(&self.keys[key.asset_id.index()], &self.white, &layer_params);
            self.draw_mesh(
                &mut encoder,
                &canvas,
                &self.pipelines[0],
                &group,
                (&quad, &[0, 1, 2, 0, 2, 3]),
                false,
            );
        }
        let params = [
            1.,
            1.,
            0.,
            0.,
            1.,
            1.,
            1.,
            1.,
            0.,
            0.,
            0.,
            0.,
            self.config.width as f32,
            self.config.height as f32,
            0.,
            0.,
            if visible {
                f32::from(options.opacity_percent) / 100. * opacity_multiplier
            } else {
                0.
            },
            crate::corner_radius_uniform(
                options.corner_radius_percent,
                self.config.width as f32,
                self.config.height as f32,
            )[0],
            0.,
            0.,
        ];
        let group = self.group(&self.canvas, &self.white, &params);
        let vertices = [
            Vertex {
                position: [-1., -1.],
                uv: [0., 0.],
            },
            Vertex {
                position: [1., -1.],
                uv: [1., 0.],
            },
            Vertex {
                position: [1., 1.],
                uv: [1., 1.],
            },
            Vertex {
                position: [-1., 1.],
                uv: [0., 1.],
            },
        ];
        if let Some(output) = &output {
            self.draw_mesh(
                &mut encoder,
                &output.texture.create_view(&Default::default()),
                &self.present,
                &group,
                (&vertices, &[0, 1, 2, 0, 2, 3]),
                true,
            );
        }
        self.queue.submit([encoder.finish()]);
        if let Some(output) = output {
            self.queue.present(output);
        }
        Ok(if visible {
            OverlayTickOutcome::Presented
        } else {
            OverlayTickOutcome::Hidden
        })
    }
    fn transform(&self, frame: &RenderFrame) -> [f32; 4] {
        let b = frame.snapshot.bounds;
        let center = b.center();
        let aspect = self.config.width as f32 / self.config.height as f32;
        let sy = 2. / b.height().max(b.width() / aspect);
        let sx = sy / aspect
            * if frame.snapshot.mirror_horizontal {
                -1.
            } else {
                1.
            };
        [sx, sy, -center[0] * sx, -center[1] * sy]
    }
    fn params(&self, frame: &RenderFrame, d: &DrawableSnapshot, masked: bool) -> [f32; 20] {
        let [sx, sy, ox, oy] = self.transform(frame);
        [
            sx,
            sy,
            ox,
            oy,
            d.multiply_color[0],
            d.multiply_color[1],
            d.multiply_color[2],
            1.,
            d.screen_color[0],
            d.screen_color[1],
            d.screen_color[2],
            0.,
            self.config.width as f32,
            self.config.height as f32,
            if masked { 1. } else { 0. },
            if d.inverted_mask { 1. } else { 0. },
            d.opacity * frame.snapshot.model_opacity,
            0.,
            0.,
            0.,
        ]
    }
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn target(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: u32,
    h: u32,
    data: &[u8],
) -> wgpu::Texture {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        t.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        t.size(),
    );
    t
}
fn load_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: &std::path::Path,
) -> Result<wgpu::Texture, OverlayError> {
    let image = image::open(path).map_err(error)?.to_rgba8();
    if image.width() > device.limits().max_texture_dimension_2d
        || image.height() > device.limits().max_texture_dimension_2d
    {
        return Err(error("model texture exceeds GPU limits"));
    }
    Ok(upload(device, queue, image.width(), image.height(), &image))
}
fn load_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &RenderResources,
) -> Result<Vec<wgpu::Texture>, OverlayError> {
    r.textures
        .iter()
        .map(|t| load_image(device, queue, &t.path))
        .collect()
}

fn factor(f: crate::BlendFactor) -> wgpu::BlendFactor {
    match f {
        crate::BlendFactor::Zero => wgpu::BlendFactor::Zero,
        crate::BlendFactor::One => wgpu::BlendFactor::One,
        crate::BlendFactor::OneMinusSourceAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        crate::BlendFactor::DestinationColor => wgpu::BlendFactor::Dst,
    }
}
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry: &str,
    blend: Option<wgpu::BlendState>,
    cull: Option<wgpu::Face>,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl Renderer {
    pub fn readback(&self) -> Result<crate::cover::CapturedFrame, OverlayError> {
        let row = (self.config.width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row) * u64::from(self.config.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.canvas.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(self.config.height),
                },
            },
            self.canvas.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(5)),
            })
            .map_err(error)?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(error)?
            .map_err(error)?;
        let mapped = buffer.slice(..).get_mapped_range().map_err(error)?;
        let result = crate::cover::CapturedFrame::from_premultiplied_bgra(
            &mapped,
            row as usize,
            self.config.width,
            self.config.height,
        );
        drop(mapped);
        buffer.unmap();
        result
    }
}
