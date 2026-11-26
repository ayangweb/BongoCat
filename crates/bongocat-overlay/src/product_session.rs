//! The session, and the input it owns.
//!
//! One session owns the overlay's window, its GPU surface and its input
//! registration, and it is the only thing that can shut them down in order. A
//! denied input permission is reported once as a degraded state rather than
//! retried every frame, because a permission the user has declined will not
//! become granted by asking again.

use super::*;
use bongocat_plugin_hud::{HudConsumer, HudViewportReporter};

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
    #[cfg(target_os = "linux")]
    pub(crate) inner: linux::ProductOverlaySession,
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
    /// The newest composed plugin layer, taken once per tick and drawn as the
    /// topmost layer. `None` disables plugin UI, which is what a build or a test
    /// with no plugin host looks like.
    pub hud_consumer: Option<HudConsumer>,
    /// Where a click that lands on a plugin's control is reported.
    pub hud_interaction_sender: Option<SyncSender<OverlayPluginInteraction>>,
    /// Where the window reports its own size and backing scale.
    ///
    /// A composed layer has to match the window it is drawn into, so the native
    /// session publishes the numbers it just took and the plugin service composes
    /// against the newest report. `None` disables plugin UI for the same reason
    /// the layer channel does.
    pub hud_viewport_reporter: Option<HudViewportReporter>,
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
                hud_consumer: None,
                hud_interaction_sender: None,
                hud_viewport_reporter: None,
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

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            #[cfg(target_os = "linux")]
            use linux as backend;
            #[cfg(target_os = "windows")]
            use windows as backend;
            backend::ProductOverlaySession::start(
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

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.inner.run_for(duration)
        }
    }

    pub fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.tick()
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.inner.tick()
        }
    }

    pub fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.window_bounds()
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
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
    pub fn window_mode_is_active(&self) -> bool {
        self.inner.overlay.window._state.window_mode
    }

    /// Windows only: apply the configured taskbar button to the model window.
    ///
    /// macOS has no per-window equivalent: the Dock belongs to the process
    /// activation policy, which `show_dock_icon` owns.
    #[cfg(target_os = "windows")]
    pub fn set_taskbar_icon_visible(&mut self, visible: bool) -> Result<(), OverlayError> {
        self.inner.set_taskbar_icon_visible(visible)
    }

    /// Windows only: what the model window's taskbar currently shows.
    #[cfg(target_os = "windows")]
    pub fn taskbar_icon_is_visible(&self) -> bool {
        self.inner.taskbar_icon_is_visible()
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

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.inner.stop_input()
        }
    }

    pub fn finish_after_runtime_shutdown(self) -> Result<ProductOverlayReport, OverlayError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.finish_after_runtime_shutdown()
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.inner.finish_after_runtime_shutdown()
        }
    }
}

#[cfg(target_os = "linux")]
impl ProductOverlaySession {
    pub fn tick_linux_window(
        &mut self,
        settings: bongocat_runtime::OverlaySettings,
        local_modifiers: bongocat_input::PressedModifiers,
        input_available: bool,
    ) -> Result<OverlayTickOutcome, OverlayError> {
        self.inner
            .tick_with_window_behavior(Some((settings, local_modifiers, input_available)))
    }

    pub fn set_linux_display_selected(&mut self, selected: bool) {
        self.inner.set_display_selected(selected);
    }

    pub fn restore_linux_position(&self, x: i32, y: i32) {
        self.inner.restore_position(x, y);
    }

    pub fn start_linux_peer(
        &self,
        runtime: RuntimeClient,
        consumer: RenderConsumer,
        options: OverlaySessionOptions,
        saved_bounds: Option<OverlayWindowBounds>,
    ) -> Result<Self, OverlayError> {
        self.inner
            .start_peer(runtime, consumer, options, saved_bounds)
            .map(|inner| Self { inner })
    }

    pub fn sync_linux_alpha_scene<'a>(
        &self,
        primary_selected: bool,
        peers: impl IntoIterator<Item = (&'a mut Self, bool)>,
    ) {
        self.inner.sync_alpha_scene(
            primary_selected,
            peers
                .into_iter()
                .map(|(peer, selected)| (&mut peer.inner, selected)),
        );
    }

    pub fn linux_close_requested(&self) -> bool {
        self.inner.close_requested()
    }
    pub fn set_linux_alpha_output_enabled(
        &mut self,
        enabled: bool,
        environment: bongocat_config::BuildEnvironment,
    ) {
        self.inner.set_alpha_output_enabled(enabled, environment);
    }

    pub fn linux_alpha_output_status(&mut self) -> crate::AlphaOutputStatus {
        self.inner.alpha_output_status()
    }

    pub fn linux_always_on_top_available(&self) -> bool {
        self.inner.always_on_top_available()
    }
    pub fn set_linux_menu_presentation(
        &mut self,
        presentation: bongocat_platform::SystemMenuPresentation,
        palette: bongocat_platform::SystemMenuPalette,
    ) {
        self.inner.set_menu_presentation(presentation, palette);
    }
    pub fn take_linux_menu_action(&mut self) -> Option<bongocat_platform::SystemMenuAction> {
        self.inner.take_menu_action()
    }
    /// Start input capture after the Linux permission explanation is confirmed.
    pub fn start_linux_input(&mut self) {
        self.inner.start_input();
    }
}
