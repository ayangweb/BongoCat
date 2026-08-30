#![forbid(unsafe_code)]

mod cursor;
mod input;
mod rendering;

use bongocat_model::{CommittedModel, ModelSnapshot};
use bongocat_render::{ModelCommitErrorCode, ModelCommitOutcome, ModelCommitToken, RenderConsumer};
use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cursor::CursorSlot;
pub use cursor::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorSampleError,
    CursorSnapshot, CursorTransportDiagnostics, CursorViewport, NormalizedCursorPosition,
};
pub use input::{
    HandSide, InputBindings, InputControl, InputDiagnostics, InputDisposition, InputEdge,
    InputEvent, InputResetReason, InputSnapshot, InputSource, InputTransportDiagnostics,
    ModelInputSnapshot, MonotonicMillis, MouseButton, PhysicalKey, ReconciliationPolicy,
    SequencedInputEvent,
};
use input::{InputState, InputTransportCounters};
use rendering::{RuntimeRenderBootstrap, RuntimeRenderer};

const CURSOR_SAMPLE_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRenderErrorCode {
    ModelLoadFailed,
    ModelEvaluationFailed,
    GpuPreparationFailed,
    PlatformUnsupported,
    TransportClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeCommandFailure {
    pub sequence: u64,
    pub code: RuntimeRenderErrorCode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingModelSnapshot {
    pub token: ModelCommitToken,
    pub model: ModelSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCommand {
    SetOverlayVisible(bool),
    SetInputBindings(Arc<InputBindings>),
    ResetInput(InputResetReason),
    ApplyInput(Arc<SequencedInputEvent>),
    ActivateModel(Arc<CommittedModel>),
    ActivateModelWithBindings {
        model: Arc<CommittedModel>,
        input_bindings: Arc<InputBindings>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub overlay_visible: bool,
    pub active_model: Option<ModelSnapshot>,
    pub pending_model: Option<PendingModelSnapshot>,
    pub input: InputSnapshot,
    pub cursor: CursorSnapshot,
    pub model_input: ModelInputSnapshot,
    pub render_error: Option<RuntimeRenderErrorCode>,
    pub last_command_failure: Option<RuntimeCommandFailure>,
    pub last_command_sequence: Option<u64>,
}

impl RuntimeSnapshot {
    fn starting(overlay_visible: bool) -> Self {
        Self {
            revision: 0,
            state: RuntimeState::Starting,
            overlay_visible,
            active_model: None,
            pending_model: None,
            input: InputSnapshot::default(),
            cursor: CursorSnapshot::default(),
            model_input: ModelInputSnapshot::default(),
            render_error: None,
            last_command_failure: None,
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
    cursor_slot: Arc<CursorSlot>,
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
        self.with_transport_diagnostics(snapshot)
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
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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

    pub fn wait_for_state(
        &self,
        expected: RuntimeState,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot.state == expected {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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
            if result.timed_out() && snapshot.state != expected {
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
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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

    pub fn wait_for_model_preparation(
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
            let prepared = snapshot
                .pending_model
                .as_ref()
                .is_some_and(|pending| pending.token.command_sequence == command_sequence);
            let completed = snapshot
                .last_command_sequence
                .is_some_and(|sequence| sequence >= command_sequence);
            if prepared || completed {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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
            if result.timed_out() {
                let prepared = snapshot
                    .pending_model
                    .as_ref()
                    .is_some_and(|pending| pending.token.command_sequence == command_sequence);
                let completed = snapshot
                    .last_command_sequence
                    .is_some_and(|sequence| sequence >= command_sequence);
                if !prepared && !completed {
                    return None;
                }
            }
        }
    }

    pub fn wait_for_input_sequence(
        &self,
        input_sequence: u64,
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
                .input
                .last_input_sequence
                .is_some_and(|sequence| sequence >= input_sequence)
            {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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
                    .input
                    .last_input_sequence
                    .is_some_and(|sequence| sequence >= input_sequence)
            {
                return None;
            }
        }
    }

    pub fn wait_for_cursor_samples(
        &self,
        minimum_consumed: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if self.cursor_slot.diagnostics().consumed >= minimum_consumed {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
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
            if result.timed_out() && self.cursor_slot.diagnostics().consumed < minimum_consumed {
                return None;
            }
        }
    }

    fn with_transport_diagnostics(&self, mut snapshot: RuntimeSnapshot) -> RuntimeSnapshot {
        snapshot.input.transport = self.input_transport.snapshot();
        snapshot.cursor.transport = self.cursor_slot.diagnostics();
        snapshot
    }
}

pub struct RuntimeOwner {
    client: RuntimeClient,
    worker: Option<JoinHandle<()>>,
}

impl RuntimeOwner {
    pub fn start(initial_overlay_visible: bool, command_capacity: usize) -> Self {
        Self::start_internal(initial_overlay_visible, command_capacity, None)
    }

    pub fn start_with_rendering(
        initial_overlay_visible: bool,
        command_capacity: usize,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(initial_overlay_visible, command_capacity, Some(renderer)),
            consumer,
        )
    }

    fn start_internal(
        initial_overlay_visible: bool,
        command_capacity: usize,
        renderer: Option<RuntimeRenderBootstrap>,
    ) -> Self {
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
        let cursor_slot = Arc::new(CursorSlot::default());
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
            }),
            snapshot: Arc::clone(&snapshot),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
            cursor_slot: Arc::clone(&cursor_slot),
        };
        let worker = thread::Builder::new()
            .name("bongocat-runtime".into())
            .spawn(move || {
                run_worker(
                    receiver,
                    snapshot,
                    cursor_slot,
                    initial_overlay_visible,
                    renderer,
                )
            })
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

    pub fn cursor_producer(&self) -> CursorProducer {
        CursorProducer::new(Arc::clone(&self.client.cursor_slot))
    }

    pub fn shutdown(mut self, timeout: Duration) -> Result<RuntimeSnapshot, ShutdownError> {
        let current = self.client.snapshot();
        if current.state == RuntimeState::Stopped {
            self.join_worker()?;
            return Ok(current);
        }
        self.request_shutdown();
        let stopped = self
            .client
            .wait_for_state(RuntimeState::Stopped, timeout)
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
        self.client.cursor_slot.stop();
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

struct PendingModelActivation {
    token: ModelCommitToken,
    model: Arc<CommittedModel>,
    input_bindings: Option<Arc<InputBindings>>,
}

fn run_worker(
    receiver: Receiver<CommandEnvelope>,
    snapshot: Arc<SnapshotCell>,
    cursor_slot: Arc<CursorSlot>,
    initial_overlay_visible: bool,
    renderer: Option<RuntimeRenderBootstrap>,
) {
    let mut renderer = renderer.map(RuntimeRenderer::start);
    let mut active_model = None;
    let mut input_state = InputState::default();
    let mut input_bindings = InputBindings::default();
    let mut normalized_cursor = NormalizedCursorPosition::default();
    let mut overlay_visible = initial_overlay_visible;
    let mut pending_model = None;
    let mut deferred_commands = VecDeque::new();
    publish(&snapshot, |current| current.state = RuntimeState::Ready);
    loop {
        process_model_commit_feedback(
            renderer.as_mut(),
            &mut pending_model,
            &input_state,
            &mut input_bindings,
            normalized_cursor,
            &mut active_model,
            overlay_visible,
            &snapshot,
        );
        let received = if pending_model.is_none() {
            deferred_commands
                .pop_front()
                .map_or_else(|| receiver.recv_timeout(CURSOR_SAMPLE_INTERVAL), Ok)
        } else {
            receiver.recv_timeout(CURSOR_SAMPLE_INTERVAL)
        };
        match received {
            Ok(envelope) => {
                consume_cursor(
                    &cursor_slot,
                    &snapshot,
                    &input_state,
                    &input_bindings,
                    &mut normalized_cursor,
                );
                if pending_model.is_some()
                    && !matches!(envelope.command, WorkerCommand::Shutdown)
                    && !matches!(
                        envelope.command,
                        WorkerCommand::Product(RuntimeCommand::ApplyInput(_))
                    )
                {
                    deferred_commands.push_back(envelope);
                    continue;
                }
                let sequence = envelope.sequence;
                let mut evaluate_after_command = true;
                match envelope.command {
                    WorkerCommand::Product(RuntimeCommand::SetOverlayVisible(visible)) => {
                        overlay_visible = visible;
                        publish(&snapshot, |current| {
                            current.overlay_visible = visible;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetInputBindings(bindings)) => {
                        input_bindings = Arc::unwrap_or_clone(bindings);
                        publish(&snapshot, |current| {
                            current.model_input =
                                input_state.model_snapshot(&input_bindings, normalized_cursor);
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::ResetInput(reason)) => {
                        input_state.force_reset(reason);
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input =
                                input_state.model_snapshot(&input_bindings, normalized_cursor);
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        })
                    }
                    WorkerCommand::Product(RuntimeCommand::ApplyInput(envelope)) => {
                        input_state.apply(Arc::unwrap_or_clone(envelope));
                        let activation_pending = pending_model.is_some();
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input =
                                input_state.model_snapshot(&input_bindings, normalized_cursor);
                            current.last_command_failure = None;
                            if !activation_pending {
                                current.last_command_sequence = Some(sequence);
                            }
                        });
                        evaluate_after_command = !activation_pending;
                    }
                    WorkerCommand::Product(RuntimeCommand::ActivateModel(committed)) => {
                        evaluate_after_command = false;
                        begin_model_activation(
                            sequence,
                            committed,
                            None,
                            renderer.as_mut(),
                            &input_state,
                            &mut input_bindings,
                            normalized_cursor,
                            &mut active_model,
                            &mut pending_model,
                            &snapshot,
                        );
                    }
                    WorkerCommand::Product(RuntimeCommand::ActivateModelWithBindings {
                        model,
                        input_bindings: proposed_bindings,
                    }) => {
                        evaluate_after_command = false;
                        begin_model_activation(
                            sequence,
                            model,
                            Some(proposed_bindings),
                            renderer.as_mut(),
                            &input_state,
                            &mut input_bindings,
                            normalized_cursor,
                            &mut active_model,
                            &mut pending_model,
                            &snapshot,
                        );
                    }
                    WorkerCommand::Shutdown => {
                        publish(&snapshot, |current| {
                            current.state = RuntimeState::Stopping;
                            current.pending_model = None;
                            current.last_command_sequence = Some(sequence);
                        });
                        if let Some(renderer) = &renderer {
                            renderer.close();
                        }
                        publish(&snapshot, |current| current.state = RuntimeState::Stopped);
                        drop(active_model);
                        return;
                    }
                }
                if evaluate_after_command && overlay_visible && pending_model.is_none() {
                    evaluate_renderer(
                        renderer.as_mut(),
                        input_state.model_snapshot(&input_bindings, normalized_cursor),
                        &snapshot,
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                consume_cursor(
                    &cursor_slot,
                    &snapshot,
                    &input_state,
                    &input_bindings,
                    &mut normalized_cursor,
                );
                process_model_commit_feedback(
                    renderer.as_mut(),
                    &mut pending_model,
                    &input_state,
                    &mut input_bindings,
                    normalized_cursor,
                    &mut active_model,
                    overlay_visible,
                    &snapshot,
                );
                if overlay_visible && pending_model.is_none() {
                    evaluate_renderer(
                        renderer.as_mut(),
                        input_state.model_snapshot(&input_bindings, normalized_cursor),
                        &snapshot,
                    );
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    consume_cursor(
        &cursor_slot,
        &snapshot,
        &input_state,
        &input_bindings,
        &mut normalized_cursor,
    );
    if let Some(renderer) = &renderer {
        renderer.close();
    }
    publish(&snapshot, |current| current.state = RuntimeState::Stopped);
}

#[allow(clippy::too_many_arguments)]
fn begin_model_activation(
    sequence: u64,
    committed: Arc<CommittedModel>,
    proposed_bindings: Option<Arc<InputBindings>>,
    renderer: Option<&mut RuntimeRenderer>,
    input_state: &InputState,
    input_bindings: &mut InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    active_model: &mut Option<Arc<CommittedModel>>,
    pending_model: &mut Option<PendingModelActivation>,
    snapshot: &SnapshotCell,
) {
    let activation_bindings = proposed_bindings.as_deref().unwrap_or(input_bindings);
    let model_input = input_state.model_snapshot(activation_bindings, normalized_cursor);
    let Some(renderer) = renderer else {
        if let Some(bindings) = proposed_bindings {
            *input_bindings = Arc::unwrap_or_clone(bindings);
        }
        let model_snapshot = committed.snapshot();
        *active_model = Some(committed);
        publish(snapshot, |current| {
            current.state = RuntimeState::Ready;
            current.active_model = Some(model_snapshot);
            current.model_input = model_input;
            current.render_error = None;
            current.last_command_failure = None;
            current.last_command_sequence = Some(sequence);
        });
        return;
    };
    match renderer.prepare(sequence, &committed, model_input) {
        Ok(token) => {
            let model_snapshot = committed.snapshot();
            *pending_model = Some(PendingModelActivation {
                token,
                model: committed,
                input_bindings: proposed_bindings,
            });
            publish(snapshot, |current| {
                current.pending_model = Some(PendingModelSnapshot {
                    token,
                    model: model_snapshot,
                });
                current.last_command_failure = None;
            });
        }
        Err(code) => publish(snapshot, |current| {
            current.last_command_failure = Some(RuntimeCommandFailure { sequence, code });
            current.last_command_sequence = Some(sequence);
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn process_model_commit_feedback(
    renderer: Option<&mut RuntimeRenderer>,
    pending_model: &mut Option<PendingModelActivation>,
    input_state: &InputState,
    input_bindings: &mut InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    active_model: &mut Option<Arc<CommittedModel>>,
    overlay_visible: bool,
    snapshot: &SnapshotCell,
) {
    let Some(renderer) = renderer else {
        return;
    };
    let Some(feedback) = renderer.take_model_commit_feedback() else {
        return;
    };
    let Some(pending) = pending_model.as_ref() else {
        renderer.record_stale_model_commit_feedback();
        return;
    };
    if pending.token != feedback.token {
        renderer.record_stale_model_commit_feedback();
        return;
    }
    let pending = pending_model.take().expect("checked pending model");
    match feedback.outcome {
        ModelCommitOutcome::Prepared if renderer.commit(feedback.token) => {
            if let Some(bindings) = pending.input_bindings {
                *input_bindings = Arc::unwrap_or_clone(bindings);
            }
            let model_input = input_state.model_snapshot(input_bindings, normalized_cursor);
            let model_snapshot = pending.model.snapshot();
            *active_model = Some(pending.model);
            publish(snapshot, |current| {
                current.state = RuntimeState::Ready;
                current.active_model = Some(model_snapshot);
                current.pending_model = None;
                current.model_input = model_input;
                current.render_error = None;
                current.last_command_failure = None;
                current.last_command_sequence = Some(feedback.token.command_sequence);
            });
            if overlay_visible {
                evaluate_renderer(Some(renderer), model_input, snapshot);
            }
        }
        ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed)
            if renderer.reject(feedback.token) =>
        {
            let model_input = input_state.model_snapshot(input_bindings, normalized_cursor);
            publish(snapshot, |current| {
                current.pending_model = None;
                current.model_input = model_input;
                current.last_command_failure = Some(RuntimeCommandFailure {
                    sequence: feedback.token.command_sequence,
                    code: RuntimeRenderErrorCode::GpuPreparationFailed,
                });
                current.last_command_sequence = Some(feedback.token.command_sequence);
            });
            if overlay_visible {
                evaluate_renderer(Some(renderer), model_input, snapshot);
            }
        }
        _ => {
            *pending_model = Some(pending);
            renderer.record_stale_model_commit_feedback();
        }
    }
}

fn evaluate_renderer(
    renderer: Option<&mut RuntimeRenderer>,
    input: ModelInputSnapshot,
    snapshot: &SnapshotCell,
) {
    let Some(renderer) = renderer else {
        return;
    };
    match renderer.evaluate(input) {
        Ok(false) => {}
        Ok(true) => {
            let should_recover = snapshot
                .value
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .render_error
                .is_some();
            if should_recover {
                publish(snapshot, |current| {
                    current.state = RuntimeState::Ready;
                    current.render_error = None;
                });
            }
        }
        Err(code) => {
            let should_publish = snapshot
                .value
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .render_error
                != Some(code);
            if should_publish {
                publish(snapshot, |current| {
                    current.state = RuntimeState::Degraded;
                    current.render_error = Some(code);
                });
            }
        }
    }
}

fn consume_cursor(
    cursor_slot: &CursorSlot,
    snapshot: &SnapshotCell,
    input_state: &InputState,
    input_bindings: &InputBindings,
    normalized_cursor: &mut NormalizedCursorPosition,
) {
    let Some(sample) = cursor_slot.take() else {
        return;
    };
    *normalized_cursor = sample.normalized();
    publish(snapshot, |current| {
        current.cursor.sample = Some(sample);
        current.model_input = input_state.model_snapshot(input_bindings, *normalized_cursor);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use bongocat_model::PresetModelCatalog;
    use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, ModelStore};
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use bongocat_render::{
        ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome, RenderConsumer, RenderFrame,
    };
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use std::collections::BTreeMap;
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

    fn cursor_sample(x: f64, y: f64, at: u64) -> CursorSample {
        CursorSample::new(
            CursorPosition { x, y },
            CursorViewport {
                origin: CursorPosition { x: 0.0, y: 0.0 },
                width: 100.0,
                height: 100.0,
            },
            MonotonicMillis::new(at),
        )
        .expect("valid cursor sample")
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn preset_model(id: &str) -> CommittedModel {
        PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model")
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_render_frame(
        consumer: &RenderConsumer,
        predicate: impl Fn(&RenderFrame) -> bool,
    ) -> RenderFrame {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(frame) = consumer.take_latest()
                && predicate(&frame)
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "render frame timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_prepared_model(
        client: &RuntimeClient,
        consumer: &RenderConsumer,
        command_sequence: u64,
    ) -> RenderFrame {
        let prepared = client
            .wait_for_model_preparation(command_sequence, TIMEOUT)
            .expect("model prepared");
        assert_eq!(prepared.last_command_failure, None);
        let frame = wait_for_render_frame(consumer, |frame| {
            frame
                .model_commit
                .is_some_and(|token| token.command_sequence == command_sequence)
        });
        assert_eq!(
            prepared.pending_model.as_ref().map(|pending| pending.token),
            frame.model_commit
        );
        frame
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn report_model_prepared(
        client: &RuntimeClient,
        consumer: &RenderConsumer,
        frame: &RenderFrame,
    ) -> RuntimeSnapshot {
        let token = frame.model_commit.expect("model commit token");
        consumer
            .report_model_commit(ModelCommitFeedback {
                token,
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("report prepared model");
        client
            .wait_for_command(token.command_sequence, TIMEOUT)
            .expect("model committed")
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
    fn runtime_coalesces_cursor_flood_without_delaying_reliable_release() {
        let owner = RuntimeOwner::start(true, 4);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        let input = owner.input_producer();
        let cursor = owner.cursor_producer();
        let down_sequence = input
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(0),
            })
            .expect("down accepted");
        client
            .wait_for_input_sequence(down_sequence, TIMEOUT)
            .expect("down consumed");

        for index in 1_u32..=10_000 {
            cursor
                .publish(cursor_sample(
                    f64::from(index % 100),
                    f64::from(index % 80),
                    u64::from(index),
                ))
                .expect("cursor sample accepted");
        }
        let release_sequence = input
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Up,
                source: InputSource::Capture,
                at: MonotonicMillis::new(10_001),
            })
            .expect("release accepted independently of cursor flood");
        let released = client
            .wait_for_input_sequence(release_sequence, TIMEOUT)
            .expect("release consumed");
        assert_eq!(released.input.pressed_key_count, 0);
        assert_eq!(released.input.transport.queue_full, 0);

        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.cursor.transport.published, 10_000);
        assert!(stopped.cursor.transport.coalesced > 0);
        assert_eq!(stopped.cursor.transport.pending, 0);
        assert_eq!(
            stopped.cursor.transport.published,
            stopped
                .cursor
                .transport
                .coalesced
                .saturating_add(stopped.cursor.transport.consumed)
        );
    }

    #[test]
    fn runtime_projects_display_relative_cursor_into_model_snapshot() {
        let owner = RuntimeOwner::start(true, 4);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        owner
            .cursor_producer()
            .publish(cursor_sample(25.0, 40.0, 1))
            .expect("cursor accepted");
        let snapshot = client
            .wait_for_cursor_samples(1, TIMEOUT)
            .expect("cursor consumed");
        assert_eq!(snapshot.cursor.sample, Some(cursor_sample(25.0, 40.0, 1)));
        assert_eq!(snapshot.model_input.pointer_x, 0.5);
        assert!((snapshot.model_input.pointer_y - 0.2).abs() < f32::EPSILON);
        assert!((snapshot.model_input.pointer_z + 0.1).abs() < f32::EPSILON);
        owner.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn shutdown_flushes_a_pending_cursor_before_stopped_state() {
        let owner = RuntimeOwner::start(true, 4);
        owner
            .client()
            .wait_for_revision(1, TIMEOUT)
            .expect("runtime ready");
        owner
            .cursor_producer()
            .publish(cursor_sample(75.0, 60.0, 1))
            .expect("pending cursor accepted");
        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
        assert_eq!(stopped.cursor.sample, Some(cursor_sample(75.0, 60.0, 1)));
        assert_eq!(stopped.cursor.transport.consumed, 1);
        assert_eq!(stopped.cursor.transport.pending, 0);
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
        let committed = CommittedModel::from(
            store
                .import(
                    ModelId::parse("unicode").expect("model id"),
                    repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
                )
                .expect("installed model"),
        );
        let owner = RuntimeOwner::start(true, 4);
        let client = owner.client();
        let ready = client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        client
            .send(RuntimeCommand::ActivateModel(Arc::new(committed)))
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn runtime_worker_owns_model_evaluation_and_render_publication() {
        let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let bindings = InputBindings::new(BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]));
        let binding_sequence = client
            .send(RuntimeCommand::SetInputBindings(Arc::new(bindings)))
            .expect("bindings command");
        client
            .wait_for_command(binding_sequence, TIMEOUT)
            .expect("bindings applied");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
                "standard",
            ))))
            .expect("activation command");
        let initial = wait_for_prepared_model(&client, &consumer, activation_sequence);
        assert!(client.snapshot().active_model.is_none());
        let activated = report_model_prepared(&client, &consumer, &initial);
        assert_eq!(activated.state, RuntimeState::Ready);
        assert_eq!(activated.last_command_failure, None);
        assert_eq!(
            activated
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        let committed_baseline = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == initial.model_generation
                && frame.frame_number > initial.frame_number
        });
        let input = owner.input_producer();
        let down_sequence = input
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(1),
            })
            .expect("key down");
        client
            .wait_for_input_sequence(down_sequence, TIMEOUT)
            .expect("key down applied");
        let pressed = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == initial.model_generation
                && frame.transport_sequence > committed_baseline.transport_sequence
                && frame.snapshot != committed_baseline.snapshot
        });
        assert_ne!(pressed.snapshot, committed_baseline.snapshot);

        let stopped = owner.shutdown(TIMEOUT).expect("runtime shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
        while consumer.take_latest().is_some() {}
        let diagnostics = consumer.diagnostics();
        assert_eq!(diagnostics.pending, 0);
        assert_eq!(
            diagnostics.published,
            diagnostics.coalesced.saturating_add(diagnostics.consumed)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn reliable_input_bypasses_deferred_commands_during_model_preparation() {
        let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
                "standard",
            ))))
            .expect("activation command");
        let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
        let visibility_sequence = client
            .send(RuntimeCommand::SetOverlayVisible(false))
            .expect("deferred visibility command");
        let input_sequence = owner
            .input_producer()
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(1),
            })
            .expect("key down behind deferred command");
        let input_applied = client
            .wait_for_input_sequence(input_sequence, TIMEOUT)
            .expect("reliable input bypasses deferred non-input command");
        assert_eq!(input_applied.input.pressed_key_count, 1);
        assert!(input_applied.overlay_visible);

        report_model_prepared(&client, &consumer, &candidate);
        let visibility_applied = client
            .wait_for_command(visibility_sequence, TIMEOUT)
            .expect("deferred command resumes after model commit");
        assert!(!visibility_applied.overlay_visible);
        owner.shutdown(TIMEOUT).expect("runtime shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn cpu_and_gpu_model_failures_preserve_the_active_model_and_bindings() {
        let data = tempdir().expect("data root");
        let store = ModelStore::new(
            data.path().join("models"),
            data.path().join("locks/models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("model store");
        let broken = CommittedModel::from(
            store
                .import(
                    ModelId::parse("broken").expect("model id"),
                    repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
                )
                .expect("install structurally valid model"),
        );
        let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let left_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
            PhysicalKey::KEY_A,
            HandSide::Left,
        )])));
        let first_sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(preset_model("standard")),
                input_bindings: left_bindings,
            })
            .expect("standard activation");
        let first = wait_for_prepared_model(&client, &consumer, first_sequence);
        let first_active = report_model_prepared(&client, &consumer, &first);
        assert_eq!(
            first_active
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );

        let right_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
            PhysicalKey::KEY_A,
            HandSide::Right,
        )])));
        let broken_sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(broken),
                input_bindings: right_bindings,
            })
            .expect("broken activation command");
        let rejected = client
            .wait_for_command(broken_sequence, TIMEOUT)
            .expect("broken activation result");
        assert_eq!(
            rejected.last_command_failure,
            Some(RuntimeCommandFailure {
                sequence: broken_sequence,
                code: RuntimeRenderErrorCode::ModelLoadFailed,
            })
        );
        assert_eq!(rejected.state, RuntimeState::Ready);
        assert_eq!(
            rejected
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        let preserved = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == first.model_generation
                && frame.frame_number > first.frame_number
        });
        assert_eq!(preserved.model_generation, 0);
        let input_producer = owner.input_producer();
        let down_sequence = input_producer
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(1),
            })
            .expect("key down");
        let input_after_rejection = client
            .wait_for_input_sequence(down_sequence, TIMEOUT)
            .expect("key down applied");
        assert!(input_after_rejection.model_input.left_hand_down);
        assert!(!input_after_rejection.model_input.right_hand_down);

        let gpu_rejected_sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(preset_model("keyboard")),
                input_bindings: Arc::new(InputBindings::new(BTreeMap::from([(
                    PhysicalKey::KEY_A,
                    HandSide::Right,
                )]))),
            })
            .expect("GPU-rejected activation");
        let gpu_candidate = wait_for_prepared_model(&client, &consumer, gpu_rejected_sequence);
        assert_eq!(gpu_candidate.model_generation, 1);
        let pending_release_sequence = input_producer
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Up,
                source: InputSource::Capture,
                at: MonotonicMillis::new(2),
            })
            .expect("key up while model commit is pending");
        let released_while_pending = client
            .wait_for_input_sequence(pending_release_sequence, TIMEOUT)
            .expect("pending model does not block key release");
        assert!(!released_while_pending.model_input.left_hand_down);
        assert!(!released_while_pending.model_input.right_hand_down);
        let pending_down_sequence = input_producer
            .publish(InputEvent::Edge {
                control: InputControl::Key(PhysicalKey::KEY_A),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(3),
            })
            .expect("key down while model commit is pending");
        let while_pending = client
            .wait_for_input_sequence(pending_down_sequence, TIMEOUT)
            .expect("pending model does not block key down");
        assert_eq!(
            while_pending
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert!(while_pending.model_input.left_hand_down);
        assert!(!while_pending.model_input.right_hand_down);

        let rejected_token = gpu_candidate.model_commit.expect("candidate token");
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: rejected_token,
                outcome: ModelCommitOutcome::Rejected(
                    ModelCommitErrorCode::ResourcePreparationFailed,
                ),
            })
            .expect("reject GPU candidate");
        let gpu_rejected = client
            .wait_for_command(gpu_rejected_sequence, TIMEOUT)
            .expect("GPU rejection applied");
        assert_eq!(
            gpu_rejected.last_command_failure,
            Some(RuntimeCommandFailure {
                sequence: gpu_rejected_sequence,
                code: RuntimeRenderErrorCode::GpuPreparationFailed,
            })
        );
        assert_eq!(
            gpu_rejected
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert!(gpu_rejected.model_input.left_hand_down);
        assert!(!gpu_rejected.model_input.right_hand_down);
        let resumed = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == first.model_generation
                && frame.transport_sequence > gpu_candidate.transport_sequence
        });
        assert!(resumed.frame_number > first.frame_number);

        let replacement_sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(preset_model("keyboard")),
                input_bindings: Arc::new(InputBindings::new(BTreeMap::from([(
                    PhysicalKey::KEY_A,
                    HandSide::Right,
                )]))),
            })
            .expect("replacement activation");
        let replacement = wait_for_prepared_model(&client, &consumer, replacement_sequence);
        assert_eq!(replacement.model_generation, 2);
        let replaced = report_model_prepared(&client, &consumer, &replacement);
        assert_eq!(replaced.last_command_failure, None);
        assert_eq!(
            replaced
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("keyboard")
        );
        assert!(replaced.model_input.right_hand_down);
        assert!(!replaced.model_input.left_hand_down);
        owner.shutdown(TIMEOUT).expect("runtime shutdown");
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
            cursor_slot: Arc::new(CursorSlot::default()),
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
            cursor_slot: Arc::new(CursorSlot::default()),
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
