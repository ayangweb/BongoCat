//! The panic hook, which is the one writer that cannot wait for a lock.
//!
//! A panic can happen while the log lock is held — inside the log, in fact — so
//! the hook's record drops rather than blocking. A deadlock on the way out is worse
//! than a missing line, and the hook still restores whatever hook was there before
//! it, so installing ours twice cannot nest.

use super::*;

use std::{
    panic::{self, PanicHookInfo},
    sync::Arc,
};

pub(crate) type PanicHook = dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static;

pub struct ApplicationPanicHook {
    pub(crate) previous: Option<Box<PanicHook>>,
}

impl std::fmt::Debug for ApplicationPanicHook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationPanicHook")
            .finish_non_exhaustive()
    }
}

impl Drop for ApplicationPanicHook {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if let Some(previous) = self.previous.take() {
            panic::set_hook(previous);
        }
    }
}

impl ApplicationLogHandle {
    pub fn install_panic_hook(&self) -> ApplicationPanicHook {
        let previous = panic::take_hook();
        let sink = Arc::clone(&self.sink);
        let directory = self
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .directory
            .clone();
        panic::set_hook(Box::new(move |_| {
            sink.try_record(ApplicationLogEvent::panicked());
            let _ = write_run_marker(&directory.join(RUN_MARKER_NAME), RUN_MARKER_PANICKED);
        }));
        ApplicationPanicHook {
            previous: Some(previous),
        }
    }
}
