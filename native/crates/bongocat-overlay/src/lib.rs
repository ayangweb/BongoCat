#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::PlatformInputServiceStatus;
use bongocat_platform::{PlatformInputDiagnostics, PlatformInputError, ShortcutDispatcher};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_render::CanvasInfo;
use bongocat_render::{RenderConsumer, RenderTransportDiagnostics};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_runtime::PlatformInputDiagnosticsProducer;
use bongocat_runtime::{
    CursorProducer, GamepadAxisProducer, InputProducer, OverlaySettings, RuntimeClient,
};
use std::{fmt, path::Path, time::Duration};

pub const DEFAULT_OVERLAY_WINDOW_WIDTH: u32 = 350;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const MIN_OVERLAY_WINDOW_DIMENSION: f32 = 64.0;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn default_overlay_window_dimensions(canvas: CanvasInfo) -> (f32, f32) {
    let canvas_width = canvas.width.max(MIN_OVERLAY_WINDOW_DIMENSION);
    let canvas_height = canvas.height.max(MIN_OVERLAY_WINDOW_DIMENSION);
    let width = DEFAULT_OVERLAY_WINDOW_WIDTH as f32;
    let height = (width * canvas_height / canvas_width).max(MIN_OVERLAY_WINDOW_DIMENSION);
    (width, height)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySessionOptions {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    pub maximum_fps: u16,
    pub window_bounds: Option<OverlayWindowBounds>,
}

impl OverlaySessionOptions {
    pub const fn with_runtime_settings(self, settings: OverlaySettings) -> Self {
        Self {
            click_through: settings.click_through,
            always_on_top: settings.always_on_top,
            scale_percent: settings.scale_percent,
            opacity_percent: settings.opacity_percent,
            maximum_fps: self.maximum_fps,
            window_bounds: self.window_bounds,
        }
    }
}

impl Default for OverlaySessionOptions {
    fn default() -> Self {
        Self {
            click_through: false,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            maximum_fps: 60,
            window_bounds: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl OverlayWindowBounds {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn validate(self) -> Result<Self, OverlayError> {
        const MAX_COORDINATE: i32 = 1_000_000;
        const MIN_DIMENSION: u32 = 64;
        const MAX_DIMENSION: u32 = 16_384;
        if !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&self.x)
            || !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&self.y)
            || !(MIN_DIMENSION..=MAX_DIMENSION).contains(&self.width)
            || !(MIN_DIMENSION..=MAX_DIMENSION).contains(&self.height)
        {
            return Err(OverlayError::new("overlay window bounds are invalid"));
        }
        Ok(self)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn rescale(self, previous_percent: u16, next_percent: u16) -> Self {
        let ratio = f64::from(next_percent) / f64::from(previous_percent);
        Self {
            width: ((f64::from(self.width) * ratio).round() as u32).clamp(64, 16_384),
            height: ((f64::from(self.height) * ratio).round() as u32).clamp(64, 16_384),
            ..self
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
#[derive(Default)]
pub(crate) struct OverlayPresentationState {
    has_presented_frame: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl OverlayPresentationState {
    pub(crate) fn record_presented_frame(&mut self) {
        self.has_presented_frame = true;
    }

    pub(crate) fn require_presented_frame(&self) -> Result<(), OverlayError> {
        if !self.has_presented_frame {
            return Err(OverlayError::new(
                "overlay cannot become visible before its first presented frame",
            ));
        }
        Ok(())
    }
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
        Self::start_with_shortcuts(
            runtime_client,
            input_producer,
            cursor_producer,
            gamepad_axis_producer,
            render_consumer,
            options,
            None,
        )
    }

    pub fn start_with_shortcuts(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
        shortcut_dispatcher: Option<ShortcutDispatcher>,
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
                shortcut_dispatcher,
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
                shortcut_dispatcher,
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
                shortcut_dispatcher,
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

    pub fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.window_bounds()
        }

        #[cfg(target_os = "windows")]
        {
            self.inner.window_bounds()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(OverlayError::new(
                "the product Live2D overlay is available on Windows and macOS",
            ))
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn model_generation(&self) -> u64 {
        self.inner.model_generation()
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
    fn overlay_visibility_requires_a_successfully_presented_frame() {
        let mut presentation = OverlayPresentationState::default();
        assert!(presentation.require_presented_frame().is_err());

        presentation.record_presented_frame();
        presentation
            .require_presented_frame()
            .expect("presented overlay may become visible");
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
    fn persisted_overlay_bounds_are_bounded_and_scale_explicitly() {
        let bounds = OverlayWindowBounds::new(-640, 120, 400, 600)
            .validate()
            .expect("valid overlay bounds");
        assert_eq!(
            bounds.rescale(100, 125),
            OverlayWindowBounds::new(-640, 120, 500, 750)
        );
        assert!(OverlayWindowBounds::new(0, 0, 63, 600).validate().is_err());
        assert!(
            OverlayWindowBounds::new(1_000_001, 0, 400, 600)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn default_overlay_height_follows_the_model_aspect_ratio() {
        let landscape = CanvasInfo {
            width: 700.0,
            height: 400.0,
            origin_x: 350.0,
            origin_y: 200.0,
            pixels_per_unit: 400.0,
        };
        let portrait = CanvasInfo {
            width: 350.0,
            height: 700.0,
            origin_x: 175.0,
            origin_y: 350.0,
            pixels_per_unit: 350.0,
        };

        assert_eq!(default_overlay_window_dimensions(landscape), (350.0, 200.0));
        assert_eq!(default_overlay_window_dimensions(portrait), (350.0, 700.0));
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
