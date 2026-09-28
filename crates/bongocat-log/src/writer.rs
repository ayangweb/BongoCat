//! The writer, and the state it keeps.
//!
//! The writer owns its open file, its size so far and its rotation counter, and
//! it holds them behind a mutex because two threads may log at once. The size is
//! tracked rather than measured so that the per-file guard costs a stat once per
//! open rather than once per line.

use super::*;

pub(crate) struct TextLogState {
    pub(crate) directory: PathBuf,
    pub(crate) stream: LogStream,
    pub(crate) day: UtcDate,
    pub(crate) path: Option<PathBuf>,
    pub(crate) file: Option<File>,
    pub(crate) active_bytes: u64,
    pub(crate) written: u64,
    pub(crate) dropped: u64,
    pub(crate) rotated: u64,
    pub(crate) retention_ready: bool,
    pub(crate) last_retention_check: Option<SystemTime>,
}

impl TextLogState {
    pub(crate) fn active_path(&self) -> PathBuf {
        self.directory.join(self.stream.active_file_name(self.day))
    }

    pub(crate) fn open_active(&mut self) -> io::Result<()> {
        if self.file.is_some() {
            if self.path.as_ref().is_some_and(|path| path.is_file()) {
                return Ok(());
            }
            self.file = None;
            self.path = None;
            self.active_bytes = 0;
        }

        let path = self.active_path();
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to append through a log symlink",
            ));
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        set_private_file(&file)?;
        self.active_bytes = fs::metadata(&path)?.len();
        self.path = Some(path);
        self.file = Some(file);
        Ok(())
    }

    pub(crate) fn switch_day(&mut self, next_day: UtcDate) -> io::Result<()> {
        if self.day == next_day {
            return self.open_active();
        }
        if let Some(path) = self.path.take() {
            self.file = None;
            if rotate_path(&self.directory, self.stream, &path).is_ok() {
                self.rotated = self.rotated.saturating_add(1);
            } else {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        self.day = next_day;
        self.open_active()
    }

    pub(crate) fn rotate_for_size(&mut self, incoming_bytes: u64) -> io::Result<()> {
        if self.active_bytes == 0
            || self.active_bytes.saturating_add(incoming_bytes) <= MAX_LOG_FILE_BYTES
        {
            return Ok(());
        }
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        self.file = None;
        rotate_path(&self.directory, self.stream, &path)?;
        self.rotated = self.rotated.saturating_add(1);
        self.open_active()
    }

    pub(crate) fn refresh_retention(
        &mut self,
        controller: &LogSettingsController,
        now: SystemTime,
    ) {
        let sweep = enforce_directory_retention_sweep(
            &self.directory,
            now,
            controller.settings().retention_days,
        );
        controller.record_pruned(LogStream::Application, sweep.application_pruned);
        controller.record_pruned(LogStream::CubismCore, sweep.core_pruned);
    }

    pub(crate) fn maybe_refresh_retention(
        &mut self,
        controller: &LogSettingsController,
        now: SystemTime,
    ) {
        if !self.retention_ready {
            return;
        }
        let due = self.last_retention_check.is_none_or(|last| {
            now.duration_since(last)
                .is_ok_and(|elapsed| elapsed >= RETENTION_CHECK_INTERVAL)
        });
        if due {
            self.refresh_retention(controller, now);
            self.last_retention_check = Some(now);
        }
    }

    pub(crate) fn refresh_stream_totals(&mut self) {
        if let Some(path) = self.path.as_ref()
            && let Ok(metadata) = fs::metadata(path)
        {
            self.active_bytes = metadata.len();
        }
    }
}

/// Synchronous, bounded text writer shared by one log stream.
///
/// Writes happen only from low-frequency app/service boundaries. The panic
/// hook uses [`Self::try_record`] so lock contention cannot recurse or block.
pub struct TextLogWriter {
    pub(crate) state: Mutex<TextLogState>,
    pub(crate) controller: LogSettingsController,
}

impl fmt::Debug for TextLogWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextLogWriter")
            .field("stream", &self.state.lock().map(|state| state.stream))
            .field("controller", &self.controller)
            .finish()
    }
}

impl TextLogWriter {
    pub fn open(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
    ) -> io::Result<Self> {
        Self::open_impl(directory, stream, controller, true)
    }

