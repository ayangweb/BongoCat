//! Rendering a model's own cover, off-screen.
//!
//! The capture reuses the product's real renderer against an off-screen
//! texture so the stored cover is the model as the user will see it, not a
//! separate preview path that can drift from it. It is driven one step at a
//! time so the caller's main loop keeps running between frames.

use super::*;

/// One model cover capture, drawn a frame at a time.
///
/// This is the model preview's own setup — a private runtime, the model activated
/// into it, and a native window sized from the model's canvas — with two
/// differences that make it a capture rather than a preview: the window is never
/// ordered front, so nothing the user did not ask for appears on screen, and the
/// frame is read out of the drawable instead of only sampled.
///
/// It must run on the main thread, like every other AppKit window owner. The
/// session exists so the owner does not have to stay busy for the whole capture:
/// `start` sets everything up and draws the first, verified frame, every `step`
/// draws one more frame, and `finish` tears the runtime down. The caller waits
/// between steps with [`Self::frame_interval`], which is what keeps the product's
/// windows redrawing while the capture's model settles (ADR-0055).
pub(crate) struct CoverCaptureSession {
    pub(crate) mtm: MainThreadMarker,
    pub(crate) overlay: NativeOverlay,
    pub(crate) runtime: RuntimeOwner,
    pub(crate) render_consumer: RenderConsumer,
    pub(crate) captured: Option<CapturedFrame>,
    pub(crate) deadline: Instant,
    pub(crate) frames_drawn: u32,
    pub(crate) frame_interval: Duration,
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
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| OverlayError::new("cover capture must run on the main thread"))?;
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        application.finishLaunching();
        let options = OverlaySessionOptions {
            scale_percent: COVER_CAPTURE_SCALE_PERCENT,
            keep_inside_screen: false,
            ..OverlaySessionOptions::default()
        };
        let mut overlay = match NativeOverlay::create(mtm, &initial_frame, options, None) {
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
        let captured = Some(overlay.draw_capturing(true)?);
        Ok(Self {
            mtm,
            overlay,
            runtime,
            render_consumer,
            captured,
            deadline: Instant::now() + COVER_CAPTURE_TIMEOUT,
            frames_drawn: 1,
            frame_interval: FRAME_INTERVAL,
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
        pump_application_events(&NSApplication::sharedApplication(self.mtm));
        if let Some(frame) = self.render_consumer.take_latest() {
            self.overlay.sync_frame(&frame)?;
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
