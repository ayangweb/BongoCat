//! Headless Vulkan model cover capture.

use super::*;

pub(crate) struct CoverCaptureSession {
    renderer: Renderer,
    runtime: RuntimeOwner,
    render_consumer: RenderConsumer,
    captured: Option<CapturedFrame>,
    deadline: Instant,
    frames_drawn: u32,
    frame_interval: Duration,
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
        let options = OverlaySessionOptions {
            scale_percent: COVER_CAPTURE_SCALE_PERCENT,
            keep_inside_screen: false,
            ..OverlaySessionOptions::default()
        };
        let (width, height) =
            model_window_dimensions(initial_frame.snapshot.canvas, COVER_CAPTURE_SCALE_PERCENT);
        let mut renderer = match Renderer::create_headless(&initial_frame, options, width, height) {
            Ok(renderer) => renderer,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
                return Err(error);
            }
        };
        let captured = match renderer.draw_capturing(true) {
            Ok(captured) => captured,
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
        let frame_interval = frame_interval_for_maximum_fps(options.maximum_fps)
            .expect("cover capture options carry a validated maximum FPS");
        Ok(Self {
            renderer,
            runtime,
            render_consumer,
            captured: Some(captured),
            deadline: Instant::now() + COVER_CAPTURE_TIMEOUT,
            frames_drawn: 1,
            frame_interval,
        })
    }

    pub(crate) fn frame_interval(&self) -> Duration {
        self.frame_interval
    }

    pub(crate) fn step(&mut self) -> Result<bool, OverlayError> {
        if self.frames_drawn >= COVER_CAPTURE_FRAMES || Instant::now() >= self.deadline {
            return Ok(false);
        }
        if let Some(frame) = self.render_consumer.take_latest() {
            self.renderer.sync_frame(&frame)?;
        }
        self.captured = Some(self.renderer.draw_capturing(false)?);
        self.frames_drawn += 1;
        Ok(true)
    }

    pub(crate) fn finish(self) -> Result<CapturedFrame, OverlayError> {
        self.runtime
            .shutdown(RUNTIME_TIMEOUT)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        self.captured
            .ok_or_else(|| OverlayError::new("cover capture drew no frame"))
    }
}
