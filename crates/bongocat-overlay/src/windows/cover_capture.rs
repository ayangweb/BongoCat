//! Rendering a model's own cover, off-screen.
//!
//! The capture reuses the product's real renderer against an off-screen target
//! so the stored cover is the model as the user will see it, not a separate
//! preview path that can drift from it. It is driven one step at a time so the
//! caller's main loop keeps running between frames.

use super::*;

/// One model cover capture, drawn a frame at a time.
///
/// This is the preview's own setup — a private runtime, the model activated into
/// it, and a native window sized from the model's canvas — with two differences
/// that make it a capture rather than a preview: the window is created without
/// `WS_VISIBLE` and never shown, so nothing the user did not ask for appears on
/// screen, and every frame of it is read out of the staging texture instead of
/// being presented.
///
/// It must run on the thread that owns the window it creates, because each
/// `step` pumps that window's messages. The session exists so the owner does not
/// have to stay busy for the whole capture: `start` sets everything up and draws
/// the first, verified frame, every `step` draws one more frame, and `finish`
/// tears the runtime down. The caller waits between steps with
/// [`Self::frame_interval`], which is what keeps the product's windows pumping
/// while the capture's model settles (ADR-0055).
pub(crate) struct CoverCaptureSession {
    pub(crate) overlay: NativeOverlay,
    pub(crate) runtime: RuntimeOwner,
    pub(crate) render_consumer: RenderConsumer,
    pub(crate) captured: Option<CapturedFrame>,
    pub(crate) deadline: Instant,
    pub(crate) frames_drawn: u32,
    pub(crate) frame_interval: Duration,
    // Declared last so the guard outlives the window's COM usage on drop.
    pub(crate) _com_apartment: ComApartment,
}

impl CoverCaptureSession {
    pub(crate) fn start(model: Arc<CommittedModel>) -> Result<Self, OverlayError> {
        let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
        let runtime_client = runtime.client();
        runtime_client
            .wait_for_revision(1, RUNTIME_TIMEOUT)
            .ok_or_else(|| OverlayError::new("cover capture runtime did not become ready"))?;
        let activation = runtime_client
            .send(RuntimeCommand::ActivateModel(model))
            .map_err(|error| OverlayError::new(error.to_string()))?;
        let prepared = runtime_client
            .wait_for_model_preparation(activation, RUNTIME_TIMEOUT)
            .ok_or_else(|| OverlayError::new("cover capture model activation was not prepared"))?;
        if let Some(failure) = prepared
            .last_command_failure
            .filter(|failure| failure.sequence == activation)
        {
            return Err(OverlayError::new(format!(
                "cover capture model activation failed: {:?}",
                failure.code
            )));
        }
        let initial_frame = render_consumer
            .take_latest()
            .ok_or_else(|| OverlayError::new("cover capture runtime published no render frame"))?;
        let initial_token = initial_frame
            .model_commit
            .filter(|token| token.command_sequence == activation)
            .ok_or_else(|| {
                OverlayError::new("cover capture frame has the wrong model commit token")
            })?;
        let com_apartment = ComApartment::initialize()?;
        let options = OverlaySessionOptions {
            scale_percent: COVER_CAPTURE_SCALE_PERCENT,
            keep_inside_screen: false,
            ..OverlaySessionOptions::default()
        };
        let frame_interval = frame_interval_for_maximum_fps(options.maximum_fps)
            .expect("cover capture options carry a validated maximum FPS");
        let mut overlay = match NativeOverlay::create(&initial_frame, options, None, None, None) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
                return Err(error);
            }
        };
        report_model_commit(
            &runtime_client,
            &render_consumer,
            initial_token,
            ModelCommitOutcome::Prepared,
        )?;

        // The first frame is verified like a committed model is: a capture that
        // renders nothing must fail before it replaces a cover with a blank image.
        // The frames after it are not, because a settled animation is allowed to be
        // mostly transparent while the model moves.
        let captured = Some(overlay.draw_capturing(true)?);
        Ok(Self {
            overlay,
            runtime,
            render_consumer,
            captured,
            deadline: Instant::now() + COVER_CAPTURE_TIMEOUT,
            frames_drawn: 1,
            frame_interval,
            _com_apartment: com_apartment,
        })
    }

    /// The wait the caller should observe between two `step` calls so the
    /// capture's model animates at the pace the overlay renders at.
    pub(crate) fn frame_interval(&self) -> Duration {
        self.frame_interval
    }

    /// Draw one more capture frame. `Ok(false)` means every frame is drawn or
    /// the deadline has passed, and `finish` is the next call.
    pub(crate) fn step(&mut self) -> Result<bool, OverlayError> {
        if self.frames_drawn >= COVER_CAPTURE_FRAMES || Instant::now() >= self.deadline {
            return Ok(false);
        }
        pump_window_messages();
        if let Some(frame) = self.render_consumer.take_latest() {
            self.overlay.renderer.sync_frame(&frame)?;
        }
        self.captured = Some(self.overlay.draw_capturing(false)?);
        self.frames_drawn += 1;
        Ok(true)
    }

    /// Stop the capture runtime and return the most recent captured frame.
    ///
    /// Dropping the session without `finish` (a failed `step`, an abandoned
    /// capture) tears the runtime down through its own `Drop`, exactly like the
    /// early returns of a blocking capture did.
    pub(crate) fn finish(self) -> Result<CapturedFrame, OverlayError> {
        self.runtime
            .shutdown(RUNTIME_TIMEOUT)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        self.captured
            .ok_or_else(|| OverlayError::new("cover capture drew no frame"))
    }
}
