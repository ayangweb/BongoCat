#![forbid(unsafe_code)]

mod input;

use bongocat_model::{InstalledModel, ModelSnapshot};
use std::{
    fmt,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub use input::{
    InputControl, InputDiagnostics, InputDisposition, InputEdge, InputEvent, InputResetReason,
    InputSnapshot, InputSource, InputTransportDiagnostics, MonotonicMillis, MouseButton,
    PhysicalKey, ReconciliationPolicy, SequencedInputEvent,
};
use input::{InputState, InputTransportCounters};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCommand {
    SetOverlayVisible(bool),
    ResetInput(InputResetReason),
    ApplyInput(Arc<SequencedInputEvent>),
    ActivateModel(Arc<InstalledModel>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub overlay_visible: bool,
    pub active_model: Option<ModelSnapshot>,
    pub input: InputSnapshot,
    pub last_command_sequence: Option<u64>,
}

impl RuntimeSnapshot {
    fn starting(overlay_visible: bool) -> Self {
        Self {
            revision: 0,
            state: RuntimeState::Starting,
            overlay_visible,
            active_model: None,
            input: InputSnapshot::default(),
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

#[derive(Default)]
struct InputProducerState {
    next_sequence: u64,
    recovery_pending: bool,
}

#[derive(Clone)]
pub struct InputProducer {
    runtime: RuntimeClient,
    state: Arc<Mutex<InputProducerState>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputPublishError {
    QueueFull(InputEvent),
    RuntimeStopped(InputEvent),
}

impl fmt::Display for InputPublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull(_) => formatter.write_str("runtime input queue is full"),
            Self::RuntimeStopped(_) => formatter.write_str("runtime is stopped"),
        }
    }
}

impl std::error::Error for InputPublishError {}

impl InputProducer {
    fn new(runtime: RuntimeClient) -> Self {
        let state = Arc::clone(&runtime.input_producer_state);
        Self { runtime, state }
    }

    pub fn publish(&self, event: InputEvent) -> Result<u64, InputPublishError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input_sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.wrapping_add(1);
        let command = RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
            sequence: input_sequence,
            event: event.clone(),
        }));
        match self.runtime.send(command) {
            Ok(_) => {
                self.runtime.input_transport.enqueued();
                if state.recovery_pending {
                    state.recovery_pending = false;
                    self.runtime.input_transport.recovered_after_overflow();
                }
                Ok(input_sequence)
            }
            Err(SendError::QueueFull(_)) => {
                state.recovery_pending = true;
                self.runtime.input_transport.queue_full();
                Err(InputPublishError::QueueFull(event))
            }
            Err(SendError::RuntimeStopped(_)) => {
                self.runtime.input_transport.runtime_stopped();
                Err(InputPublishError::RuntimeStopped(event))
            }
        }
    }

    pub fn recover(
        &self,
        reason: InputResetReason,
        at: MonotonicMillis,
    ) -> Result<u64, InputPublishError> {
        self.publish(InputEvent::Reset { reason, at })
    }

    pub fn diagnostics(&self) -> InputTransportDiagnostics {
        self.runtime.input_transport.snapshot()
    }
}

#[derive(Clone)]
pub struct RuntimeClient {
    producer: Arc<Producer>,
    snapshot: Arc<SnapshotCell>,
    input_transport: Arc<InputTransportCounters>,
    input_producer_state: Arc<Mutex<InputProducerState>>,
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
        let snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.with_input_transport(snapshot)
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
                return Some(self.with_input_transport(snapshot.clone()));
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

