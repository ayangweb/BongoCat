//! The service that owns the worker thread.
//!
//! Shutdown is the part worth reading: the voice is stopped, the worker is asked
//! to finish, and the join has a timeout — a blocked audio device must not be able
//! to hold up the application's exit. A worker that panicked is reported rather
//! than swallowed, because a sound that stopped working and said nothing is the
//! failure this whole crate exists to prevent.

use super::*;

pub(crate) struct SharedState {
    pub(crate) diagnostics: Mutex<MotionAudioDiagnostics>,
    pub(crate) changed: Condvar,
    pub(crate) publish_lock: Mutex<()>,
    pub(crate) shutdown_requested: AtomicBool,
    pub(crate) overflow_recovery_requested: AtomicBool,
    pub(crate) next_sequence: AtomicU64,
}

impl SharedState {
    pub(crate) fn publish(&self, update: impl FnOnce(&mut MotionAudioDiagnostics)) {
        let mut diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut diagnostics);
        self.changed.notify_all();
    }

    pub(crate) fn snapshot(&self) -> MotionAudioDiagnostics {
        self.diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

pub struct MotionAudioService {
    pub(crate) client: MotionAudioClient,
    pub(crate) worker: Option<JoinHandle<()>>,
}

impl MotionAudioService {
    pub fn start(command_capacity: usize) -> Result<Self, MotionAudioStartError> {
        Self::start_with_backend(command_capacity, Box::<SystemAudioBackend>::default())
    }

    pub(crate) fn start_with_backend(
        command_capacity: usize,
        backend: Box<dyn AudioBackend>,
    ) -> Result<Self, MotionAudioStartError> {
        Self::start_with_backend_internal(command_capacity, backend, false)
    }

    #[cfg(test)]
    pub(crate) fn start_with_worker_panic(
        command_capacity: usize,
        backend: Box<dyn AudioBackend>,
    ) -> Result<Self, MotionAudioStartError> {
        Self::start_with_backend_internal(command_capacity, backend, true)
    }

    pub(crate) fn start_with_backend_internal(
        command_capacity: usize,
        backend: Box<dyn AudioBackend>,
        panic_after_stopped: bool,
    ) -> Result<Self, MotionAudioStartError> {
        assert!(
            command_capacity > 0,
            "audio command capacity must be non-zero"
        );
        let (sender, receiver) = mpsc::sync_channel(command_capacity);
        let shared = Arc::new(SharedState {
            diagnostics: Mutex::new(MotionAudioDiagnostics::starting()),
            changed: Condvar::new(),
            publish_lock: Mutex::new(()),
            shutdown_requested: AtomicBool::new(false),
            overflow_recovery_requested: AtomicBool::new(false),
            next_sequence: AtomicU64::new(0),
        });
        let client = MotionAudioClient {
            sender,
            shared: Arc::clone(&shared),
        };
        let worker = thread::Builder::new()
            .name("bongocat-motion-audio".into())
            .spawn(move || run_worker(receiver, shared, backend, panic_after_stopped))
            .map_err(MotionAudioStartError)?;
        Ok(Self {
            client,
            worker: Some(worker),
        })
    }

    pub fn client(&self) -> MotionAudioClient {
        self.client.clone()
    }

    pub fn shutdown(
        mut self,
        timeout: Duration,
    ) -> Result<MotionAudioDiagnostics, MotionAudioShutdownError> {
        self.request_shutdown();
        let stopped = match self.wait_until_stopped(timeout) {
            Ok(stopped) => stopped,
            Err(error @ MotionAudioShutdownError::TimedOut) => {
                // A bounded shutdown must not fall through to `Drop`, whose
                // fallback join is intentionally only used for normal owner
                // destruction. The worker has received the stop request and
                // will finish its drain asynchronously.
                self.worker.take();
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        self.join_worker()?;
        Ok(stopped)
    }

    pub(crate) fn request_shutdown(&self) {
        self.client
            .shared
            .shutdown_requested
            .store(true, Ordering::Release);
        self.client.shared.changed.notify_all();
    }

    pub(crate) fn wait_until_stopped(
        &self,
        timeout: Duration,
    ) -> Result<MotionAudioDiagnostics, MotionAudioShutdownError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(MotionAudioShutdownError::TimedOut)?;
        let mut diagnostics = self
            .client
            .shared
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while diagnostics.state != MotionAudioState::Stopped {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(MotionAudioShutdownError::TimedOut);
            }
            let (next, result) = self
                .client
                .shared
                .changed
                .wait_timeout(diagnostics, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            diagnostics = next;
            if result.timed_out() && diagnostics.state != MotionAudioState::Stopped {
                return Err(MotionAudioShutdownError::TimedOut);
            }
        }
        Ok(diagnostics.clone())
    }

    pub(crate) fn join_worker(&mut self) -> Result<(), MotionAudioShutdownError> {
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| MotionAudioShutdownError::WorkerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for MotionAudioService {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        self.request_shutdown();
        let _ = self.join_worker();
    }
}
