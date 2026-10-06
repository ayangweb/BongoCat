//! The GPU handle owner shared by the ordinary and layer-shell window paths.
use super::{OverlayError, err, layer_shell::LayerTarget};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use std::sync::Arc;
use winit::{
    dpi::{LogicalSize, PhysicalSize},
    window::Window,
};

pub(super) enum WindowTarget {
    Ordinary(Arc<Window>),
    Layer(Arc<LayerTarget>),
}
impl HasWindowHandle for WindowTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self {
            Self::Ordinary(window) => window.window_handle(),
            Self::Layer(window) => window.window_handle(),
        }
    }
}
impl HasDisplayHandle for WindowTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        match self {
            Self::Ordinary(window) => window.display_handle(),
            Self::Layer(window) => window.display_handle(),
        }
    }
}
impl WindowTarget {
    pub(super) fn scale_factor(&self) -> f64 {
        match self {
            Self::Ordinary(window) => window.scale_factor(),
            Self::Layer(window) => window.scale_factor(),
        }
    }
    pub(super) fn inner_size(&self) -> PhysicalSize<u32> {
        match self {
            Self::Ordinary(window) => window.inner_size(),
            Self::Layer(window) => window.physical_size(),
        }
    }
    pub(super) fn request_inner_size(&self, size: LogicalSize<u32>) {
        match self {
            Self::Ordinary(window) => {
                let _ = window.request_inner_size(size);
            }
            Self::Layer(window) => window.resize(size.width, size.height),
        }
    }
    pub(super) fn set_cursor_hittest(&self, enabled: bool) -> Result<(), OverlayError> {
        match self {
            Self::Ordinary(window) => window.set_cursor_hittest(enabled).map_err(err),
            Self::Layer(window) => window.set_cursor_hittest(enabled),
        }
    }
    pub(super) fn drag_window(&self) {
        if let Self::Ordinary(window) = self {
            let _ = window.drag_window();
        }
    }
    pub(super) fn pre_present_notify(&self) {
        if let Self::Ordinary(window) = self {
            window.pre_present_notify();
        }
    }
}