    pub fn wait_for_command(
        &self,
        command_sequence: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot
                .last_command_sequence
                .is_some_and(|sequence| sequence >= command_sequence)
            {
                return Some(self.with_input_transport(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
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
            if result.timed_out()
                && !snapshot
                    .last_command_sequence
                    .is_some_and(|sequence| sequence >= command_sequence)
            {
                return None;
            }
        }
    }

    fn with_input_transport(&self, mut snapshot: RuntimeSnapshot) -> RuntimeSnapshot {
        snapshot.input.transport = self.input_transport.snapshot();
        snapshot
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
        let input_transport = Arc::new(InputTransportCounters::default());
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::clone(&snapshot),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
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

    pub fn input_producer(&self) -> InputProducer {
        InputProducer::new(self.client())
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
    let mut active_model = None;
    let mut input_state = InputState::default();
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
            WorkerCommand::Product(RuntimeCommand::ResetInput(reason)) => {
                input_state.force_reset(reason);
                publish(&snapshot, |current| {
                    current.input = input_state.snapshot();
                    current.last_command_sequence = Some(sequence);
                })
            }
            WorkerCommand::Product(RuntimeCommand::ApplyInput(envelope)) => {
                input_state.apply(Arc::unwrap_or_clone(envelope));
                publish(&snapshot, |current| {
                    current.input = input_state.snapshot();
                    current.last_command_sequence = Some(sequence);
                });
            }
            WorkerCommand::Product(RuntimeCommand::ActivateModel(prepared)) => {
                let model_snapshot = prepared.snapshot();
                active_model = Some(prepared);
                publish(&snapshot, |current| {
                    current.active_model = Some(model_snapshot);
                    current.last_command_sequence = Some(sequence);
                });
            }
            WorkerCommand::Shutdown => {
                publish(&snapshot, |current| {
                    current.state = RuntimeState::Stopping;
                    current.last_command_sequence = Some(sequence);
                });
                publish(&snapshot, |current| current.state = RuntimeState::Stopped);
                drop(active_model);
                return;
            }
        }
    }
    publish(&snapshot, |current| current.state = RuntimeState::Stopped);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{ModelId, ModelPackageLimits, ModelStore};
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    const TIMEOUT: Duration = Duration::from_secs(2);

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

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
            .send(RuntimeCommand::ResetInput(InputResetReason::ServiceRestart))
            .expect("reset accepted");
        let reset = client
            .wait_for_revision(ready.revision + 1, TIMEOUT)
            .expect("reset snapshot");
        assert_eq!(reset.input.diagnostics.reset_count, 1);
        assert_eq!(
            reset.input.last_reset_reason,
            Some(InputResetReason::ServiceRestart)
        );
        owner.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn runtime_applies_reliable_input_and_reconciled_release() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        let down_sequence = client
            .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
                sequence: 0,
                event: InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::KEY_A),
                    edge: InputEdge::Down,
                    source: InputSource::Capture,
                    at: MonotonicMillis::new(0),
                },
            })))
            .expect("key down accepted");
        let pressed = client
            .wait_for_command(down_sequence, TIMEOUT)
            .expect("pressed snapshot");
        assert_eq!(pressed.input.pressed_key_count, 1);

        let release_sequence = client
            .send(RuntimeCommand::ApplyInput(Arc::new(SequencedInputEvent {
                sequence: 1,
                event: InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::KEY_A),
                    edge: InputEdge::Up,
                    source: InputSource::Reconciliation,
                    at: MonotonicMillis::new(500),
                },
            })))
            .expect("reconciled key up accepted");
        let released = client
            .wait_for_command(release_sequence, TIMEOUT)
            .expect("released snapshot");
        assert_eq!(released.input.pressed_key_count, 0);
        assert_eq!(released.input.diagnostics.reconciled_release, 1);
        owner.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn runtime_owns_the_committed_installed_model() {
        let data = tempdir().expect("data root");
        let store = ModelStore::new(
            data.path().join("models"),
            data.path().join("locks/models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("model store");
        let installed = store
            .import(
                ModelId::parse("unicode").expect("model id"),
                repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
            )
            .expect("installed model");
        let owner = RuntimeOwner::start(true, 4);
        let client = owner.client();
        let ready = client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        client
            .send(RuntimeCommand::ActivateModel(Arc::new(installed)))
            .expect("model command");
        let activated = client
            .wait_for_revision(ready.revision + 1, TIMEOUT)
            .expect("model snapshot");
        assert_eq!(
            activated
                .active_model
                .as_ref()
                .expect("active model")
                .id
                .as_str(),
            "unicode"
        );
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
        let input_transport = Arc::new(InputTransportCounters::default());
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::new(SnapshotCell {
                value: Mutex::new(RuntimeSnapshot::starting(true)),
                changed: Condvar::new(),
            }),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
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

    #[test]
    fn input_producer_overflow_is_observable_and_recovery_resets_state() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let input_transport = Arc::new(InputTransportCounters::default());
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::new(SnapshotCell {
                value: Mutex::new(RuntimeSnapshot::starting(true)),
                changed: Condvar::new(),
            }),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
        };
        let producer = InputProducer::new(client.clone());
        let sibling_producer = InputProducer::new(client.clone());
        let down = InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(0),
        };
        producer.publish(down).expect("down enqueued");
        client
            .send(RuntimeCommand::SetOverlayVisible(false))
            .expect("queue filler");
        let release = InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        };
        assert_eq!(
            sibling_producer.publish(release.clone()),
            Err(InputPublishError::QueueFull(release))
        );

        let first = receiver.recv().expect("queued input");
        let WorkerCommand::Product(RuntimeCommand::ApplyInput(first)) = first.command else {
            panic!("first command must be input");
        };
        let mut state = InputState::default();
        state.apply(Arc::unwrap_or_clone(first));
        assert_eq!(state.snapshot().pressed_key_count, 1);
        receiver.recv().expect("queue filler");

        producer
            .recover(InputResetReason::QueueOverflow, MonotonicMillis::new(2))
            .expect("recovery enqueued");
        let recovery = receiver.recv().expect("recovery input");
        let WorkerCommand::Product(RuntimeCommand::ApplyInput(recovery)) = recovery.command else {
            panic!("recovery command must be input");
        };
        assert_eq!(
            state.apply(Arc::unwrap_or_clone(recovery)),
            InputDisposition::AppliedAfterSequenceGap { missing: 1 }
        );
        let snapshot = state.snapshot();
        assert_eq!(snapshot.pressed_key_count, 0);
        assert_eq!(
            snapshot.last_reset_reason,
            Some(InputResetReason::QueueOverflow)
        );
        assert_eq!(snapshot.diagnostics.reset_count, 1);
        assert_eq!(
            producer.diagnostics(),
            InputTransportDiagnostics {
                enqueued: 2,
                queue_full: 1,
                recovered_after_overflow: 1,
                runtime_stopped: 0,
            }
        );
        assert_eq!(client.snapshot().input.transport, producer.diagnostics());
    }
}
