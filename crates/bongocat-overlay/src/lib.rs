//! The product overlay: the window the cat is drawn in, and what may go wrong.
//!
//! The crate is one native window with a GPU surface on it, so almost everything
//! here is a question about that window — how large it is, where it may sit, what
//! a frame of it contained, and when it is really visible. The modules below are
//! those questions; the platform sessions beside them own the two implementations.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Pointer hover hide is only reachable through the native sessions, so the
/// module shares their platform gate rather than warning as dead code on the
/// targets that cannot create an overlay.
mod hover;

/// Overlay placement constraint and its settle delay. Gated with the native
/// sessions for the same reason as [`hover`].
mod placement;

/// Right-button drag resizing of the model window. Gated with the native
/// sessions for the same reason as [`hover`]: only they receive the pointer
/// messages the state machine consumes.
mod resize_drag;

/// The backend-independent half of the model cover capture. Gated with the native
/// sessions: only they can produce the pixels, and only they carry the image
/// dependency the PNG encoding needs.
mod cover;
pub use cover::ModelCoverCapture;

use bongocat_input::{
    CursorProducer, GamepadAxisProducer, InputProducer, PlatformInputDiagnostics,
    PlatformInputDiagnosticsProducer, PlatformInputServiceStatus,
};
use bongocat_platform::PlatformInputError;
use bongocat_render::BlendMode;
use bongocat_render::CanvasInfo;
use bongocat_render::{RenderConsumer, RenderTransportDiagnostics};
use bongocat_runtime::{OverlaySettings, RuntimeClient, hover_hide_delay_ms};
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::{path::Path, sync::mpsc::SyncSender, time::Duration};

pub const DEFAULT_OVERLAY_WINDOW_WIDTH: u32 = 350;
pub(crate) const FRAME_SMOKE_GRID_DIMENSION: u64 = 17;
const MIN_OVERLAY_WINDOW_DIMENSION: f32 = 64.0;
const MAX_OVERLAY_WINDOW_DIMENSION: u32 = 16_384;

/// Upper bound of the overlay corner radius, in percent of the window box.
///
/// The overlay radius keeps the legacy `border-radius: N%` semantics, where the
/// four corner arcs are ellipses whose semi-axes are `N%` of the window width
/// and height. At `50%` the arcs meet and the content is clipped to the full
/// inscribed ellipse; the legacy implementation scaled every larger radius back
/// down to that same ellipse, so `50` is the effective ceiling rather than an
/// arbitrary limit.
///
/// Gated with the native sessions for the same reason as [`hover`]: the only
/// readers are the platform modules and the renderer payload they share, so an
/// unguarded declaration would be dead code on the targets that cannot create
/// an overlay.
pub(crate) const MAXIMUM_CORNER_RADIUS_PERCENT: u8 = 50;

/// Build the renderer's corner-radius uniform payload.
///
/// `x` is the radius as a fraction of the window box (clamped to the full
/// ellipse), and `y`/`z` carry the drawable dimensions so the fragment shader
/// can recover pixel coordinates and size its antialiased band in device
/// pixels. The corner coverage is evaluated per drawable pixel and multiplied
/// into each drawable's alpha. Presentation opacity is applied later, once to
/// the completed native surface, so overlapping Live2D parts do not each fade
/// independently.
pub(crate) fn corner_radius_uniform(
    corner_radius_percent: u8,
    width: f32,
    height: f32,
) -> [f32; 4] {
    let percent = corner_radius_percent.min(MAXIMUM_CORNER_RADIUS_PERCENT);
    [f32::from(percent) / 100.0, width, height, 0.0]
}

mod blend;
mod bounds;
mod dimensions;
mod error;
mod frame;
mod presentation;
mod preview;
mod product_session;
mod report;
#[cfg(test)]
mod tests;
mod timing;

pub(crate) use blend::*;
pub(crate) use bounds::*;
pub(crate) use dimensions::*;
pub(crate) use frame::*;
pub(crate) use presentation::*;
pub(crate) use preview::*;
pub(crate) use product_session::*;
// `cover` and `timing` hold items that are only `pub(crate)`, so the root
// names them: a glob would carry nothing and an existing `pub use` line
// cannot reach a narrower item.
#[cfg(any(target_os = "macos", test))]
pub(crate) use timing::FrameTimingCollector;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use bounds::{OverlaySessionOptions, OverlayWindowBounds};
pub use cover::{ModelCoverCaptureSession, capture_model_cover};
pub use error::OverlayError;
pub use preview::{run_interactive_model_preview, run_model_preview, run_model_switch_preview};
pub use product_session::{
    OverlayContextMenuRequest, OverlayInteractionSinks, OverlayResizeOutcome, ProductOverlaySession,
};
pub use report::{OverlayTickOutcome, ProductOverlayReport};
pub use timing::{FrameTimingSummary, PreviewReport};
