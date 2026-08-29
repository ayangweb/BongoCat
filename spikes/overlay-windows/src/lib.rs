#![cfg(target_os = "windows")]

use std::{rc::Rc, thread::ThreadId};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
                D3D_FEATURE_LEVEL_11_0, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile,
                ID3DBlob, ID3DInclude,
            },
            Direct3D11::{
                D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA,
                D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CULL_NONE, D3D11_FILL_SOLID,
                D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
                D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT, D3D11CreateDevice,
                ID3D11BlendState, ID3D11Buffer, ID3D11ClassLinkage, ID3D11DepthStencilView,
                ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader,
                ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_UNKNOWN,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter,
                IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
            },
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentProcess, GetProcessHandleCount},
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, IsWindowVisible,
                RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                SWP_NOZORDER, SetWindowPos, ShowWindow, UnregisterClassW, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
                WS_POPUP,
            },
        },
    },
    core::{Error, HRESULT, Interface, PCSTR, Result as WindowsResult, s, w},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const HANDLE_GROWTH_LIMIT: u32 = 4;
const SHADER_SOURCE: &str = r#"
    struct VertexInput {
        float2 position : POSITION;
        float4 color : COLOR;
    };

    struct PixelInput {
        float4 position : SV_POSITION;
        float4 color : COLOR;
    };

    PixelInput vertex_main(VertexInput input) {
        PixelInput output;
        output.position = float4(input.position, 0.0, 1.0);
        output.color = input.color;
        return output;
    }

    float4 pixel_main(PixelInput input) : SV_TARGET {
        return input.color;
    }
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct OverlayVertex {
    position: [f32; 2],
    premultiplied_color: [f32; 4],
}

const OVERLAY_VERTICES: [OverlayVertex; 3] = [
    OverlayVertex {
        position: [0.0, 0.72],
        premultiplied_color: [0.72, 0.25, 0.06, 0.78],
    },
    OverlayVertex {
        position: [-0.68, -0.62],
        premultiplied_color: [0.16, 0.58, 0.68, 0.78],
    },
    OverlayVertex {
        position: [0.68, -0.62],
        premultiplied_color: [0.48, 0.16, 0.64, 0.78],
    },
];

pub struct NativeOverlay {
    // Renderer must be released before the HWND it targets.
    renderer: Renderer,
    window: OverlayWindow,
}

impl NativeOverlay {
    pub fn create() -> Result<Self, String> {
        Self::create_with_logging(true)
    }

    fn create_with_logging(log_lifecycle: bool) -> Result<Self, String> {
        // SAFETY: creation and every later method are confined to the current
        // GPUI/Win32 UI thread. Owned COM interfaces and the HWND are wrapped
        // in RAII owners whose field order releases GPU state before the HWND.
        unsafe { Self::create_inner(log_lifecycle).map_err(format_windows_error) }
    }

    unsafe fn create_inner(log_lifecycle: bool) -> WindowsResult<Self> {
        let mut window = unsafe { OverlayWindow::create(log_lifecycle)? };
        let mut renderer = unsafe { Renderer::create(window.hwnd, log_lifecycle)? };
        let (physical_width, physical_height) = window.physical_dimensions(WIDTH, HEIGHT)?;
        if physical_width != WIDTH || physical_height != HEIGHT {
            renderer.resize(physical_width, physical_height)?;
            window.resize(physical_width, physical_height)?;
        }
        Ok(Self { renderer, window })
    }

    pub fn show(&self) -> Result<(), String> {
        self.window.show().map_err(format_windows_error)
    }

    pub fn hide(&self) -> Result<(), String> {
        self.window.hide().map_err(format_windows_error)
    }

    pub fn clear_present(&self) -> Result<(), String> {
        self.renderer
            .clear_present(true)
            .map_err(format_windows_error)
    }

    pub fn render_scheduled_frame(&self) -> Result<(), String> {
        self.renderer
            .clear_present(false)
            .map_err(format_windows_error)
    }

