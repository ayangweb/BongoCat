#![cfg(target_os = "windows")]

use std::{rc::Rc, thread::ThreadId};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
                ID3D11Texture2D,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
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
                SetWindowPos, ShowWindow, UnregisterClassW, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{Error, HRESULT, Interface, Result as WindowsResult, w},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const HANDLE_GROWTH_LIMIT: u32 = 4;

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
        let window = unsafe { OverlayWindow::create(log_lifecycle)? };
        let renderer = unsafe { Renderer::create(window.hwnd, log_lifecycle)? };
        Ok(Self { renderer, window })
    }

    pub fn show(&self) -> Result<(), String> {
        self.window.show().map_err(format_windows_error)
    }

    pub fn hide(&self) -> Result<(), String> {
        self.window.hide().map_err(format_windows_error)
    }

    pub fn clear_present(&self) -> Result<(), String> {
        self.renderer.clear_present().map_err(format_windows_error)
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
    render_target: ID3D11RenderTargetView,
    swap_chain: IDXGISwapChain1,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    driver_name: &'static str,
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

        let back_buffer: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
        let mut render_target = None;
        unsafe {
            device.CreateRenderTargetView(&back_buffer, None, Some(&mut render_target))?;
        }

        Ok(Self {
            visual,
            target,
            composition_device,
            render_target: render_target.expect("CreateRenderTargetView returned no view"),
            swap_chain,
            context,
            device,
            driver_name,
            owner_thread: std::thread::current().id(),
            log_lifecycle,
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    fn clear_present(&self) -> WindowsResult<()> {
        self.assert_owner_thread();
        // SAFETY: all interfaces are owned by this renderer on the creation
        // thread. The RTV belongs to swap-chain buffer 0 and remains live
        // across this clear/present call.
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(self.render_target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            self.context
                .ClearRenderTargetView(&self.render_target, &[0.0, 0.0, 0.0, 0.0]);
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.device.GetDeviceRemovedReason()?;
        }
        if self.log_lifecycle {
            println!("gpui-overlay-spike: Windows transparent clear/present submitted");
        }
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
