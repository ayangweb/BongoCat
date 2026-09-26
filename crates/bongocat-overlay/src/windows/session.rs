//! The overlay session the product drives.
//!
//! This is the adapter's public surface: start the window and the frame source,
//! hand it a runtime snapshot per tick, answer its context menu and resize
//! requests, and shut it down in order. Everything it coordinates lives in the
//! modules beside it.

use super::*;

pub(crate) const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct NativeOverlay {
    pub(crate) renderer: Renderer,
    pub(crate) window: OverlayWindow,
    pub(crate) presentation: OverlayPresentationState,
    /// Window opacity currently applied to the DirectComposition effect,
    /// including the hover fade. It lives here rather than on the session so
    /// replacing the native window resets it together with the renderer that
    /// carries it.
    pub(crate) applied_alpha: f32,
    pub(crate) applied_click_through: bool,
}

impl NativeOverlay {
    pub(crate) fn create(
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
        context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> Result<Self, OverlayError> {
        validate_options(options)?;
        let window = OverlayWindow::create(
            options,
            frame.snapshot.canvas,
            bounds,
            context_menu_sender,
            resize_sender,
        )?;
        let renderer = Renderer::create(&window, frame, options)?;
        Ok(Self {
            renderer,
            window,
            presentation: OverlayPresentationState::default(),
            applied_alpha: f32::from(options.opacity_percent) / 100.0,
            applied_click_through: options.click_through,
        })
    }

    pub(crate) fn set_visible(&self, visible: bool) -> Result<(), OverlayError> {
        if visible {
            self.presentation.require_presented_frame()?;
            self.window.show()?;
        } else if self.window.is_visible() {
            // SAFETY: the HWND is live and accessed only from its owner thread.
            let _ = unsafe { ShowWindow(self.window.hwnd, SW_HIDE) };
        }
        Ok(())
    }

    pub(crate) fn set_always_on_top(&self, always_on_top: bool) -> Result<(), OverlayError> {
        self.window.set_always_on_top(always_on_top)
    }

    pub(crate) fn set_click_through(&self, click_through: bool) -> Result<(), OverlayError> {
        self.window.set_click_through(click_through)
    }

    /// Resize the existing native window and its swap-chain-backed renderer.
    pub(crate) fn resize(&mut self, bounds: OverlayWindowBounds) -> Result<(), OverlayError> {
        let visible = self.window.is_visible();
        self.window.resize(bounds)?;
        let resized = self.renderer.resize(bounds.width, bounds.height)?;
        if visible && resized {
            // ResizeBuffers leaves a new back buffer without content until the
            // next draw. Fill it before returning to the window loop so the
            // compositor cannot show the resized HWND as a transparent frame.
            match self.draw(false) {
                Ok(()) => {}
                // The normal frame path below owns retry/backoff for a
                // temporarily occluded swap chain.
                Err(error) if error.is_temporary_presentation_unavailable() => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Apply the per-frame presentation state without replacing the window.
    ///
    /// `alpha` is the configured window opacity multiplied by the hover fade,
    /// and `click_through` is the effective pointer routing. The alpha is
    /// applied once to the completed DirectComposition visual, after all
    /// drawables have been blended. The hover hide forces pass-through on so an
    /// invisible overlay cannot swallow a click meant for whatever is
    /// underneath it.
    pub(crate) fn apply_presentation(
        &mut self,
        alpha: f32,
        click_through: bool,
    ) -> Result<(), OverlayError> {
        if alpha != self.applied_alpha {
            self.renderer.set_opacity(alpha)?;
            self.applied_alpha = alpha;
        }
        if click_through != self.applied_click_through {
            self.set_click_through(click_through)?;
            self.applied_click_through = click_through;
        }
        Ok(())
    }

    /// Adapt the window to a newly prepared model while keeping its live width.
    ///
    /// The model-switch probe updates the D3D11 model in place instead of
    /// replacing the HWND, so it applies the same canvas-aspect rule as the
    /// product session explicitly. The swap chain follows the native resize
    /// immediately, before the next frame is drawn.
    pub(crate) fn resize_for_model(&mut self, canvas: CanvasInfo) -> Result<(), OverlayError> {
        let bounds = model_switch_window_bounds(self.window.bounds()?, canvas);
        self.window.resize(bounds)?;
        self.renderer.resize(bounds.width, bounds.height)?;
        Ok(())
    }

    /// Match the swap chain and the mask targets to the window's current size.
    ///
    /// A right-button resize drag changes the window size directly through
    /// `SetWindowPos`, so the renderer keeps the size it was created with until
    /// this runs.
    ///
    /// Returns whether the size changed.
    pub(crate) fn sync_window_size(&mut self) -> Result<bool, OverlayError> {
        let bounds = self.window.bounds()?;
        self.renderer.resize(bounds.width, bounds.height)
    }

    pub(crate) fn draw(&mut self, verify: bool) -> Result<(), OverlayError> {
        self.renderer.draw(verify)?;
        self.presentation.record_presented_frame();
        Ok(())
    }

    /// Draw one frame and read it back as cover pixels.
    ///
    /// The window is created without `WS_VISIBLE` and a capture never calls
    /// `set_visible`, so a frame drawn here reaches the staging readback and nothing
    /// the user did not ask for reaches the screen.
    pub(crate) fn draw_capturing(&mut self, verify: bool) -> Result<CapturedFrame, OverlayError> {
        let captured = self.renderer.draw_capturing(verify)?;
        self.presentation.record_presented_frame();
        Ok(captured)
    }
}

pub(crate) struct ProductOverlaySession {
    pub(crate) overlay: NativeOverlay,
    pub(crate) runtime_client: RuntimeClient,
    pub(crate) render_consumer: RenderConsumer,
    pub(crate) _com_apartment: ComApartment,
    pub(crate) input_service: Option<WindowsInputService>,
    pub(crate) input_start_error: Option<PlatformInputError>,
    pub(crate) input_diagnostics: Option<PlatformInputDiagnostics>,
    pub(crate) input_stopped: bool,
    pub(crate) frames_presented: u64,
    pub(crate) dynamic_snapshots: u64,
    pub(crate) model_commit_rejections: u64,
    pub(crate) previous_snapshot: Arc<RenderSnapshot>,
    pub(crate) options: OverlaySessionOptions,
    pub(crate) last_frame: RenderFrame,
    pub(crate) retry_backoff: FrameRetryBackoff,
    pub(crate) context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub(crate) resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    pub(crate) hover: PointerHoverHide,
    pub(crate) placement: OverlayPlacementConstraint,
    /// Monotonic base for every time-based rule in this session. The hover fade
    /// and the placement settle delay both measure elapsed time from it, so the
    /// session needs exactly one wall-clock reading at start.
    pub(crate) session_started: Instant,
}

impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.overlay.window.hwnd.0 as isize)
            .ok_or(HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(hwnd);
        // SAFETY: `OverlayWindow` owns this HWND for its lifetime, and the
        // returned handle is borrowed from this session.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
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
        let OverlayInteractionSinks {
            context_menu_sender,
            resize_sender,
        } = interaction_sinks;
        validate_options(options)?;
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime did not publish an initial render frame"))?;
        let token = initial_frame
            .model_commit
            .ok_or_else(|| OverlayError::new("initial render frame has no model commit token"))?;
        let com_apartment = ComApartment::initialize()?;
        let mut overlay = match NativeOverlay::create(
            &initial_frame,
            options,
            options.window_bounds,
            context_menu_sender.clone(),
            resize_sender.clone(),
        ) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
        };
        let mut frames_presented = 0;
        if runtime_client.snapshot().overlay_visible {
            if let Err(error) = overlay.draw(true).and_then(|()| overlay.set_visible(true)) {
                reject_model_commit(&runtime_client, &render_consumer, token)?;
                return Err(error);
            }
            frames_presented = 1;
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
                WindowsInputService::start_with_diagnostics(
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
            _com_apartment: com_apartment,
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
            retry_backoff: FrameRetryBackoff::default(),
            context_menu_sender,
            resize_sender,
            hover: PointerHoverHide::default(),
            placement: OverlayPlacementConstraint::default(),
            session_started: Instant::now(),
        })
    }

    pub(crate) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while duration.is_zero() || started.elapsed() < duration {
            pump_window_messages();
            if self.tick()? == OverlayTickOutcome::Hidden {
                break;
            }
            next_frame += frame_interval_for_maximum_fps(self.options.maximum_fps)
                .expect("product overlay stores a validated maximum FPS");
            if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            } else {
                next_frame = Instant::now();
            }
        }
        Ok(())
    }

    pub(crate) fn tick(&mut self) -> Result<OverlayTickOutcome, OverlayError> {
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
            if self.options.requires_window_recreation(next_options) {
                let bounds = self.overlay.window.bounds()?;
                let bounds = if next_options.scale_percent != self.options.scale_percent
                    && !self.bounds_match_scale(bounds, next_options.scale_percent)
                {
                    bounds.rescale(self.options.scale_percent, next_options.scale_percent)
                } else {
                    bounds
                };
                let mut replacement = self.create_overlay(
                    &self.last_frame,
                    next_options,
                    Some(bounds),
                    self.context_menu_sender.clone(),
                    self.resize_sender.clone(),
                )?;
                if runtime_snapshot.overlay_visible {
                    replacement.draw(self.frames_presented == 0)?;
                    replacement.set_visible(true)?;
                    self.frames_presented = self.frames_presented.saturating_add(1);
                }
                self.overlay = replacement;
            } else {
                if next_options.scale_percent != self.options.scale_percent {
                    let bounds = self.overlay.window.bounds()?;
                    let bounds = if self.bounds_match_scale(bounds, next_options.scale_percent) {
                        bounds
                    } else {
                        bounds.rescale(self.options.scale_percent, next_options.scale_percent)
                    };
                    self.overlay.resize(bounds)?;
                }
                if next_options.always_on_top != self.options.always_on_top {
                    self.overlay.set_always_on_top(next_options.always_on_top)?;
                }
            }
            self.options = next_options;
        }
        // A right-button resize drag changes the window size directly, so the
        // swap chain and the mask targets follow here, before anything draws
        // against them.
        self.overlay.sync_window_size()?;
        if self.options.keep_inside_screen {
            let bounds = self.overlay.window.bounds()?;
            // A box that is still outside the displays is only corrected once it
            // has been observed at rest for the settle delay, so a drag that is
            // still in progress is never interrupted.
            let correction =
                self.placement
                    .observe(bounds, self.session_started.elapsed(), screen_bounds_all);
            if let Some(correction) = correction {
                self.overlay.window.set_origin(correction)?;
            }
        }
        // Pointer routing and window opacity are applied every tick rather than
        // only when the settings change, because the hover hide changes both
        // while the session keeps running.
        self.update_hover_presentation(
            self.options,
            runtime_snapshot.cursor.sample,
            runtime_snapshot.platform_input.service_status == PlatformInputServiceStatus::Running,
        )?;
        self.options.maximum_fps = runtime_snapshot.maximum_fps;
        let overlay_visible = runtime_snapshot.overlay_visible;
        if !overlay_visible {
            self.overlay.set_visible(false)?;
        }
        let next_frame = if overlay_visible {
            self.render_consumer.take_latest()
        } else {
            self.render_consumer.take_model_commit()
        };
        if let Some(frame) = next_frame {
            let model_changed = frame.model_generation != self.overlay.renderer.model_generation;
            if model_changed {
                let bounds = model_switch_window_bounds(
                    self.overlay.window.bounds()?,
                    frame.snapshot.canvas,
                );
                let mut replacement = match self.create_overlay(
                    &frame,
                    self.options,
                    Some(bounds),
                    self.context_menu_sender.clone(),
                    self.resize_sender.clone(),
                ) {
                    Ok(replacement) => replacement,
                    Err(error) if frame.model_commit.is_some() => {
                        reject_model_commit(
                            &self.runtime_client,
                            &self.render_consumer,
                            frame.model_commit.expect("checked model commit token"),
                        )?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        if overlay_visible {
                            self.overlay.draw(self.frames_presented == 0)?;
                            self.frames_presented = self.frames_presented.saturating_add(1);
                            self.overlay.set_visible(true)?;
                            return Ok(OverlayTickOutcome::Presented);
                        }
                        return Ok(OverlayTickOutcome::Hidden);
                    }
                    Err(error) => return Err(error),
                };
                let candidate = replacement.draw(true).and_then(|()| {
                    if overlay_visible {
                        replacement.set_visible(true)
                    } else {
                        Ok(())
                    }
                });
                if let Err(error) = candidate {
                    if let Some(token) = frame.model_commit {
                        reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        if overlay_visible {
                            self.overlay.draw(self.frames_presented == 0)?;
                            self.frames_presented = self.frames_presented.saturating_add(1);
                            self.overlay.set_visible(true)?;
                            return Ok(OverlayTickOutcome::Presented);
                        }
                        return Ok(OverlayTickOutcome::Hidden);
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
                if frame.snapshot.as_ref() != self.previous_snapshot.as_ref() {
                    self.dynamic_snapshots = self.dynamic_snapshots.saturating_add(1);
                }
                self.last_frame = frame.clone();
                self.previous_snapshot = frame.snapshot;
                self.overlay = replacement;
                self.frames_presented = self.frames_presented.saturating_add(1);
                return Ok(if overlay_visible {
                    OverlayTickOutcome::Presented
                } else {
                    OverlayTickOutcome::Hidden
                });
            }
            match self.overlay.renderer.sync_frame(&frame) {
                Ok(switched) => {
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
                    debug_assert!(!switched);
                    self.last_frame = frame.clone();
                    self.previous_snapshot = frame.snapshot;
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
        if !overlay_visible {
            return Ok(OverlayTickOutcome::Hidden);
        }
        match self.overlay.draw(self.frames_presented == 0) {
            Ok(()) => self.retry_backoff.record_success(),
            Err(error) if error.is_temporary_presentation_unavailable() => {
                return Ok(OverlayTickOutcome::Deferred(
                    self.retry_backoff.register_temporary_failure(),
                ));
            }
            Err(error) => return Err(error),
        }
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.overlay.set_visible(true)?;
        Ok(OverlayTickOutcome::Presented)
    }

    pub(crate) fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        self.overlay.window.bounds()
    }

    /// Advance the hover hide and push the resulting window presentation.
    ///
    /// Hover hide needs a trustworthy pointer position. A missing sample (no
    /// pointer event has arrived yet) and a platform input service that is not
    /// running both count as "not inside", so a degraded pointer pipeline can
    /// never leave the overlay stuck invisible.
    ///
    /// `GetCursorPos` and `GetWindowRect` both report virtual-screen pixels, so
    /// unlike macOS this needs no coordinate conversion.
    pub(crate) fn update_hover_presentation(
        &mut self,
        options: OverlaySessionOptions,
        cursor: Option<CursorSample>,
        input_running: bool,
    ) -> Result<(), OverlayError> {
        let bounds = self.overlay.window.bounds()?;
        // A right-button resize drag keeps the overlay visible: the hover hide
        // fades the window out and starts passing pointer events through, which
        // would end the drag the window itself is running.
        let pointer_inside = !self.overlay.window.is_resize_dragging()
            && cursor.is_some_and(|sample| {
                pointer_inside_window(bounds, sample.position.x, sample.position.y)
            });
        let fade = self.hover.observe(PointerHoverObservation {
            enabled: options.hide_on_pointer_hover && input_running,
            delay: Duration::from_millis(u64::from(options.hide_on_pointer_hover_delay_ms)),
            pointer_inside,
            now: self.session_started.elapsed(),
        });
        let alpha = f32::from(options.opacity_percent) / 100.0 * fade as f32;
        self.overlay
            .apply_presentation(alpha, options.click_through || self.hover.hidden())?;
        Ok(())
    }

    /// Create a native window that already carries the current hover fade.
    ///
    /// A replacement window is created with the configured opacity, so a
    /// resource or model change while the overlay is hover-hidden would
    /// otherwise show the new window at full opacity before the next tick could
    /// correct it.
    pub(crate) fn create_overlay(
        &self,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
        context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> Result<NativeOverlay, OverlayError> {
        let mut overlay =
            NativeOverlay::create(frame, options, bounds, context_menu_sender, resize_sender)?;
        let alpha = f32::from(options.opacity_percent) / 100.0 * self.hover.visible() as f32;
        overlay.apply_presentation(alpha, options.click_through || self.hover.hidden())?;
        Ok(overlay)
    }

    /// Whether the live window box already matches a scale.
    ///
    /// A resize drag resizes the window before the scale reaches the
    /// configuration, so the in-place resize that follows the write-back must
    /// not scale the box a second time. The base is derived in physical pixels,
    /// which is the unit the box itself is in. See
    /// [`crate::bounds_match_scale`].
    pub(crate) fn bounds_match_scale(
        &self,
        bounds: OverlayWindowBounds,
        scale_percent: u16,
    ) -> bool {
        let (base_width, base_height) =
            default_overlay_window_dimensions(self.last_frame.snapshot.canvas);
        // SAFETY: the HWND is live and read on its owner thread.
        let dpi = unsafe { GetDpiForWindow(self.overlay.window.hwnd) };
        resize_base_for_dpi(base_width, base_height, dpi)
            .is_some_and(|base| crate::bounds_match_scale(bounds, base, scale_percent))
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.overlay.window.is_visible()
    }

    pub(crate) fn model_generation(&self) -> u64 {
        self.overlay.renderer.model_generation
    }

    pub(crate) fn system_termination_requested(&self) -> bool {
        self.input_service
            .as_ref()
            .is_some_and(WindowsInputService::system_termination_requested)
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
        self,
    ) -> Result<ProductOverlayReport, OverlayError> {
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
        let bounds = self.overlay.window.bounds()?;
        let placement_fully_visible =
            !self.options.keep_inside_screen || bounds_inside_screens(&screen_bounds_all(), bounds);
        Ok(ProductOverlayReport {
            frames_presented: self.frames_presented,
            placement_fully_visible,
            dynamic_snapshots: self.dynamic_snapshots,
            model_commit_rejections: self.model_commit_rejections,
            input_start_error: self.input_start_error,
            input_diagnostics: self.input_diagnostics,
            render_diagnostics: self.render_consumer.diagnostics(),
            model_generation: self.overlay.renderer.model_generation,
            drawable_count: self.overlay.renderer.model.meshes.len(),
            masked_drawable_count: self.overlay.renderer.model.masked_drawable_count,
            texture_count: self.overlay.renderer.model.textures.len(),
        })
    }
}

pub(crate) fn validate_options(options: OverlaySessionOptions) -> Result<(), OverlayError> {
    if let Some(bounds) = options.window_bounds {
        bounds.validate()?;
    }
    if !(25..=400).contains(&options.scale_percent) {
        return Err(OverlayError::new(
            "overlay scale must be between 25 and 400 percent",
        ));
    }
    if options.opacity_percent == 0 || options.opacity_percent > 100 {
        return Err(OverlayError::new(
            "overlay opacity must be between 1 and 100 percent",
        ));
    }
    if options.corner_radius_percent > MAXIMUM_CORNER_RADIUS_PERCENT {
        return Err(OverlayError::new(
            "overlay corner radius must be between 0 and 50 percent",
        ));
    }
    if options.hide_on_pointer_hover_delay_ms > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS {
        return Err(OverlayError::new(
            "overlay hover hide delay must be between 0 and 60000 milliseconds",
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
