use crate::{
    OverlayError, OverlaySessionOptions, PreviewReport, ProductOverlayReport,
    validate_model_generation_advance,
};
use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_platform::{PlatformInputDiagnostics, PlatformInputError, WindowsInputService};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableId, ModelCommitErrorCode, ModelCommitFeedback,
    ModelCommitOutcome, ModelCommitToken, RenderConsumer, RenderFrame, RenderResources,
    RenderSnapshot, TextureAsset, TextureId,
};
use bongocat_runtime::{
    CursorProducer, HandSide, InputBindings, InputControl, InputEdge, InputEvent, InputProducer,
    InputSource, MonotonicMillis, PhysicalKey, RuntimeClient, RuntimeCommand, RuntimeOwner,
    RuntimeRenderErrorCode, RuntimeState,
};
use image::ImageReader;
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::{size_of, size_of_val},
    path::Path,
    rc::Rc,
    sync::Arc,
    thread,
    thread::ThreadId,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_MORE_FILES, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT,
            WPARAM,
        },
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
                D3D_FEATURE_LEVEL_11_0, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile,
                ID3DBlob, ID3DInclude,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_INDEX_BUFFER, D3D11_BIND_RENDER_TARGET,
                D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC,
                D3D11_BLEND_DEST_COLOR, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
                D3D11_BLEND_OP_ADD, D3D11_BLEND_ZERO, D3D11_BUFFER_DESC,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CULL_NONE, D3D11_FILL_SOLID,
                D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC,
                D3D11_INPUT_PER_VERTEX_DATA, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC,
                D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
                D3D11CreateDevice, ID3D11BlendState, ID3D11Buffer, ID3D11ClassLinkage,
                ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
                ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
                ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32G32_FLOAT,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_PRESENT, DXGI_QUERY_VIDEO_MEMORY_INFO,
                DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIAdapter3, IDXGIDevice,
                IDXGIFactory2, IDXGISwapChain1,
            },
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessHandleCount},
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE,
                GetWindowLongPtrW, HTTRANSPARENT, HWND_NOTOPMOST, HWND_TOPMOST, IsWindowVisible,
                MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE,
                SWP_NOACTIVATE, SWP_NOMOVE, SetWindowPos, ShowWindow, TranslateMessage,
                UnregisterClassW, WM_CLOSE, WM_NCHITTEST, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{Error, HRESULT, Interface, PCSTR, Result as WindowsResult, s, w},
};

const WINDOW_WIDTH: u32 = 420;
const WINDOW_HEIGHT: u32 = 520;
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);
const WINDOW_CLASS: windows::core::PCWSTR = w!("BongoCatProductOverlayWindow");
const PRESET_MODEL_IDS: [&str; 3] = ["standard", "keyboard", "gamepad"];
const HANDLE_GROWTH_LIMIT: u32 = 4;
const SWITCH_WARMUP_CYCLES: u64 = 2;
const THREAD_SETTLE_INTERVAL: Duration = Duration::from_millis(10);
const THREAD_SETTLE_SAMPLES: u32 = 25;
const THREAD_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

const SHADER_SOURCE: &str = r#"
    cbuffer UniformBuffer : register(b0) {
        float4 scale_offset;
        float4 multiply_color;
        float4 screen_color;
        float4 mask_settings;
        float opacity;
        float3 padding;
    };

    struct VertexInput {
        float2 position : POSITION;
        float2 uv : TEXCOORD;
    };

    struct RasterVertex {
        float4 position : SV_POSITION;
        float2 uv : TEXCOORD;
    };

    RasterVertex cubism_vertex(VertexInput input) {
        RasterVertex output;
        float2 clip = input.position * scale_offset.xy + scale_offset.zw;
        output.position = float4(clip, 0.0, 1.0);
        output.uv = float2(input.uv.x, 1.0 - input.uv.y);
        return output;
    }

    Texture2D<float4> model_texture : register(t0);
    Texture2D<float4> mask_texture : register(t1);
    SamplerState texture_sampler : register(s0);

    float4 cubism_fragment(RasterVertex input) : SV_TARGET {
        float4 texture_color = model_texture.Sample(texture_sampler, input.uv);
        float3 color = texture_color.rgb * multiply_color.rgb;
        color = color + screen_color.rgb - color * screen_color.rgb;
        float mask = 1.0;
        if (mask_settings.z > 0.5) {
            float2 mask_uv = input.position.xy / mask_settings.xy;
            mask = mask_texture.Sample(texture_sampler, mask_uv).a;
            if (mask_settings.w > 0.5) {
                mask = 1.0 - mask;
            }
        }
        float alpha = texture_color.a * opacity * mask;
        return float4(color * alpha, alpha);
    }

    float4 cubism_mask_fragment(RasterVertex input) : SV_TARGET {
        float alpha = model_texture.Sample(texture_sampler, input.uv).a;
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
    vertex_buffer: ID3D11Buffer,
    vertex_bytes: usize,
    index_buffer: ID3D11Buffer,
    index_bytes: usize,
    index_count: u32,
    texture_id: TextureId,
    opacity: f32,
    blend_mode: BlendMode,
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
    masks: Vec<DrawableId>,
    visible: bool,
    inverted_mask: bool,
    mask_target: Option<MaskTarget>,
}

struct MaskTarget {
    _texture: ID3D11Texture2D,
    render_target: ID3D11RenderTargetView,
    shader_resource: ID3D11ShaderResourceView,
}

struct TextureResource {
    _texture: ID3D11Texture2D,
    shader_resource: ID3D11ShaderResourceView,
}

struct GpuModel {
    textures: BTreeMap<TextureId, TextureResource>,
    meshes: Vec<Mesh>,
    empty_mask: TextureResource,
    canvas: CanvasInfo,
    masked_drawable_count: usize,
}

