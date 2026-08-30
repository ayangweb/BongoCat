#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use bongocat_platform::{PlatformInputDiagnostics, PlatformInputError};
use bongocat_render::{RenderConsumer, RenderTransportDiagnostics};
use bongocat_runtime::{CursorProducer, InputProducer, RuntimeClient};
use std::{fmt, path::Path, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySessionOptions {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    pub maximum_fps: u16,
}

impl Default for OverlaySessionOptions {
    fn default() -> Self {
        Self {
            click_through: true,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            maximum_fps: 60,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductOverlayReport {
    pub frames_presented: u64,
    pub dynamic_snapshots: u64,
    pub model_commit_rejections: u64,
    pub input_start_error: Option<PlatformInputError>,
    pub input_diagnostics: Option<PlatformInputDiagnostics>,
    pub render_diagnostics: RenderTransportDiagnostics,
    pub model_generation: u64,
    pub drawable_count: usize,
    pub masked_drawable_count: usize,
    pub texture_count: usize,
}

pub struct ProductOverlaySession {
    #[cfg(target_os = "macos")]
    inner: macos::ProductOverlaySession,
    #[cfg(target_os = "windows")]
    inner: windows::ProductOverlaySession,
}

impl ProductOverlaySession {
    pub fn start(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
    ) -> Result<Self, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            macos::ProductOverlaySession::start(
                runtime_client,
                input_producer,
                cursor_producer,
                render_consumer,
                options,
            )
            .map(|inner| Self { inner })
        }

        #[cfg(target_os = "windows")]
        {
            windows::ProductOverlaySession::start(
                runtime_client,
                input_producer,
                cursor_producer,
                render_consumer,
                options,
            )
            .map(|inner| Self { inner })
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (
                runtime_client,
                input_producer,
                cursor_producer,
                render_consumer,
                options,
            );
            Err(OverlayError::new(
                "the product Live2D overlay is available on Windows and macOS",
            ))
        }
    }

    pub fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.run_for(duration)
        }

        #[cfg(target_os = "windows")]
        {
            self.inner.run_for(duration)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = duration;
            Err(OverlayError::new(
                "the product Live2D overlay is available on Windows and macOS",
            ))
        }
    }

    pub fn stop_input(&mut self) -> Result<(), OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.stop_input()
        }

        #[cfg(target_os = "windows")]
        {
            self.inner.stop_input()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(OverlayError::new(
                "the product Live2D overlay is available on Windows and macOS",
            ))
        }
    }

    pub fn finish_after_runtime_shutdown(self) -> Result<ProductOverlayReport, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.finish_after_runtime_shutdown()
        }

        #[cfg(target_os = "windows")]
        {
            self.inner.finish_after_runtime_shutdown()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(OverlayError::new(
                "the product Live2D overlay is available on Windows and macOS",
            ))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewReport {
    pub frames_presented: u64,
    pub dynamic_snapshots: u64,
    pub runtime_input_events: u64,
    pub platform_input_edges: u64,
    pub runtime_cursor_published: u64,
    pub runtime_cursor_coalesced: u64,
    pub runtime_cursor_consumed: u64,
    pub platform_cursor_samples: u64,
    pub render_frames_published: u64,
    pub render_frames_coalesced: u64,
    pub render_frames_consumed: u64,
    pub model_switches: u64,
    pub failed_gpu_prepare_preserved: bool,
    pub metal_bytes_before: u64,
    pub metal_bytes_after: u64,
    pub drawable_count: usize,
    pub masked_drawable_count: usize,
    pub texture_count: usize,
}

#[derive(Debug)]
pub struct OverlayError(String);

impl OverlayError {
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OverlayError {}

pub fn run_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
) -> Result<PreviewReport, OverlayError> {
    #[cfg(target_os = "macos")]
    {
        macos::run_model_preview(model_id, model_root, duration, false, None)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (model_id, model_root, duration);
        Err(OverlayError::new(
            "the first visible Live2D renderer is currently available on macOS",
        ))
    }
}

pub fn run_interactive_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
) -> Result<PreviewReport, OverlayError> {
    #[cfg(target_os = "macos")]
    {
        macos::run_model_preview(model_id, model_root, duration, true, None)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (model_id, model_root, duration);
        Err(OverlayError::new(
            "the first interactive Live2D preview is currently available on macOS",
        ))
    }
}

pub fn run_model_switch_preview(
    model_id: &str,
    model_root: &Path,
    switch_cycles: u32,
) -> Result<PreviewReport, OverlayError> {
    #[cfg(target_os = "macos")]
    {
        macos::run_model_preview(
            model_id,
            model_root,
            Duration::ZERO,
            false,
            Some(switch_cycles),
        )
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (model_id, model_root, switch_cycles);
        Err(OverlayError::new(
            "the first Live2D model-switch preview is currently available on macOS",
        ))
    }
}
