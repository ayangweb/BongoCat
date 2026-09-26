//! The policy the application sets and the logger obeys.
//!
//! The level can change while the logger is running — the settings window offers
//! it — so the policy lives behind a lock the writer reads per line rather than
//! being copied into the writer when it is built. The inner type's `Debug` is
//! what a settings snapshot shows, and it prints the level rather than the lock.

use super::*;

/// Runtime policy shared by every writer participating in one application
/// environment. Updating it takes effect for both application and Core logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogSettings {
    pub level: LogLevel,
    pub retention_days: u64,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            level: LogLevel::default(),
            retention_days: DEFAULT_RETENTION_DAYS,
        }
    }
}

pub(crate) struct LogSettingsInner {
    pub(crate) settings: RwLock<LogSettings>,
    pub(crate) application_pruned: AtomicU64,
    pub(crate) core_pruned: AtomicU64,
}

impl fmt::Debug for LogSettingsInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogSettingsInner")
            .field("settings", &*read_settings(&self.settings))
            .field("application_pruned", &self.application_pruned)
            .field("core_pruned", &self.core_pruned)
            .finish()
    }
}

/// Cloneable owner for the active log policy and cross-writer prune counters.
#[derive(Clone, Debug)]
pub struct LogSettingsController {
    pub(crate) inner: Arc<LogSettingsInner>,
}

impl LogSettingsController {
    pub fn new(settings: LogSettings) -> Self {
        Self {
            inner: Arc::new(LogSettingsInner {
                settings: RwLock::new(settings),
                application_pruned: AtomicU64::new(0),
                core_pruned: AtomicU64::new(0),
            }),
        }
    }

    pub fn settings(&self) -> LogSettings {
        *read_settings(&self.inner.settings)
    }

    pub fn replace_settings(&self, settings: LogSettings) -> LogSettings {
        let mut current = write_settings(&self.inner.settings);
        std::mem::replace(&mut *current, settings)
    }

    pub(crate) fn record_pruned(&self, stream: LogStream, count: u64) {
        if count == 0 {
            return;
        }
        let counter = match stream {
            LogStream::Application => &self.inner.application_pruned,
            LogStream::CubismCore => &self.inner.core_pruned,
        };
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(count))
        });
    }

    pub(crate) fn pruned(&self, stream: LogStream) -> u64 {
        let counter = match stream {
            LogStream::Application => &self.inner.application_pruned,
            LogStream::CubismCore => &self.inner.core_pruned,
        };
        counter.load(Ordering::Relaxed)
    }
}

impl Default for LogSettingsController {
    fn default() -> Self {
        Self::new(LogSettings::default())
    }
}

pub(crate) fn read_settings(lock: &RwLock<LogSettings>) -> RwLockReadGuard<'_, LogSettings> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn write_settings(lock: &RwLock<LogSettings>) -> RwLockWriteGuard<'_, LogSettings> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