struct Pipelines {
    vertex_shader: ID3D11VertexShader,
    fragment_shader: ID3D11PixelShader,
    mask_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    constant_buffer: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    normal_blend: ID3D11BlendState,
    additive_blend: ID3D11BlendState,
    multiplicative_blend: ID3D11BlendState,
    mask_blend: ID3D11BlendState,
}

impl Pipelines {
    fn blend(&self, mode: BlendMode) -> &ID3D11BlendState {
        match mode {
            BlendMode::Normal => &self.normal_blend,
            BlendMode::Additive => &self.additive_blend,
            BlendMode::Multiplicative => &self.multiplicative_blend,
        }
    }
}

struct ComApartment {
    owner_thread: ThreadId,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl ComApartment {
    fn initialize() -> Result<Self, OverlayError> {
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

struct OverlayWindow {
    hwnd: HWND,
    instance: HINSTANCE,
    owner_thread: ThreadId,
    width: u32,
    height: u32,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl OverlayWindow {
    fn create(options: OverlaySessionOptions) -> Result<Self, OverlayError> {
        // SAFETY: the class and HWND are created and subsequently used only on
        // the current UI thread. No borrowed Win32 pointers escape this owner.
        unsafe { Self::create_inner(options) }.map_err(windows_error("create Win32 overlay"))
    }

    unsafe fn create_inner(options: OverlaySessionOptions) -> WindowsResult<Self> {
        let module = unsafe { GetModuleHandleW(None)? };
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(Error::from_thread());
        }
        let mut extended = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP;
        if options.click_through {
            extended |= WS_EX_TRANSPARENT;
        }
        let scale = u32::from(options.scale_percent);
        let logical_width = WINDOW_WIDTH.saturating_mul(scale) / 100;
        let logical_height = WINDOW_HEIGHT.saturating_mul(scale) / 100;
        let hwnd = match unsafe {
            CreateWindowExW(
                extended,
                WINDOW_CLASS,
                w!("BongoCat"),
                WS_POPUP,
                80,
                80,
                logical_width as i32,
                logical_height as i32,
                None,
                None,
                Some(instance),
                None,
            )
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
                return Err(error);
            }
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            let _ = unsafe { DestroyWindow(hwnd) };
            let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
            return Err(invariant_error("GetDpiForWindow returned zero"));
        }
        let width = logical_to_physical(logical_width, dpi)?;
        let height = logical_to_physical(logical_height, dpi)?;
        unsafe {
            SetWindowPos(
                hwnd,
                if options.always_on_top {
                    Some(HWND_TOPMOST)
                } else {
                    Some(HWND_NOTOPMOST)
                },
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOMOVE | SWP_NOACTIVATE,
            )?;
        }
        Ok(Self {
            hwnd,
            instance,
            owner_thread: thread::current().id(),
            width,
            height,
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    fn show(&self) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND is live, owned, and accessed only on its creation
        // thread; showing without activation does not transfer ownership.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        if !unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            return Err(OverlayError::new("Win32 overlay did not become visible"));
        }
        Ok(())
    }

    fn visible(&self) -> bool {
        self.assert_owner_thread();
        // SAFETY: this is a read-only query on the live thread-confined HWND.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    fn assert_owner_thread(&self) {
        assert_eq!(self.owner_thread, thread::current().id());
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: Renderer has already dropped, so no composition target still
        // uses this HWND. Class unregistration follows window destruction.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(WINDOW_CLASS, Some(self.instance));
        }
    }
}

struct Renderer {
    visual: IDCompositionVisual,
    target: IDCompositionTarget,
    composition_device: IDCompositionDevice,
    render_target: ID3D11RenderTargetView,
    staging_texture: ID3D11Texture2D,
    back_buffer: ID3D11Texture2D,
    swap_chain: IDXGISwapChain1,
    memory_adapter: IDXGIAdapter3,
    pipelines: Pipelines,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    model_generation: u64,
    resources: Arc<RenderResources>,
    model: GpuModel,
    width: u32,
    height: u32,
    opacity: f32,
    owner_thread: ThreadId,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl Renderer {
    fn create(
        window: &OverlayWindow,
        frame: &RenderFrame,
        opacity_percent: u8,
    ) -> Result<Self, OverlayError> {
        // SAFETY: all interfaces and resources are created for one live HWND
        // and remain confined to the current ProductOverlaySession thread.
        unsafe { Self::create_inner(window, frame, opacity_percent) }
            .map_err(windows_error("create D3D11 renderer"))
    }

    unsafe fn create_inner(
        window: &OverlayWindow,
        frame: &RenderFrame,
        opacity_percent: u8,
    ) -> WindowsResult<Self> {
        let (device, context) = unsafe { create_d3d11_device()? };
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let memory_adapter: IDXGIAdapter3 = adapter.cast()?;
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let descriptor = DXGI_SWAP_CHAIN_DESC1 {
            Width: window.width,
            Height: window.height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
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
        unsafe {
            visual.SetContent(&swap_chain)?;
            target.SetRoot(&visual)?;
            composition_device.Commit()?;
        }
        let back_buffer: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
        let render_target = unsafe { create_render_target(&device, &back_buffer)? };
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
            target,
            composition_device,
            render_target,
            staging_texture,
            back_buffer,
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
            opacity: f32::from(opacity_percent) / 100.0,
            owner_thread: thread::current().id(),
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    fn sync_frame(&mut self, frame: &RenderFrame) -> Result<bool, OverlayError> {
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

    fn current_local_memory_usage(&self) -> Result<u64, OverlayError> {
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

    fn draw(&self, verify: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: every interface belongs to this renderer and current thread;
        // all bound buffers/textures outlive the synchronous immediate context.
        unsafe { self.draw_inner(verify) }.map_err(windows_error("draw D3D11 model"))
    }

    unsafe fn draw_inner(&self, verify: bool) -> WindowsResult<()> {
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
        let scale_offset =
            model_transform(self.model.canvas, self.width as f32, self.height as f32);
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
                    opacity: 1.0,
                    padding: [0.0; 3],
                };
                unsafe {
                    self.bind_mesh(source, &uniforms, &self.model.empty_mask.shader_resource)?
                };
            }
        }
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(self.render_target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            self.context
                .ClearRenderTargetView(&self.render_target, &[0.0; 4]);
            self.context
                .PSSetShader(&self.pipelines.fragment_shader, None);
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
                opacity: mesh.opacity * self.opacity,
                padding: [0.0; 3],
            };
            unsafe {
                self.context
                    .OMSetBlendState(self.pipelines.blend(mesh.blend_mode), None, u32::MAX);
                self.bind_mesh(mesh, &uniforms, mask)?;
            }
        }
        if verify {
            unsafe {
                self.context
                    .CopyResource(&self.staging_texture, &self.back_buffer);
                verify_non_empty_frame(
                    &self.context,
                    &self.staging_texture,
                    self.width,
                    self.height,
                )?;
            }
        }
        unsafe {
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.device.GetDeviceRemovedReason()?;
        }
        Ok(())
    }

    unsafe fn bind_mesh(
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

    fn assert_owner_thread(&self) {
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

struct NativeOverlay {
    renderer: Renderer,
    window: OverlayWindow,
}

impl NativeOverlay {
    fn create(frame: &RenderFrame, options: OverlaySessionOptions) -> Result<Self, OverlayError> {
        validate_options(options)?;
        let window = OverlayWindow::create(options)?;
        let renderer = Renderer::create(&window, frame, options.opacity_percent)?;
        Ok(Self { renderer, window })
    }
}

pub(super) struct ProductOverlaySession {
    overlay: NativeOverlay,
    runtime_client: RuntimeClient,
    render_consumer: RenderConsumer,
    _com_apartment: ComApartment,
    input_service: Option<WindowsInputService>,
    input_start_error: Option<PlatformInputError>,
    input_diagnostics: Option<PlatformInputDiagnostics>,
    input_stopped: bool,
    frame_interval: Duration,
    frames_presented: u64,
    dynamic_snapshots: u64,
    model_commit_rejections: u64,
    previous_snapshot: Arc<RenderSnapshot>,
}

impl ProductOverlaySession {
    pub(super) fn start(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
    ) -> Result<Self, OverlayError> {
        validate_options(options)?;
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime did not publish an initial render frame"))?;
        let token = initial_frame
            .model_commit
            .ok_or_else(|| OverlayError::new("initial render frame has no model commit token"))?;
        let com_apartment = ComApartment::initialize()?;
        let overlay = match NativeOverlay::create(&initial_frame, options) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
        };
        report_model_commit(
            &runtime_client,
            &render_consumer,
            token,
            ModelCommitOutcome::Prepared,
        )?;
        overlay.window.show()?;
        let (input_service, input_start_error) =
            match WindowsInputService::start(input_producer, cursor_producer) {
                Ok(service) => (Some(service), None),
                Err(error) => (None, Some(error)),
            };
        Ok(Self {
            overlay,
            runtime_client,
            render_consumer,
            _com_apartment: com_apartment,
            input_service,
            input_start_error,
            input_diagnostics: None,
            input_stopped: false,
            frame_interval: Duration::from_secs_f64(1.0 / f64::from(options.maximum_fps)),
            frames_presented: 0,
            dynamic_snapshots: 0,
            model_commit_rejections: 0,
            previous_snapshot: initial_frame.snapshot,
        })
    }

    pub(super) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while self.overlay.window.visible() && (duration.is_zero() || started.elapsed() < duration)
        {
            pump_window_messages();
            let runtime_snapshot = self.runtime_client.snapshot();
            if runtime_snapshot.state == RuntimeState::Stopped {
                return Err(OverlayError::new(
                    "runtime stopped while the product overlay was active",
                ));
            }
            if !runtime_snapshot.overlay_visible {
                break;
            }
            let mut model_switched = false;
            if let Some(frame) = self.render_consumer.take_latest() {
                match self.overlay.renderer.sync_frame(&frame) {
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
                        model_switched = switched;
                        self.previous_snapshot = frame.snapshot;
                    }
                    Err(error) if frame.model_commit.is_some() => {
                        reject_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            frame.model_commit.expect("checked model commit token"),
                        )?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                    }
                    Err(error) => return Err(error),
                }
            }
            self.overlay
                .renderer
                .draw(self.frames_presented == 0 || model_switched)?;
            self.frames_presented = self.frames_presented.saturating_add(1);
            next_frame += self.frame_interval;
            if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            } else {
                next_frame = Instant::now();
            }
        }
        Ok(())
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
            model_generation: self.overlay.renderer.model_generation,
            drawable_count: self.overlay.renderer.model.meshes.len(),
            masked_drawable_count: self.overlay.renderer.model.masked_drawable_count,
            texture_count: self.overlay.renderer.model.textures.len(),
        })
    }
}

