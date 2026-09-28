//! Drawing one frame.
//!
//! The renderer owns the D3D11 device, the DirectComposition visual and the
//! swap chain, and it composites the model over a transparent target in one
//! pass. It reads only the snapshot it is handed: deciding what to show is the
//! runtime's job, not the renderer's, so a frame can never change the product's
//! state.

use super::*;

pub(crate) struct Mesh {
    pub(crate) id: DrawableId,
    pub(crate) render_order: i32,
    pub(crate) vertex_buffer: ID3D11Buffer,
    pub(crate) vertex_bytes: usize,
    pub(crate) index_buffer: ID3D11Buffer,
    pub(crate) index_bytes: usize,
    pub(crate) indices: Vec<u16>,
    pub(crate) index_count: u32,
    pub(crate) texture_id: TextureId,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) multiply_color: [f32; 4],
    pub(crate) screen_color: [f32; 4],
    pub(crate) masks: Vec<DrawableId>,
    pub(crate) visible: bool,
    pub(crate) double_sided: bool,
    pub(crate) inverted_mask: bool,
    pub(crate) mask_target: Option<MaskTarget>,
}

pub(crate) struct ComApartment {
    pub(crate) owner_thread: ThreadId,
    pub(crate) _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl ComApartment {
    pub(crate) fn initialize() -> Result<Self, OverlayError> {
        // SAFETY: ProductOverlaySession owns this STA on the current UI thread,
        // and its field order releases every COM interface before this guard.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
            .map_err(windows_error("initialize COM apartment"))?;
        Ok(Self {
            owner_thread: thread::current().id(),
            _not_send_or_sync: std::marker::PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        assert_eq!(self.owner_thread, thread::current().id());
        // SAFETY: this balances the successful CoInitializeEx call on the same
        // thread after all session-owned COM interfaces have been released.
        unsafe { CoUninitialize() };
    }
}

/// The swap chain buffers and the views that read and write them.
///
/// They are grouped because `ResizeBuffers` only succeeds once every reference
/// to the old buffers is released, so a resize has to drop all three together
/// and rebuild them from the resized chain.
pub(crate) struct RenderTargets {
    pub(crate) render_target: ID3D11RenderTargetView,
    pub(crate) staging_texture: ID3D11Texture2D,
    pub(crate) back_buffer: ID3D11Texture2D,
}

pub(crate) struct Renderer {
    pub(crate) visual: IDCompositionVisual,
    /// Applies presentation opacity after every model, mask, background, and
    /// key drawable has already been composited into the swap-chain surface.
    /// Keeping this at the visual subtree is important: multiplying the
    /// configured alpha into each drawable makes overlapping Live2D parts
    /// accumulate transparency and produces a ghosted image.
    pub(crate) opacity_effect: IDCompositionEffectGroup,
    pub(crate) target: IDCompositionTarget,
    pub(crate) composition_device: IDCompositionDevice,
    /// `None` only while a resize is between dropping the old buffers and
    /// rebuilding them; every other path requires them to be present.
    pub(crate) targets: Option<RenderTargets>,
    pub(crate) swap_chain: IDXGISwapChain1,
    pub(crate) memory_adapter: IDXGIAdapter3,
    pub(crate) pipelines: Pipelines,
    pub(crate) context: ID3D11DeviceContext,
    pub(crate) device: ID3D11Device,
    pub(crate) model_generation: u64,
    pub(crate) resources: Arc<RenderResources>,
    pub(crate) model: GpuModel,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) corner_radius_percent: u8,
    pub(crate) corner_radius: [f32; 4],
    pub(crate) owner_thread: ThreadId,
    pub(crate) _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl Renderer {
    pub(crate) fn create(
        window: &OverlayWindow,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
    ) -> Result<Self, OverlayError> {
        // SAFETY: all interfaces and resources are created for one live HWND
        // and remain confined to the current ProductOverlaySession thread.
        unsafe { Self::create_inner(window, frame, options) }
            .map_err(windows_error("create D3D11 renderer"))
    }

    pub(crate) unsafe fn create_inner(
        window: &OverlayWindow,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
    ) -> WindowsResult<Self> {
        let (device, context) = unsafe { create_d3d11_device()? };
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let memory_adapter: IDXGIAdapter3 = adapter.cast()?;
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let descriptor = DXGI_SWAP_CHAIN_DESC1 {
            Width: window.width,
            Height: window.height,
            Format: COMPOSITION_FORMAT,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        let swap_chain =
            unsafe { factory.CreateSwapChainForComposition(&device, &descriptor, None)? };
        let composition_device: IDCompositionDevice =
            unsafe { DCompositionCreateDevice(&dxgi_device)? };
        let target = unsafe { composition_device.CreateTargetForHwnd(window.hwnd, true)? };
        let visual = unsafe { composition_device.CreateVisual()? };
        // The default DirectComposition layer opacity mode treats this visual's
        // swap-chain subtree as one surface. That is the final-composite
        // boundary we need; Multiply mode would reintroduce per-surface fading.
        let opacity_effect = unsafe { composition_device.CreateEffectGroup()? };
        let initial_opacity = f32::from(options.opacity_percent) / 100.0;
        unsafe {
            opacity_effect.SetOpacity2(initial_opacity)?;
            visual.SetContent(&swap_chain)?;
            visual.SetEffect(&opacity_effect)?;
            target.SetRoot(&visual)?;
            composition_device.Commit()?;
        }
        let back_buffer: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
        let render_target = unsafe {
            create_render_target(&device, &back_buffer, COMPOSITION_RENDER_TARGET_FORMAT)?
        };
        let staging_texture = unsafe { create_staging_texture(&device, &back_buffer)? };
        let pipelines = unsafe { create_pipelines(&device)? };
        let model = unsafe {
            GpuModel::prepare(
                &device,
                &frame.resources,
                &frame.snapshot,
                window.width,
                window.height,
            )?
        };
        Ok(Self {
            visual,
            opacity_effect,
            target,
            composition_device,
            targets: Some(RenderTargets {
                render_target,
                staging_texture,
                back_buffer,
            }),
            swap_chain,
            memory_adapter,
            pipelines,
            context,
            device,
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model,
            width: window.width,
            height: window.height,
            corner_radius_percent: options.corner_radius_percent,
            corner_radius: corner_radius_uniform(
                options.corner_radius_percent,
                window.width as f32,
                window.height as f32,
            ),
            owner_thread: thread::current().id(),
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    /// Apply the effective presentation opacity to the completed composition
    /// subtree rather than to individual drawables.
    ///
    /// The renderer keeps the swap-chain pixels at their model-authored alpha;
    /// DirectComposition applies this value once after all model, background,
    /// mask, and key layers have been blended. This preserves a coherent image
    /// for models with many overlapping Live2D parts.
    pub(crate) fn set_opacity(&self, opacity: f32) -> Result<(), OverlayError> {
        let alpha = opacity.clamp(0.0, 1.0);
        // SAFETY: the renderer owns both COM interfaces, they are confined to
        // its owner thread, and the effect has already been attached to the
        // visual before this method can be called. The value is finite after
        // clamping, as required by IDCompositionEffectGroup::SetOpacity.
        unsafe {
            self.opacity_effect
                .SetOpacity2(alpha)
                .map_err(windows_error("set DirectComposition opacity"))?;
            self.composition_device
                .Commit()
                .map_err(windows_error("commit DirectComposition opacity"))?;
        }
        Ok(())
    }

    /// Match the swap chain, the render targets and the mask targets to a new
    /// window size.
    ///
    /// A right-button resize drag changes the window size while the frame loop
    /// keeps running, so the buffers are resized in place rather than rebuilt:
    /// the D3D11 device, the pipelines, the composition graph and every model
    /// texture stay alive, and only the size-dependent resources are replaced.
    ///
    /// Returns whether the size changed.
    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<bool, OverlayError> {
        self.assert_owner_thread();
        if (width, height) == (self.width, self.height) {
            return Ok(false);
        }
        // SAFETY: the swap chain, its buffers and the device belong to this
        // renderer and thread; the immediate context is flushed before the old
        // buffers are released.
        unsafe { self.resize_inner(width, height) }
            .map_err(windows_error("resize D3D11 overlay"))?;
        Ok(true)
    }

    pub(crate) unsafe fn resize_inner(&mut self, width: u32, height: u32) -> WindowsResult<()> {
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            self.context.PSSetShaderResources(0, Some(&[None, None]));
            self.context.ClearState();
            self.context.Flush();
        }
        // Dropping the views and the buffer handles is what makes
        // `ResizeBuffers` legal: DXGI refuses while any reference to a back
        // buffer is alive.
        self.targets = None;
        unsafe {
            self.swap_chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0)? };
        let render_target = unsafe {
            create_render_target(&self.device, &back_buffer, COMPOSITION_RENDER_TARGET_FORMAT)?
        };
        let staging_texture = unsafe { create_staging_texture(&self.device, &back_buffer)? };
        self.targets = Some(RenderTargets {
            render_target,
            staging_texture,
            back_buffer,
        });
        for mesh in &mut self.model.meshes {
            if mesh.mask_target.is_some() {
                mesh.mask_target =
                    Some(unsafe { create_mask_target(&self.device, width, height)? });
            }
        }
        self.width = width;
        self.height = height;
        self.corner_radius =
            corner_radius_uniform(self.corner_radius_percent, width as f32, height as f32);
        Ok(())
    }

    pub(crate) fn sync_frame(&mut self, frame: &RenderFrame) -> Result<bool, OverlayError> {
        self.assert_owner_thread();
        if frame.model_generation != self.model_generation {
            validate_model_generation_advance(self.model_generation, frame.model_generation)?;
            // SAFETY: candidate resources are prepared against this renderer's
            // live device; self is changed only after every allocation succeeds.
            let candidate = unsafe {
                GpuModel::prepare(
                    &self.device,
                    &frame.resources,
                    &frame.snapshot,
                    self.width,
                    self.height,
                )
            }
            .map_err(windows_error("prepare D3D11 model resources"))?;
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
        // SAFETY: buffer sizes and topology are checked before UpdateSubresource
        // copies from immutable snapshot slices into device-owned buffers.
        unsafe { self.model.sync_snapshot(&self.context, &frame.snapshot) }
            .map_err(windows_error("update D3D11 model snapshot"))?;
        Ok(false)
    }

    pub(crate) fn current_local_memory_usage(&self) -> Result<u64, OverlayError> {
        self.assert_owner_thread();
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        // SAFETY: node zero is the primary adapter node and info is writable
        // for the complete synchronous QueryVideoMemoryInfo call.
        unsafe {
            self.memory_adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
        }
        .map_err(windows_error("query renderer local video memory"))?;
        Ok(info.CurrentUsage)
    }

    pub(crate) fn draw(&self, verify: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: every interface belongs to this renderer and current thread;
        // all bound buffers/textures outlive the synchronous immediate context.
        unsafe { self.draw_inner(verify, false) }
            .map(|_| ())
            .map_err(|error| {
                if error.code() == DXGI_STATUS_OCCLUDED {
                    OverlayError::temporary_presentation_unavailable("DXGI swap chain is occluded")
                } else {
                    windows_error("draw D3D11 model")(error)
                }
            })
    }

    /// Draw one frame and read it back as cover pixels, without presenting it.
    ///
    /// The cover capture owns a window the user never sees, so there is nothing to
    /// present to: the frame is copied into the staging texture and read from there
    /// and the swap chain keeps the buffer it already had. Skipping the present is
    /// also what keeps the capture off the drivers' occlusion rules — a hidden
    /// composition swap chain can report DXGI_STATUS_OCCLUDED on Present, which is a
    /// property of the capture window rather than of the model being captured.
    pub(crate) fn draw_capturing(&self, verify: bool) -> Result<CapturedFrame, OverlayError> {
        self.assert_owner_thread();
        // SAFETY: every interface belongs to this renderer and current thread, and
        // the readback maps the staging texture copied from this renderer's own back
        // buffer.
        unsafe { self.draw_inner(verify, true) }
            .map_err(windows_error("capture D3D11 model frame"))?
            .ok_or_else(|| OverlayError::new("captured D3D11 frame was not read back"))
    }

    pub(crate) unsafe fn draw_inner(
        &self,
        verify: bool,
        capture: bool,
    ) -> WindowsResult<Option<CapturedFrame>> {
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.width as f32,
            Height: self.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            self.context.RSSetViewports(Some(&[viewport]));
            self.context.RSSetState(&self.pipelines.rasterizer);
            self.context.IASetInputLayout(&self.pipelines.input_layout);
            self.context
                .IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context
                .VSSetShader(&self.pipelines.vertex_shader, None);
            self.context
                .VSSetConstantBuffers(0, Some(&[Some(self.pipelines.constant_buffer.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.pipelines.constant_buffer.clone())]));
            self.context
                .PSSetSamplers(0, Some(&[Some(self.pipelines.sampler.clone())]));
        }
        let scale_offset = model_transform(
            self.model.bounds,
            self.width as f32,
            self.height as f32,
            self.model.mirror_horizontal,
        );
        for mesh in &self.model.meshes {
            let Some(mask_target) = &mesh.mask_target else {
                continue;
            };
            unsafe {
                self.context.PSSetShaderResources(0, Some(&[None, None]));
                self.context.OMSetRenderTargets(
                    Some(&[Some(mask_target.render_target.clone())]),
                    None::<&ID3D11DepthStencilView>,
                );
                self.context
                    .ClearRenderTargetView(&mask_target.render_target, &[0.0; 4]);
                self.context.PSSetShader(&self.pipelines.mask_shader, None);
                self.context
                    .OMSetBlendState(&self.pipelines.mask_blend, None, u32::MAX);
            }
            for source_id in &mesh.masks {
                let source = self
                    .model
                    .meshes
                    .iter()
                    .find(|source| source.id == *source_id)
                    .ok_or_else(|| invariant_error("mask source is unavailable"))?;
                let uniforms = Uniforms {
                    scale_offset,
                    multiply_color: [1.0; 4],
                    screen_color: [0.0; 4],
                    mask_settings: [0.0; 4],
                    corner_radius: [0.0; 4],
                    opacity: 1.0,
                    padding: [0.0; 3],
                };
                unsafe {
                    self.bind_mesh(source, &uniforms, &self.model.empty_mask.shader_resource)?
                };
            }
        }
        let targets = self
            .targets
            .as_ref()
            .ok_or_else(|| invariant_error("renderer targets are unavailable"))?;
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(targets.render_target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            self.context
                .ClearRenderTargetView(&targets.render_target, &[0.0; 4]);
            self.context
                .PSSetShader(&self.pipelines.fragment_shader, None);
        }
        if let Some(background) = &self.model.background {
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: [1.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius: self.corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            unsafe {
                self.context.RSSetState(&self.pipelines.rasterizer);
                self.context
                    .OMSetBlendState(&self.pipelines.normal_blend, None, u32::MAX);
                self.context.UpdateSubresource(
                    &self.pipelines.constant_buffer,
                    0,
                    None,
                    std::ptr::from_ref(&uniforms).cast(),
                    0,
                    0,
                );
                let vertex_buffer = Some(self.model.background_vertex_buffer.clone());
                let stride = size_of::<bongocat_render::Vertex>() as u32;
                let offset = 0_u32;
                self.context.IASetVertexBuffers(
                    0,
                    1,
                    Some(&raw const vertex_buffer),
                    Some(&raw const stride),
                    Some(&raw const offset),
                );
                self.context.IASetIndexBuffer(
                    &self.model.background_index_buffer,
                    DXGI_FORMAT_R16_UINT,
                    0,
                );
                self.context.PSSetShaderResources(
                    0,
                    Some(&[
                        Some(background.shader_resource.clone()),
                        Some(self.model.empty_mask.shader_resource.clone()),
                    ]),
                );
                self.context.DrawIndexed(6, 0, 0);
            }
        }
        for mesh in &self.model.meshes {
            if !mesh.visible || mesh.opacity <= 0.0 {
                continue;
            }
            let mask = mesh
                .mask_target
                .as_ref()
                .map_or(&self.model.empty_mask.shader_resource, |target| {
                    &target.shader_resource
                });
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: mesh.multiply_color,
                screen_color: mesh.screen_color,
                mask_settings: [
                    self.width as f32,
                    self.height as f32,
                    f32::from(mesh.mask_target.is_some()),
                    f32::from(mesh.inverted_mask),
                ],
                corner_radius: self.corner_radius,
                opacity: mesh.opacity * self.model.model_opacity,
                padding: [0.0; 3],
            };
            unsafe {
                self.context
                    .OMSetBlendState(self.pipelines.blend(mesh.blend_mode), None, u32::MAX);
                self.bind_mesh(mesh, &uniforms, mask)?;
            }
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
                corner_radius: self.corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            unsafe {
                self.context.RSSetState(&self.pipelines.rasterizer);
                self.context
                    .OMSetBlendState(&self.pipelines.normal_blend, None, u32::MAX);
                self.context.UpdateSubresource(
                    &self.pipelines.constant_buffer,
                    0,
                    None,
                    std::ptr::from_ref(&uniforms).cast(),
                    0,
                    0,
                );
                let vertex_buffer = Some(self.model.background_vertex_buffer.clone());
                let stride = size_of::<bongocat_render::Vertex>() as u32;
                let offset = 0_u32;
                self.context.IASetVertexBuffers(
                    0,
                    1,
                    Some(&raw const vertex_buffer),
                    Some(&raw const stride),
                    Some(&raw const offset),
                );
                self.context.IASetIndexBuffer(
                    &self.model.background_index_buffer,
                    DXGI_FORMAT_R16_UINT,
                    0,
                );
                self.context.PSSetShaderResources(
                    0,
                    Some(&[
                        Some(texture.shader_resource.clone()),
                        Some(self.model.empty_mask.shader_resource.clone()),
                    ]),
                );
                self.context.DrawIndexed(6, 0, 0);
            }
        }
        if verify || capture {
            unsafe {
                self.context
                    .CopyResource(&targets.staging_texture, &targets.back_buffer);
                if verify {
                    verify_frame_smoke(
                        &self.context,
                        &targets.staging_texture,
                        self.width,
                        self.height,
                    )?;
                }
            }
        }
        if capture {
            // Deliberately no present here: see `draw_capturing`.
            let captured = unsafe {
                read_staging_frame(
                    &self.context,
                    &targets.staging_texture,
                    self.width,
                    self.height,
                )
            }?;
            return Ok(Some(captured));
        }
        unsafe {
            let present = self.swap_chain.Present(1, DXGI_PRESENT(0));
            if present == DXGI_STATUS_OCCLUDED {
                return Err(Error::new(
                    DXGI_STATUS_OCCLUDED,
                    "DXGI swap chain is occluded",
                ));
            }
            present.ok()?;
            self.device.GetDeviceRemovedReason()?;
        }
        Ok(None)
    }

    pub(crate) unsafe fn bind_mesh(
        &self,
        mesh: &Mesh,
        uniforms: &Uniforms,
        mask: &ID3D11ShaderResourceView,
    ) -> WindowsResult<()> {
        let vertex_buffer = Some(mesh.vertex_buffer.clone());
        let stride = size_of::<bongocat_render::Vertex>() as u32;
        let offset = 0_u32;
        let texture = self
            .model
            .textures
            .get(&mesh.texture_id)
            .ok_or_else(|| invariant_error("drawable texture is unavailable"))?;
        unsafe {
            let rasterizer =
                match drawable_cull_mode(mesh.double_sided, self.model.mirror_horizontal) {
                    DrawableCullMode::None => &self.pipelines.rasterizer,
                    DrawableCullMode::Front => &self.pipelines.cull_front_rasterizer,
                    DrawableCullMode::Back => &self.pipelines.cull_back_rasterizer,
                };
            self.context.RSSetState(rasterizer);
            self.context.UpdateSubresource(
                &self.pipelines.constant_buffer,
                0,
                None,
                std::ptr::from_ref(uniforms).cast(),
                0,
                0,
            );
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(&raw const vertex_buffer),
                Some(&raw const stride),
                Some(&raw const offset),
            );
            self.context
                .IASetIndexBuffer(&mesh.index_buffer, DXGI_FORMAT_R16_UINT, 0);
            self.context.PSSetShaderResources(
                0,
                Some(&[Some(texture.shader_resource.clone()), Some(mask.clone())]),
            );
            self.context.DrawIndexed(mesh.index_count, 0, 0);
        }
        Ok(())
    }

    pub(crate) fn assert_owner_thread(&self) {
        assert_eq!(self.owner_thread, thread::current().id());
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: teardown occurs on the owner thread. The composition graph
        // is detached and the immediate context flushed before COM release.
        unsafe {
            self.context.PSSetShaderResources(0, Some(&[None, None]));
            self.context.ClearState();
            self.context.Flush();
            let _ = self.visual.SetContent(None::<&windows::core::IUnknown>);
            let _ = self.target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self.composition_device.Commit();
        }
    }
}

pub(crate) unsafe fn create_d3d11_device() -> WindowsResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut last_error = None;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        match unsafe { try_create_d3d11_device(driver) } {
            Ok(result) => return Ok(result),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(Error::from_thread))
}

pub(crate) unsafe fn try_create_d3d11_device(
    driver: D3D_DRIVER_TYPE,
) -> WindowsResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    Ok((
        device.ok_or_else(|| invariant_error("D3D11CreateDevice returned no device"))?,
        context.ok_or_else(|| invariant_error("D3D11CreateDevice returned no context"))?,
    ))
}

pub(crate) fn model_transform(
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