    /// Open a stream without deleting historical files.
    ///
    /// Application startup uses this while the persisted configuration is still
    /// being loaded, so a user-selected retention period longer than the
    /// bootstrap default cannot be irreversibly shortened before it is known.
    /// Call [`Self::refresh_policy`] immediately after loading the settings.
    pub fn open_deferred(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
    ) -> io::Result<Self> {
        Self::open_impl(directory, stream, controller, false)
    }

    pub(crate) fn open_impl(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
        enforce_retention: bool,
    ) -> io::Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        create_private_dir_all(&directory)?;
        let now = SystemTime::now();
        let day = UtcDate::from_system_time(now);
        let mut retired_at_open = 0;
        if enforce_retention {
            retired_at_open = retire_other_day_bases(&directory, stream, day);
            let sweep = enforce_directory_retention_sweep(
                &directory,
                now,
                controller.settings().retention_days,
            );
            controller.record_pruned(LogStream::Application, sweep.application_pruned);
            controller.record_pruned(LogStream::CubismCore, sweep.core_pruned);
        }

        let mut state = TextLogState {
            directory,
            stream,
            day,
            path: None,
            file: None,
            active_bytes: 0,
            written: 0,
            dropped: 0,
            rotated: retired_at_open,
            retention_ready: enforce_retention,
            last_retention_check: enforce_retention.then_some(now),
        };
        state.open_active()?;
        Ok(Self {
            state: Mutex::new(state),
            controller,
        })
    }

    /// Record `record` if the shared level filter admits it.
    ///
    /// `Ok(false)` means the record was intentionally filtered. `Ok(true)`
    /// means it was written and flushed.
    pub fn record(&self, record: LogRecord) -> io::Result<bool> {
        let mut state = self.lock_state();
        self.record_locked(&mut state, record)
    }

    /// Non-blocking variant used by the process panic hook.
    pub fn try_record(&self, record: LogRecord) -> Option<io::Result<bool>> {
        let mut state = self.state.try_lock().ok()?;
        Some(self.record_locked(&mut state, record))
    }

    pub(crate) fn record_locked(
        &self,
        state: &mut TextLogState,
        record: LogRecord,
    ) -> io::Result<bool> {
        let settings = self.controller.settings();
        if !record.level.is_enabled(settings.level) {
            return Ok(false);
        }

        let line = format_log_line(&record);
        let bytes = line.as_bytes();
        let record_day = UtcDate::from_system_time(record.timestamp);
        if let Err(error) = state.switch_day(record_day) {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }
        if let Err(error) = state.rotate_for_size(bytes.len() as u64) {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }

        let write_result = state
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file is not open"))
            .and_then(|file| {
                file.write_all(bytes)?;
                file.flush()
            });
        if let Err(error) = write_result {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }

        state.active_bytes = state.active_bytes.saturating_add(bytes.len() as u64);
        state.written = state.written.saturating_add(1);
        state.maybe_refresh_retention(&self.controller, SystemTime::now());
        Ok(true)
    }

    /// Apply a newly replaced shared settings controller immediately. Rotation
    /// and cleanup remain best effort and never fail a settings transaction.
    pub fn refresh_policy(&self) {
        let mut state = self.lock_state();
        let now = SystemTime::now();
        let retired = retire_other_day_bases(&state.directory, state.stream, state.day);
        state.rotated = state.rotated.saturating_add(retired);
        state.refresh_retention(&self.controller, now);
        state.retention_ready = true;
        state.last_retention_check = Some(now);
    }

    pub fn stats(&self) -> TextLogStats {
        let mut state = self.lock_state();
        state.refresh_stream_totals();
        let today = UtcDate::from_system_time(SystemTime::now());
        let files = collect_log_files(&state.directory, today)
            .into_iter()
            .filter(|file| file.stream == state.stream);
        let mut retained_files = 0_u64;
        let mut retained_bytes = 0_u64;
        for file in files {
            retained_files = retained_files.saturating_add(1);
            retained_bytes = retained_bytes.saturating_add(file.bytes);
        }
        TextLogStats {
            written: state.written,
            dropped: state.dropped,
            rotated: state.rotated,
            pruned: self.controller.pruned(state.stream),
            active_bytes: state.active_bytes,
            retained_files,
            retained_bytes,
        }
    }

    pub(crate) fn lock_state(&self) -> MutexGuard<'_, TextLogState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