pub(crate) fn run_model_switch_preview(
    model_id: &str,
    model_root: &Path,
    switch_cycles: u32,
) -> Result<PreviewReport, OverlayError> {
    if switch_cycles == 0 {
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
    let models = PRESET_MODEL_IDS
        .iter()
        .map(|id| {
            let id = ModelId::parse(*id).map_err(|error| OverlayError::new(error.to_string()))?;
            catalog
                .load(&id)
                .map(Arc::new)
                .map_err(|error| OverlayError::new(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut current_model_index = PRESET_MODEL_IDS
        .iter()
        .position(|id| *id == model_id.as_str())
        .ok_or_else(|| OverlayError::new("initial preset is not in the switch sequence"))?;

    let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
    let runtime_client = runtime.client();
    runtime_client
        .wait_for_revision(1, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview runtime did not become ready"))?;
    let input_producer = runtime.input_producer();
    let (initial_token, initial_frame) = prepare_switch_frame(
        &runtime_client,
        &render_consumer,
        Arc::clone(&models[current_model_index]),
        Arc::new(preview_input_bindings(model_id.as_str())),
    )?;

    let com_apartment = ComApartment::initialize()?;
    let mut overlay = match NativeOverlay::create(&initial_frame, OverlaySessionOptions::default())
    {
        Ok(overlay) => overlay,
        Err(error) => {
            reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
            return Err(error);
        }
    };
    overlay.window.show()?;
    overlay.renderer.draw(true)?;
    report_model_commit(
        &runtime_client,
        &render_consumer,
        initial_token,
        ModelCommitOutcome::Prepared,
    )?;

    let mut frames_presented = 1_u64;
    let mut dynamic_snapshots = 0_u64;
    let mut previous_snapshot = Arc::clone(&initial_frame.snapshot);
    let initial_generation = overlay.renderer.model_generation;

    let pressed_sequence = publish_preview_key_edge(&input_producer, InputEdge::Down, 1)?;
    let pressed = runtime_client
        .wait_for_input_sequence(pressed_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview key press did not reach the runtime"))?;
    if !pressed.model_input.left_hand_down || pressed.model_input.right_hand_down {
        return Err(OverlayError::new(
            "initial model bindings did not map the probe key to the left hand",
        ));
    }

    let rejected_model_index = (current_model_index + 1) % models.len();
    let rejected_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
        PhysicalKey::KEY_A,
        HandSide::Right,
    )])));
    let (rejected_token, rejected_frame) = prepare_switch_frame(
        &runtime_client,
        &render_consumer,
        Arc::clone(&models[rejected_model_index]),
        rejected_bindings,
    )?;
    let mut invalid_resources = rejected_frame.resources.as_ref().clone();
    let Some(first_texture) = invalid_resources.textures.first_mut() else {
        return Err(OverlayError::new(
            "model-switch probe requires at least one texture",
        ));
    };
    first_texture.path = model_root.join(".missing-d3d11-prepare-texture.png");
    let invalid_frame = RenderFrame {
        resources: Arc::new(invalid_resources),
        ..rejected_frame
    };
    if overlay.renderer.sync_frame(&invalid_frame).is_ok() {
        return Err(OverlayError::new(
            "invalid D3D11 model preparation unexpectedly succeeded",
        ));
    }
    if overlay.renderer.model_generation != initial_generation {
        return Err(OverlayError::new(
            "failed D3D11 preparation replaced the active GPU generation",
        ));
    }
    reject_model_commit(&runtime_client, &render_consumer, rejected_token)?;
    let rejected = runtime_client.snapshot();
    if rejected.pending_model.is_some()
        || rejected
            .active_model
            .as_ref()
            .is_none_or(|active| active.id != model_id)
        || !rejected.model_input.left_hand_down
        || rejected.model_input.right_hand_down
    {
        return Err(OverlayError::new(
            "GPU rejection did not preserve the active CPU model and input bindings",
        ));
    }
    overlay.renderer.draw(true)?;
    frames_presented = frames_presented.saturating_add(1);

    let released_sequence = publish_preview_key_edge(&input_producer, InputEdge::Up, 2)?;
    let released = runtime_client
        .wait_for_input_sequence(released_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview key release did not reach the runtime"))?;
    if released.model_input.left_hand_down || released.model_input.right_hand_down {
        return Err(OverlayError::new(
            "preview key release left a pressed hand after GPU rejection",
        ));
    }

    // Initialize the DXGI memory-query path before sampling the warmup thread
    // high-water mark. Some drivers create a helper thread on the first query.
    let _ = overlay.renderer.current_local_memory_usage()?;
    let switches_per_cycle = models.len() as u64;
    let warmup_cycles = SWITCH_WARMUP_CYCLES.max(u64::from(switch_cycles));
    let warmup_switches = warmup_cycles.saturating_mul(switches_per_cycle);
    let target_switches = u64::from(switch_cycles).saturating_mul(switches_per_cycle);
    let total_target_switches = warmup_switches.saturating_add(target_switches);
    let mut total_switches = 0_u64;
    let mut model_switches = 0_u64;
    let mut gpu_bytes_before = None;
    let mut handles_before = None;
    let mut warmup_thread_high_water = 0_u32;
    while total_switches < total_target_switches {
        pump_window_messages();
        current_model_index = (current_model_index + 1) % models.len();
        let target_id = PRESET_MODEL_IDS[current_model_index];
        let generation_before = overlay.renderer.model_generation;
        let (token, frame) = prepare_switch_frame(
            &runtime_client,
            &render_consumer,
            Arc::clone(&models[current_model_index]),
            Arc::new(preview_input_bindings(target_id)),
        )?;
        if frame.snapshot.as_ref() != previous_snapshot.as_ref() {
            dynamic_snapshots = dynamic_snapshots.saturating_add(1);
        }
        if !overlay.renderer.sync_frame(&frame)? {
            return Err(OverlayError::new(
                "D3D11 renderer did not replace a newer model generation",
            ));
        }
        if overlay.renderer.model_generation <= generation_before {
            return Err(OverlayError::new(
                "D3D11 renderer committed a non-monotonic model generation",
            ));
        }
        overlay.renderer.draw(true)?;
        report_model_commit(
            &runtime_client,
            &render_consumer,
            token,
            ModelCommitOutcome::Prepared,
        )?;
        let committed = runtime_client.snapshot();
        if committed.pending_model.is_some()
            || committed
                .active_model
                .as_ref()
                .is_none_or(|active| active.id.as_str() != target_id)
        {
            return Err(OverlayError::new(
                "runtime and D3D11 renderer did not commit the same model",
            ));
        }
        previous_snapshot = frame.snapshot;
        frames_presented = frames_presented.saturating_add(1);
        total_switches = total_switches.saturating_add(1);

        if total_switches <= warmup_switches {
            warmup_thread_high_water = warmup_thread_high_water.max(
                process_thread_count().map_err(windows_error("count warmup process threads"))?,
            );
        } else {
            model_switches = model_switches.saturating_add(1);
        }
        if total_switches == warmup_switches {
            gpu_bytes_before = Some(overlay.renderer.current_local_memory_usage()?);
            handles_before =
                Some(process_handle_count().map_err(windows_error("count process handles"))?);
            warmup_thread_high_water =
                settle_process_thread_high_water(warmup_thread_high_water, THREAD_SETTLE_TIMEOUT)?;
        }
    }

    let gpu_bytes_before = gpu_bytes_before
        .ok_or_else(|| OverlayError::new("model-switch probe did not finish its warmup cycle"))?;
    if warmup_thread_high_water == 0 {
        return Err(OverlayError::new(
            "model-switch probe did not sample thread usage",
        ));
    }
    let handles_before = handles_before
        .ok_or_else(|| OverlayError::new("model-switch probe did not sample handle usage"))?;
    let gpu_bytes_after = overlay.renderer.current_local_memory_usage()?;
    let threads_after =
        process_thread_count().map_err(windows_error("count process threads after switching"))?;
    let handles_after =
        process_handle_count().map_err(windows_error("count process handles after switching"))?;
    if gpu_bytes_after > gpu_bytes_before {
        return Err(OverlayError::new(format!(
            "DXGI local memory usage grew from {gpu_bytes_before} to {gpu_bytes_after} bytes during model switching"
        )));
    }
    if threads_after > warmup_thread_high_water {
        return Err(OverlayError::new(format!(
            "process thread count exceeded the warmup high-water mark {warmup_thread_high_water} with {threads_after} threads during model switching"
        )));
    }
    if handles_after > handles_before.saturating_add(HANDLE_GROWTH_LIMIT) {
        return Err(OverlayError::new(format!(
            "process handle count grew from {handles_before} to {handles_after} during model switching"
        )));
    }

    let final_snapshot = runtime_client.snapshot();
    if final_snapshot
        .active_model
        .as_ref()
        .is_none_or(|active| active.id != model_id)
    {
        return Err(OverlayError::new(
            "complete model-switch cycles did not return to the initial model",
        ));
    }
    let drawable_count = overlay.renderer.model.meshes.len();
    let masked_drawable_count = overlay.renderer.model.masked_drawable_count;
    let texture_count = overlay.renderer.model.textures.len();
    let stopped = runtime
        .shutdown(RUNTIME_TIMEOUT)
        .map_err(|error| OverlayError::new(error.to_string()))?;
    while render_consumer.take_latest().is_some() {}
    let render_diagnostics = render_consumer.diagnostics();
    drop(overlay);
    drop(com_apartment);

    Ok(PreviewReport {
        frames_presented,
        dynamic_snapshots,
        runtime_input_events: stopped.input.transport.enqueued,
        platform_input_edges: 0,
        runtime_cursor_published: stopped.cursor.transport.published,
        runtime_cursor_coalesced: stopped.cursor.transport.coalesced,
        runtime_cursor_consumed: stopped.cursor.transport.consumed,
        platform_cursor_samples: 0,
        render_frames_published: render_diagnostics.published,
        render_frames_coalesced: render_diagnostics.coalesced,
        render_frames_consumed: render_diagnostics.consumed,
        model_switches,
        failed_gpu_prepare_preserved: true,
        gpu_bytes_before,
        gpu_bytes_after,
        drawable_count,
        masked_drawable_count,
        texture_count,
    })
}

fn prepare_switch_frame(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    model: Arc<CommittedModel>,
    input_bindings: Arc<InputBindings>,
) -> Result<(ModelCommitToken, RenderFrame), OverlayError> {
    let sequence = runtime_client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model,
            input_bindings,
        })
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let prepared = runtime_client
        .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("model switch was not prepared"))?;
    if let Some(failure) = prepared
        .last_command_failure
        .filter(|failure| failure.sequence == sequence)
    {
        return Err(OverlayError::new(format!(
            "model switch failed before GPU preparation: {:?}",
            failure.code
        )));
    }
    let token = prepared
        .pending_model
        .as_ref()
        .filter(|pending| pending.token.command_sequence == sequence)
        .map(|pending| pending.token)
        .ok_or_else(|| OverlayError::new("runtime published the wrong pending model token"))?;
    let frame = render_consumer
        .take_latest()
        .filter(|frame| frame.model_commit == Some(token))
        .ok_or_else(|| OverlayError::new("runtime did not publish the matching model frame"))?;
    Ok((token, frame))
}

