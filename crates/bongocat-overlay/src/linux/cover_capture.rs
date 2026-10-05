use super::*;
use bongocat_runtime::{RuntimeCommand, RuntimeOwner};
pub struct CoverCaptureSession {
    runtime: RuntimeOwner,
    consumer: RenderConsumer,
    renderer: renderer::Renderer,
    frame: RenderFrame,
    remaining: u32,
    started: Instant,
}
impl CoverCaptureSession {
    pub fn start(model: Arc<bongocat_model::CommittedModel>) -> Result<Self, OverlayError> {
        let (runtime, consumer) = RuntimeOwner::start_with_rendering(true, 64);
        let client = runtime.client();
        let timeout = Duration::from_secs(2);
        client
            .wait_for_revision(1, timeout)
            .ok_or_else(|| err("cover runtime not ready"))?;
        let command = client
            .send(RuntimeCommand::ActivateModel(model))
            .map_err(err)?;
        client
            .wait_for_model_preparation(command, timeout)
            .ok_or_else(|| err("cover model not prepared"))?;
        let frame = consumer
            .take_latest()
            .ok_or_else(|| err("cover frame missing"))?;
        let (w, h) = model_window_dimensions(
            frame.snapshot.canvas,
            crate::cover::COVER_CAPTURE_SCALE_PERCENT,
        );
        let renderer = renderer::Renderer::new(None, &frame, w, h)?;
        feedback(&client, &consumer, &frame, ModelCommitOutcome::Prepared)?;
        Ok(Self {
            runtime,
            consumer,
            renderer,
            frame,
            remaining: crate::cover::COVER_CAPTURE_FRAMES,
            started: Instant::now(),
        })
    }
    pub fn frame_interval(&self) -> Duration {
        Duration::from_millis(16)
    }
    pub fn step(&mut self) -> Result<bool, OverlayError> {
        if self.remaining == 0 || self.started.elapsed() >= crate::cover::COVER_CAPTURE_TIMEOUT {
            return Ok(false);
        }
        if let Some(frame) = self.consumer.take_latest() {
            self.frame = frame;
        }
        self.renderer
            .draw(&self.frame, OverlaySessionOptions::default(), true, 1.0)?;
        self.remaining -= 1;
        Ok(self.remaining > 0)
    }
    pub fn finish(self) -> Result<crate::cover::CapturedFrame, OverlayError> {
        let frame = self.renderer.readback();
        self.runtime.shutdown(Duration::from_secs(2)).map_err(err)?;
        frame
    }
}
