//! Product session and Wayland window lifecycle.

use super::*;
use crate::idle::{IdleHide, IdleObservation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveOverlayBackend {
    Xdg,
    Layer,
}

struct NativeOverlay {
    active: ActiveOverlayBackend,
    xdg: Option<SctkXdgOverlay>,
    layer: Option<LayerOverlay>,
    layer_bounds: Option<OverlayWindowBounds>,
    layer_shell_available: Option<bool>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    system_menu_presentation: Option<bongocat_platform::SystemMenuPresentation>,
    system_menu_actions: std::collections::VecDeque<bongocat_platform::SystemMenuAction>,
}

impl NativeOverlay {
    fn create(
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
        _context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> Result<Self, OverlayError> {
        if options.always_on_top
            && let Some(layer) =
                LayerOverlay::create_optional(frame, options, bounds, resize_sender.clone(), None)?
        {
            return Ok(Self {
                active: ActiveOverlayBackend::Layer,
                xdg: None,
                layer_bounds: Some(layer.bounds()),
                layer: Some(layer),
                layer_shell_available: Some(true),
                resize_sender,
                system_menu_presentation: None,
                system_menu_actions: std::collections::VecDeque::new(),
            });
        }
        let xdg = SctkXdgOverlay::create(frame, options, bounds, resize_sender.clone())?;
        let layer_shell_available = if options.always_on_top {
            false
        } else {
            LayerOverlay::available()?
        };
        Ok(Self {
            active: ActiveOverlayBackend::Xdg,
            xdg: Some(xdg),
            layer: None,
            layer_bounds: bounds,
            layer_shell_available: Some(layer_shell_available),
            resize_sender,
            system_menu_presentation: None,
            system_menu_actions: std::collections::VecDeque::new(),
        })
    }

    fn apply_backend(
        &mut self,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
    ) -> Result<(), OverlayError> {
        let requested = if options.always_on_top {
            ActiveOverlayBackend::Layer
        } else {
            ActiveOverlayBackend::Xdg
        };
        if requested == ActiveOverlayBackend::Layer && self.layer_shell_available == Some(false) {
            return Ok(());
        }
        if requested == self.active {
            if let Some(layer) = self.layer.as_mut() {
                layer.set_keep_inside_screen(options.keep_inside_screen);
            }
            return Ok(());
        }

        let was_visible = self.is_visible();
        match requested {
            ActiveOverlayBackend::Layer => {
                let current = self.bounds();
                let bounds = self
                    .layer_bounds
                    .map(|saved| {
                        OverlayWindowBounds::new(saved.x, saved.y, current.width, current.height)
                    })
                    .unwrap_or(current);
                let mut layer = LayerOverlay::create_optional(
                    frame,
                    options,
                    Some(bounds),
                    self.resize_sender.clone(),
                    None,
                )?
                .ok_or_else(|| {
                    OverlayError::new(
                        "Wayland layer shell disappeared before enabling always-on-top",
                    )
                })?;
                layer.set_keep_inside_screen(options.keep_inside_screen);
                if let Some(presentation) = self.system_menu_presentation.clone() {
                    layer.set_system_menu_presentation(presentation);
                }
                if was_visible {
                    layer.prepare_show(options)?;
                }
                self.layer_bounds = Some(layer.bounds());
                self.layer = Some(layer);
                self.xdg = None;
                self.active = ActiveOverlayBackend::Layer;
            }
            ActiveOverlayBackend::Xdg => {
                let bounds = self.bounds();
                self.layer_bounds = Some(bounds);
                let mut xdg = SctkXdgOverlay::create(
                    frame,
                    options,
                    Some(bounds),
                    self.resize_sender.clone(),
                )?;
                if was_visible {
                    xdg.prepare_show(options)?;
                }
                if let Some(presentation) = self.system_menu_presentation.clone() {
                    xdg.set_system_menu_presentation(presentation);
                }
                self.xdg = Some(xdg);
                self.layer = None;
                self.active = ActiveOverlayBackend::Xdg;
            }
        }
        Ok(())
    }

    fn recover_presentation_surface(
        &mut self,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        prepare_visible: bool,
    ) -> Result<(), OverlayError> {
        let bounds = self.bounds();
        match self.active {
            ActiveOverlayBackend::Xdg => {
                let mut replacement = SctkXdgOverlay::create(
                    frame,
                    options,
                    Some(bounds),
                    self.resize_sender.clone(),
                )?;
                if prepare_visible {
                    replacement.prepare_show(options)?;
                }
                if let Some(presentation) = self.system_menu_presentation.clone() {
                    replacement.set_system_menu_presentation(presentation);
                }
                self.xdg = Some(replacement);
            }
            ActiveOverlayBackend::Layer => {
                let mut replacement = LayerOverlay::create_optional(
                    frame,
                    options,
                    Some(bounds),
                    self.resize_sender.clone(),
                    None,
                )?
                .ok_or_else(|| {
                    OverlayError::new(
                        "Wayland layer shell disappeared while recovering the overlay surface",
                    )
                })?;
                replacement.set_keep_inside_screen(options.keep_inside_screen);
                if let Some(presentation) = self.system_menu_presentation.clone() {
                    replacement.set_system_menu_presentation(presentation);
                }
                if prepare_visible {
                    replacement.prepare_show(options)?;
                }
                self.layer_bounds = Some(replacement.bounds());
                self.layer = Some(replacement);
            }
        }
        Ok(())
    }

    fn pump_events(
        &mut self,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
    ) -> Result<bool, OverlayError> {
        let closed = match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .pump_events()?,
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .pump_events()?,
        };
        while let Some(action) = match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .take_system_menu_action(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .take_system_menu_action(),
        } {
            self.system_menu_actions.push_back(action);
        }
        if self.active == ActiveOverlayBackend::Layer && !closed {
            let relocation = self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .take_relocation();
            if let Some(relocation) = relocation
                && let Ok(Some(mut replacement)) = LayerOverlay::create_optional(
                    frame,
                    options,
                    Some(relocation.bounds),
                    self.resize_sender.clone(),
                    Some(relocation.output),
                )
            {
                replacement.set_keep_inside_screen(options.keep_inside_screen);
                if let Some(presentation) = self.system_menu_presentation.clone() {
                    replacement.set_system_menu_presentation(presentation);
                }
                self.layer_bounds = Some(replacement.bounds());
                self.layer = Some(replacement);
                return Ok(false);
            }
        }
        if self.active == ActiveOverlayBackend::Xdg || !closed {
            return Ok(closed);
        }

        let bounds = self
            .layer
            .as_ref()
            .expect("closed layer overlay is present")
            .bounds();
        self.layer = None;
        let mut replacement = LayerOverlay::create_optional(
            frame,
            options,
            Some(bounds),
            self.resize_sender.clone(),
            None,
        )?
        .ok_or_else(|| {
            OverlayError::new("Wayland layer shell disappeared while recreating the overlay")
        })?;
        replacement.set_keep_inside_screen(options.keep_inside_screen);
        if let Some(presentation) = self.system_menu_presentation.clone() {
            replacement.set_system_menu_presentation(presentation);
        }
        self.layer_bounds = Some(replacement.bounds());
        self.layer = Some(replacement);
        Ok(false)
    }

    fn draw(&mut self, verify: bool) -> Result<(), OverlayError> {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .draw(verify),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .draw(verify),
        }
    }

    fn set_visible(
        &mut self,
        visible: bool,
        options: OverlaySessionOptions,
    ) -> Result<(), OverlayError> {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .set_visible(visible, options),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .set_visible(visible, options),
        }
    }

    fn prepare_show(&mut self, options: OverlaySessionOptions) -> Result<(), OverlayError> {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .prepare_show(options),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .prepare_show(options),
        }
    }

    fn set_click_through(&mut self, click_through: bool) -> Result<(), OverlayError> {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .set_click_through(click_through),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .set_click_through(click_through),
        }
    }

    fn resize(&mut self, bounds: OverlayWindowBounds) {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .resize(bounds),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .resize(bounds),
        }
    }

    fn set_resize_base(&mut self, canvas: CanvasInfo) {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .set_resize_base(canvas),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .set_resize_base(canvas),
        }
    }

    fn bounds(&self) -> OverlayWindowBounds {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_ref()
                .expect("active xdg overlay is present")
                .bounds(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_ref()
                .expect("active layer overlay is present")
                .bounds(),
        }
    }

    fn renderer(&self) -> &Renderer {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_ref()
                .expect("active xdg overlay is present")
                .renderer(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_ref()
                .expect("active layer overlay is present")
                .renderer(),
        }
    }

    fn renderer_mut(&mut self) -> &mut Renderer {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .renderer_mut(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .renderer_mut(),
        }
    }

    fn verify_composition(&mut self) -> Result<(), OverlayError> {
        self.renderer_mut().verify_composition()
    }

    fn scale_transition_bounds(
        &self,
        previous_percent: u16,
        next_percent: u16,
    ) -> Option<OverlayWindowBounds> {
        let current = self.bounds();
        let base = match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_ref()
                .expect("active xdg overlay is present")
                .resize_base(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_ref()
                .expect("active layer overlay is present")
                .resize_base(),
        };
        desired_bounds_for_scale(current, base, previous_percent, next_percent)
    }

    fn is_visible(&self) -> bool {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_ref()
                .expect("active xdg overlay is present")
                .is_visible(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_ref()
                .expect("active layer overlay is present")
                .is_visible(),
        }
    }

    fn has_output_relative_bounds(&self) -> bool {
        self.active == ActiveOverlayBackend::Layer
    }

    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_ref()
                .expect("active xdg overlay is present")
                .window_handle(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_ref()
                .expect("active layer overlay is present")
                .window_handle(),
        }
    }

    fn capabilities(&self) -> OverlayCapabilities {
        let layer_shell_available = self.layer_shell_available == Some(true);
        OverlayCapabilities {
            always_on_top: layer_shell_available,
            output_relative_geometry: layer_shell_available,
            pointer_hover: false,
        }
    }

    fn set_system_menu_presentation(
        &mut self,
        presentation: bongocat_platform::SystemMenuPresentation,
    ) {
        self.system_menu_presentation = Some(presentation.clone());
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .set_system_menu_presentation(presentation),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .set_system_menu_presentation(presentation),
        }
    }

    fn take_system_menu_action(&mut self) -> Option<bongocat_platform::SystemMenuAction> {
        if let Some(action) = self.system_menu_actions.pop_front() {
            return Some(action);
        }
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .take_system_menu_action(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .take_system_menu_action(),
        }
    }

    fn dismiss_context_menu(&mut self) {
        match self.active {
            ActiveOverlayBackend::Xdg => self
                .xdg
                .as_mut()
                .expect("active xdg overlay is present")
                .dismiss_context_menu(),
            ActiveOverlayBackend::Layer => self
                .layer
                .as_mut()
                .expect("active layer overlay is present")
                .dismiss_context_menu(),
        }
    }
}