fn publish_preview_key_edge(
    producer: &InputProducer,
    edge: InputEdge,
    at_millis: u64,
) -> Result<u64, OverlayError> {
    producer
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge,
            source: InputSource::Capture,
            at: MonotonicMillis::new(at_millis),
        })
        .map_err(|error| OverlayError::new(error.to_string()))
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
    InputBindings::new(key_hands)
}

impl GpuModel {
    unsafe fn prepare(
        device: &ID3D11Device,
        resources: &RenderResources,
        snapshot: &RenderSnapshot,
        width: u32,
        height: u32,
    ) -> WindowsResult<Self> {
        let textures = resources
            .textures
            .iter()
            .map(|asset| unsafe { load_texture(device, asset) }.map(|texture| (asset.id, texture)))
            .collect::<WindowsResult<BTreeMap<_, _>>>()?;
        if textures.len() != resources.textures.len() {
            return Err(invariant_error("texture resource ids are not unique"));
        }
        let ids = snapshot
            .drawables
            .iter()
            .map(|drawable| drawable.id)
            .collect::<BTreeSet<_>>();
        if ids.len() != snapshot.drawables.len() {
            return Err(invariant_error("drawable resource ids are not unique"));
        }
        let mut meshes = Vec::with_capacity(snapshot.drawables.len());
        for drawable in &snapshot.drawables {
            if !textures.contains_key(&drawable.texture_id) {
                return Err(invariant_error("drawable references a missing texture"));
            }
            if drawable.masks.iter().any(|mask| !ids.contains(mask)) {
                return Err(invariant_error("drawable references a missing mask source"));
            }
            if drawable.vertices.is_empty() || drawable.indices.is_empty() {
                return Err(invariant_error("drawable geometry is empty"));
            }
            let vertex_buffer = unsafe {
                create_buffer(
                    device,
                    &drawable.vertices,
                    D3D11_BIND_VERTEX_BUFFER.0 as u32,
                )?
            };
            let index_buffer = unsafe {
                create_buffer(device, &drawable.indices, D3D11_BIND_INDEX_BUFFER.0 as u32)?
            };
            meshes.push(Mesh {
                id: drawable.id,
                render_order: drawable.render_order,
                vertex_buffer,
                vertex_bytes: size_of_val(drawable.vertices.as_slice()),
                index_buffer,
                index_bytes: size_of_val(drawable.indices.as_slice()),
                index_count: drawable.indices.len() as u32,
                texture_id: drawable.texture_id,
                opacity: drawable.opacity,
                blend_mode: drawable.blend_mode,
                multiply_color: drawable.multiply_color,
                screen_color: drawable.screen_color,
                masks: drawable.masks.clone(),
                visible: drawable.visible,
                inverted_mask: drawable.inverted_mask,
                mask_target: if drawable.masks.is_empty() {
                    None
                } else {
                    Some(unsafe { create_mask_target(device, width, height)? })
                },
            });
        }
        meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        Ok(Self {
            textures,
            meshes,
            empty_mask: unsafe { create_empty_mask(device)? },
            canvas: snapshot.canvas,
            masked_drawable_count: snapshot
                .drawables
                .iter()
                .filter(|drawable| !drawable.masks.is_empty())
                .count(),
        })
    }

