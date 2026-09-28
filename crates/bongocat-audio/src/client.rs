//! The handle the runtime holds.
//!
//! The client is cloneable and the clones share one queue and one sequence
//! counter, so two threads asking for a sound get two different sequences in the
//! order they asked rather than the same one. Publishing is non-blocking and says
//! when it refused: a caller that cannot be told would assume its command was
//! queued.

use super::*;

#[derive(Clone)]
pub struct MotionAudioClient {
    pub(crate) sender: SyncSender<MotionAudioCommand>,
    pub(crate) shared: Arc<SharedState>,
}

impl MotionAudioClient {
    pub fn unavailable() -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        Self {
            sender,
            shared: Arc::new(SharedState {
                diagnostics: Mutex::new(MotionAudioDiagnostics::unavailable()),
                changed: Condvar::new(),
                publish_lock: Mutex::new(()),
                shutdown_requested: AtomicBool::new(true),
                overflow_recovery_requested: AtomicBool::new(false),
                next_sequence: AtomicU64::new(0),
            }),
        }
    }

    /// Publishes a command with a caller-supplied sequence for protocol tests.
    ///
    /// Production callers use [`Self::try_publish_with_sequence`], which
    /// allocates and enqueues under one lock.
    #[cfg(test)]
    pub(crate) fn try_publish(
        &self,
        command: MotionAudioCommand,
    ) -> Result<(), MotionAudioPublishError> {
        let _publish_guard = self
            .shared
            .publish_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.try_publish_locked(command)
    }

    /// Allocates and enqueues one audio command as a single operation.
    ///
    /// The returned sequence is only valid when the command was accepted by
    /// the queue. Allocation and enqueue share the publish lock, so cloned
    /// clients cannot publish sequence `n + 1` before an earlier allocated
    /// sequence has either been accepted or rejected by the queue.
    pub fn try_publish_with_sequence<F>(&self, build: F) -> Result<u64, MotionAudioPublishError>
    where
        F: FnOnce(u64) -> MotionAudioCommand,
    {
        let _publish_guard = self
            .shared
            .publish_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sequence = self.shared.next_sequence.fetch_add(1, Ordering::Relaxed);
        let command = build(sequence);
        self.try_publish_locked(command)?;
        Ok(sequence)
    }

    pub(crate) fn try_publish_locked(
        &self,
        command: MotionAudioCommand,
    ) -> Result<(), MotionAudioPublishError> {
        if self.shared.shutdown_requested.load(Ordering::Acquire) {
            self.shared.publish(|diagnostics| {
                diagnostics.rejected_after_shutdown =
                    diagnostics.rejected_after_shutdown.saturating_add(1);
            });
            return Err(MotionAudioPublishError::ServiceStopped(command));
        }
        if self
            .shared
            .overflow_recovery_requested
            .load(Ordering::Acquire)
        {
            return Err(MotionAudioPublishError::RecoveryPending(command));
        }
        match self.sender.try_send(command) {
            Ok(()) => {
                self.shared.publish(|diagnostics| {
                    diagnostics.enqueued_commands = diagnostics.enqueued_commands.saturating_add(1);
                });
                Ok(())
            }
            Err(TrySendError::Full(command)) => {
                self.shared
                    .overflow_recovery_requested
                    .store(true, Ordering::Release);
                self.shared.publish(|diagnostics| {
                    diagnostics.queue_overflows = diagnostics.queue_overflows.saturating_add(1);
                });
                Err(MotionAudioPublishError::QueueFull(command))
            }
            Err(TrySendError::Disconnected(command)) => {
                self.shared.publish(|diagnostics| {
                    diagnostics.rejected_after_shutdown =
                        diagnostics.rejected_after_shutdown.saturating_add(1);
                });
                Err(MotionAudioPublishError::ServiceStopped(command))
            }
        }
    }

    pub fn diagnostics(&self) -> MotionAudioDiagnostics {
        self.shared.snapshot()
    }

    pub fn wait_for_sequence(
        &self,
        sequence: u64,
        timeout: Duration,
    ) -> Option<MotionAudioDiagnostics> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut diagnostics = self
            .shared
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if diagnostics
                .last_processed_sequence
                .is_some_and(|processed| sequence_reached(processed, sequence))
            {
                return Some(diagnostics.clone());
            }
            if diagnostics.state == MotionAudioState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .shared
                .changed
                .wait_timeout(diagnostics, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            diagnostics = next;
            if result.timed_out()
                && !diagnostics
                    .last_processed_sequence
                    .is_some_and(|processed| sequence_reached(processed, sequence))
            {
                return None;
            }
        }
    }
}
