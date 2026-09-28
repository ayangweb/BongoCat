//! The overlay session the product drives.
//!
//! This is the adapter's public surface: start the panel and the frame source,
//! hand it a runtime snapshot per tick, answer its context menu and resize
//! requests, and shut it down in order. Everything it coordinates lives in the
//! modules beside it.

use super::*;

pub(crate) const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);

pub(crate) struct ProductOverlaySession {
    pub(crate) application: Retained<NSApplication>,
    pub(crate) overlay: NativeOverlay,
    pub(crate) runtime_client: RuntimeClient,
    pub(crate) render_consumer: RenderConsumer,
    pub(crate) input_service: Option<MacInputService>,
    pub(crate) input_start_error: Option<PlatformInputError>,
    pub(crate) input_diagnostics: Option<PlatformInputDiagnostics>,
    pub(crate) input_stopped: bool,
    pub(crate) frames_presented: u64,
    pub(crate) dynamic_snapshots: u64,
    pub(crate) model_commit_rejections: u64,
    pub(crate) previous_snapshot: Arc<RenderSnapshot>,
    pub(crate) options: OverlaySessionOptions,
    pub(crate) last_frame: RenderFrame,
    pub(crate) pending_initial_model_commit: Option<ModelCommitToken>,
    pub(crate) pending_model_frame: Option<RenderFrame>,
    pub(crate) retry_backoff: FrameRetryBackoff,
    pub(crate) context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub(crate) resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    pub(crate) right_button_monitor: Option<RightButtonMonitor>,
    pub(crate) hover: PointerHoverHide,
    pub(crate) placement: OverlayPlacementConstraint,
    /// Monotonic base for every time-based rule in this session. The hover fade
    /// and the placement settle delay both measure elapsed time from it, so the
    /// session needs exactly one wall-clock reading at start.
    pub(crate) session_started: Instant,
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
        validate_product_options(options)?;
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("runtime did not publish an initial render frame"))?;
        let token = initial_frame
            .model_commit
            .ok_or_else(|| OverlayError::new("initial render frame has no model commit token"))?;
        let Some(mtm) = MainThreadMarker::new() else {
            reject_model_commit(&runtime_client, &render_consumer, token)?;
            return Err(OverlayError::new(
                "macOS overlay must start on the main thread",
            ));
        };
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        application.finishLaunching();
        let mut overlay =
            match NativeOverlay::create(mtm, &initial_frame, options, options.window_bounds) {
                Ok(overlay) => overlay,
                Err(error) => {
                    reject_model_commit(&runtime_client, &render_consumer, token)?;
                    return Err(error);
                }
            };
        let mut frames_presented = 0;
        let mut retry_backoff = FrameRetryBackoff::default();
        let mut pending_initial_model_commit = None;
        if runtime_client.snapshot().overlay_visible {
            match overlay.draw(true) {
                Ok(()) => {
                    retry_backoff.record_success();
                    if let Err(error) = overlay.set_visible(true) {
                        reject_model_commit(&runtime_client, &render_consumer, token)?;
                        return Err(error);
                    }
                    frames_presented = 1;
                }
                Err(error) if error.is_temporary_presentation_unavailable() => {
                    pending_initial_model_commit = Some(token);
                    let _ = retry_backoff.register_temporary_failure();
                }
                Err(error) => {
                    reject_model_commit(&runtime_client, &render_consumer, token)?;
                    return Err(error);
                }
            }
        }
        if pending_initial_model_commit.is_none() {
            report_model_commit(
                &runtime_client,
                &render_consumer,
                token,
                ModelCommitOutcome::Prepared,
            )?;
        }
        let diagnostics_producer = runtime_client.platform_input_diagnostics_producer();
        let (input_service, input_start_error) =
            crate::start_platform_input(&diagnostics_producer, || {
                MacInputService::start_with_diagnostics(
                    input_producer,
                    cursor_producer,
                    gamepad_axis_producer,
                    diagnostics_producer.clone(),
                )
            });
        let (base_width, base_height) =
            default_overlay_window_dimensions(initial_frame.snapshot.canvas);
        let context_menu_monitor = install_context_menu_monitor(
            mtm,
            Retained::clone(&overlay.panel),
            ResizeBase::new(f64::from(base_width), f64::from(base_height)),
            context_menu_sender.clone(),
            resize_sender.clone(),
        );
        Ok(Self {
            application,
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
            pending_initial_model_commit,
            pending_model_frame: None,
            retry_backoff,
            context_menu_sender,
            resize_sender,
            right_button_monitor: context_menu_monitor,
            hover: PointerHoverHide::default(),
            placement: OverlayPlacementConstraint::default(),
            session_started: Instant::now(),
        })
    }

    pub(crate) fn run_for(&mut self, duration: Duration) -> Result<(), OverlayError> {
        let started = Instant::now();
        let mut next_frame = started;
        while duration.is_zero() || started.elapsed() < duration {
            pump_application_events(&self.application);
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
                let bounds = self.window_bounds()?;
                let bounds = if next_options.scale_percent != self.options.scale_percent
                    && !self.bounds_match_scale(bounds, next_options.scale_percent)
                {
                    bounds.rescale(self.options.scale_percent, next_options.scale_percent)
                } else {
                    bounds
                };
                let mut replacement = self.create_overlay(
                    MainThreadMarker::new().ok_or_else(|| {
                        OverlayError::new("macOS overlay settings update lost the main thread")
                    })?,
                    &self.last_frame,
                    next_options,
                    Some(bounds),
                )?;
                if runtime_snapshot.overlay_visible {
                    match replacement.draw(self.frames_presented == 0) {
                        Ok(()) => self.retry_backoff.record_success(),
                        Err(error) if error.is_temporary_presentation_unavailable() => {
                            return Ok(self.defer_drawable_unavailable());
                        }
                        Err(error) => return Err(error),
                    }
                    replacement.set_visible(true)?;
                    self.frames_presented = self.frames_presented.saturating_add(1);
                }
                self.overlay = replacement;
                let mtm = MainThreadMarker::new().ok_or_else(|| {
                    OverlayError::new("macOS overlay settings update lost the main thread")
                })?;
                self.refresh_right_button_monitor(mtm);
            } else {
                if next_options.scale_percent != self.options.scale_percent {
                    let bounds = self.window_bounds()?;
                    let bounds = if self.bounds_match_scale(bounds, next_options.scale_percent) {
                        bounds
                    } else {
                        bounds.rescale(self.options.scale_percent, next_options.scale_percent)
                    };
                    self.overlay.resize(bounds)?;
                }
                if next_options.always_on_top != self.options.always_on_top {
                    self.overlay.set_always_on_top(next_options.always_on_top);
                }
            }
            self.options = next_options;
        }
        // A right-button resize drag changes the panel frame directly, so the
        // drawable and the mask targets follow here, before anything draws
        // against them.
        self.overlay.sync_window_size()?;
        if self.options.keep_inside_screen {
            let mtm = MainThreadMarker::new().ok_or_else(|| {
                OverlayError::new("macOS overlay placement check lost the main thread")
            })?;
            let bounds = self.window_bounds()?;
            // A box that is still outside the displays is only corrected once it
            // has been observed at rest for the settle delay, so a drag that is
            // still in progress is never interrupted.
            let correction = self
                .placement
                .observe(bounds, self.session_started.elapsed(), || {
                    screen_bounds_all(mtm).unwrap_or_default()
                });
            if let Some(correction) = correction {
                self.overlay.set_origin(correction);
            }
        }
        // Pointer routing and window alpha are applied every tick rather than
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

        if let Some(token) = self.pending_initial_model_commit {
            match self.overlay.draw(true) {
                Ok(()) => {
                    self.retry_backoff.record_success();
                    if overlay_visible {
                        self.overlay.set_visible(true)?;
                    }
                    report_model_commit(
                        &self.runtime_client,
                        &self.render_consumer,
                        token,
                        ModelCommitOutcome::Prepared,
                    )?;
                    self.pending_initial_model_commit = None;
                    self.frames_presented = self.frames_presented.saturating_add(1);
                    return Ok(if overlay_visible {
                        OverlayTickOutcome::Presented
                    } else {
                        OverlayTickOutcome::Hidden
                    });
                }
                Err(error) if error.is_temporary_presentation_unavailable() => {
                    return Ok(self.defer_drawable_unavailable());
                }
                Err(error) => {
                    reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                    return Err(error);
                }
            }
        }

        let next_frame = self.pending_model_frame.take().or_else(|| {
            if overlay_visible {
                self.render_consumer.take_latest()
            } else {
                self.render_consumer.take_model_commit()
            }
        });
        if let Some(frame) = next_frame {
            let model_changed = frame.model_generation != self.overlay.model_generation;
            if model_changed {
                let bounds =
                    model_switch_window_bounds(self.window_bounds()?, frame.snapshot.canvas);
                let replacement = self.create_overlay(
                    MainThreadMarker::new().ok_or_else(|| {
                        OverlayError::new("macOS overlay model update lost the main thread")
                    })?,
                    &frame,
                    self.options,
                    Some(bounds),
                );
                let mut replacement = match replacement {
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
                            match self.overlay.draw(self.frames_presented == 0) {
                                Ok(()) => self.retry_backoff.record_success(),
                                Err(error) if error.is_temporary_presentation_unavailable() => {
                                    return Ok(self.defer_drawable_unavailable());
                                }
                                Err(error) => return Err(error),
                            }
                            self.frames_presented = self.frames_presented.saturating_add(1);
                            self.overlay.set_visible(true)?;
                            return Ok(OverlayTickOutcome::Presented);
                        }
                        return Ok(OverlayTickOutcome::Hidden);
                    }
                    Err(error) => return Err(error),
                };
                let candidate = match replacement.draw(true) {
                    Ok(()) => {
                        self.retry_backoff.record_success();
                        if overlay_visible {
                            replacement.set_visible(true)
                        } else {
                            Ok(())
                        }
                    }
                    Err(error) if error.is_temporary_presentation_unavailable() => {
                        self.pending_model_frame = Some(frame);
                        return Ok(self.defer_drawable_unavailable());
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = candidate {
                    if let Some(token) = frame.model_commit {
                        reject_model_commit(&self.runtime_client, &self.render_consumer, token)?;
                        self.model_commit_rejections =
                            self.model_commit_rejections.saturating_add(1);
                        let _ = error;
                        if overlay_visible {
                            match self.overlay.draw(self.frames_presented == 0) {
                                Ok(()) => self.retry_backoff.record_success(),
                                Err(error) if error.is_temporary_presentation_unavailable() => {
                                    return Ok(self.defer_drawable_unavailable());
                                }
                                Err(error) => return Err(error),
                            }
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
                let mtm = MainThreadMarker::new().ok_or_else(|| {
                    OverlayError::new("macOS overlay model update lost the main thread")
                })?;
                self.refresh_right_button_monitor(mtm);
                self.frames_presented = self.frames_presented.saturating_add(1);
                return Ok(if overlay_visible {
                    OverlayTickOutcome::Presented
                } else {
                    OverlayTickOutcome::Hidden
                });
            }
            match self.overlay.sync_frame(&frame) {
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
                return Ok(self.defer_drawable_unavailable());
            }
            Err(error) => return Err(error),
        }
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.overlay.set_visible(true)?;
        Ok(OverlayTickOutcome::Presented)
    }

    pub(crate) fn defer_drawable_unavailable(&mut self) -> OverlayTickOutcome {
        OverlayTickOutcome::Deferred(self.retry_backoff.register_temporary_failure())
    }

    /// Advance the hover hide and push the resulting window presentation.
    ///
    /// Hover hide needs a trustworthy pointer position. A missing sample (no
    /// pointer event has arrived yet) and a platform input service that is not
    /// running both count as "not inside", so a degraded pointer pipeline can
    /// never leave the overlay stuck invisible.
    pub(crate) fn update_hover_presentation(
        &mut self,
        options: OverlaySessionOptions,
        cursor: Option<CursorSample>,
        input_running: bool,
    ) -> Result<(), OverlayError> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("macOS overlay hover update lost the main thread"))?;
        let bounds = self.window_bounds()?;
        // A right-button resize drag keeps the overlay visible: the hover hide
        // fades the window out and starts passing pointer events through, which
        // would end the drag.
        let resizing = self
            .right_button_monitor
            .as_ref()
            .is_some_and(RightButtonMonitor::is_resize_dragging);
        let pointer_inside = !resizing
            && cursor
                .and_then(|sample| appkit_cursor_position(sample, mtm))
                .is_some_and(|position| pointer_inside_window(bounds, position.x, position.y));
        let fade = self.hover.observe(PointerHoverObservation {
            enabled: options.hide_on_pointer_hover && input_running,
            delay: Duration::from_millis(u64::from(options.hide_on_pointer_hover_delay_ms)),
            pointer_inside,
            now: self.session_started.elapsed(),
        });
        let alpha = f64::from(options.opacity_percent) / 100.0 * fade;
        self.overlay
            .apply_presentation(alpha, options.click_through || self.hover.hidden());
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
        mtm: MainThreadMarker,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
    ) -> Result<NativeOverlay, OverlayError> {
        let mut overlay = NativeOverlay::create(mtm, frame, options, bounds)?;
        let alpha = f64::from(options.opacity_percent) / 100.0 * self.hover.visible();
        overlay.apply_presentation(alpha, options.click_through || self.hover.hidden());
        Ok(overlay)
    }

    pub(crate) fn window_bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        let frame = self.overlay.panel.frame();
        OverlayWindowBounds::new(
            rounded_i32(frame.origin.x)?,
            rounded_i32(frame.origin.y)?,
            rounded_u32(frame.size.width)?,
            rounded_u32(frame.size.height)?,
        )
        .validate()
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.overlay.panel.isVisible()
    }

    pub(crate) fn model_generation(&self) -> u64 {
        self.overlay.model_generation
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
        self.remove_right_button_monitor();
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
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("macOS overlay shutdown lost the main thread"))?;
        let bounds = self.window_bounds()?;
        let placement_fully_visible = !self.options.keep_inside_screen
            || bounds_inside_screens(&screen_bounds_all(mtm)?, bounds);
        Ok(ProductOverlayReport {
            frames_presented: self.frames_presented,
            placement_fully_visible,
            dynamic_snapshots: self.dynamic_snapshots,
            model_commit_rejections: self.model_commit_rejections,
            input_start_error: self.input_start_error,
            input_diagnostics: self.input_diagnostics,
            render_diagnostics: self.render_consumer.diagnostics(),
            model_generation: self.overlay.model_generation,
            drawable_count: self.overlay.model.meshes.len(),
            masked_drawable_count: self.overlay.model.masked_drawable_count,
            texture_count: self.overlay.model.textures.len(),
        })
    }
}