    unsafe fn sync_snapshot(
        &mut self,
        context: &ID3D11DeviceContext,
        snapshot: &RenderSnapshot,
    ) -> WindowsResult<()> {
        if snapshot.drawables.len() != self.meshes.len() {
            return Err(invariant_error(
                "drawable count changed within a generation",
            ));
        }
        for drawable in &snapshot.drawables {
            let mesh = self
                .meshes
                .iter_mut()
                .find(|mesh| mesh.id == drawable.id)
                .ok_or_else(|| invariant_error("drawable source is unavailable"))?;
            if mesh.vertex_bytes != size_of_val(drawable.vertices.as_slice())
                || mesh.index_bytes != size_of_val(drawable.indices.as_slice())
            {
                return Err(invariant_error("drawable buffer size changed"));
            }
            if mesh.mask_target.is_some() != !drawable.masks.is_empty() {
                return Err(invariant_error("drawable clipping topology changed"));
            }
            unsafe {
                context.UpdateSubresource(
                    &mesh.vertex_buffer,
                    0,
                    None,
                    drawable.vertices.as_ptr().cast(),
                    0,
                    0,
                );
                context.UpdateSubresource(
                    &mesh.index_buffer,
                    0,
                    None,
                    drawable.indices.as_ptr().cast(),
                    0,
                    0,
                );
            }
            mesh.render_order = drawable.render_order;
            mesh.index_count = drawable.indices.len() as u32;
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
        self.canvas = snapshot.canvas;
        Ok(())
    }
}

fn validate_options(options: OverlaySessionOptions) -> Result<(), OverlayError> {
    if !(25..=400).contains(&options.scale_percent) {
        return Err(OverlayError::new(
            "overlay scale must be between 25 and 400 percent",
        ));
    }
    if options.opacity_percent == 0 || options.opacity_percent > 100 {
        return Err(OverlayError::new(
            "overlay opacity must be between 1 and 100 percent",
        ));
    }
    if !(1..=240).contains(&options.maximum_fps) {
        return Err(OverlayError::new("maximum FPS must be between 1 and 240"));
    }
    Ok(())
}

fn pump_window_messages() {
    let mut message = MSG::default();
    // SAFETY: message storage is valid for each synchronous call and dispatch
    // remains on the HWND owner thread.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn process_handle_count() -> WindowsResult<u32> {
    let mut count = 0;
    // SAFETY: GetCurrentProcess returns a process pseudo-handle and count is
    // writable for the complete synchronous query.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count)? };
    Ok(count)
}

fn process_thread_count() -> WindowsResult<u32> {
    // SAFETY: the returned snapshot handle is immediately wrapped and closed
    // by Drop after enumeration; THREADENTRY32 carries the required size.
    let snapshot = ThreadSnapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? });
    let process_id = unsafe { GetCurrentProcessId() };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    unsafe { Thread32First(snapshot.0, &mut entry)? };
    let mut count = 0_u32;
    loop {
        if entry.th32OwnerProcessID == process_id {
            count = count.saturating_add(1);
        }
        match unsafe { Thread32Next(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(error) => return Err(error),
        }
    }
    if count == 0 {
        return Err(invariant_error(
            "thread snapshot did not contain the current process",
        ));
    }
    Ok(count)
}

