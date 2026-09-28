//! The place the callback writes into, and the one global slot it is reached
//! through.
//!
//! Cubism exposes a single global log callback and no user-data pointer, so the
//! only way back from C to Rust state is a process-wide slot. That slot is a
//! mutex rather than a lock-free atomic because the callback must not block: if
//! the holder is busy, the record is dropped and counted. Blocking here would
//! let a Cubism write stall the render thread, which is the one thread that
//! cannot afford to stall.

use super::*;

pub(crate) const CALLBACK_QUEUE_CAPACITY: usize = 128;

#[derive(Debug)]
pub(crate) struct CoreLogState {
    pub(crate) writer: TextLogWriter,
}

#[derive(Clone, Copy)]
pub(crate) struct CoreLogMessage {
    pub(crate) bytes: [u8; MAX_MESSAGE_BYTES],
    pub(crate) length: usize,
}

#[derive(Debug)]
pub(crate) struct CoreLogSink {
    pub(crate) state: Mutex<CoreLogState>,
    pub(crate) sender: SyncSender<CoreLogMessage>,
    pub(crate) accepting: AtomicBool,
    pub(crate) callback_dropped: AtomicU64,
    pub(crate) global_drop_baseline: u64,
}

pub(crate) static CORE_LOG_SINK: OnceLock<Mutex<Option<Arc<CoreLogSink>>>> = OnceLock::new();

pub(crate) static CORE_LOG_CALLBACK_DROPS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn sink_slot() -> &'static Mutex<Option<Arc<CoreLogSink>>> {
    CORE_LOG_SINK.get_or_init(|| Mutex::new(None))
}

pub(crate) unsafe extern "C" fn core_log_callback(message: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if message.is_null() {
            return;
        }
        let sink = match sink_slot().try_lock() {
            Ok(slot) => slot.as_ref().cloned(),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner().as_ref().cloned(),
            Err(std::sync::TryLockError::WouldBlock) => {
                CORE_LOG_CALLBACK_DROPS.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        let Some(sink) = sink else {
            return;
        };
        // SAFETY: Cubism documents a valid null-terminated message for the
        // callback duration. This copies at most MAX_MESSAGE_BYTES without
        // allocating or reading past that fixed bound.
        let message = unsafe { CoreLogMessage::copy_from_callback(message) };
        sink.enqueue(message);
    }));
}

impl CoreLogSink {
    pub(crate) fn stats(&self) -> CoreLogStats {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let writer = state.writer.stats();
        let mut stats = CoreLogStats {
            written: writer.written,
            dropped: writer.dropped,
            rotated: writer.rotated,
            pruned: writer.pruned,
            bytes: writer.active_bytes,
            retained_files: writer.retained_files,
            retained_bytes: writer.retained_bytes,
        };
        let local_drops = self.callback_dropped.load(Ordering::Relaxed);
        let global_drops = CORE_LOG_CALLBACK_DROPS
            .load(Ordering::Relaxed)
            .saturating_sub(self.global_drop_baseline);
        stats.dropped = stats
            .dropped
            .saturating_add(local_drops)
            .saturating_add(global_drops);
        stats
    }

    pub(crate) fn enqueue(&self, message: CoreLogMessage) {
        if !self.accepting.load(Ordering::Acquire) {
            self.callback_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match self.sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.callback_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn run_worker(&self, receiver: Receiver<CoreLogMessage>) {
        loop {
            match receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(message) => self.record(message),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !self.accepting.load(Ordering::Acquire) {
                        self.drain_worker_queue(&receiver);
                        return;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    pub(crate) fn drain_worker_queue(&self, receiver: &Receiver<CoreLogMessage>) {
        loop {
            match receiver.try_recv() {
                Ok(message) => self.record(message),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    pub(crate) fn record(&self, message: CoreLogMessage) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let message = sanitize_message(&message.bytes[..message.length]);
        // Cubism's callback has no typed severity. Treating every vendor
        // message as debug prevents routine Core chatter from becoming the
        // default user log; actual Core/renderer failures are logged by the
        // app owner with a stable error or warning code.
        let _ = state.writer.record(LogRecord::new(
            SystemTime::now(),
            LogLevel::Debug,
            "cubism.core",
            "cubism/core/message",
            message,
        ));
    }
}

impl CoreLogMessage {
    pub(crate) unsafe fn copy_from_callback(message: *const c_char) -> Self {
        let mut copied = Self {
            bytes: [0; MAX_MESSAGE_BYTES],
            length: 0,
        };
        // SAFETY: the Core callback contract supplies a readable,
        // null-terminated string for this invocation. `CStr::from_ptr` is the
        // standard wrapper for that C boundary; the subsequent copy is capped
        // at MAX_MESSAGE_BYTES and never exposes the original pointer to the
        // queue.
        let source = unsafe { CStr::from_ptr(message) }.to_bytes();
        copied.length = source.len().min(MAX_MESSAGE_BYTES);
        copied.bytes[..copied.length].copy_from_slice(&source[..copied.length]);
        copied
    }
}
