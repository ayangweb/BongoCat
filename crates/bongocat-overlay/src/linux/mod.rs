use super::*;
use crate::idle::{IdleHide, IdleObservation};
use crate::resize_drag::{ResizeBase, ResizeDrag};
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
mod layer_shell;
mod renderer;
mod window;
pub(crate) use cover_capture::CoverCaptureSession;
use layer_shell::{LayerWindow, PointerInput};
use window::WindowTarget;

struct WindowEvents {
    window: Arc<WindowTarget>,
    closed: bool,
    pointer: (f64, f64),
    resize_base: ResizeBase,
    drag: Option<ResizeDrag>,
    resized_scale: Option<u16>,
    sinks: OverlayInteractionSinks,
}
impl ApplicationHandler for WindowEvents {
    fn resumed(&mut self, _: &ActiveEventLoop) {}
    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let WindowTarget::Ordinary(window) = self.window.as_ref() else {
            return;
        };
        if id != window.id() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.closed = true,
            WindowEvent::Focused(false) => self.input(PointerInput::Cancel),
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f64>(self.window.scale_factor());
                self.input(PointerInput::Motion((logical.x, logical.y)));
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.input(PointerInput::LeftPressed),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => self.input(PointerInput::Right(state == ElementState::Pressed)),
            _ => {}
        }
    }
}
impl WindowEvents {
    fn input(&mut self, event: PointerInput) {
        match event {
            PointerInput::Cancel => self.finish_drag(),
            PointerInput::LeftPressed => self.window.drag_window(),
            PointerInput::Motion(position) => {
                self.pointer = position;
                if let Some(drag) = &mut self.drag
                    && let Some(resize) = drag.observe(position)
                {
                    self.resized_scale = Some(resize.scale_percent);
                    self.window.request_inner_size(winit::dpi::LogicalSize::new(
                        resize.width,
                        resize.height,
                    ));
                }
            }
            PointerInput::Right(true) => {
                let width = self
                    .window
                    .inner_size()
                    .to_logical::<f64>(self.window.scale_factor())
                    .width
                    .round() as u32;
                let scale = self.resize_base.scale_percent_for_width(width);
                self.drag = Some(ResizeDrag::begin(self.pointer, self.resize_base, scale));
            }
            PointerInput::Right(false) => {
                if self.drag.as_ref().is_some_and(|drag| !drag.dragging())
                    && let Some(sender) = &self.sinks.context_menu_sender
                {
                    let _ = sender.try_send(OverlayContextMenuRequest);
                }
                self.finish_drag();
            }
        }
    }
    fn finish_drag(&mut self) {
        if let Some(drag) = self.drag.take()
            && let Some(scale_percent) = drag.finish()
            && let Some(sender) = &self.sinks.resize_sender
        {
            let _ = sender.try_send(OverlayResizeOutcome { scale_percent });
        }
    }
}