fn settle_process_thread_high_water(
    mut high_water: u32,
    timeout: Duration,
) -> Result<u32, OverlayError> {
    let deadline = Instant::now() + timeout;
    let mut stable_samples = 0_u32;
    while Instant::now() < deadline {
        pump_window_messages();
        thread::sleep(THREAD_SETTLE_INTERVAL);
        let current = process_thread_count()
            .map_err(windows_error("count process threads while settling warmup"))?;
        if current > high_water {
            high_water = current;
            stable_samples = 0;
        } else {
            stable_samples = stable_samples.saturating_add(1);
            if stable_samples >= THREAD_SETTLE_SAMPLES {
                return Ok(high_water);
            }
        }
    }
    Err(OverlayError::new(format!(
        "process thread count did not stabilize below high-water mark {high_water} after warmup"
    )))
}

struct ThreadSnapshot(HANDLE);

impl Drop for ThreadSnapshot {
    fn drop(&mut self) {
        // SAFETY: this owner contains one successful ToolHelp snapshot handle
        // and Drop runs once after enumeration has stopped.
        let _ = unsafe { CloseHandle(self.0) };
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

unsafe fn create_d3d11_device() -> WindowsResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut last_error = None;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        match unsafe { try_create_d3d11_device(driver) } {
            Ok(result) => return Ok(result),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(Error::from_thread))
}

unsafe fn try_create_d3d11_device(
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

unsafe fn create_pipelines(device: &ID3D11Device) -> WindowsResult<Pipelines> {
    let vertex_blob = unsafe { compile_shader(s!("cubism_vertex"), s!("vs_5_0"))? };
    let fragment_blob = unsafe { compile_shader(s!("cubism_fragment"), s!("ps_5_0"))? };
    let mask_blob = unsafe { compile_shader(s!("cubism_mask_fragment"), s!("ps_5_0"))? };
    let vertex_bytes = unsafe { blob_bytes(&vertex_blob) };
    let fragment_bytes = unsafe { blob_bytes(&fragment_blob) };
    let mask_bytes = unsafe { blob_bytes(&mask_blob) };
    let mut vertex_shader = None;
    let mut fragment_shader = None;
    let mut mask_shader = None;
    unsafe {
        device.CreateVertexShader(
            vertex_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut vertex_shader),
        )?;
        device.CreatePixelShader(
            fragment_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut fragment_shader),
        )?;
        device.CreatePixelShader(
            mask_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut mask_shader),
        )?;
    }
    let elements = [
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 8,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];
    let mut input_layout = None;
    unsafe { device.CreateInputLayout(&elements, vertex_bytes, Some(&mut input_layout))? };
    let constant_desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<Uniforms>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut constant_buffer = None;
    unsafe { device.CreateBuffer(&constant_desc, None, Some(&mut constant_buffer))? };
    let sampler_desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxLOD: f32::MAX,
        ..Default::default()
    };
    let mut sampler = None;
    unsafe { device.CreateSamplerState(&sampler_desc, Some(&mut sampler))? };
    let rasterizer_desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        ..Default::default()
    };
    let mut rasterizer = None;
    unsafe { device.CreateRasterizerState(&rasterizer_desc, Some(&mut rasterizer))? };
    Ok(Pipelines {
        vertex_shader: required(vertex_shader, "vertex shader")?,
        fragment_shader: required(fragment_shader, "fragment shader")?,
        mask_shader: required(mask_shader, "mask shader")?,
        input_layout: required(input_layout, "input layout")?,
        constant_buffer: required(constant_buffer, "constant buffer")?,
        sampler: required(sampler, "sampler")?,
        rasterizer: required(rasterizer, "rasterizer")?,
        normal_blend: unsafe {
            create_blend_state(
                device,
                D3D11_BLEND_ONE,
                D3D11_BLEND_INV_SRC_ALPHA,
                D3D11_BLEND_ONE,
                D3D11_BLEND_INV_SRC_ALPHA,
            )?
        },
        additive_blend: unsafe {
            create_blend_state(
                device,
                D3D11_BLEND_ONE,
                D3D11_BLEND_ONE,
                D3D11_BLEND_ZERO,
                D3D11_BLEND_ONE,
            )?
        },
        multiplicative_blend: unsafe {
            create_blend_state(
                device,
                D3D11_BLEND_DEST_COLOR,
                D3D11_BLEND_INV_SRC_ALPHA,
                D3D11_BLEND_ZERO,
                D3D11_BLEND_ONE,
            )?
        },
        mask_blend: unsafe {
            create_blend_state(
                device,
                D3D11_BLEND_ONE,
                D3D11_BLEND_INV_SRC_ALPHA,
                D3D11_BLEND_ONE,
                D3D11_BLEND_INV_SRC_ALPHA,
            )?
        },
    })
}