pub(crate) struct ProductOverlaySession {
    overlay: NativeOverlay,
    runtime_client: RuntimeClient,
    render_consumer: RenderConsumer,
    input_service: Option<LinuxInputService>,
    input_start_error: Option<PlatformInputError>,
    input_diagnostics: Option<PlatformInputDiagnostics>,
    input_stopped: bool,
    frames_presented: u64,
    dynamic_snapshots: u64,
    model_commit_rejections: u64,
    previous_snapshot: Arc<RenderSnapshot>,
    options: OverlaySessionOptions,
    last_frame: RenderFrame,
    idle: IdleHide,
    session_started: Instant,
    retry_backoff: FrameRetryBackoff,
}

impl ProductOverlaySession {
    pub(crate) fn start(
        runtime_client: RuntimeClient,
        input_producer: InputProducer,
        cursor_producer: CursorProducer,
        gamepad_axis_producer: GamepadAxisProducer,
        render_consumer: RenderConsumer,
        options: OverlaySessionOptions,
        interaction_sinks: OverlayInteractionSinks,
    ) -> Result<Self, OverlayError> {
        validate_options(options)?;
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime did not publish an initial render frame"))?;
        let token = initial_frame
            .model_commit
            .ok_or_else(|| OverlayError::new("initial render frame has no model commit token"))?;
        let OverlayInteractionSinks {
            context_menu_sender,
            resize_sender,
        } = interaction_sinks;
        let mut overlay = match NativeOverlay::create(
            &initial_frame,
            options,
            options.window_bounds,
            context_menu_sender,
            resize_sender,
        ) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
        };
        if let Err(error) = overlay.verify_composition() {
            reject_model_commit(&runtime_client, &render_consumer, token)?;
            return Err(error);
        }
        let mut frames_presented = 0;
        if runtime_client.snapshot().overlay_visible {
            if let Err(error) = overlay.prepare_show(options) {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
            match overlay.draw(false) {
                Ok(()) => {
                    if let Err(error) = overlay.set_visible(true, options) {
                        reject_model_commit(&runtime_client, &render_consumer, token)?;
                        return Err(error);
                    }
                    frames_presented = 1;
                }
                Err(error) if error.is_temporary_presentation_unavailable() => {}
                Err(error) if error.is_presentation_surface_lost() => {
                    if let Err(error) =
                        overlay.recover_presentation_surface(&initial_frame, options, true)
                    {
                        reject_model_commit(&runtime_client, &render_consumer, token)?;
                        return Err(error);
                    }
                }
                Err(error) => {
                    reject_model_commit(&runtime_client, &render_consumer, token)?;
                    return Err(error);
                }
            }
        }
        report_model_commit(
            &runtime_client,
            &render_consumer,
            token,
            ModelCommitOutcome::Prepared,
        )?;

        let diagnostics_producer = runtime_client.platform_input_diagnostics_producer();
        let (input_service, input_start_error) =
            crate::start_platform_input(&diagnostics_producer, || {
                LinuxInputService::start_with_diagnostics(
                    input_producer,
                    cursor_producer,
                    gamepad_axis_producer,
                    diagnostics_producer.clone(),
                )
            });
        Ok(Self {
            overlay,
            runtime_client,
            render_consumer,
            input_service,
            input_start_error,
            input_diagnostics: None,
            input_stopped: false,
            frames_presented,
            dynamic_snapshots: 0,
            model_commit_rejections: 0,
            previous_snapshot: Arc::clone(&initial_frame.snapshot),
            options,
            last_frame: initial_frame,
            idle: IdleHide::default(),
            session_started: Instant::now(),
            retry_backoff: FrameRetryBackoff::default(),
        })
    }