    pub fn resize(&mut self, logical_width: u32, logical_height: u32) -> Result<(), String> {
        let (physical_width, physical_height) = self
            .window
            .physical_dimensions(logical_width, logical_height)
            .map_err(format_windows_error)?;
        self.renderer
            .resize(physical_width, physical_height)
            .and_then(|()| self.window.resize(physical_width, physical_height))
            .map_err(format_windows_error)?;
        if self.window.log_lifecycle {
            println!(
                "gpui-overlay-spike: Windows logical resize width={logical_width} height={logical_height} physical_width={physical_width} physical_height={physical_height} dpi={}",
                self.window.dpi
            );
        }
        Ok(())
    }

    pub fn driver_name(&self) -> &'static str {
        self.renderer.driver_name
    }

    pub fn dpi(&self) -> u32 {
        self.window.dpi
    }
}

pub struct CycleReport {
    pub cycles: u32,
    pub non_empty_frames: u32,
    pub handles_before: u32,
    pub handles_after: u32,
}

pub fn run_creation_cycles(cycles: u32) -> Result<CycleReport, String> {
    if cycles == 0 {
        return Err("overlay cycle count must be greater than zero".into());
    }
    let _com_apartment = ComApartment::initialize().map_err(format_windows_error)?;

    // Warm up process-global D3D/DXGI state before measuring owned resources.
    {
        let overlay = NativeOverlay::create_with_logging(false)?;
        overlay.show()?;
        overlay.clear_present()?;
    }
    let handles_before = process_handle_count().map_err(format_windows_error)?;

    for _ in 0..cycles {
        let overlay = NativeOverlay::create_with_logging(false)?;
        overlay.show()?;
        overlay.clear_present()?;
        overlay.hide()?;
    }

    let handles_after = process_handle_count().map_err(format_windows_error)?;
    if handles_after > handles_before.saturating_add(HANDLE_GROWTH_LIMIT) {
        return Err(format!(
            "process handle count grew from {handles_before} to {handles_after} after {cycles} cycles"
        ));
    }
    Ok(CycleReport {
        cycles,
        non_empty_frames: cycles,
        handles_before,
        handles_after,
    })
}

struct ComApartment {
    owner_thread: ThreadId,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl ComApartment {
    fn initialize() -> WindowsResult<Self> {
        // SAFETY: the cycle probe initializes one STA on its current thread,
        // retains this owner for the complete COM object lifetime, and pairs
        // every successful initialization with CoUninitialize on that thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self {
            owner_thread: std::thread::current().id(),
            _not_send_or_sync: std::marker::PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        assert_eq!(
            self.owner_thread,
            std::thread::current().id(),
            "COM apartment dropped outside its owner thread"
        );
        // SAFETY: this owner represents a successful CoInitializeEx call on
        // the current thread and all overlay COM owners have already dropped.
        unsafe { CoUninitialize() };
    }
}

struct OverlayWindow {
    hwnd: HWND,
    instance: HINSTANCE,
    dpi: u32,
    owner_thread: ThreadId,
    log_lifecycle: bool,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl OverlayWindow {
    unsafe fn create(log_lifecycle: bool) -> WindowsResult<Self> {
        let module = unsafe { GetModuleHandleW(None)? };
        let instance = HINSTANCE(module.0);
        let class_name = w!("BongoCatNativeOverlaySpikeWindow");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(Error::from_thread());
        }

        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
                class_name,
                w!("BongoCat D3D11 Overlay Spike"),
                WS_POPUP,
                80,
                80,
                WIDTH as i32,
                HEIGHT as i32,
                None,
                None,
                Some(instance),
                None,
            )
        };
        let hwnd = match window {
            Ok(window) => window,
            Err(error) => {
                let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
                return Err(error);
            }
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            let error = invariant_error("GetDpiForWindow returned zero for the live overlay HWND");
            let _ = unsafe { DestroyWindow(hwnd) };
            let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
            return Err(error);
        }