pub struct ProductOverlaySession {
    renderer: renderer::Renderer,
    events: EventLoop<()>,
    layer: Option<LayerWindow>,
    layer_shell_available: bool,
    layer_bounds: Option<OverlayWindowBounds>,
    state: WindowEvents,
    runtime: RuntimeClient,
    consumer: RenderConsumer,
    frame: RenderFrame,
    input: Option<bongocat_platform::LinuxInputService>,
    input_sources: Option<(InputProducer, CursorProducer, GamepadAxisProducer)>,
    pending_scale: Option<u16>,
    configured_scale: u16,
    options: OverlaySessionOptions,
    idle: IdleHide,
    session_started: Instant,
    cursor_hittest: bool,
    context_menu_active: bool,
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
        sinks: OverlayInteractionSinks,
    ) -> Result<Self, OverlayError> {
        let frame = consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime published no initial frame"))?;
        let mut builder = EventLoop::builder();
        builder.with_wayland().with_any_thread(true);
        let events = builder.build().map_err(err)?;
        let (width, height) = model_window_dimensions(frame.snapshot.canvas, options.scale_percent);
        let layer_shell_available = LayerWindow::available()?;
        let layer = if options.always_on_top && layer_shell_available {
            Some(LayerWindow::create(
                width,
                height,
                options.window_bounds,
                options.keep_inside_screen,
            )?)
        } else {
            None
        };
        let window = Arc::new(match &layer {
            Some(layer) => WindowTarget::Layer(layer.target.clone()),
            None => WindowTarget::Ordinary(create_ordinary_window(&events, width, height)?),
        });
        window.set_cursor_hittest(!options.click_through)?;
        let size = window.inner_size();
        let renderer =
            match renderer::Renderer::new(Some(window.clone()), &frame, size.width, size.height) {
                Ok(renderer) => renderer,
                Err(e) => {
                    feedback(
                        &runtime,
                        &consumer,
                        &frame,
                        ModelCommitOutcome::Rejected(
                            ModelCommitErrorCode::ResourcePreparationFailed,
                        ),
                    )?;
                    return Err(e);
                }
            };
        feedback(&runtime, &consumer, &frame, ModelCommitOutcome::Prepared)?;
        let (base_width, base_height) = model_window_dimensions(frame.snapshot.canvas, 100);
        Ok(Self {
            renderer,
            events,
            layer,
            layer_shell_available,
            layer_bounds: options.window_bounds,
            state: WindowEvents {
                window,
                closed: false,
                pointer: (0.0, 0.0),
                resize_base: ResizeBase::new(f64::from(base_width), f64::from(base_height))
                    .expect("model dimensions are positive"),
                drag: None,
                resized_scale: None,
                sinks,
            },
            runtime,
            consumer,
            frame,
            input: None,
            input_sources: Some((producer, cursor, axes)),
            pending_scale: None,
            configured_scale: options.scale_percent,
            options,
            idle: IdleHide::default(),
            session_started: Instant::now(),
            cursor_hittest: !options.click_through,
            context_menu_active: false,
            frames: 0,
            snapshots: 0,
            rejections: 0,
            input_error: None,
            diagnostics: None,
        })
    }
    /// Only the application's explicit confirmation starts the privileged helper.
    pub fn start_input(&mut self) {
        let Some((producer, cursor, axes)) = self.input_sources.take() else {
            return;
        };
        let diagnostics = self.runtime.platform_input_diagnostics_producer();
        let (input, error) = start_platform_input(&diagnostics, || {
            bongocat_platform::LinuxInputService::start_with_diagnostics(
                producer,
                cursor,
                axes,
                diagnostics.clone(),
            )
        });
        self.input = input;
        self.input_error = error;
    }
    pub fn always_on_top_available(&self) -> bool {
        self.layer_shell_available
    }
    pub fn set_context_menu_active(&mut self, active: bool) {
        self.context_menu_active = active;
    }
    pub fn close_requested(&self) -> bool {
        self.state.closed
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
        if let Some(layer) = &mut self.layer {
            let (closed, inputs) = layer.pump()?;
            self.state.closed |= closed;
            for input in inputs {
                self.state.input(input);
            }
        }
        if self.state.closed {
            return Ok(OverlayTickOutcome::Hidden);
        }
        let snapshot = self.runtime.snapshot();
        let mut next = self
            .options
            .with_runtime_settings(snapshot.overlay_settings);
        if let Some(scale) = self.state.resized_scale.take() {
            self.pending_scale = Some(scale);
        }
        if let Some(scale) = self.pending_scale {
            if next.scale_percent == scale || next.scale_percent != self.configured_scale {
                self.pending_scale = None;
            } else {
                next.scale_percent = scale;
            }
        }
        self.apply_window_backend(next)?;
        if let Some(layer) = &self.layer {
            layer.target.set_keep_inside(next.keep_inside_screen);
        }
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
        self.configured_scale = snapshot.overlay_settings.scale_percent;
        // An ordinary GPUI menu is below the layer surface. Leave that surface
        // empty and click-through while its application-owned menu is open.
        let menu_covers_layer = self.context_menu_active && self.layer.is_some();
        let cursor_hittest = snapshot.overlay_visible
            && !menu_covers_layer
            && (!next.click_through || held)
            && !self.idle.hidden();
        if cursor_hittest != self.cursor_hittest {
            if !cursor_hittest {
                self.state.finish_drag();
                if let Some(layer) = &mut self.layer {
                    layer.cancel_move();
                }
            }
            self.state
                .window
                .set_cursor_hittest(cursor_hittest)
                .map_err(err)?;
            self.cursor_hittest = cursor_hittest;
        }
        if next.scale_percent != self.options.scale_percent {
            let (w, h) = model_window_dimensions(self.frame.snapshot.canvas, next.scale_percent);
            self.state
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
            if self.frame.model_generation != frame.model_generation {
                self.state.finish_drag();
                self.pending_scale = None;
                let (base_width, base_height) = model_window_dimensions(frame.snapshot.canvas, 100);
                self.state.resize_base =
                    ResizeBase::new(f64::from(base_width), f64::from(base_height))
                        .expect("model dimensions are positive");
            }
            if self.frame.snapshot.canvas != frame.snapshot.canvas {
                let (w, h) =
                    model_window_dimensions(frame.snapshot.canvas, self.options.scale_percent);
                self.state
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
            snapshot.overlay_visible && !menu_covers_layer,
            self.idle.visible() as f32,
        )?;
        if outcome == OverlayTickOutcome::Presented {
            self.frames += 1;
            if let Some(layer) = &mut self.layer {
                layer.presented();
            }
        }
        Ok(outcome)
    }
    fn apply_window_backend(&mut self, options: OverlaySessionOptions) -> Result<(), OverlayError> {
        let use_layer = options.always_on_top && self.layer_shell_available;
        if use_layer == self.layer.is_some() {
            return Ok(());
        }
        self.state.finish_drag();
        let (width, height) =
            model_window_dimensions(self.frame.snapshot.canvas, options.scale_percent);
        let layer = if use_layer {
            Some(LayerWindow::create(
                width,
                height,
                self.layer_bounds,
                options.keep_inside_screen,
            )?)
        } else {
            None
        };
        let target = Arc::new(match &layer {
            Some(layer) => WindowTarget::Layer(layer.target.clone()),
            None => WindowTarget::Ordinary(create_ordinary_window(&self.events, width, height)?),
        });
        target.set_cursor_hittest(self.cursor_hittest)?;
        let size = target.inner_size();
        self.renderer
            .replace_window(target.clone(), size.width, size.height)?;
        if let Some(old) = &self.layer
            && let Ok(bounds) = old.target.bounds()
        {
            self.layer_bounds = Some(bounds);
        }
        self.state.window = target;
        self.state.pointer = (0., 0.);
        self.layer = layer;
        Ok(())
    }
    pub fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        match &self.layer {
            Some(layer) => layer.target.bounds(),
            None => Err(err("Wayland does not expose ordinary window placement")),
        }
    }
    pub fn is_visible(&self) -> bool {
        !self.state.closed && self.runtime.snapshot().overlay_visible
    }
    pub fn model_generation(&self) -> u64 {
        self.frame.model_generation
    }
    pub fn stop_input(&mut self) -> Result<(), OverlayError> {
        self.input_sources.take();
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

fn create_ordinary_window(
    events: &EventLoop<()>,
    width: u32,
    height: u32,
) -> Result<Arc<Window>, OverlayError> {
    #[allow(deprecated)]
    events
        .create_window(
            Window::default_attributes()
                .with_title("BongoCat")
                .with_decorations(false)
                .with_transparent(true)
                .with_inner_size(winit::dpi::LogicalSize::new(width, height)),
        )
        .map(Arc::new)
        .map_err(err)
}
