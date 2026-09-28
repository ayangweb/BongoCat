//! The handle the application holds, and the counters it reads.
//!
//! The service owns the thread and nothing else, so `stop` is the whole teardown:
//! it asks the owner to finish, joins it with a timeout, and reports what the
//! platform said on the way out. The counters are atomic rather than locked
//! because the owner thread writes them and the application reads them from a
//! different thread while the service is still running.

use super::*;

/// How often the owner thread re-reads the shared shortcut table. Every
/// table publisher (`set_shortcuts`, capture suspend/resume, behavior
/// toggles) goes through `ShortcutTable::replace`, so polling covers all
/// of them without a dedicated notification channel.
pub(crate) const TABLE_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum GlobalShortcutServiceError {
    #[error("global hotkey manager unavailable: {0}")]
    ManagerUnavailable(String),
    #[error("global shortcut service startup timed out")]
    StartupTimedOut,
    #[error("global shortcut worker panicked")]
    WorkerPanicked,
}

/// Live counters for diagnostics consumers; the input pipeline no longer
/// counts shortcut dispatch because it no longer performs matching.
#[derive(Debug, Default)]
pub struct GlobalShortcutCounters {
    pub registration_failures: AtomicU64,
    pub queue_overflows: AtomicU64,
    pub runtime_stopped_events: AtomicU64,
}

/// A long-lived owner of the platform global hotkey manager. Dropping or
/// stopping the service unregisters every binding it registered.
pub struct GlobalShortcutService {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) owner: Option<std::thread::JoinHandle<()>>,
    pub(crate) registration_failures: Arc<Mutex<Vec<String>>>,
    pub(crate) counters: Arc<GlobalShortcutCounters>,
}

impl GlobalShortcutService {
    /// Starts the owner thread. The manager is created on that thread, so
    /// callers do not need to be on the platform main thread.
    pub fn start(
        table: ShortcutTable,
        dispatcher: ShortcutDispatcher,
    ) -> Result<Self, GlobalShortcutServiceError> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let registration_failures = Arc::new(Mutex::new(Vec::new()));
        let counters = Arc::new(GlobalShortcutCounters::default());
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let thread_failures = Arc::clone(&registration_failures);
        let thread_counters = Arc::clone(&counters);
        let owner = std::thread::Builder::new()
            .name("bongocat-global-shortcuts".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_shortcut_owner(
                        table,
                        dispatcher,
                        worker_stop,
                        &startup_sender,
                        thread_failures,
                        thread_counters,
                    )
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        let _ = startup_sender.send(Err(error));
                    }
                    Err(_) => {
                        let _ =
                            startup_sender.send(Err(GlobalShortcutServiceError::WorkerPanicked));
                    }
                }
            })
            .map_err(|_| GlobalShortcutServiceError::WorkerPanicked)?;
        match startup_receiver.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stop,
                owner: Some(owner),
                registration_failures,
                counters,
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = owner.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = owner.join();
                Err(GlobalShortcutServiceError::StartupTimedOut)
            }
        }
    }

    /// Stops the owner thread and unregisters every binding. `Drop` covers
    /// the remaining paths.
    pub fn stop(mut self) -> Result<(), GlobalShortcutServiceError> {
        self.stop.store(true, Ordering::Release);
        match self.owner.take() {
            Some(owner) => owner
                .join()
                .map_err(|_| GlobalShortcutServiceError::WorkerPanicked),
            None => Ok(()),
        }
    }

    /// Bindings the platform refused (already taken by another app, or no
    /// platform scancode). The remaining bindings stay registered.
    pub fn registration_failures(&self) -> Vec<String> {
        self.registration_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn counters(&self) -> &GlobalShortcutCounters {
        &self.counters
    }
}

impl Drop for GlobalShortcutService {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            self.stop.store(true, Ordering::Release);
            let _ = owner.join();
        }
    }
}