    pub(crate) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while duration.is_zero() || started.elapsed() < duration {
            let outcome = self.tick()?;
            if outcome == OverlayTickOutcome::Hidden {
                break;
            }
            let interval = outcome.retry_after().unwrap_or_else(|| {
                frame_interval_for_maximum_fps(self.options.maximum_fps)
                    .expect("product overlay stores a validated maximum FPS")
            });
            if outcome.retry_after().is_some() {
                next_frame = Instant::now();
            }
            next_frame += interval;
            if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            } else {
                next_frame = Instant::now();
            }
        }
        Ok(())
    }

    pub(crate) fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
        let close_requested = self.overlay.pump_events(&self.last_frame, self.options)?;
        if close_requested {
            self.runtime_client
                .send(RuntimeCommand::SetOverlayVisible(false))
                .map_err(|error| {
                    OverlayError::new(format!(
                        "hide Wayland overlay after a close request: {error}"
                    ))
                })?;
        }
        let runtime_snapshot = self.runtime_client.snapshot();
        if runtime_snapshot.state == RuntimeState::Stopped {
            return Err(OverlayError::new(
                "runtime stopped while the product overlay was active",
            ));
        }
        let next_options = self
            .options
            .with_runtime_settings(runtime_snapshot.overlay_settings);
        if next_options != self.options {
            self.overlay.apply_backend(&self.last_frame, next_options)?;
            if next_options.scale_percent != self.options.scale_percent
                && let Some(bounds) = self
                    .overlay
                    .scale_transition_bounds(self.options.scale_percent, next_options.scale_percent)
            {
                self.overlay.resize(bounds);
            }
            if self.overlay.is_visible() {
                self.overlay
                    .renderer_mut()
                    .set_presentation_opacity(f32::from(next_options.opacity_percent) / 100.0);
            }
            self.overlay
                .renderer_mut()
                .set_corner_radius(next_options.corner_radius_percent);
            if self.overlay.is_visible() {
                self.overlay.set_click_through(next_options.click_through)?;
            }
            self.options = next_options;
        }
        self.idle.observe(IdleObservation {
            enabled: self.options.hide_on_idle
                && runtime_snapshot.platform_input.service_status
                    == PlatformInputServiceStatus::Running,
            delay: Duration::from_millis(u64::from(self.options.hide_on_idle_delay_ms)),
            input_sequence: runtime_snapshot.input.last_input_sequence,
            cursor_at: runtime_snapshot.cursor.sample.map(|sample| sample.at),
            gamepad_axis_published: runtime_snapshot.gamepad_axis_transport.published,
            now: self.session_started.elapsed(),
        });
        self.options.maximum_fps = runtime_snapshot.maximum_fps;
        let overlay_visible = runtime_snapshot.overlay_visible && !close_requested;
        if !overlay_visible {
            if let Err(error) = self.overlay.set_visible(false, self.options) {
                if error.is_presentation_surface_lost() {
                    self.overlay.recover_presentation_surface(
                        &self.last_frame,
                        self.options,
                        false,
                    )?;
                    return Ok(OverlayTickOutcome::Deferred(
                        self.retry_backoff.register_temporary_failure(),
                    ));
                }
                return Err(error);
            }
        } else {
            self.overlay.prepare_show(self.options)?;
        }
        let next_frame = if overlay_visible {
            self.render_consumer.take_latest()
        } else if !overlay_visible {
            self.render_consumer.take_model_commit()
        } else {
            None
        };
        if let Some(frame) = next_frame {
            if frame.model_generation != self.overlay.renderer().model_generation {
                let candidate = match self.overlay.renderer().prepare_model(&frame) {
                    Ok(candidate) => candidate,
                    Err(error) if frame.model_commit.is_some() => {
                        reject_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            frame.model_commit.expect("checked model commit token"),
                        )?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        return self.draw_current(overlay_visible);
                    }
                    Err(error) => return Err(error),
                };
                let previous = self.overlay.renderer_mut().install_model(candidate);
                let verified = self.overlay.verify_composition();
                if let Err(error) = verified {
                    let _candidate = self.overlay.renderer_mut().install_model(previous);
                    if let Some(token) = frame.model_commit {
                        reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        return self.draw_current(overlay_visible);
                    }
                    return Err(error);
                }
                if let Some(token) = frame.model_commit {
                    report_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        token,
                        ModelCommitOutcome::Prepared,
                    )?;
                }
                let bounds =
                    model_switch_window_bounds(self.overlay.bounds(), frame.snapshot.canvas);
                self.overlay.resize(bounds);
                self.overlay.set_resize_base(frame.snapshot.canvas);
                if frame.snapshot.as_ref() != self.previous_snapshot.as_ref() {
                    self.dynamic_snapshots = self.dynamic_snapshots.saturating_add(1);
                }
                self.previous_snapshot = Arc::clone(&frame.snapshot);
                self.last_frame = frame;
                return self.draw_current(overlay_visible);
            }
            match self.overlay.renderer_mut().sync_frame(&frame) {
                Ok(_) => {
                    if let Some(token) = frame.model_commit {
                        report_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            token,
                            ModelCommitOutcome::Prepared,
                        )?;
                    }
                    if frame.snapshot.as_ref() != self.previous_snapshot.as_ref() {
                        self.dynamic_snapshots = self.dynamic_snapshots.saturating_add(1);
                    }
                    self.previous_snapshot = Arc::clone(&frame.snapshot);
                    self.last_frame = frame;
                }
                Err(error) if frame.model_commit.is_some() => {
                    reject_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        frame.model_commit.expect("checked model commit token"),
                    )?;
                    self.model_commit_rejections = self.model_commit_rejections.saturating_add(1);
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
        self.draw_current(overlay_visible)
    }

    fn draw_current(&mut self, visible: bool) -> Result<OverlayTickOutcome, OverlayError> {
        if !visible {
            return Ok(OverlayTickOutcome::Hidden);
        }
        let alpha = f32::from(self.options.opacity_percent) / 100.0 * self.idle.visible() as f32;
        self.overlay.renderer_mut().set_presentation_opacity(alpha);
        match self.overlay.draw(self.frames_presented == 0) {
            Ok(()) => {
                self.retry_backoff.record_success();
            }
            Err(error) if error.is_temporary_presentation_unavailable() => {
                return Ok(OverlayTickOutcome::Deferred(
                    self.retry_backoff.register_temporary_failure(),
                ));
            }
            Err(error) if error.is_presentation_surface_lost() => {
                self.overlay
                    .recover_presentation_surface(&self.last_frame, self.options, true)?;
                return Ok(OverlayTickOutcome::Deferred(
                    self.retry_backoff.register_temporary_failure(),
                ));
            }
            Err(error) => return Err(error),
        }
        self.frames_presented = self.frames_presented.saturating_add(1);
        let mut options = self.options;
        options.click_through = self.options.click_through || self.idle.hidden();
        self.overlay.set_visible(true, options)?;
        Ok(OverlayTickOutcome::Presented)
    }

    pub(crate) fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        if self.overlay.has_output_relative_bounds() {
            Ok(self.overlay.bounds())
        } else {
            Err(OverlayError::new(
                "xdg-shell does not expose global overlay window bounds",
            ))
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.overlay.is_visible()
    }

    pub(crate) fn model_generation(&self) -> u64 {
        self.overlay.renderer().model_generation
    }

    pub(crate) fn capabilities(&self) -> OverlayCapabilities {
        self.overlay.capabilities()
    }

    pub(crate) fn set_system_menu_presentation(
        &mut self,
        presentation: bongocat_platform::SystemMenuPresentation,
    ) {
        self.overlay.set_system_menu_presentation(presentation);
    }

    pub(crate) fn take_system_menu_action(
        &mut self,
    ) -> Option<bongocat_platform::SystemMenuAction> {
        self.overlay.take_system_menu_action()
    }

    pub(crate) fn stop_input(&mut self) -> Result<(), OverlayError> {
        if self.input_stopped {
            return Ok(());
        }
        self.input_stopped = true;
        if let Some(service) = self.input_service.take() {
            self.input_diagnostics = Some(
                service
                    .stop()
                    .map_err(|error| OverlayError::new(error.to_string()))?,
            );
        }
        Ok(())
    }

    pub(crate) fn finish_after_runtime_shutdown(
        mut self,
    ) -> Result<ProductOverlayReport, OverlayError> {
        self.overlay.dismiss_context_menu();
        if !self.input_stopped {
            return Err(OverlayError::new(
                "platform input must stop before the runtime",
            ));
        }
        if self.runtime_client.snapshot().state != RuntimeState::Stopped {
            return Err(OverlayError::new(
                "runtime must stop before releasing the product overlay",
            ));
        }
        while self.render_consumer.take_latest().is_some() {}
        Ok(ProductOverlayReport {
            frames_presented: self.frames_presented,
            // Global placement is not a generic Wayland capability, so it is
            // omitted rather than reported as a product shutdown failure.
            placement_fully_visible: true,
            dynamic_snapshots: self.dynamic_snapshots,
            model_commit_rejections: self.model_commit_rejections,
            input_start_error: self.input_start_error,
            input_diagnostics: self.input_diagnostics.take(),
            render_diagnostics: self.render_consumer.diagnostics(),
            model_generation: self.overlay.renderer().model_generation,
            drawable_count: self.overlay.renderer().drawable_count(),
            masked_drawable_count: self.overlay.renderer().masked_drawable_count(),
            texture_count: self.overlay.renderer().texture_count(),
        })
    }
}

