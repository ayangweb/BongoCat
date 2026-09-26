//! The session, and the input it owns.
//!
//! One session owns the overlay's window, its GPU surface and its input
//! registration, and it is the only thing that can shut them down in order. A
//! denied input permission is reported once as a degraded state rather than
//! retried every frame, because a permission the user has declined will not
//! become granted by asking again.

use super::*;

impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.inner.window_handle()
    }
}

pub(crate) fn input_start_failure_diagnostics(
    error: PlatformInputError,
) -> PlatformInputDiagnostics {
    PlatformInputDiagnostics {
        service_status: match error {
            PlatformInputError::PermissionDenied => PlatformInputServiceStatus::PermissionDenied,
            PlatformInputError::BackendUnavailable => {
                PlatformInputServiceStatus::BackendUnavailable
            }
            _ => PlatformInputServiceStatus::Failed,
        },
        service_error_code: Some(error.as_str()),
        service_start_attempts: 1,
        ..PlatformInputDiagnostics::default()
    }
}

pub(crate) fn start_platform_input<T>(
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
    pub(crate) inner: macos::ProductOverlaySession,
    #[cfg(target_os = "windows")]
    pub(crate) inner: windows::ProductOverlaySession,
}

/// A right-click on the model window. The application owns menu presentation
/// and action handling; the overlay only reports this platform input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayContextMenuRequest;

/// The end of a right-button drag that resized the model window.
///
/// The overlay reports only the scale the drag settled on. Window geometry is
/// already published through the placement path the frame loop owns, and the
/// application is the only owner of configuration, so the scale is a request
/// rather than a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayResizeOutcome {
    /// The scale the drag ended on, clamped to the `25–400` configuration
    /// contract by the state machine.
    pub scale_percent: u16,
}

/// Optional application-owned event handoffs consumed by the native overlay.
/// They carry no overlay state and are never used to render or mutate config.
pub struct OverlayInteractionSinks {
    pub context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
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
        Self::start_with_interaction_sinks(
            runtime_client,
            input_producer,
            cursor_producer,
            gamepad_axis_producer,
            render_consumer,
            options,
            OverlayInteractionSinks {
                context_menu_sender: None,
                resize_sender: None,
            },
        )
    }

    pub fn start_with_interaction_sinks(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
        interaction_sinks: OverlayInteractionSinks,
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
                interaction_sinks,
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
                interaction_sinks,
            )
            .map(|inner| Self { inner })
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
    }

    pub fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }

    pub fn model_generation(&self) -> u64 {
        self.inner.model_generation()
    }

    #[cfg(target_os = "windows")]
    pub fn system_termination_requested(&self) -> bool {
        self.inner.system_termination_requested()
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
    }
}
