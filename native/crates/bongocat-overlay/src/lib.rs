#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::PlatformInputServiceStatus;
use bongocat_platform::{PlatformInputDiagnostics, PlatformInputError};
use bongocat_render::{RenderConsumer, RenderTransportDiagnostics};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_runtime::PlatformInputDiagnosticsProducer;
use bongocat_runtime::{
    CursorProducer, GamepadAxisProducer, InputProducer, OverlaySettings, RuntimeClient,
};
use std::{fmt, path::Path, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySessionOptions {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    pub maximum_fps: u16,
}

impl OverlaySessionOptions {
    pub const fn with_runtime_settings(self, settings: OverlaySettings) -> Self {
        Self {
            click_through: settings.click_through,
            always_on_top: settings.always_on_top,
            scale_percent: settings.scale_percent,
            opacity_percent: settings.opacity_percent,
            maximum_fps: self.maximum_fps,
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayTickOutcome {
    Presented,
    Hidden,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn input_start_failure_diagnostics(error: PlatformInputError) -> PlatformInputDiagnostics {
    PlatformInputDiagnostics {
        service_status: match error {
            PlatformInputError::PermissionDenied => PlatformInputServiceStatus::PermissionDenied,
            PlatformInputError::BackendUnavailable => {
                PlatformInputServiceStatus::BackendUnavailable
            }
            _ => PlatformInputServiceStatus::Failed,
        },
        service_start_attempts: 1,
        ..PlatformInputDiagnostics::default()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn start_platform_input<T>(
    diagnostics_producer: &PlatformInputDiagnosticsProducer,
    start: impl FnOnce() -> Result<T, PlatformInputError>,
) -> (Option<T>, Option<PlatformInputError>) {
    match start() {
        Ok(service) => (Some(service), None),
        Err(error) => {
            let _ = diagnostics_producer.publish(input_start_failure_diagnostics(error));
            (None, Some(error))
        }
    }
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
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
    ) -> Result<Self, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            macos::ProductOverlaySession::start(
                runtime_client,
                input_producer,
                cursor_producer,
                gamepad_axis_producer,
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
                gamepad_axis_producer,
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
                gamepad_axis_producer,
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

    pub fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.tick()
        }

        #[cfg(target_os = "windows")]
        {
            self.inner.tick()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
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
    pub gpu_bytes_before: u64,
    pub gpu_bytes_after: u64,
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

    #[cfg(target_os = "windows")]
    {
        windows::run_model_switch_preview(model_id, model_root, switch_cycles)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (model_id, model_root, switch_cycles);
        Err(OverlayError::new(
            "the Live2D model-switch preview is available on Windows and macOS",
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn validate_model_generation_advance(
    active_generation: u64,
    candidate_generation: u64,
) -> Result<(), OverlayError> {
    if candidate_generation <= active_generation {
        return Err(OverlayError::new(format!(
            "GPU model generation did not advance from {active_generation} to {candidate_generation}"
        )));
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn model_generation_may_skip_rejected_candidates_but_never_regress() {
        validate_model_generation_advance(4, 7).expect("rejected generations may be skipped");
        assert!(validate_model_generation_advance(4, 4).is_err());
        assert!(validate_model_generation_advance(4, 3).is_err());
    }

    #[test]
    fn input_start_failures_publish_one_anonymous_degraded_attempt() {
        for (error, expected) in [
            (
                PlatformInputError::PermissionDenied,
                PlatformInputServiceStatus::PermissionDenied,
            ),
            (
                PlatformInputError::BackendUnavailable,
                PlatformInputServiceStatus::BackendUnavailable,
            ),
            (
                PlatformInputError::TapCreateFailed,
                PlatformInputServiceStatus::Failed,
            ),
        ] {
            let diagnostics = input_start_failure_diagnostics(error);
            assert_eq!(diagnostics.service_status, expected);
            assert_eq!(diagnostics.service_start_attempts, 1);
            assert_eq!(diagnostics.captured_edges, 0);
        }
    }

    #[test]
    fn platform_input_owner_attempts_a_denied_start_only_once() {
        let attempts = AtomicUsize::new(0);
        let producer = PlatformInputDiagnosticsProducer::default();
        let (service, error) = start_platform_input(&producer, || {
            attempts.fetch_add(1, Ordering::Relaxed);
            Err::<(), _>(PlatformInputError::PermissionDenied)
        });

        assert_eq!(service, None);
        assert_eq!(error, Some(PlatformInputError::PermissionDenied));
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(
            producer.diagnostics(),
            PlatformInputDiagnostics {
                service_status: PlatformInputServiceStatus::PermissionDenied,
                service_start_attempts: 1,
                ..PlatformInputDiagnostics::default()
            }
        );
    }
}
