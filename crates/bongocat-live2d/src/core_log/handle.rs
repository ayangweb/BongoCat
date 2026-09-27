//! The handle the application installs and drops, and the counters it reads.
//!
//! The handle owns the callback's installation, which is the whole safety
//! argument for the FFI boundary: Cubism keeps calling the callback pointer
//! until it is removed, so the sink must outlive every possible call and the
//! callback must be removed before the sink is released. `Drop` is where that
//! happens, and it is why the handle is the type that owns the sink rather than
//! the other way round.

use super::*;

pub(crate) const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, thiserror::Error)]
pub enum CoreLogError {
    #[error("cannot create Core log directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("cannot open Core log file: {0}")]
    OpenFile(std::io::Error),
    #[error("cannot start Core log worker: {0}")]
    StartWorker(std::io::Error),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogStats {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

/// Owns the process-wide Cubism Core callback installation.
///
/// Cubism exposes one global callback and no user-data pointer. The handle
/// keeps the sink alive until it is dropped and removes the callback before
/// releasing the sink, so the FFI callback cannot observe freed Rust state.
#[derive(Debug)]
pub struct CoreLogHandle {
    pub(crate) sink: Arc<CoreLogSink>,
    pub(crate) worker: Option<JoinHandle<()>>,
}

/// Read-only access to the anonymous retention counters maintained by the
/// process-wide Core log callback.
#[derive(Clone, Debug)]
pub struct CoreLogReporter {
    pub(crate) sink: Arc<CoreLogSink>,
}

impl CoreLogHandle {
    pub fn install(
        directory: impl AsRef<Path>,
        settings: LogSettingsController,
    ) -> Result<Self, CoreLogError> {
        let directory = directory.as_ref();
        fs::create_dir_all(directory).map_err(CoreLogError::CreateDirectory)?;
        set_private_directory(directory).map_err(CoreLogError::CreateDirectory)?;
        let writer = TextLogWriter::open(directory, LogStream::CubismCore, settings)
            .map_err(CoreLogError::OpenFile)?;
        let (sender, receiver) = sync_channel(CALLBACK_QUEUE_CAPACITY);
        let sink = Arc::new(CoreLogSink {
            state: Mutex::new(CoreLogState { writer }),
            sender,
            accepting: AtomicBool::new(true),
            callback_dropped: AtomicU64::new(0),
            global_drop_baseline: CORE_LOG_CALLBACK_DROPS.load(Ordering::Relaxed),
        });
        let worker_sink = Arc::clone(&sink);
        let worker = thread::Builder::new()
            .name("bongocat-core-log".to_owned())
            .spawn(move || worker_sink.run_worker(receiver))
            .map_err(CoreLogError::StartWorker)?;
        let mut slot = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Core has one callback slot. Replacing an existing sink is safe only
        // after the old callback is disabled and its Arc is removed.
        // SAFETY: the callback is process-global and no Rust pointer is passed
        // through it; setting it to None prevents future callback entry.
        unsafe { sys::csmSetLogFunction(None) };
        *slot = Some(Arc::clone(&sink));
        // SAFETY: `core_log_callback` has the ABI generated for this exact
        // Cubism Core header and never lets a panic cross the FFI boundary.
        unsafe { sys::csmSetLogFunction(Some(core_log_callback)) };
        drop(slot);
        Ok(Self {
            sink,
            worker: Some(worker),
        })
    }

    pub fn stats(&self) -> CoreLogStats {
        self.sink.stats()
    }

    pub fn reporter(&self) -> CoreLogReporter {
        CoreLogReporter {
            sink: Arc::clone(&self.sink),
        }
    }
}

impl CoreLogReporter {
    pub fn stats(&self) -> CoreLogStats {
        self.sink.stats()
    }
}

impl Drop for CoreLogHandle {
    fn drop(&mut self) {
        let mut slot = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.sink))
        {
            // SAFETY: disabling the callback happens before the final Arc is
            // released, so Core cannot enter Rust with a dangling sink.
            unsafe { sys::csmSetLogFunction(None) };
            *slot = None;
        }
        drop(slot);
        self.sink.accepting.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
