use super::*;
use crate::idle::{IdleHide, IdleObservation};
use bongocat_render::{ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome, RenderFrame};
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    platform::{pump_events::EventLoopExtPumpEvents, wayland::EventLoopBuilderExtWayland},
    window::{Window, WindowId},
};
mod cover_capture;
mod renderer;
pub(crate) use cover_capture::CoverCaptureSession;

struct WindowEvents {
    window: Arc<Window>,
    closed: bool,
    open_settings: bool,
}
impl ApplicationHandler for WindowEvents {
    fn resumed(&mut self, _events: &ActiveEventLoop) {}
    fn window_event(&mut self, _events: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.closed = true,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let _ = self.window.drag_window();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                self.open_settings = true;
            }
            _ => {}
        }
    }
}
pub struct ProductOverlaySession {
    renderer: renderer::Renderer,
    events: EventLoop<()>,
    state: WindowEvents,
    runtime: RuntimeClient,
    consumer: RenderConsumer,
    frame: RenderFrame,
    input: Option<bongocat_platform::LinuxInputService>,
    options: OverlaySessionOptions,
    idle: IdleHide,
    session_started: Instant,
    cursor_hittest: bool,
    frames: u64,
    snapshots: u64,
    rejections: u64,
    input_error: Option<PlatformInputError>,
    diagnostics: Option<PlatformInputDiagnostics>,
}
impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.state.window.window_handle()
    }
}
impl ProductOverlaySession {
    pub fn start(
        runtime: RuntimeClient,
        producer: InputProducer,
        cursor: CursorProducer,
        axes: GamepadAxisProducer,
        consumer: RenderConsumer,
        options: OverlaySessionOptions,
        _sinks: OverlayInteractionSinks,
    ) -> Result<Self, OverlayError> {
        let frame = consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime published no initial frame"))?;
        let mut builder = EventLoop::builder();
        builder.with_wayland().with_any_thread(true);
        let events = builder.build().map_err(err)?;
        let (width, height) = model_window_dimensions(frame.snapshot.canvas, options.scale_percent);
        // This backend owns an independent Wayland connection; GPUI owns settings.
        #[allow(deprecated)]
        let window = Arc::new(
            events
                .create_window(
                    Window::default_attributes()
                        .with_title("BongoCat")
                        .with_decorations(false)
                        .with_transparent(true)
                        .with_inner_size(winit::dpi::LogicalSize::new(width, height)),
                )
                .map_err(err)?,
        );
        window
            .set_cursor_hittest(!options.click_through)
            .map_err(err)?;
        let renderer = match renderer::Renderer::new(Some(window.clone()), &frame, width, height) {
            Ok(renderer) => renderer,
            Err(e) => {
                feedback(
                    &runtime,
                    &consumer,
                    &frame,
                    ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
                )?;
                return Err(e);
            }
        };
        feedback(&runtime, &consumer, &frame, ModelCommitOutcome::Prepared)?;
        let diagnostic_sender = runtime.platform_input_diagnostics_producer();
        let (input, input_error) = start_platform_input(&diagnostic_sender, || {
            bongocat_platform::LinuxInputService::start_with_diagnostics(
                producer,
                cursor,
                axes,
                diagnostic_sender.clone(),
            )
        });
        Ok(Self {
            renderer,
            events,
            state: WindowEvents {
                window,
                closed: false,
                open_settings: false,
            },
            runtime,
            consumer,
            frame,
            input,
            options,
            idle: IdleHide::default(),
            session_started: Instant::now(),
            cursor_hittest: !options.click_through,
            frames: 0,
            snapshots: 0,
            rejections: 0,
            input_error,
            diagnostics: None,
        })
    }
    pub fn take_open_settings(&mut self) -> bool {
        std::mem::take(&mut self.state.open_settings)
    }
    pub fn close_requested(&self) -> bool {
        self.state.closed
    }
    pub fn set_shortcuts(
        &mut self,
        table: bongocat_config::ShortcutTable,
        dispatcher: bongocat_platform::ShortcutDispatcher,
    ) {
        if let Some(input) = &self.input {
            input.set_shortcuts(table, dispatcher);
        }
    }
    pub fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let start = Instant::now();
        while !self.state.closed && (duration.is_zero() || start.elapsed() < duration) {
            self.tick()?;
            std::thread::sleep(Duration::from_secs_f64(
                1. / f64::from(self.options.maximum_fps),
            ));
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
        self.events
            .pump_app_events(Some(Duration::ZERO), &mut self.state);
        if self.state.closed {
            return Ok(OverlayTickOutcome::Hidden);
        }
        let snapshot = self.runtime.snapshot();
        let next = self
            .options
            .with_runtime_settings(snapshot.overlay_settings);
        let held = next.hold_modifier_pressed(snapshot.input.pressed_modifiers);
        self.idle.observe(IdleObservation {
            enabled: next.hide_on_idle
                && snapshot.platform_input.service_status == PlatformInputServiceStatus::Running,
            delay: Duration::from_millis(u64::from(next.hide_on_idle_delay_ms)),
            input_sequence: snapshot.input.last_input_sequence,
            cursor_at: snapshot.cursor.sample.map(|sample| sample.at),
            gamepad_axis_published: snapshot.gamepad_axis_transport.published,
            now: self.session_started.elapsed(),
        });
        let cursor_hittest = (!next.click_through || held) && !self.idle.hidden();
        if cursor_hittest != self.cursor_hittest {
            self.state
                .window
                .set_cursor_hittest(cursor_hittest)
                .map_err(err)?;
            self.cursor_hittest = cursor_hittest;
        }
        if next.scale_percent != self.options.scale_percent {
            let (w, h) = model_window_dimensions(self.frame.snapshot.canvas, next.scale_percent);
            let _ = self
                .state
                .window
                .request_inner_size(winit::dpi::LogicalSize::new(w, h));
        }
        self.options = next;
        if let Some(frame) = self.consumer.take_latest() {
            if frame.model_commit.is_some() {
                match self.renderer.prepare(&frame) {
                    Ok(()) => feedback(
                        &self.runtime,
                        &self.consumer,
                        &frame,
                        ModelCommitOutcome::Prepared,
                    )?,
                    Err(_) => {
                        self.rejections += 1;
                        feedback(
                            &self.runtime,
                            &self.consumer,
                            &frame,
                            ModelCommitOutcome::Rejected(
                                ModelCommitErrorCode::ResourcePreparationFailed,
                            ),
                        )?;
                        return Ok(OverlayTickOutcome::Deferred(Duration::from_millis(16)));
                    }
                }
            }
            if self.frame.snapshot.canvas != frame.snapshot.canvas {
                let (w, h) =
                    model_window_dimensions(frame.snapshot.canvas, self.options.scale_percent);
                let _ = self
                    .state
                    .window
                    .request_inner_size(winit::dpi::LogicalSize::new(w, h));
            }
            self.frame = frame;
            self.snapshots += 1;
        }
        let size = self.state.window.inner_size();
        self.renderer.resize(size.width, size.height);
        self.state.window.pre_present_notify();
        let outcome = self.renderer.draw(
            &self.frame,
            self.options,
            snapshot.overlay_visible,
            self.idle.visible() as f32,
        )?;
        if outcome == OverlayTickOutcome::Presented {
            self.frames += 1;
        }
        Ok(outcome)
    }
    pub fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        Err(OverlayError::new(
            "Wayland does not expose global window placement",
        ))
    }
    pub fn is_visible(&self) -> bool {
        !self.state.closed && self.runtime.snapshot().overlay_visible
    }
    pub fn model_generation(&self) -> u64 {
        self.frame.model_generation
    }
    pub fn stop_input(&mut self) -> Result<(), OverlayError> {
        if let Some(mut input) = self.input.take() {
            self.diagnostics = Some(input.stop().map_err(err)?);
        }
        Ok(())
    }
    pub fn finish_after_runtime_shutdown(mut self) -> Result<ProductOverlayReport, OverlayError> {
        self.stop_input()?;
        Ok(ProductOverlayReport {
            frames_presented: self.frames,
            placement_fully_visible: false,
            dynamic_snapshots: self.snapshots,
            model_commit_rejections: self.rejections,
            input_start_error: self.input_error,
            input_diagnostics: self.diagnostics,
            render_diagnostics: self.consumer.diagnostics(),
            model_generation: self.frame.model_generation,
            drawable_count: self.frame.snapshot.drawables.len(),
            masked_drawable_count: self
                .frame
                .snapshot
                .drawables
                .iter()
                .filter(|d| !d.masks.is_empty())
                .count(),
            texture_count: self.frame.resources.textures.len(),
        })
    }
}
fn err(e: impl std::fmt::Display) -> OverlayError {
    OverlayError::new(e.to_string())
}
fn feedback(
    runtime: &RuntimeClient,
    consumer: &RenderConsumer,
    frame: &RenderFrame,
    outcome: ModelCommitOutcome,
) -> Result<(), OverlayError> {
    if let Some(token) = frame.model_commit {
        consumer
            .report_model_commit(ModelCommitFeedback { token, outcome })
            .map_err(err)?;
        let completed = runtime
            .wait_for_command(token.command_sequence, Duration::from_secs(2))
            .ok_or_else(|| OverlayError::new("model commit did not finish"))?;
        let failure = completed
            .last_command_failure
            .filter(|f| f.sequence == token.command_sequence);
        match (outcome, failure) {
            (ModelCommitOutcome::Prepared, None) => {}
            (
                ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
                Some(f),
            ) if f.code == bongocat_runtime::RuntimeRenderErrorCode::GpuPreparationFailed => {}
            _ => {
                return Err(OverlayError::new(
                    "runtime and renderer disagree on model commit",
                ));
            }
        }
    }
    Ok(())
}
