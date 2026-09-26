//! The runtime's lifetime: starting the worker, requesting a stop and joining it.
//!
//! Shutdown never blocks the caller indefinitely. The owner asks, the worker
//! acknowledges by publishing `Stopped`, and the join is bounded; a worker that
//! overruns its budget is reported with the diagnostics it managed to publish
//! rather than being waited on forever.

use crate::pacing::SystemMonotonicClock;
use crate::transport::{
    CommandTransportCounters, Producer, RuntimeInputSubmitter, ShutdownDiagnosticsCounters,
    SnapshotCell,
};
use crate::worker::{RuntimeWorkerBootstrap, run_worker};
use crate::*;

pub struct RuntimeOwner {
    pub(crate) client: RuntimeClient,
    pub(crate) worker: Option<JoinHandle<()>>,
    pub(crate) shutdown: Arc<ShutdownSignal>,
    pub(crate) shutdown_diagnostics: Arc<ShutdownDiagnosticsCounters>,
}

#[derive(Default)]
pub(crate) struct ShutdownSignal {
    pub(crate) state: Mutex<ShutdownState>,
}

#[derive(Default)]
pub(crate) struct ShutdownState {
    pub(crate) sequence: Option<u64>,
    pub(crate) automatic_in_flight: usize,
}

pub(crate) struct AutomaticSideEffectGuard<'a> {
    pub(crate) signal: &'a ShutdownSignal,
}

impl Drop for AutomaticSideEffectGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .signal
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.automatic_in_flight = state.automatic_in_flight.saturating_sub(1);
    }
}

impl ShutdownSignal {
    pub(crate) fn request(&self, sequence: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.sequence = Some(sequence);
    }

    pub(crate) fn sequence(&self) -> Option<u64> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sequence
    }

    /// Admits an automatic side effect before shutdown, then releases the
    /// state lock while the action runs. The request path can therefore mark
    /// shutdown and begin its bounded wait immediately, while an action that
    /// was admitted first still finishes on the worker thread.
    pub(crate) fn run_if_not_shutdown(&self, action: impl FnOnce()) {
        let guard = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.sequence.is_some() {
                return;
            }
            state.automatic_in_flight = state.automatic_in_flight.saturating_add(1);
            AutomaticSideEffectGuard { signal: self }
        };
        action();
        drop(guard);
    }
}