fn desired_bounds_for_scale(
    current: OverlayWindowBounds,
    base: ResizeBase,
    previous_percent: u16,
    next_percent: u16,
) -> Option<OverlayWindowBounds> {
    (!bounds_match_scale(current, base, next_percent))
        .then(|| current.rescale(previous_percent, next_percent))
}

impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.overlay.window_handle()
    }
}

pub(super) fn validate_options(options: OverlaySessionOptions) -> Result<(), OverlayError> {
    if let Some(bounds) = options.window_bounds {
        bounds.validate()?;
    }
    if !(25..=400).contains(&options.scale_percent) {
        return Err(OverlayError::new(
            "overlay scale must be between 25 and 400 percent",
        ));
    }
    if !(1..=100).contains(&options.opacity_percent) {
        return Err(OverlayError::new(
            "overlay opacity must be between 1 and 100 percent",
        ));
    }
    if options.corner_radius_percent > MAXIMUM_CORNER_RADIUS_PERCENT {
        return Err(OverlayError::new(
            "overlay corner radius must be between 0 and 50 percent",
        ));
    }
    if !maximum_fps_is_valid(options.maximum_fps) {
        return Err(OverlayError::new("maximum FPS must be between 15 and 240"));
    }
    Ok(())
}

pub(crate) fn reject_model_commit(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    token: ModelCommitToken,
) -> Result<(), OverlayError> {
    report_model_commit(
        runtime_client,
        render_consumer,
        token,
        ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
    )
}