        Ok(Self {
            hwnd,
            instance,
            dpi,
            owner_thread: std::thread::current().id(),
            log_lifecycle,
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    fn show(&self) -> WindowsResult<()> {
        self.assert_owner_thread();
        // SAFETY: the HWND is owned by this object and used only on its
        // creation thread. TOPMOST is applied once as a state transition.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )?;
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            if !IsWindowVisible(self.hwnd).as_bool() {
                return Err(invariant_error("overlay did not become visible"));
            }
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows overlay shown");
        }
        Ok(())
    }

    fn hide(&self) -> WindowsResult<()> {
        self.assert_owner_thread();
        // SAFETY: the owned HWND remains live and thread-confined.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            if IsWindowVisible(self.hwnd).as_bool() {
                return Err(invariant_error("overlay remained visible after hide"));
            }
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows overlay hidden");
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> WindowsResult<()> {
        self.assert_owner_thread();
        validate_dimensions(width, height)?;
        // SAFETY: the HWND is live and thread-confined. This changes only the
        // client-sized popup dimensions and does not reactivate or reorder it.
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows overlay resized width={width} height={height}");
        }
        Ok(())
    }

    fn physical_dimensions(
        &self,
        logical_width: u32,
        logical_height: u32,
    ) -> WindowsResult<(u32, u32)> {
        Ok((
            logical_to_physical(logical_width, self.dpi)?,
            logical_to_physical(logical_height, self.dpi)?,
        ))
    }

    fn assert_owner_thread(&self) {
        assert_eq!(
            self.owner_thread,
            std::thread::current().id(),
            "Win32 overlay accessed outside its owner thread"
        );
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: renderer fields have already been dropped, the HWND is
        // still owned here, and class unregistration follows destruction.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(w!("BongoCatNativeOverlaySpikeWindow"), Some(self.instance));
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows overlay window destroyed");
        }
    }
}

