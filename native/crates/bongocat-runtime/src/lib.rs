#![forbid(unsafe_code)]

use std::{
    fmt,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputResetReason {
    QueueOverflow,
    SessionChanged,
    ProducerRestarted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCommand {
    SetOverlayVisible(bool),
    ResetInput(InputResetReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub overlay_visible: bool,
    pub input_reset_count: u64,
    pub last_command_sequence: Option<u64>,
}

impl RuntimeSnapshot {
    const fn starting(overlay_visible: bool) -> Self {
        Self {
            revision: 0,
            state: RuntimeState::Starting,
            overlay_visible,
            input_reset_count: 0,
            last_command_sequence: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandEnvelope {
    sequence: u64,
    command: WorkerCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkerCommand {
    Product(RuntimeCommand),
    Shutdown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    QueueFull(RuntimeCommand),
    RuntimeStopped(RuntimeCommand),
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull(_) => formatter.write_str("runtime command queue is full"),
            Self::RuntimeStopped(_) => formatter.write_str("runtime is stopped"),
        }
    }
}

impl std::error::Error for SendError {}

#[derive(Debug, PartialEq, Eq)]
pub enum ShutdownError {
    TimedOut,
    WorkerPanicked,
}

impl fmt::Display for ShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("runtime shutdown timed out"),
            Self::WorkerPanicked => formatter.write_str("runtime worker panicked"),
        }
    }
}

impl std::error::Error for ShutdownError {}

struct SnapshotCell {
    value: Mutex<RuntimeSnapshot>,
    changed: Condvar,
}

struct Producer {
    sender: SyncSender<CommandEnvelope>,
    next_sequence: Mutex<u64>,
}

#[derive(Clone)]
pub struct RuntimeClient {
    producer: Arc<Producer>,
    snapshot: Arc<SnapshotCell>,
}

impl RuntimeClient {
    pub fn send(&self, command: RuntimeCommand) -> Result<u64, SendError> {
        let mut next_sequence = self
            .producer
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let envelope = CommandEnvelope {
            sequence: *next_sequence,
            command: WorkerCommand::Product(command),
        };
        match self.producer.sender.try_send(envelope) {
            Ok(()) => {
                let accepted = *next_sequence;
                *next_sequence = next_sequence.wrapping_add(1);
                Ok(accepted)
            }
            Err(TrySendError::Full(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => Err(SendError::QueueFull(command)),
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
            Err(TrySendError::Disconnected(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => Err(SendError::RuntimeStopped(command)),
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
        }
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn wait_for_revision(
        &self,
        minimum_revision: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot.revision >= minimum_revision {
                return Some(snapshot.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out() && snapshot.revision < minimum_revision {
                return None;
            }
        }
    }
}

pub struct RuntimeOwner {
    client: RuntimeClient,
    worker: Option<JoinHandle<()>>,
}

impl RuntimeOwner {
    pub fn start(initial_overlay_visible: bool, command_capacity: usize) -> Self {
        assert!(
            command_capacity > 0,
            "runtime command capacity must be non-zero"
        );
        let (sender, receiver) = mpsc::sync_channel(command_capacity);
        let snapshot = Arc::new(SnapshotCell {
            value: Mutex::new(RuntimeSnapshot::starting(initial_overlay_visible)),
            changed: Condvar::new(),
        });
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::clone(&snapshot),
        };
        let worker = thread::Builder::new()
            .name("bongocat-runtime".into())
            .spawn(move || run_worker(receiver, snapshot))
            .expect("failed to start runtime thread");
        Self {
            client,
            worker: Some(worker),
        }
    }

    pub fn client(&self) -> RuntimeClient {
        self.client.clone()
    }

    pub fn shutdown(mut self, timeout: Duration) -> Result<RuntimeSnapshot, ShutdownError> {
        let current = self.client.snapshot();
        if current.state == RuntimeState::Stopped {
            self.join_worker()?;
            return Ok(current);
        }
        self.request_shutdown();
        let minimum_revision = current.revision.saturating_add(2);
        let stopped = self
            .client
            .wait_for_revision(minimum_revision, timeout)
            .filter(|snapshot| snapshot.state == RuntimeState::Stopped)
            .ok_or(ShutdownError::TimedOut)?;
        self.join_worker()?;
        Ok(stopped)
    }

    fn join_worker(&mut self) -> Result<(), ShutdownError> {
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| ShutdownError::WorkerPanicked)?;
        }
        Ok(())
    }

    fn request_shutdown(&self) {
        let mut next_sequence = self
            .client
            .producer
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let envelope = CommandEnvelope {
            sequence: *next_sequence,
            command: WorkerCommand::Shutdown,
        };
        if self.client.producer.sender.send(envelope).is_ok() {
            *next_sequence = next_sequence.wrapping_add(1);
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

fn publish(snapshot_cell: &SnapshotCell, update: impl FnOnce(&mut RuntimeSnapshot)) {
    let mut snapshot = snapshot_cell
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    update(&mut snapshot);
    snapshot.revision = snapshot.revision.saturating_add(1);
    snapshot_cell.changed.notify_all();
}

fn run_worker(receiver: Receiver<CommandEnvelope>, snapshot: Arc<SnapshotCell>) {
    publish(&snapshot, |current| current.state = RuntimeState::Ready);
    while let Ok(envelope) = receiver.recv() {
        let sequence = envelope.sequence;
        match envelope.command {
            WorkerCommand::Product(RuntimeCommand::SetOverlayVisible(visible)) => {
                publish(&snapshot, |current| {
                    current.overlay_visible = visible;
                    current.last_command_sequence = Some(sequence);
                });
            }
            WorkerCommand::Product(RuntimeCommand::ResetInput(_)) => {
                publish(&snapshot, |current| {
                    current.input_reset_count = current.input_reset_count.saturating_add(1);
                    current.last_command_sequence = Some(sequence);
                })
            }
            WorkerCommand::Shutdown => {
                publish(&snapshot, |current| {
                    current.state = RuntimeState::Stopping;
                    current.last_command_sequence = Some(sequence);
                });
                publish(&snapshot, |current| current.state = RuntimeState::Stopped);
                return;
            }
        }
    }
    publish(&snapshot, |current| current.state = RuntimeState::Stopped);
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(2);

    #[test]
    fn lifecycle_publishes_typed_snapshots_and_stops_cleanly() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        let ready = client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        assert_eq!(ready.state, RuntimeState::Ready);

        let sequence = client
            .send(RuntimeCommand::SetOverlayVisible(false))
            .expect("command accepted");
        let changed = client
            .wait_for_revision(ready.revision + 1, TIMEOUT)
            .expect("updated snapshot");
        assert!(!changed.overlay_visible);
        assert_eq!(changed.last_command_sequence, Some(sequence));

        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }

    #[test]
    fn input_reset_is_observable() {
        let owner = RuntimeOwner::start(true, 4);
        let client = owner.client();
        let ready = client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        client
            .send(RuntimeCommand::ResetInput(
                InputResetReason::ProducerRestarted,
            ))
            .expect("reset accepted");
        let reset = client
            .wait_for_revision(ready.revision + 1, TIMEOUT)
            .expect("reset snapshot");
        assert_eq!(reset.input_reset_count, 1);
        owner.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn zero_capacity_is_rejected() {
        let panic = std::panic::catch_unwind(|| RuntimeOwner::start(true, 0));
        assert!(panic.is_err());
    }

    #[test]
    fn full_queue_returns_the_original_typed_command() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::new(SnapshotCell {
                value: Mutex::new(RuntimeSnapshot::starting(true)),
                changed: Condvar::new(),
            }),
        };
        client
            .send(RuntimeCommand::SetOverlayVisible(false))
            .expect("first command accepted");
        assert_eq!(
            client.send(RuntimeCommand::ResetInput(InputResetReason::QueueOverflow,)),
            Err(SendError::QueueFull(RuntimeCommand::ResetInput(
                InputResetReason::QueueOverflow,
            )))
        );
    }
}