unsafe fn create_blend_state(
    device: &ID3D11Device,
    source_rgb: windows::Win32::Graphics::Direct3D11::D3D11_BLEND,
    destination_rgb: windows::Win32::Graphics::Direct3D11::D3D11_BLEND,
    source_alpha: windows::Win32::Graphics::Direct3D11::D3D11_BLEND,
    destination_alpha: windows::Win32::Graphics::Direct3D11::D3D11_BLEND,
) -> WindowsResult<ID3D11BlendState> {
    let target = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: source_rgb,
        DestBlend: destination_rgb,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: source_alpha,
        DestBlendAlpha: destination_alpha,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut descriptor = D3D11_BLEND_DESC::default();
    descriptor.RenderTarget[0] = target;
    let mut state = None;
    unsafe { device.CreateBlendState(&descriptor, Some(&mut state))? };
    required(state, "blend state")
}

unsafe fn create_buffer<T>(
    device: &ID3D11Device,
    values: &[T],
    bind_flags: u32,
) -> WindowsResult<ID3D11Buffer> {
    let byte_width = u32::try_from(size_of_val(values))
        .map_err(|_| invariant_error("GPU buffer exceeds D3D11 size limits"))?;
    if byte_width == 0 {
        return Err(invariant_error("GPU buffer cannot be empty"));
    }
    let descriptor = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind_flags,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: values.as_ptr().cast(),
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&descriptor, Some(&data), Some(&mut buffer))? };
    required(buffer, "GPU buffer")
}

unsafe fn load_texture(
    device: &ID3D11Device,
    asset: &TextureAsset,
) -> WindowsResult<TextureResource> {
    let image = ImageReader::open(&asset.path)
        .map_err(|error| Error::new(HRESULT(0x80004005_u32 as i32), error.to_string()))?
        .decode()
        .map_err(|error| Error::new(HRESULT(0x80004005_u32 as i32), error.to_string()))?
        .into_rgba8();
    if image.width() != asset.width || image.height() != asset.height {
        return Err(invariant_error(
            "texture dimensions changed after validation",
        ));
    }
    unsafe {
        create_texture_resource(
            device,
            asset.width,
            asset.height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            image.as_ptr(),
            asset.width.saturating_mul(4),
        )
    }
}