struct Renderer {
    visual: IDCompositionVisual,
    target: IDCompositionTarget,
    composition_device: IDCompositionDevice,
    render_target: Option<ID3D11RenderTargetView>,
    staging_texture: Option<ID3D11Texture2D>,
    blend_state: ID3D11BlendState,
    rasterizer_state: ID3D11RasterizerState,
    vertex_buffer: ID3D11Buffer,
    input_layout: ID3D11InputLayout,
    pixel_shader: ID3D11PixelShader,
    vertex_shader: ID3D11VertexShader,
    back_buffer: Option<ID3D11Texture2D>,
    swap_chain: IDXGISwapChain1,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    driver_name: &'static str,
    width: u32,
    height: u32,
    owner_thread: ThreadId,
    log_lifecycle: bool,
    _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

impl Renderer {
    unsafe fn create(hwnd: HWND, log_lifecycle: bool) -> WindowsResult<Self> {
        let (device, context, driver_name) = unsafe { create_d3d11_device()? };
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let descriptor = DXGI_SWAP_CHAIN_DESC1 {
            Width: WIDTH,
            Height: HEIGHT,
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
        let target = unsafe { composition_device.CreateTargetForHwnd(hwnd, true)? };
        let visual = unsafe { composition_device.CreateVisual()? };
        unsafe {
            visual.SetContent(&swap_chain)?;
            target.SetRoot(&visual)?;
            composition_device.Commit()?;
        }

        let (back_buffer, render_target, staging_texture) =
            unsafe { create_render_targets(&device, &swap_chain)? };
        let (
            vertex_shader,
            pixel_shader,
            input_layout,
            vertex_buffer,
            blend_state,
            rasterizer_state,
        ) = unsafe { create_geometry_pipeline(&device)? };

        Ok(Self {
            visual,
            target,
            composition_device,
            render_target: Some(render_target),
            staging_texture: Some(staging_texture),
            blend_state,
            rasterizer_state,
            vertex_buffer,
            input_layout,
            pixel_shader,
            vertex_shader,
            back_buffer: Some(back_buffer),
            swap_chain,
            context,
            device,
            driver_name,
            width: WIDTH,
            height: HEIGHT,
            owner_thread: std::thread::current().id(),
            log_lifecycle,
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    fn clear_present(&self, log_frame: bool) -> WindowsResult<()> {
        self.assert_owner_thread();
        let render_target = self
            .render_target
            .as_ref()
            .ok_or_else(|| invariant_error("D3D11 render target is unavailable"))?;
        let staging_texture = self
            .staging_texture
            .as_ref()
            .ok_or_else(|| invariant_error("D3D11 staging texture is unavailable"))?;
        let back_buffer = self
            .back_buffer
            .as_ref()
            .ok_or_else(|| invariant_error("D3D11 back buffer is unavailable"))?;
        // SAFETY: all interfaces are owned by this renderer on the creation
        // thread. The RTV belongs to swap-chain buffer 0 and remains live
        // across this clear/present call.
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(render_target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            self.context
                .ClearRenderTargetView(render_target, &[0.0, 0.0, 0.0, 0.0]);
            let vertex_buffer = Some(self.vertex_buffer.clone());
            let stride = size_of::<OverlayVertex>() as u32;
            let offset = 0_u32;
            self.context.IASetInputLayout(&self.input_layout);
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(&raw const vertex_buffer),
                Some(&raw const stride),
                Some(&raw const offset),
            );
            self.context
                .IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.VSSetShader(&self.vertex_shader, None);
            self.context.PSSetShader(&self.pixel_shader, None);
            self.context
                .OMSetBlendState(&self.blend_state, None, u32::MAX);
            self.context.RSSetState(&self.rasterizer_state);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context.Draw(OVERLAY_VERTICES.len() as u32, 0);
            self.context.CopyResource(staging_texture, back_buffer);
            verify_non_empty_frame(&self.context, staging_texture, self.width, self.height)?;
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.device.GetDeviceRemovedReason()?;
        }
        if self.log_lifecycle && log_frame {
            println!(
                "gpui-overlay-spike: Windows non-empty premultiplied-alpha draw/present submitted"
            );
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> WindowsResult<()> {
        self.assert_owner_thread();
        validate_dimensions(width, height)?;
        if self.width == width && self.height == height {
            return Ok(());
        }

        // SAFETY: all swap-chain references are owned here. The immediate
        // context is detached and flushed before the old RTV, staging texture,
        // and back buffer are released, satisfying ResizeBuffers ownership.
        unsafe {
            self.context
                .OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
            self.context.ClearState();
            self.context.Flush();
        }
        drop(self.render_target.take());
        drop(self.staging_texture.take());
        drop(self.back_buffer.take());

        unsafe {
            self.swap_chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        let (back_buffer, render_target, staging_texture) =
            unsafe { create_render_targets(&self.device, &self.swap_chain)? };
        self.back_buffer = Some(back_buffer);
        self.render_target = Some(render_target);
        self.staging_texture = Some(staging_texture);
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn assert_owner_thread(&self) {
        assert_eq!(
            self.owner_thread,
            std::thread::current().id(),
            "D3D11 overlay accessed outside its owner thread"
        );
    }
}

unsafe fn create_render_targets(
    device: &ID3D11Device,
    swap_chain: &IDXGISwapChain1,
) -> WindowsResult<(ID3D11Texture2D, ID3D11RenderTargetView, ID3D11Texture2D)> {
    let back_buffer: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
    let mut render_target = None;
    unsafe { device.CreateRenderTargetView(&back_buffer, None, Some(&mut render_target))? };
    let staging_texture = unsafe { create_staging_texture(device, &back_buffer)? };
    Ok((
        back_buffer,
        render_target.ok_or_else(|| invariant_error("CreateRenderTargetView returned no view"))?,
        staging_texture,
    ))
}

unsafe fn create_staging_texture(
    device: &ID3D11Device,
    back_buffer: &ID3D11Texture2D,
) -> WindowsResult<ID3D11Texture2D> {
    let mut descriptor = D3D11_TEXTURE2D_DESC::default();
    unsafe { back_buffer.GetDesc(&mut descriptor) };
    descriptor.Usage = D3D11_USAGE_STAGING;
    descriptor.BindFlags = 0;
    descriptor.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    descriptor.MiscFlags = 0;

    let mut staging_texture = None;
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut staging_texture))? };
    staging_texture.ok_or_else(|| invariant_error("CreateTexture2D returned no staging texture"))
}

unsafe fn create_geometry_pipeline(
    device: &ID3D11Device,
) -> WindowsResult<(
    ID3D11VertexShader,
    ID3D11PixelShader,
    ID3D11InputLayout,
    ID3D11Buffer,
    ID3D11BlendState,
    ID3D11RasterizerState,
)> {
    let vertex_bytecode = unsafe { compile_shader(s!("vertex_main"), s!("vs_5_0"))? };
    let pixel_bytecode = unsafe { compile_shader(s!("pixel_main"), s!("ps_5_0"))? };
    let vertex_bytes = unsafe { blob_bytes(&vertex_bytecode) };
    let pixel_bytes = unsafe { blob_bytes(&pixel_bytecode) };

    let mut vertex_shader = None;
    unsafe {
        device.CreateVertexShader(
            vertex_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut vertex_shader),
        )?;
    }
    let mut pixel_shader = None;
    unsafe {
        device.CreatePixelShader(
            pixel_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut pixel_shader),
        )?;
    }

    let input_elements = [
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
            SemanticName: s!("COLOR"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: size_of::<[f32; 2]>() as u32,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];
    let mut input_layout = None;
    unsafe { device.CreateInputLayout(&input_elements, vertex_bytes, Some(&mut input_layout))? };

    let vertex_descriptor = D3D11_BUFFER_DESC {
        ByteWidth: size_of_val(&OVERLAY_VERTICES) as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let vertex_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: OVERLAY_VERTICES.as_ptr().cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut vertex_buffer = None;
    unsafe {
        device.CreateBuffer(
            &vertex_descriptor,
            Some(&vertex_data),
            Some(&mut vertex_buffer),
        )?;
    }

    let premultiplied_blend = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_ONE,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut blend_descriptor = D3D11_BLEND_DESC::default();
    blend_descriptor.RenderTarget[0] = premultiplied_blend;
    let mut blend_state = None;
    unsafe { device.CreateBlendState(&blend_descriptor, Some(&mut blend_state))? };

    let rasterizer_descriptor = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        ..Default::default()
    };
    let mut rasterizer_state = None;
    unsafe {
        device.CreateRasterizerState(&rasterizer_descriptor, Some(&mut rasterizer_state))?;
    }

    Ok((
        vertex_shader
            .ok_or_else(|| invariant_error("CreateVertexShader returned no vertex shader"))?,
        pixel_shader
            .ok_or_else(|| invariant_error("CreatePixelShader returned no pixel shader"))?,
        input_layout.ok_or_else(|| invariant_error("CreateInputLayout returned no layout"))?,
        vertex_buffer.ok_or_else(|| invariant_error("CreateBuffer returned no vertex buffer"))?,
        blend_state.ok_or_else(|| invariant_error("CreateBlendState returned no blend state"))?,
        rasterizer_state
            .ok_or_else(|| invariant_error("CreateRasterizerState returned no rasterizer state"))?,
    ))
}

unsafe fn compile_shader(entry_point: PCSTR, target: PCSTR) -> WindowsResult<ID3DBlob> {
    let mut code = None;
    let mut diagnostics = None;
    let result = unsafe {
        D3DCompile(
            SHADER_SOURCE.as_ptr().cast(),
            SHADER_SOURCE.len(),
            s!("BongoCatOverlaySpike.hlsl"),
            None,
            None::<&ID3DInclude>,
            entry_point,
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
    code.ok_or_else(|| invariant_error("D3DCompile returned no shader bytecode"))
}

unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    let pointer = unsafe { blob.GetBufferPointer() }.cast::<u8>();
    let length = unsafe { blob.GetBufferSize() };
    // SAFETY: ID3DBlob owns a contiguous allocation of GetBufferSize bytes,
    // and the returned slice is borrowed only for the blob's live scope.
    unsafe { std::slice::from_raw_parts(pointer, length) }
}

unsafe fn blob_message(blob: &ID3DBlob) -> String {
    let bytes = unsafe { blob_bytes(blob) };
    String::from_utf8_lossy(bytes)
        .trim_end_matches(['\0', '\r', '\n'])
        .to_string()
}

unsafe fn verify_non_empty_frame(
    context: &ID3D11DeviceContext,
    staging_texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> WindowsResult<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let validation = if mapped.pData.is_null() {
        Err(invariant_error("D3D11 staging texture mapped to null"))
    } else if mapped.RowPitch < width * 4 {
        Err(invariant_error(
            "D3D11 staging texture row pitch is too small",
        ))
    } else {
        let offset = (height as usize / 2) * mapped.RowPitch as usize + (width as usize / 2) * 4;
        // SAFETY: RowPitch was checked for a complete row, the selected x/y are
        // inside the fixed texture dimensions, and BGRA8 stores four bytes.
        let pixel = unsafe {
            let pointer = mapped.pData.cast::<u8>().add(offset);
            [*pointer, *pointer.add(1), *pointer.add(2), *pointer.add(3)]
        };
        let [blue, green, red, alpha] = pixel;
        if alpha == 0 {
            Err(invariant_error(
                "D3D11 readback found a transparent center pixel after draw",
            ))
        } else if red > alpha || green > alpha || blue > alpha {
            Err(invariant_error(&format!(
                "D3D11 readback violated premultiplied alpha: bgra={pixel:?}"
            )))
        } else {
            Ok(())
        }
    };
    unsafe { context.Unmap(staging_texture, 0) };
    validation
}

fn validate_dimensions(width: u32, height: u32) -> WindowsResult<()> {
    if width == 0 || height == 0 {
        return Err(invariant_error("overlay dimensions must be non-zero"));
    }
    if width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(invariant_error("overlay dimensions exceed Win32 limits"));
    }
    Ok(())
}

fn logical_to_physical(logical: u32, dpi: u32) -> WindowsResult<u32> {
    if logical == 0 || dpi == 0 {
        return Err(invariant_error(
            "logical dimensions and DPI must be non-zero",
        ));
    }
    let physical = (u64::from(logical) * u64::from(dpi) + 48) / 96;
    if physical == 0 || physical > i32::MAX as u64 {
        return Err(invariant_error(
            "scaled overlay dimension exceeds Win32 limits",
        ));
    }
    Ok(physical as u32)
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: teardown runs on the owner thread. The composition graph is
        // detached before COM fields release in declaration order; pending D3D
        // commands are cleared and flushed before the device is dropped.
        unsafe {
            let _ = self.visual.SetContent(None::<&windows::core::IUnknown>);
            let _ = self.target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self.composition_device.Commit();
            self.context.ClearState();
            self.context.Flush();
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows overlay GPU released");
        }
    }
}

unsafe fn create_d3d11_device() -> WindowsResult<(ID3D11Device, ID3D11DeviceContext, &'static str)>
{
    let mut last_error = None;
    for (driver_type, driver_name) in [
        (D3D_DRIVER_TYPE_HARDWARE, "hardware"),
        (D3D_DRIVER_TYPE_WARP, "warp"),
    ] {
        match unsafe { try_create_d3d11_device(driver_type) } {
            Ok((device, context)) => return Ok((device, context, driver_name)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(Error::from_thread))
}

unsafe fn try_create_d3d11_device(
    driver_type: D3D_DRIVER_TYPE,
) -> WindowsResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
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
        device.expect("D3D11CreateDevice returned no device"),
        context.expect("D3D11CreateDevice returned no immediate context"),
    ))
}

fn process_handle_count() -> WindowsResult<u32> {
    let mut count = 0;
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the process;
    // the output pointer references initialized writable storage.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count)? };
    Ok(count)
}

fn format_windows_error(error: Error) -> String {
    format!(
        "{} (HRESULT 0x{:08X})",
        error.message(),
        error.code().0 as u32
    )
}

fn invariant_error(message: &str) -> Error {
    Error::new(HRESULT(0x8000_4005_u32 as i32), message)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: this callback does not retain pointers or panic across Win32;
    // all messages use the default window procedure.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::{OVERLAY_VERTICES, OverlayVertex, logical_to_physical};

    #[test]
    fn vertex_layout_and_colors_match_d3d_input_contract() {
        assert_eq!(size_of::<OverlayVertex>(), 24);
        for vertex in OVERLAY_VERTICES {
            let [red, green, blue, alpha] = vertex.premultiplied_color;
            assert!(alpha > 0.0);
            assert!(red <= alpha && green <= alpha && blue <= alpha);
        }
    }

    #[test]
    fn scales_logical_dimensions_for_per_monitor_dpi() {
        assert_eq!(logical_to_physical(400, 96).unwrap(), 400);
        assert_eq!(logical_to_physical(400, 120).unwrap(), 500);
        assert_eq!(logical_to_physical(400, 144).unwrap(), 600);
        assert_eq!(logical_to_physical(400, 192).unwrap(), 800);
        assert!(logical_to_physical(0, 96).is_err());
        assert!(logical_to_physical(400, 0).is_err());
    }
}