impl RuntimeOwner {
    pub fn start(initial_overlay_visible: bool, command_capacity: usize) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            false,
            command_capacity,
            None,
            MotionAudioClient::unavailable(),
            Arc::new(SystemMonotonicClock::start()),
            false,
            Duration::ZERO,
        )
    }

    pub fn start_with_audio(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
    ) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            initial_motion_audio_enabled,
            command_capacity,
            None,
            motion_audio,
            Arc::new(SystemMonotonicClock::start()),
            false,
            Duration::ZERO,
        )
    }

    pub fn start_with_rendering(
        initial_overlay_visible: bool,
        command_capacity: usize,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                false,
                command_capacity,
                Some(renderer),
                MotionAudioClient::unavailable(),
                Arc::new(SystemMonotonicClock::start()),
                false,
                Duration::ZERO,
            ),
            consumer,
        )
    }

    pub fn start_with_rendering_and_audio(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                command_capacity,
                Some(renderer),
                motion_audio,
                Arc::new(SystemMonotonicClock::start()),
                false,
                Duration::ZERO,
            ),
            consumer,
        )
    }

    pub fn start_with_rendering_and_clock(
        initial_overlay_visible: bool,
        command_capacity: usize,
        clock: Arc<dyn MonotonicClock>,
    ) -> (Self, RenderConsumer) {
        Self::start_with_rendering_audio_and_clock(
            initial_overlay_visible,
            false,
            command_capacity,
            MotionAudioClient::unavailable(),
            clock,
        )
    }

    pub fn start_with_rendering_audio_and_clock(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
        clock: Arc<dyn MonotonicClock>,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                command_capacity,
                Some(renderer),
                motion_audio,
                clock,
                false,
                Duration::ZERO,
            ),
            consumer,
        )
    }

    #[cfg(test)]
    pub(crate) fn start_with_worker_panic(
        initial_overlay_visible: bool,
        command_capacity: usize,
    ) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            false,
            command_capacity,
            None,
            MotionAudioClient::unavailable(),
            Arc::new(SystemMonotonicClock::start()),
            true,
            Duration::ZERO,
        )
    }

    #[cfg(test)]
    pub(crate) fn start_with_shutdown_delay(
        initial_overlay_visible: bool,
        command_capacity: usize,
        shutdown_delay: Duration,
    ) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            false,
            command_capacity,
            None,
            MotionAudioClient::unavailable(),
            Arc::new(SystemMonotonicClock::start()),
            false,
            shutdown_delay,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_internal(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        renderer: Option<RuntimeRenderBootstrap>,
        motion_audio: MotionAudioClient,
        clock: Arc<dyn MonotonicClock>,
        panic_after_stopped: bool,
        shutdown_delay: Duration,
    ) -> Self {
        assert!(
            command_capacity > 0,
            "runtime command capacity must be non-zero"
        );
        let (sender, receiver) = mpsc::sync_channel(command_capacity);
        let snapshot = Arc::new(SnapshotCell {
            value: Mutex::new(RuntimeSnapshot::starting(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                motion_audio.diagnostics(),
            )),
            changed: Condvar::new(),
        });
        let platform_input_diagnostics = PlatformInputDiagnosticsProducer::default();
        let command_transport = Arc::new(CommandTransportCounters::default());
        let shutdown_diagnostics = Arc::new(ShutdownDiagnosticsCounters::default());
        let accepting = Arc::new(AtomicBool::new(true));
        let producer = Arc::new(Producer {
            sender,
            next_sequence: Mutex::new(0),
            command_transport: Arc::clone(&command_transport),
            accepting: Arc::clone(&accepting),
        });
        let input_producer = InputProducer::new(Arc::new(RuntimeInputSubmitter {
            producer: Arc::clone(&producer),
        }));
        let cursor_producer = CursorProducer::new();
        let gamepad_axis_producer =
            GamepadAxisProducer::with_capacity(DEFAULT_GAMEPAD_AXIS_CAPACITY);
        let shutdown = Arc::new(ShutdownSignal::default());
        let worker_shutdown = Arc::clone(&shutdown);
        let client = RuntimeClient {
            producer,
            snapshot: Arc::clone(&snapshot),
            input_producer,
            cursor_producer: cursor_producer.clone(),
            gamepad_axis_producer: gamepad_axis_producer.clone(),
            platform_input_diagnostics,
            motion_audio: motion_audio.clone(),
            shutdown_diagnostics: Arc::clone(&shutdown_diagnostics),
        };
        let worker = thread::Builder::new()
            .name("bongocat-runtime".into())
            .spawn(move || {
                run_worker(
                    receiver,
                    RuntimeWorkerBootstrap {
                        snapshot,
                        cursor_producer,
                        gamepad_axis_producer,
                        initial_overlay_visible,
                        initial_motion_audio_enabled,
                        renderer,
                        motion_audio,
                        clock,
                        command_transport,
                        shutdown: worker_shutdown,
                        panic_after_stopped,
                        shutdown_delay,
                    },
                )
            })
            .expect("failed to start runtime thread");
        Self {
            client,
            worker: Some(worker),
            shutdown,
            shutdown_diagnostics,
        }
    }

    pub fn client(&self) -> RuntimeClient {
        self.client.clone()
    }

    pub fn input_producer(&self) -> InputProducer {
        self.client.input_producer.clone()
    }

    pub fn cursor_producer(&self) -> CursorProducer {
        self.client.cursor_producer.clone()
    }

    pub fn gamepad_axis_producer(&self) -> GamepadAxisProducer {
        self.client.gamepad_axis_producer()
    }

    pub fn platform_input_diagnostics_producer(&self) -> PlatformInputDiagnosticsProducer {
        self.client.platform_input_diagnostics_producer()
    }

    pub fn shutdown(mut self, timeout: Duration) -> Result<RuntimeSnapshot, ShutdownError> {
        let current = self.client.snapshot();
        if current.state == RuntimeState::Stopped {
            self.join_worker()?;
            return Ok(current);
        }
        self.request_shutdown();
        let Some(stopped) = self.client.wait_for_state(RuntimeState::Stopped, timeout) else {
            // An explicit timeout is a bounded API contract. Move the join into a
            // small watcher so the worker can finish its admitted drain without
            // making the caller wait, while still aggregating a late panic.
            self.detach_worker_join();
            self.shutdown_diagnostics
                .timed_out
                .fetch_add(1, Ordering::Relaxed);
            return Err(ShutdownError::TimedOut);
        };
        self.join_worker()?;
        Ok(stopped)
    }

    pub(crate) fn detach_worker_join(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let diagnostics = Arc::clone(&self.shutdown_diagnostics);
        let _ = thread::Builder::new()
            .name("bongocat-runtime-shutdown-join".into())
            .spawn(move || {
                if worker.join().is_err() {
                    diagnostics.worker_panicked.fetch_add(1, Ordering::Relaxed);
                }
            });
    }

    pub(crate) fn join_worker(&mut self) -> Result<(), ShutdownError> {
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| {
                self.shutdown_diagnostics
                    .worker_panicked
                    .fetch_add(1, Ordering::Relaxed);
                ShutdownError::WorkerPanicked
            })?;
        }
        Ok(())
    }

    pub(crate) fn request_shutdown(&self) {
        self.client.cursor_producer.stop();
        self.client.gamepad_axis_producer.stop();
        self.client.platform_input_diagnostics.stop();
        let mut next_sequence = self
            .client
            .producer
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.client.producer.accepting.swap(false, Ordering::AcqRel) {
            let sequence = *next_sequence;
            *next_sequence = next_sequence.wrapping_add(1);
            self.shutdown.request(sequence);
        }
    }
}

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        self.request_shutdown();
        let _ = self.join_worker();
    }
}