pub(crate) fn report_model_commit(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    token: ModelCommitToken,
    outcome: ModelCommitOutcome,
) -> Result<(), OverlayError> {
    render_consumer
        .report_model_commit(ModelCommitFeedback { token, outcome })
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let completed = runtime_client
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("runtime did not finish the model commit"))?;
    let failure = completed
        .last_command_failure
        .filter(|failure| failure.sequence == token.command_sequence);
    match (outcome, failure) {
        (ModelCommitOutcome::Prepared, None)
        | (
            ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
            Some(bongocat_runtime::RuntimeCommandFailure {
                code: RuntimeRenderErrorCode::GpuPreparationFailed,
                ..
            }),
        ) => Ok(()),
        (ModelCommitOutcome::Prepared, Some(failure)) => Err(OverlayError::new(format!(
            "runtime rejected prepared model generation: {:?}",
            failure.code
        ))),
        (ModelCommitOutcome::Rejected(_), None) => Err(OverlayError::new(
            "runtime committed a renderer-rejected model generation",
        )),
        (ModelCommitOutcome::Rejected(_), Some(failure)) => Err(OverlayError::new(format!(
            "runtime reported the wrong model rejection: {:?}",
            failure.code
        ))),
    }
}

#[cfg(test)]
mod scale_transition_tests {
    use super::*;

    #[test]
    fn a_resize_drag_acknowledgement_does_not_apply_the_scale_twice() {
        let base = ResizeBase::new(350.0, 200.0).expect("valid resize base");
        let dragged = OverlayWindowBounds::new(40, 50, 525, 300);

        assert_eq!(desired_bounds_for_scale(dragged, base, 100, 150), None);
    }

    #[test]
    fn a_settings_scale_change_resizes_bounds_that_are_still_at_the_old_scale() {
        let base = ResizeBase::new(350.0, 200.0).expect("valid resize base");
        let current = OverlayWindowBounds::new(40, 50, 350, 200);

        assert_eq!(
            desired_bounds_for_scale(current, base, 100, 150),
            Some(OverlayWindowBounds::new(40, 50, 525, 300))
        );
    }
}
