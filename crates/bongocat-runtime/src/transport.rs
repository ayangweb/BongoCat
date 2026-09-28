//! The transport between the runtime's owner and its worker thread.
//!
//! Commands travel on a bounded queue and the published snapshot lives behind one
//! mutex and condition variable. A command carries a sequence so a caller can
//! wait for exactly its own effect, and a failed send is returned as the original
//! typed command rather than as an opaque overflow count, because the caller is
//! the only one that can decide what the lost action meant.

use crate::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommandEnvelope {
    pub(crate) sequence: u64,
    pub(crate) command: WorkerCommand,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WorkerCommand {
    Product(RuntimeCommand),
    Shutdown,
}

pub(crate) struct SnapshotCell {
    pub(crate) value: Mutex<RuntimeSnapshot>,
    pub(crate) changed: Condvar,
}

pub(crate) struct Producer {
    pub(crate) sender: SyncSender<CommandEnvelope>,
    pub(crate) next_sequence: Mutex<u64>,
    pub(crate) command_transport: Arc<CommandTransportCounters>,
    pub(crate) accepting: Arc<AtomicBool>,
}

#[derive(Default)]
pub(crate) struct CommandTransportCounters {
    pub(crate) enqueued: AtomicU64,
    pub(crate) queue_full: AtomicU64,
    pub(crate) runtime_stopped: AtomicU64,
    pub(crate) sequence_gap_count: AtomicU64,
    pub(crate) missing_sequence_count: AtomicU64,
    pub(crate) duplicate_sequence_count: AtomicU64,
    pub(crate) out_of_order_sequence_count: AtomicU64,
}

#[derive(Default)]
pub(crate) struct ShutdownDiagnosticsCounters {
    pub(crate) timed_out: AtomicU64,
    pub(crate) worker_panicked: AtomicU64,
}

impl ShutdownDiagnosticsCounters {
    pub(crate) fn snapshot(&self) -> RuntimeShutdownDiagnostics {
        RuntimeShutdownDiagnostics {
            timed_out: self.timed_out.load(Ordering::Acquire),
            worker_panicked: self.worker_panicked.load(Ordering::Acquire),
        }
    }
}

impl CommandTransportCounters {
    pub(crate) fn enqueued(&self) {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn queue_full(&self) {
        self.queue_full.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn runtime_stopped(&self) {
        self.runtime_stopped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn sequence_gap(&self, missing: u64) {
        self.sequence_gap_count.fetch_add(1, Ordering::Relaxed);
        self.missing_sequence_count
            .fetch_add(missing, Ordering::Relaxed);
    }

    pub(crate) fn duplicate_sequence(&self) {
        self.duplicate_sequence_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn out_of_order_sequence(&self) {
        self.out_of_order_sequence_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> RuntimeCommandTransportDiagnostics {
        RuntimeCommandTransportDiagnostics {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            runtime_stopped: self.runtime_stopped.load(Ordering::Relaxed),
            sequence_gap_count: self.sequence_gap_count.load(Ordering::Relaxed),
            missing_sequence_count: self.missing_sequence_count.load(Ordering::Relaxed),
            duplicate_sequence_count: self.duplicate_sequence_count.load(Ordering::Relaxed),
            out_of_order_sequence_count: self.out_of_order_sequence_count.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandSequenceDisposition {
    First,
    InOrder,
    Deferred,
    Gap { missing: u64 },
    Duplicate,
    OutOfOrder,
}

#[derive(Default)]
pub(crate) struct CommandSequenceTracker {
    pub(crate) last: Option<u64>,
    pub(crate) deferred: BTreeSet<u64>,
}

/// Returns whether an observed sequence has reached a target in the forward
/// direction, including across the `u64::MAX -> 0` boundary.
///
/// Sequence producers are monotonic modulo `u64`; treating the forward half
/// of the number line as newer keeps waits correct after a wrap while still
/// rejecting stale observations from the backward half.
pub(crate) fn sequence_reached(observed: u64, target: u64) -> bool {
    observed.wrapping_sub(target) <= u64::MAX / 2
}

impl CommandSequenceTracker {
    pub(crate) fn defer(&mut self, sequence: u64) {
        self.deferred.insert(sequence);
    }

    pub(crate) fn observe(&mut self, sequence: u64) -> CommandSequenceDisposition {
        if self.deferred.remove(&sequence) {
            return CommandSequenceDisposition::Deferred;
        }
        let Some(last) = self.last else {
            self.last = Some(sequence);
            return CommandSequenceDisposition::First;
        };
        let distance = sequence.wrapping_sub(last);
        if distance == 1 {
            self.last = Some(sequence);
            CommandSequenceDisposition::InOrder
        } else if distance == 0 {
            CommandSequenceDisposition::Duplicate
        } else if distance <= u64::MAX / 2 {
            let missing = distance - 1;
            self.last = Some(sequence);
            CommandSequenceDisposition::Gap { missing }
        } else {
            CommandSequenceDisposition::OutOfOrder
        }
    }
}

impl Producer {
    pub(crate) fn send(&self, command: RuntimeCommand) -> Result<u64, SendError> {
        let mut next_sequence = self
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.accepting.load(Ordering::Acquire) {
            self.command_transport.runtime_stopped();
            return Err(SendError::RuntimeStopped(command));
        }
        let envelope = CommandEnvelope {
            sequence: *next_sequence,
            command: WorkerCommand::Product(command),
        };
        match self.sender.try_send(envelope) {
            Ok(()) => {
                let accepted = *next_sequence;
                *next_sequence = next_sequence.wrapping_add(1);
                self.command_transport.enqueued();
                Ok(accepted)
            }
            Err(TrySendError::Full(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => {
                    self.command_transport.queue_full();
                    Err(SendError::QueueFull(command))
                }
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
            Err(TrySendError::Disconnected(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => {
                    self.command_transport.runtime_stopped();
                    Err(SendError::RuntimeStopped(command))
                }
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
        }
    }
}

pub(crate) struct RuntimeInputSubmitter {
    pub(crate) producer: Arc<Producer>,
}

impl InputSubmitter for RuntimeInputSubmitter {
    fn submit(&self, event: SequencedInputEvent) -> Result<(), InputSubmitError> {
        match self
            .producer
            .send(RuntimeCommand::ApplyInput(Arc::new(event)))
        {
            Ok(_) => Ok(()),
            Err(SendError::QueueFull(_)) => Err(InputSubmitError::QueueFull),
            Err(SendError::RuntimeStopped(_)) => Err(InputSubmitError::RuntimeStopped),
        }
    }
}

pub(crate) fn publish(snapshot_cell: &SnapshotCell, update: impl FnOnce(&mut RuntimeSnapshot)) {
    let mut snapshot = snapshot_cell
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    update(&mut snapshot);
    snapshot.revision = snapshot.revision.saturating_add(1);
    snapshot_cell.changed.notify_all();
}
