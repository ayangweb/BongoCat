use super::*;
use crate::idle::{IdleHide, IdleObservation};
use crate::resize_drag::{ResizeBase, ResizeDrag};
use bongocat_render::{ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome, RenderFrame};
use std::time::Instant;
mod alpha_output;
pub use alpha_output::AlphaOutputStatus;
mod context_menu;
mod cover_capture;
// The assembly layer wraps the runtime's producer with this before the producer
// reaches this session, so the module has to be reachable from outside.
pub mod input_tee;
mod renderer;
mod wayland;
use bongocat_platform::{SystemMenuAction, SystemMenuPalette, SystemMenuPresentation};
use context_menu::PopupTrigger;
pub(crate) use cover_capture::CoverCaptureSession;
use wayland::{PointerInput, WaylandWindow, WindowTarget};

enum MenuInteraction {
    Open(PopupTrigger),
    Reveal,
    Dismiss,
}

struct WindowEvents {
    window: Arc<WindowTarget>,
    closed: bool,
    pointer: (f64, f64),
    resize_base: ResizeBase,
    drag: Option<ResizeDrag>,
    resized_scale: Option<u16>,
    sinks: OverlayInteractionSinks,
}
impl WindowEvents {
    fn input(&mut self, event: PointerInput) -> Option<MenuInteraction> {
        match event {
            PointerInput::Cancel => {
                self.finish_drag();
                return Some(MenuInteraction::Dismiss);
            }
            PointerInput::Motion(position) => {
                self.pointer = position;
                if let Some(drag) = &mut self.drag
                    && let Some(resize) = drag.observe(position)
                {
                    self.resized_scale = Some(resize.scale_percent);
                    self.window.resize(resize.width, resize.height);
                    return Some(MenuInteraction::Dismiss);
                }
            }
            PointerInput::RightPressed(trigger) => {
                self.pointer = trigger.position;
                let width = (f64::from(self.window.physical_size().0) / self.window.scale_factor())
                    .round() as u32;
                let scale = self.resize_base.scale_percent_for_width(width);
                self.drag = Some(ResizeDrag::begin(self.pointer, self.resize_base, scale));
                return Some(MenuInteraction::Open(trigger));
            }
            PointerInput::RightReleased => {
                let clicked = self.drag.as_ref().is_some_and(|drag| !drag.dragging());
                self.finish_drag();
                if clicked {
                    return Some(MenuInteraction::Reveal);
                }
            }
        }
        None
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
    display_selected: bool,
    alpha_output: Option<alpha_output::AlphaOutput>,
    alpha_peer_active: bool,
    alpha_subscription: Option<std::sync::Arc<bongocat_render::RenderSubscription>>,
    alpha_status: AlphaOutputStatus,
    alpha_requested: bool,
    alpha_environment: bongocat_config::BuildEnvironment,
    alpha_runtime_status: bongocat_runtime::VideoOutputStatus,
    renderer: renderer::Renderer,
    window: WaylandWindow,
    menu_presentation: Option<(SystemMenuPresentation, SystemMenuPalette)>,
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
        Self::start_with_instance(
            runtime, producer, cursor, axes, consumer, options, sinks, None, false,
        )
    }