unsafe fn create_empty_mask(device: &ID3D11Device) -> WindowsResult<TextureResource> {
    let pixel = [0_u8; 4];
    unsafe { create_texture_resource(device, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, pixel.as_ptr(), 4) }
}

unsafe fn create_texture_resource(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    bytes: *const u8,
    row_pitch: u32,
) -> WindowsResult<TextureResource> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.cast(),
        SysMemPitch: row_pitch,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&descriptor, Some(&data), Some(&mut texture))? };
    let texture = required(texture, "texture")?;
    let mut shader_resource = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource))? };
    Ok(TextureResource {
        _texture: texture,
        shader_resource: required(shader_resource, "texture shader resource")?,
    })
}

unsafe fn create_mask_target(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> WindowsResult<MaskTarget> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut texture))? };
    let texture = required(texture, "mask texture")?;
    let render_target = unsafe { create_render_target(device, &texture)? };
    let mut shader_resource = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource))? };
    Ok(MaskTarget {
        _texture: texture,
        render_target,
        shader_resource: required(shader_resource, "mask shader resource")?,
    })
}

unsafe fn create_render_target(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> WindowsResult<ID3D11RenderTargetView> {
    let mut target = None;
    unsafe { device.CreateRenderTargetView(texture, None, Some(&mut target))? };
    required(target, "render target")
}

unsafe fn create_staging_texture(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
) -> WindowsResult<ID3D11Texture2D> {
    let mut descriptor = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut descriptor) };
    descriptor.Usage = D3D11_USAGE_STAGING;
    descriptor.BindFlags = 0;
    descriptor.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    descriptor.MiscFlags = 0;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut staging))? };
    required(staging, "staging texture")
}

unsafe fn verify_non_empty_frame(
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> WindowsResult<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let result = if mapped.pData.is_null() || mapped.RowPitch < width.saturating_mul(4) {
        Err(invariant_error("D3D11 readback mapping is invalid"))
    } else {
        let mut found = false;
        for y in 1..16_usize {
            for x in 1..16_usize {
                let offset = (height as usize * y / 16) * mapped.RowPitch as usize
                    + (width as usize * x / 16) * 4;
                // SAFETY: grid coordinates are inside width/height and RowPitch
                // was checked to contain every four-byte BGRA pixel in a row.
                let alpha = unsafe { *mapped.pData.cast::<u8>().add(offset + 3) };
                found |= alpha != 0;
            }
        }
        if found {
            Ok(())
        } else {
            Err(invariant_error("D3D11 readback found no model pixels"))
        }
    };
    unsafe { context.Unmap(texture, 0) };
    result
}

unsafe fn compile_shader(entry: PCSTR, target: PCSTR) -> WindowsResult<ID3DBlob> {
    let mut code = None;
    let mut diagnostics = None;
    let result = unsafe {
        D3DCompile(
            SHADER_SOURCE.as_ptr().cast(),
            SHADER_SOURCE.len(),
            s!("BongoCatProductOverlay.hlsl"),
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut diagnostics),
        )
    };
    if let Err(error) = result {
        let message = diagnostics
            .as_ref()
            .map(|blob| unsafe { blob_message(blob) })
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| error.message());
        return Err(Error::new(error.code(), message));
    }
    required(code, "shader bytecode")
}

unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    let pointer = unsafe { blob.GetBufferPointer() }.cast::<u8>();
    let length = unsafe { blob.GetBufferSize() };
    // SAFETY: ID3DBlob owns a contiguous allocation for the returned length and
    // the slice is borrowed no longer than the live blob reference.
    unsafe { std::slice::from_raw_parts(pointer, length) }
}

unsafe fn blob_message(blob: &ID3DBlob) -> String {
    unsafe { String::from_utf8_lossy(blob_bytes(blob)) }
        .trim_end_matches(['\0', '\r', '\n'])
        .to_owned()
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

fn logical_to_physical(logical: u32, dpi: u32) -> WindowsResult<u32> {
    let physical = (u64::from(logical) * u64::from(dpi) + 48) / 96;
    if physical == 0 || physical > i32::MAX as u64 {
        return Err(invariant_error("overlay dimension exceeds Win32 limits"));
    }
    Ok(physical as u32)
}

fn required<T>(value: Option<T>, name: &str) -> WindowsResult<T> {
    value.ok_or_else(|| invariant_error(&format!("D3D11 returned no {name}")))
}

fn invariant_error(message: &str) -> Error {
    Error::new(HRESULT(0x80004005_u32 as i32), message)
}

fn windows_error(context: &'static str) -> impl FnOnce(Error) -> OverlayError {
    move |error| OverlayError::new(format!("{context}: {error}"))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            // SAFETY: the callback receives a live HWND from user32 and only
            // reads its current extended style on the dispatch thread.
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
            if style & WS_EX_TRANSPARENT.0 as isize != 0 {
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
        WM_CLOSE => {
            // SAFETY: WM_CLOSE is delivered to this owned top-level window and
            // destruction stays on the same UI thread.
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: unhandled messages are forwarded with the exact parameters
    // supplied by user32, as required by the window procedure contract.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
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
    fn gpu_structs_match_d3d11_layout() {
        assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
        assert_eq!(size_of::<Uniforms>(), 80);
        assert_eq!(size_of::<Uniforms>() % 16, 0);
    }

    #[test]
    fn product_options_reject_values_outside_renderer_boundaries() {
        for options in [
            OverlaySessionOptions {
                scale_percent: 24,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                opacity_percent: 0,
                ..OverlaySessionOptions::default()
            },
            OverlaySessionOptions {
                maximum_fps: 241,
                ..OverlaySessionOptions::default()
            },
        ] {
            assert!(validate_options(options).is_err());
        }
    }
}