impl HasWindowHandle for ProductOverlaySession {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let view = self
            .overlay
            .panel
            .contentView()
            .ok_or(HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(NonNull::from(&*view).cast());
        // SAFETY: the panel retains its content view while this session and the
        // returned borrow of it remain alive.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl ProductOverlaySession {
    /// The window size a resize drag treats as `100%`.
    ///
    /// It is the same size the window would be created with for the current
    /// model, so a drag maps onto the scale the settings page shows, and a
    /// model switch with a different canvas aspect ratio keeps the mapping
    /// correct because the base is recomputed from the current frame.
    pub(crate) fn resize_base(&self) -> Option<ResizeBase> {
        let (width, height) = default_overlay_window_dimensions(self.last_frame.snapshot.canvas);
        ResizeBase::new(f64::from(width), f64::from(height))
    }

    /// Whether the live window box already matches a scale.
    ///
    /// A resize drag resizes the window before the scale reaches the
    /// configuration, so the in-place resize that follows the write-back must
    /// not scale the box a second time. See [`crate::bounds_match_scale`].
    pub(crate) fn bounds_match_scale(
        &self,
        bounds: OverlayWindowBounds,
        scale_percent: u16,
    ) -> bool {
        self.resize_base()
            .is_some_and(|base| crate::bounds_match_scale(bounds, base, scale_percent))
    }

    /// Re-install the right-button monitor for the current panel.
    ///
    /// A replacement window is a different panel with a different window
    /// number, so the monitor that belonged to the old one is dropped first.
    pub(crate) fn refresh_right_button_monitor(&mut self, mtm: MainThreadMarker) {
        self.remove_right_button_monitor();
        self.right_button_monitor = install_context_menu_monitor(
            mtm,
            Retained::clone(&self.overlay.panel),
            self.resize_base(),
            self.context_menu_sender.clone(),
            self.resize_sender.clone(),
        );
    }

    pub(crate) fn remove_right_button_monitor(&mut self) {
        if let Some(monitor) = self.right_button_monitor.take() {
            // SAFETY: this monitor was created by NSEvent for this session and is
            // removed on the AppKit main thread before its callback state drops.
            unsafe { NSEvent::removeMonitor(&monitor.token) };
        }
    }
}

impl Drop for ProductOverlaySession {
    fn drop(&mut self) {
        self.remove_right_button_monitor();
    }
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

pub(crate) fn validate_product_options(options: OverlaySessionOptions) -> Result<(), OverlayError> {
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
    if options.hide_on_pointer_hover_delay_ms > MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS {
        return Err(OverlayError::new(
            "overlay hover hide delay must be between 0 and 60000 milliseconds",
        ));
    }
    if !maximum_fps_is_valid(options.maximum_fps) {
        return Err(OverlayError::new("overlay FPS must be between 15 and 240"));
    }
    Ok(())
}