    pub(super) fn start_peer(
        &self,
        runtime: RuntimeClient,
        consumer: RenderConsumer,
        mut options: OverlaySessionOptions,
        saved_bounds: Option<OverlayWindowBounds>,
    ) -> Result<Self, OverlayError> {
        options.window_bounds = saved_bounds
            .or_else(|| self.window_bounds().ok())
            .or(options.window_bounds);
        Self::start_with_instance(
            runtime.clone(),
            runtime.input_producer(),
            runtime.cursor_producer(),
            runtime.gamepad_axis_producer(),
            consumer,
            options,
            OverlayInteractionSinks {
                context_menu_sender: None,
                resize_sender: None,
                hud_consumer: None,
                hud_interaction_sender: None,
                hud_viewport_reporter: None,
            },
            Some(self.renderer.instance()),
            saved_bounds.is_none(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with_instance(
        runtime: RuntimeClient,
        producer: InputProducer,
        cursor: CursorProducer,
        axes: GamepadAxisProducer,
        consumer: RenderConsumer,
        options: OverlaySessionOptions,
        sinks: OverlayInteractionSinks,
        peer_instance: Option<wgpu::Instance>,
        place_beside_primary: bool,
    ) -> Result<Self, OverlayError> {
        let frame = consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime published no initial frame"))?;
        let (width, height) = model_window_dimensions(frame.snapshot.canvas, options.scale_percent);
        let layer_shell_available = WaylandWindow::available()?;
        let native = WaylandWindow::create(
            width,
            height,
            options.always_on_top && layer_shell_available,
            options.window_bounds,
            options.keep_inside_screen,
            place_beside_primary,
        )?;
        let window = native.target.clone();
        window.set_cursor_hittest(!options.click_through)?;
        let size = window.physical_size();
        let renderer = match peer_instance {
            Some(instance) => renderer::Renderer::with_instance(
                instance,
                Some(window.clone()),
                &frame,
                size.0,
                size.1,
            ),
            None => renderer::Renderer::new(Some(window.clone()), &frame, size.0, size.1),
        };
        let renderer = match renderer {
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
        let (base_width, base_height) = model_window_dimensions(frame.snapshot.canvas, 100);
        Ok(Self {
            display_selected: true,
            alpha_output: None,
            alpha_subscription: None,
            alpha_peer_active: false,
            alpha_status: AlphaOutputStatus::Disabled,
            alpha_requested: false,
            alpha_environment: bongocat_config::BuildEnvironment::Development,
            alpha_runtime_status: bongocat_runtime::VideoOutputStatus::Disabled,
            renderer,
            window: native,
            menu_presentation: None,
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
    pub fn set_alpha_output_enabled(
        &mut self,
        enabled: bool,
        environment: bongocat_config::BuildEnvironment,
    ) {
        self.alpha_environment = environment;
        self.alpha_requested = enabled;
        if enabled && self.alpha_output.is_none() {
            match alpha_output::AlphaOutput::start(
                self.frame.clone(),
                self.alpha_environment,
                self.renderer.instance(),
                self.consumer.subscribe_latest(),
            ) {
                Ok(output) => {
                    self.alpha_output = Some(output);
                    self.alpha_status = AlphaOutputStatus::Starting;
                }
                Err(_) => self.alpha_status = AlphaOutputStatus::WorkerFailed,
            }
        } else if enabled {
            if self
                .alpha_output
                .as_ref()
                .is_some_and(|output| output.is_stopping())
            {
                self.alpha_status = AlphaOutputStatus::Starting;
            }
        } else if let Some(output) = &self.alpha_output {
            output.request_stop();
            self.alpha_status = AlphaOutputStatus::Stopping;
        } else {
            self.alpha_status = AlphaOutputStatus::Disabled;
        }
        self.sync_video_output_state();
    }

    pub(super) fn sync_alpha_scene<'a>(
        &self,
        primary_selected: bool,
        peers: impl IntoIterator<Item = (&'a mut Self, bool)>,
    ) {
        let output = self
            .alpha_output
            .as_ref()
            .filter(|output| !output.is_stopping());
        let mut sources = Vec::with_capacity(3);
        if primary_selected && let Some(output) = output {
            sources.push(self.alpha_source(output.primary_source(), 0));
        }
        for (index, (peer, selected)) in peers.into_iter().enumerate() {
            peer.alpha_peer_active = output.is_some() && selected && peer.display_selected;
            peer.sync_video_output_state();
            if output.is_some() && selected {
                let subscription = peer
                    .alpha_subscription
                    .get_or_insert_with(|| std::sync::Arc::new(peer.consumer.subscribe_latest()))
                    .clone();
                sources.push(peer.alpha_source(subscription, index + 1));
            } else {
                peer.alpha_subscription = None;
            }
        }
        if let Some(output) = output {
            output.set_scene(sources);
        }
    }

    fn alpha_source(
        &self,
        subscription: std::sync::Arc<bongocat_render::RenderSubscription>,
        index: usize,
    ) -> alpha_output::AlphaSource {
        let bounds = self.window_bounds().unwrap_or_else(|_| {
            let (width, height) =
                model_window_dimensions(self.frame.snapshot.canvas, self.options.scale_percent);
            OverlayWindowBounds::new((index as i32) * (width as i32 + 16), 0, width, height)
        });
        alpha_output::AlphaSource {
            initial: self.frame.clone(),
            subscription,
            bounds,
            visible: self.display_selected,
        }
    }

    fn sync_video_output_state(&mut self) {
        use bongocat_platform::AlphaCameraError;
        use bongocat_runtime::VideoOutputStatus as Status;
        let status = match self.alpha_status {
            AlphaOutputStatus::Disabled if self.alpha_peer_active => Status::Running,
            AlphaOutputStatus::Disabled => Status::Disabled,
            AlphaOutputStatus::Starting => Status::Starting,
            AlphaOutputStatus::Running => Status::Running,
            AlphaOutputStatus::Stopping => Status::Stopping,
            AlphaOutputStatus::CameraFailed(AlphaCameraError::Unavailable) => {
                Status::DeviceUnavailable
            }
            AlphaOutputStatus::CameraFailed(AlphaCameraError::PermissionDenied) => {
                Status::PermissionDenied
            }
            AlphaOutputStatus::CameraFailed(AlphaCameraError::UnsupportedFormat) => {
                Status::UnsupportedFormat
            }
            AlphaOutputStatus::CameraFailed(_)
            | AlphaOutputStatus::RenderFailed
            | AlphaOutputStatus::WorkerFailed => Status::Failed,
        };
        if status != self.alpha_runtime_status
            && self
                .runtime
                .send(bongocat_runtime::RuntimeCommand::SetVideoOutputStatus(
                    status,
                ))
                .is_ok()
        {
            self.alpha_runtime_status = status;
        }
    }

    pub fn alpha_output_status(&mut self) -> AlphaOutputStatus {
        if let Some(output) = &self.alpha_output {
            let stopping = output.is_stopping();
            if !stopping && let Some(status) = output.latest_status() {
                self.alpha_status = status;
            }
            if output.is_finished() {
                self.alpha_output = None;
                if stopping && self.alpha_requested {
                    self.set_alpha_output_enabled(true, self.alpha_environment);
                } else if !self.alpha_requested {
                    self.alpha_status = AlphaOutputStatus::Disabled;
                } else if matches!(
                    self.alpha_status,
                    AlphaOutputStatus::Starting | AlphaOutputStatus::Running
                ) {
                    self.alpha_status = AlphaOutputStatus::WorkerFailed;
                }
            }
        }
        self.sync_video_output_state();
        self.alpha_status
    }

    pub fn always_on_top_available(&self) -> bool {
        self.layer_shell_available
    }
    pub fn set_menu_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
        palette: SystemMenuPalette,
    ) {
        self.window.set_presentation(presentation.clone(), palette);
        self.menu_presentation = Some((presentation, palette));
    }
    pub fn take_menu_action(&mut self) -> Option<SystemMenuAction> {
        self.window.take_menu_action()
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
        self.tick_with_window_behavior(None)
    }

    pub(super) fn tick_with_window_behavior(
        &mut self,
        behavior: Option<(
            bongocat_runtime::OverlaySettings,
            bongocat_input::PressedModifiers,
            bool,
        )>,
    ) -> Result<OverlayTickOutcome, OverlayError> {
        let (closed, inputs) = self.window.pump()?;
        self.state.closed |= closed;
        for input in inputs {
            match self.state.input(input) {
                Some(MenuInteraction::Open(trigger)) => self.window.open_context_menu(trigger)?,
                Some(MenuInteraction::Reveal) => self.window.reveal_context_menu()?,
                Some(MenuInteraction::Dismiss) => self.window.dismiss_context_menu(),
                None => {}
            }
        }
        if self.state.closed {
            return Ok(OverlayTickOutcome::Hidden);
        }
        let snapshot = self.runtime.snapshot();
        let (window_settings, local_modifiers, input_available) = behavior.unwrap_or((
            snapshot.overlay_settings,
            snapshot.input.pressed_modifiers,
            snapshot.platform_input.service_status == PlatformInputServiceStatus::Running,
        ));
        let mut next = self.options.with_runtime_settings(window_settings);
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
        self.window.target.set_keep_inside(next.keep_inside_screen);
        let held = next.hold_modifier_pressed(local_modifiers);
        self.idle.observe(IdleObservation {
            enabled: next.hide_on_idle && input_available,
            delay: Duration::from_millis(u64::from(next.hide_on_idle_delay_ms)),
            input_sequence: snapshot.input.last_input_sequence,
            cursor_at: snapshot.cursor.sample.map(|sample| sample.at),
            gamepad_axis_published: snapshot.gamepad_axis_transport.published,
            now: self.session_started.elapsed(),
        });
        self.configured_scale = window_settings.scale_percent;
        let visible = self.display_selected && snapshot.overlay_visible;
        let cursor_hittest = visible && (!next.click_through || held) && !self.idle.hidden();
        if cursor_hittest != self.cursor_hittest {
            if !cursor_hittest {
                self.state.finish_drag();
                self.window.cancel_move();
            }
            self.state
                .window
                .set_cursor_hittest(cursor_hittest)
                .map_err(err)?;
            self.cursor_hittest = cursor_hittest;
        }
        if next.scale_percent != self.options.scale_percent {
            let (w, h) = model_window_dimensions(self.frame.snapshot.canvas, next.scale_percent);
            self.state.window.resize(w, h);
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
                self.state.window.resize(w, h);
            }
            self.frame = frame;
            self.snapshots += 1;
        }
        let _ = self.alpha_output_status();
        // Do not acquire another swapchain image until the compositor asks for
        // a new frame. An occluded Wayland surface can otherwise block the
        // foreground loop and starve the independently running Alpha output.
        if self.window.frame_pending() {
            return Ok(OverlayTickOutcome::Deferred(Duration::from_millis(16)));
        }
        let size = self.state.window.physical_size();
        self.renderer.resize(size.0, size.1);
        let outcome = self.renderer.draw(
            &self.frame,
            self.options,
            visible,
            self.idle.visible() as f32,
        )?;
        if outcome == OverlayTickOutcome::Presented {
            self.frames += 1;
            self.window.presented();
        }
        Ok(outcome)
    }
    fn apply_window_backend(&mut self, options: OverlaySessionOptions) -> Result<(), OverlayError> {
        let use_layer = options.always_on_top && self.layer_shell_available;
        if use_layer == self.window.is_layer() {
            return Ok(());
        }
        self.state.finish_drag();
        let (width, height) =
            model_window_dimensions(self.frame.snapshot.canvas, options.scale_percent);
        if self.window.is_layer()
            && let Ok(bounds) = self.window.target.bounds()
        {
            self.layer_bounds = Some(bounds);
        }
        let mut window = WaylandWindow::create(
            width,
            height,
            use_layer,
            self.layer_bounds,
            options.keep_inside_screen,
            false,
        )?;
        window.target.set_cursor_hittest(self.cursor_hittest)?;
        if let Some((presentation, palette)) = &self.menu_presentation {
            window.set_presentation(presentation.clone(), *palette);
        }
        let size = window.target.physical_size();
        self.renderer
            .replace_window(window.target.clone(), size.0, size.1)?;
        self.state.window = window.target.clone();
        self.state.pointer = (0., 0.);
        self.window = window;
        Ok(())
    }
    pub(super) fn restore_position(&self, x: i32, y: i32) {
        self.window.target.restore_position(x, y);
    }

    pub fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        if self.window.is_layer() {
            self.window.target.bounds()
        } else {
            Err(err("Wayland does not expose ordinary window placement"))
        }
    }
    pub fn is_visible(&self) -> bool {
        self.display_selected && !self.state.closed && self.runtime.snapshot().overlay_visible
    }
    pub(super) fn set_display_selected(&mut self, selected: bool) {
        self.display_selected = selected;
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
