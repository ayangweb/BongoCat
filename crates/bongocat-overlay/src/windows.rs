//! The Windows overlay adapter.
//!
//! The product is one window on the user's desktop: layered, click-through
//! where it should be, and drawn by D3D11 through DirectComposition. The
//! adapter is split by concern — `session` is what the product drives, `window`
//! and `window_proc` own the native window, `renderer`, `pipelines` and
//! `textures` own the frame, and the rest each answer one question about it.

mod cover_capture;
mod geometry;
mod pipelines;
mod renderer;
mod session;
mod switch_preview;
#[cfg(test)]
mod tests;
mod textures;
mod thread_settle;
mod window;
mod window_proc;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the adapter's items are one vocabulary, and a list per
// module would be the same list ten times.
pub(crate) use cover_capture::*;
pub(crate) use geometry::*;
pub(crate) use pipelines::*;
pub(crate) use renderer::*;
pub(crate) use session::*;
pub(crate) use switch_preview::*;
pub(crate) use textures::*;
pub(crate) use thread_settle::*;
pub(crate) use window::*;
pub(crate) use window_proc::*;

use crate::{
    BlendFactor, DrawableCullMode, FRAME_SMOKE_GRID_DIMENSION, FrameRetryBackoff,
    MAXIMUM_CORNER_RADIUS_PERCENT, OverlayContextMenuRequest, OverlayError,
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
use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_platform::{
    PlatformInputDiagnostics, PlatformInputError, PlatformInputServiceStatus, WindowsInputService,
};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableId, KeyAssetId, KeyOverlay, ModelBounds, ModelCommitErrorCode,
    ModelCommitFeedback, ModelCommitOutcome, ModelCommitToken, RenderConsumer, RenderFrame,
    RenderResources, RenderSnapshot, TextureAsset, TextureId, validate_render_snapshot,
};
use bongocat_runtime::{
    CursorProducer, CursorSample, GamepadAxisProducer, GamepadButton, HandSide, InputBindings,
    InputControl, InputEdge, InputEvent, InputProducer, InputSource,
    MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS, MonotonicMillis, PhysicalKey, RuntimeClient,
    RuntimeCommand, RuntimeOwner, RuntimeRenderErrorCode, RuntimeState,
    frame_interval_for_maximum_fps, maximum_fps_is_valid,
};
use image::ImageReader;
use raw_window_handle::{HandleError, HasWindowHandle, Win32WindowHandle, WindowHandle};
use std::{
    collections::BTreeMap,
    mem::{size_of, size_of_val},
    num::NonZeroIsize,
    path::Path,
    rc::Rc,
    sync::{Arc, mpsc::SyncSender},
    thread,
    thread::ThreadId,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, DXGI_STATUS_OCCLUDED, ERROR_CLASS_ALREADY_EXISTS, ERROR_NO_MORE_FILES,
            HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
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
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CULL_BACK, D3D11_CULL_FRONT,
                D3D11_CULL_MODE, D3D11_CULL_NONE, D3D11_FILL_SOLID,
                D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC,
                D3D11_INPUT_PER_VERTEX_DATA, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
                D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SAMPLER_DESC,
                D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
                D3D11CreateDevice, ID3D11BlendState, ID3D11Buffer, ID3D11ClassLinkage,
                ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
                ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
                ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionEffectGroup,
                IDCompositionTarget, IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32G32_FLOAT,
                    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
                },
                DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_PRESENT, DXGI_QUERY_VIDEO_MEMORY_INFO,
                DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter,
                IDXGIAdapter3, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
            },
            Gdi::{
                EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST,
                MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint, MonitorFromRect,
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
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWL_EXSTYLE, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, GetWindowRect,
                HTCAPTION, HTTRANSPARENT, HWND_NOTOPMOST, HWND_TOPMOST, IsWindowVisible, MSG,
                PM_REMOVE, PeekMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW,
                WM_CAPTURECHANGED, WM_CLOSE, WM_CONTEXTMENU, WM_MOUSEMOVE, WM_NCCREATE,
                WM_NCDESTROY, WM_NCHITTEST, WM_NCMOUSEMOVE, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP,
                WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{BOOL, Error, HRESULT, Interface, PCSTR, Result as WindowsResult, s, w},
};

pub(crate) fn required<T>(value: Option<T>, name: &str) -> WindowsResult<T> {
    value.ok_or_else(|| invariant_error(&format!("D3D11 returned no {name}")))
}

pub(crate) fn invariant_error(message: &str) -> Error {
    Error::new(HRESULT(0x80004005_u32 as i32), message)
}

pub(crate) fn windows_error(context: &'static str) -> impl FnOnce(Error) -> OverlayError {
    move |error| OverlayError::new(format!("{context}: {error}"))
}
