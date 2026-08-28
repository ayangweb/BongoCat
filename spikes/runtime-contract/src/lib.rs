#![forbid(unsafe_code)]

use std::{
    collections::{BTreeSet, VecDeque},
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeResetReason {
    QueueOverflow,
    InputSession,
    WorkerRecovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidTransition {
        state: RuntimeState,
        operation: &'static str,
    },
    NonMonotonicTick {
        previous_ms: u64,
        received_ms: u64,
    },
    ShutdownDrainPending {
        remaining: usize,
    },
    WorkCompletionExceedsPending {
        pending: usize,
        completed: usize,
    },
    AlreadyStopped,
    CannotReset {
        state: RuntimeState,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { state, operation } => {
                write!(formatter, "cannot {operation} while runtime is {state:?}")
            }
            Self::NonMonotonicTick {
                previous_ms,
                received_ms,
            } => write!(
                formatter,
                "tick moved backwards from {previous_ms} ms to {received_ms} ms"
            ),
            Self::ShutdownDrainPending { remaining } => {
                write!(
                    formatter,
                    "shutdown still has {remaining} pending work item(s)"
                )
            }
            Self::WorkCompletionExceedsPending { pending, completed } => write!(
                formatter,
                "cannot complete {completed} work item(s) with only {pending} pending"
            ),
            Self::AlreadyStopped => formatter.write_str("runtime is already stopped"),
            Self::CannotReset { state } => write!(formatter, "cannot reset runtime in {state:?}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationDisposition {
    Accepted,
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownDisposition {
    Completed,
    TimedOut { discarded_work: usize },
}

#[derive(Debug)]
pub struct RuntimeContract {
    state: RuntimeState,
    last_tick_ms: Option<u64>,
    pending_work: usize,
    seen_operations: BTreeSet<u64>,
    accepted_operations: u64,
    duplicate_operations: u64,
    timed_out_shutdowns: u64,
    reset_count: u64,
    discarded_work: u64,
}

impl Default for RuntimeContract {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeContract {
    pub const fn new() -> Self {
        Self {
            state: RuntimeState::Starting,
            last_tick_ms: None,
            pending_work: 0,
            seen_operations: BTreeSet::new(),
            accepted_operations: 0,
            duplicate_operations: 0,
            timed_out_shutdowns: 0,
            reset_count: 0,
            discarded_work: 0,
        }
    }
    pub const fn state(&self) -> RuntimeState {
        self.state
    }
    pub const fn last_tick_ms(&self) -> Option<u64> {
        self.last_tick_ms
    }
    pub const fn pending_work(&self) -> usize {
        self.pending_work
    }
    pub const fn accepted_operations(&self) -> u64 {
        self.accepted_operations
    }
    pub const fn duplicate_operations(&self) -> u64 {
        self.duplicate_operations
    }
    pub const fn timed_out_shutdowns(&self) -> u64 {
        self.timed_out_shutdowns
    }
    pub const fn reset_count(&self) -> u64 {
        self.reset_count
    }
    pub const fn discarded_work(&self) -> u64 {
        self.discarded_work
    }

    pub fn mark_ready(&mut self) -> Result<(), RuntimeError> {
        if self.state != RuntimeState::Starting {
            return Err(self.invalid("mark ready"));
        }
        self.state = RuntimeState::Ready;
        Ok(())
    }
    pub fn mark_degraded(&mut self) -> Result<(), RuntimeError> {
        match self.state {
            RuntimeState::Starting | RuntimeState::Ready => {
                self.state = RuntimeState::Degraded;
                Ok(())
            }
            _ => Err(self.invalid("mark degraded")),
        }
    }
    pub fn recover(&mut self) -> Result<(), RuntimeError> {
        if self.state != RuntimeState::Degraded {
            return Err(self.invalid("recover"));
        }
        self.state = RuntimeState::Ready;
        Ok(())
    }
    pub fn tick(&mut self, at_ms: u64) -> Result<(), RuntimeError> {
        if !matches!(self.state, RuntimeState::Ready | RuntimeState::Degraded) {
            return Err(self.invalid("tick"));
        }
        if let Some(previous_ms) = self.last_tick_ms
            && at_ms < previous_ms
        {
            return Err(RuntimeError::NonMonotonicTick {
                previous_ms,
                received_ms: at_ms,
            });
        }
        self.last_tick_ms = Some(at_ms);
        Ok(())
    }
    pub fn submit_operation(
        &mut self,
        operation_id: u64,
    ) -> Result<OperationDisposition, RuntimeError> {
        if !matches!(self.state, RuntimeState::Ready | RuntimeState::Degraded) {
            return Err(self.invalid("submit operation"));
        }
        if !self.seen_operations.insert(operation_id) {
            self.duplicate_operations += 1;
            return Ok(OperationDisposition::Duplicate);
        }
        self.accepted_operations += 1;
        self.pending_work += 1;
        Ok(OperationDisposition::Accepted)
    }
    pub fn complete_work(&mut self, count: usize) -> Result<(), RuntimeError> {
        if count > self.pending_work {
            return Err(RuntimeError::WorkCompletionExceedsPending {
                pending: self.pending_work,
                completed: count,
            });
        }
        self.pending_work -= count;
        Ok(())
    }
    pub fn reset(&mut self, _reason: RuntimeResetReason) -> Result<(), RuntimeError> {
        match self.state {
            RuntimeState::Starting => {
                self.last_tick_ms = None;
                self.reset_count += 1;
                Ok(())
            }
            RuntimeState::Ready | RuntimeState::Degraded => {
                self.last_tick_ms = None;
                self.discarded_work += self.pending_work as u64;
                self.pending_work = 0;
                self.reset_count += 1;
                self.state = RuntimeState::Degraded;
                Ok(())
            }
            state => Err(RuntimeError::CannotReset { state }),
        }
    }
    pub fn begin_shutdown(&mut self) -> Result<(), RuntimeError> {
        match self.state {
            RuntimeState::Starting | RuntimeState::Ready | RuntimeState::Degraded => {
                self.state = RuntimeState::Stopping;
                Ok(())
            }
            RuntimeState::Stopped => Err(RuntimeError::AlreadyStopped),
            _ => Err(self.invalid("begin shutdown")),
        }
    }
    pub fn complete_shutdown(&mut self) -> Result<ShutdownDisposition, RuntimeError> {
        if self.state != RuntimeState::Stopping {
            return Err(self.invalid("complete shutdown"));
        }
        if self.pending_work != 0 {
            return Err(RuntimeError::ShutdownDrainPending {
                remaining: self.pending_work,
            });
        }
        self.state = RuntimeState::Stopped;
        Ok(ShutdownDisposition::Completed)
    }
    pub fn timeout_shutdown(&mut self) -> Result<ShutdownDisposition, RuntimeError> {
        if self.state != RuntimeState::Stopping {
            return Err(self.invalid("timeout shutdown"));
        }
        let discarded_work = self.pending_work;
        self.pending_work = 0;
        self.timed_out_shutdowns += 1;
        self.state = RuntimeState::Stopped;
        Ok(ShutdownDisposition::TimedOut { discarded_work })
    }
    fn invalid(&self, operation: &'static str) -> RuntimeError {
        RuntimeError::InvalidTransition {
            state: self.state,
            operation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCommand {
    MarkReady,
    MarkDegraded,
    Recover,
    Tick {
        at_ms: u64,
    },
    SubmitOperation {
        operation_id: u64,
    },
    CompleteWork {
        count: usize,
    },
    BeginShutdown,
    CompleteShutdown,
    TimeoutShutdown,
    Reset {
        reason: RuntimeResetReason,
    },
    /// Deliberately panics to verify worker isolation and diagnostics.
    PanicForTest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandQueueErrorKind {
    Full,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandQueueError {
    pub kind: CommandQueueErrorKind,
    pub command: RuntimeCommand,
    pub discarded: usize,
}

impl fmt::Display for CommandQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "runtime command queue {:?} while submitting {:?} (discarded={})",
            self.kind, self.command, self.discarded
        )
    }
}

impl std::error::Error for CommandQueueError {}

#[derive(Debug)]
struct CommandQueueState {
    capacity: usize,
    commands: VecDeque<RuntimeCommand>,
    closed: bool,
    overflow_count: u64,
    recovery_reset_count: u64,
    recovery_discard_count: u64,
}

#[derive(Debug)]
pub struct CommandQueue {
    state: Mutex<CommandQueueState>,
    wake: Condvar,
}

impl CommandQueue {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "runtime command queue capacity must be positive"
        );
        Self {
            state: Mutex::new(CommandQueueState {
                capacity,
                commands: VecDeque::with_capacity(capacity),
                closed: false,
                overflow_count: 0,
                recovery_reset_count: 0,
                recovery_discard_count: 0,
            }),
            wake: Condvar::new(),
        }
    }

    pub fn push(&self, command: RuntimeCommand) -> Result<(), CommandQueueError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.closed {
            return Err(CommandQueueError {
                kind: CommandQueueErrorKind::Closed,
                command,
                discarded: 0,
            });
        }
        if state.commands.len() == state.capacity {
            return Err(CommandQueueError {
                kind: CommandQueueErrorKind::Full,
                command,
                discarded: 0,
            });
        }
        state.commands.push_back(command);
        self.wake.notify_one();
        Ok(())
    }

    pub fn push_with_overflow_reset(
        &self,
        command: RuntimeCommand,
    ) -> Result<(), CommandQueueError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.closed {
            return Err(CommandQueueError {
                kind: CommandQueueErrorKind::Closed,
                command,
                discarded: 0,
            });
        }
        if state.commands.len() == state.capacity {
            let discarded = state.commands.len();
            state.overflow_count += 1;
            state.recovery_reset_count += 1;
            state.recovery_discard_count += discarded as u64;
            state.commands.clear();
            state.commands.push_back(RuntimeCommand::Reset {
                reason: RuntimeResetReason::QueueOverflow,
            });
            self.wake.notify_one();
            return Err(CommandQueueError {
                kind: CommandQueueErrorKind::Full,
                command,
                discarded,
            });
        }
        state.commands.push_back(command);
        self.wake.notify_one();
        Ok(())
    }

    fn pop_blocking(&self) -> Option<RuntimeCommand> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(command) = state.commands.pop_front() {
                return Some(command);
            }
            if state.closed {
                return None;
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed = true;
        self.wake.notify_all();
    }

    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .commands
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_closed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .closed
    }

    fn diagnostics(&self) -> QueueDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        QueueDiagnostics {
            queued: state.commands.len(),
            overflow_count: state.overflow_count,
            recovery_reset_count: state.recovery_reset_count,
            recovery_discard_count: state.recovery_discard_count,
            closed: state.closed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct QueueDiagnostics {
    queued: usize,
    overflow_count: u64,
    recovery_reset_count: u64,
    recovery_discard_count: u64,
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerExit {
    Clean(ShutdownDisposition),
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerStatus {
    Starting,
    Running,
    Stopped,
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub last_tick_ms: Option<u64>,
    pub pending_work: usize,
    pub accepted_operations: u64,
    pub duplicate_operations: u64,
    pub timed_out_shutdowns: u64,
    pub reset_count: u64,
    pub discarded_work: u64,
    pub command_errors: u64,
    pub worker_panics: u64,
    pub worker_status: WorkerStatus,
    pub queued_commands: usize,
    pub queue_overflows: u64,
    pub queue_recovery_resets: u64,
    pub queue_discarded_commands: u64,
    pub queue_closed: bool,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            state: RuntimeState::Starting,
            last_tick_ms: None,
            pending_work: 0,
            accepted_operations: 0,
            duplicate_operations: 0,
            timed_out_shutdowns: 0,
            reset_count: 0,
            discarded_work: 0,
            command_errors: 0,
            worker_panics: 0,
            worker_status: WorkerStatus::Starting,
            queued_commands: 0,
            queue_overflows: 0,
            queue_recovery_resets: 0,
            queue_discarded_commands: 0,
            queue_closed: false,
        }
    }
}

#[derive(Debug)]
pub struct WorkerJoinReport {
    pub exit: WorkerExit,
    pub snapshot: RuntimeSnapshot,
}

#[derive(Debug)]
pub struct RuntimeWorker {
    queue: Arc<CommandQueue>,
    snapshot: Arc<Mutex<RuntimeSnapshot>>,
    join: Option<JoinHandle<WorkerExit>>,
}

impl RuntimeWorker {
    pub fn spawn(capacity: usize) -> Self {
        let queue = Arc::new(CommandQueue::with_capacity(capacity));
        let snapshot = Arc::new(Mutex::new(RuntimeSnapshot::default()));
        let worker_queue = Arc::clone(&queue);
        let worker_snapshot = Arc::clone(&snapshot);
        let join = thread::Builder::new()
            .name("bongocat-runtime-spike".to_owned())
            .spawn(move || run_worker(worker_queue, worker_snapshot))
            .expect("runtime worker thread must start");
        Self {
            queue,
            snapshot,
            join: Some(join),
        }
    }

    pub fn send(&self, command: RuntimeCommand) -> Result<(), CommandQueueError> {
        self.queue.push_with_overflow_reset(command)
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let mut snapshot = *self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let queue = self.queue.diagnostics();
        snapshot.queued_commands = queue.queued;
        snapshot.queue_overflows = queue.overflow_count;
        snapshot.queue_recovery_resets = queue.recovery_reset_count;
        snapshot.queue_discarded_commands = queue.recovery_discard_count;
        snapshot.queue_closed = queue.closed;
        snapshot
    }

    pub fn shutdown(mut self) -> WorkerJoinReport {
        self.queue.close();
        let exit = self
            .join
            .take()
            .expect("runtime worker join handle must exist")
            .join()
            .unwrap_or(WorkerExit::Panicked);
        WorkerJoinReport {
            exit,
            snapshot: self.snapshot(),
        }
    }
}

impl Drop for RuntimeWorker {
    fn drop(&mut self) {
        self.queue.close();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run_worker(queue: Arc<CommandQueue>, snapshot: Arc<Mutex<RuntimeSnapshot>>) -> WorkerExit {
    let mut runtime = RuntimeContract::new();
    let mut command_errors = 0;
    let mut worker_panics = 0;
    let mut shutdown_disposition = None;
    update_snapshot(
        &snapshot,
        &runtime,
        command_errors,
        worker_panics,
        WorkerStatus::Running,
        &queue,
    );

    while let Some(command) = queue.pop_blocking() {
        let result = catch_unwind(AssertUnwindSafe(|| apply_command(&mut runtime, command)));
        match result {
            Ok(Ok(disposition)) => {
                if disposition.is_some() {
                    shutdown_disposition = disposition;
                }
            }
            Ok(Err(_error)) => command_errors += 1,
            Err(_) => {
                worker_panics += 1;
                update_snapshot(
                    &snapshot,
                    &runtime,
                    command_errors,
                    worker_panics,
                    WorkerStatus::Panicked,
                    &queue,
                );
                queue.close();
                return WorkerExit::Panicked;
            }
        }
        update_snapshot(
            &snapshot,
            &runtime,
            command_errors,
            worker_panics,
            WorkerStatus::Running,
            &queue,
        );
    }

    let exit = if let Some(disposition) = shutdown_disposition {
        disposition
    } else if runtime.state() == RuntimeState::Stopped {
        ShutdownDisposition::Completed
    } else {
        let _ = runtime.begin_shutdown();
        match runtime.complete_shutdown() {
            Ok(disposition) => disposition,
            Err(_) => runtime
                .timeout_shutdown()
                .unwrap_or(ShutdownDisposition::TimedOut {
                    discarded_work: runtime.pending_work(),
                }),
        }
    };
    update_snapshot(
        &snapshot,
        &runtime,
        command_errors,
        worker_panics,
        WorkerStatus::Stopped,
        &queue,
    );
    WorkerExit::Clean(exit)
}

fn apply_command(
    runtime: &mut RuntimeContract,
    command: RuntimeCommand,
) -> Result<Option<ShutdownDisposition>, RuntimeError> {
    match command {
        RuntimeCommand::MarkReady => runtime.mark_ready().map(|_| None),
        RuntimeCommand::MarkDegraded => runtime.mark_degraded().map(|_| None),
        RuntimeCommand::Recover => runtime.recover().map(|_| None),
        RuntimeCommand::Tick { at_ms } => runtime.tick(at_ms).map(|_| None),
        RuntimeCommand::SubmitOperation { operation_id } => {
            runtime.submit_operation(operation_id).map(|_| None)
        }
        RuntimeCommand::CompleteWork { count } => runtime.complete_work(count).map(|_| None),
        RuntimeCommand::BeginShutdown => runtime.begin_shutdown().map(|_| None),
        RuntimeCommand::CompleteShutdown => runtime.complete_shutdown().map(Some),
        RuntimeCommand::TimeoutShutdown => runtime.timeout_shutdown().map(Some),
        RuntimeCommand::Reset { reason } => runtime.reset(reason).map(|_| None),
        RuntimeCommand::PanicForTest => panic!("runtime worker panic probe"),
    }
}

fn update_snapshot(
    snapshot: &Mutex<RuntimeSnapshot>,
    runtime: &RuntimeContract,
    command_errors: u64,
    worker_panics: u64,
    worker_status: WorkerStatus,
    queue: &CommandQueue,
) {
    let queue_diagnostics = queue.diagnostics();
    let mut current = snapshot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    current.revision += 1;
    current.state = runtime.state();
    current.last_tick_ms = runtime.last_tick_ms();
    current.pending_work = runtime.pending_work();
    current.accepted_operations = runtime.accepted_operations();
    current.duplicate_operations = runtime.duplicate_operations();
    current.timed_out_shutdowns = runtime.timed_out_shutdowns();
    current.reset_count = runtime.reset_count();
    current.discarded_work = runtime.discarded_work();
    current.command_errors = command_errors;
    current.worker_panics = worker_panics;
    current.worker_status = worker_status;
    current.queued_commands = queue_diagnostics.queued;
    current.queue_overflows = queue_diagnostics.overflow_count;
    current.queue_recovery_resets = queue_diagnostics.recovery_reset_count;
    current.queue_discarded_commands = queue_diagnostics.recovery_discard_count;
    current.queue_closed = queue_diagnostics.closed;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_supports_degraded_recovery() {
        let mut r = RuntimeContract::new();
        r.mark_ready().unwrap();
        r.mark_degraded().unwrap();
        r.recover().unwrap();
        r.begin_shutdown().unwrap();
        assert_eq!(
            r.complete_shutdown().unwrap(),
            ShutdownDisposition::Completed
        );
    }
    #[test]
    fn tick_rejects_regression() {
        let mut r = RuntimeContract::new();
        r.mark_ready().unwrap();
        r.tick(100).unwrap();
        assert_eq!(
            r.tick(99).unwrap_err(),
            RuntimeError::NonMonotonicTick {
                previous_ms: 100,
                received_ms: 99
            }
        );
    }
    #[test]
    fn operation_id_is_idempotent() {
        let mut r = RuntimeContract::new();
        r.mark_ready().unwrap();
        assert_eq!(
            r.submit_operation(7).unwrap(),
            OperationDisposition::Accepted
        );
        assert_eq!(
            r.submit_operation(7).unwrap(),
            OperationDisposition::Duplicate
        );
        assert_eq!(r.pending_work(), 1);
    }
    #[test]
    fn shutdown_drains_work() {
        let mut r = RuntimeContract::new();
        r.mark_ready().unwrap();
        r.submit_operation(1).unwrap();
        r.begin_shutdown().unwrap();
        assert_eq!(
            r.complete_shutdown().unwrap_err(),
            RuntimeError::ShutdownDrainPending { remaining: 1 }
        );
        r.complete_work(1).unwrap();
        assert_eq!(
            r.complete_shutdown().unwrap(),
            ShutdownDisposition::Completed
        );
    }
    #[test]
    fn timeout_reports_discarded_work() {
        let mut r = RuntimeContract::new();
        r.mark_ready().unwrap();
        r.submit_operation(1).unwrap();
        r.begin_shutdown().unwrap();
        assert_eq!(
            r.timeout_shutdown().unwrap(),
            ShutdownDisposition::TimedOut { discarded_work: 1 }
        );
        assert_eq!(r.state(), RuntimeState::Stopped);
    }

    #[test]
    fn startup_can_be_cancelled_and_shut_down_cleanly() {
        let mut runtime = RuntimeContract::new();
        runtime.begin_shutdown().unwrap();
        assert_eq!(
            runtime.complete_shutdown().unwrap(),
            ShutdownDisposition::Completed
        );
    }

    #[test]
    fn duplicate_work_completion_is_an_observable_error() {
        let mut runtime = RuntimeContract::new();
        runtime.mark_ready().unwrap();
        runtime.submit_operation(1).unwrap();
        runtime.complete_work(1).unwrap();
        assert_eq!(
            runtime.complete_work(1).unwrap_err(),
            RuntimeError::WorkCompletionExceedsPending {
                pending: 0,
                completed: 1
            }
        );
    }

    #[test]
    fn reset_discards_pending_work_and_requires_recovery() {
        let mut runtime = RuntimeContract::new();
        runtime.mark_ready().unwrap();
        runtime.tick(50).unwrap();
        runtime.submit_operation(9).unwrap();
        runtime.reset(RuntimeResetReason::QueueOverflow).unwrap();
        assert_eq!(runtime.state(), RuntimeState::Degraded);
        assert_eq!(runtime.last_tick_ms(), None);
        assert_eq!(runtime.pending_work(), 0);
        assert_eq!(runtime.reset_count(), 1);
        assert_eq!(runtime.discarded_work(), 1);
        runtime.recover().unwrap();
        runtime.tick(0).unwrap();
    }

    #[test]
    fn command_queue_overflow_discards_old_commands_and_injects_reset() {
        let queue = CommandQueue::with_capacity(2);
        queue.push(RuntimeCommand::MarkReady).unwrap();
        queue.push(RuntimeCommand::Tick { at_ms: 10 }).unwrap();
        let error = queue
            .push_with_overflow_reset(RuntimeCommand::SubmitOperation { operation_id: 4 })
            .unwrap_err();
        assert_eq!(error.kind, CommandQueueErrorKind::Full);
        assert_eq!(
            error.command,
            RuntimeCommand::SubmitOperation { operation_id: 4 }
        );
        assert_eq!(error.discarded, 2);
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue.pop_blocking(),
            Some(RuntimeCommand::Reset {
                reason: RuntimeResetReason::QueueOverflow,
            })
        );
        assert!(queue.is_empty());
        assert_eq!(queue.diagnostics().overflow_count, 1);
        assert_eq!(queue.diagnostics().recovery_reset_count, 1);
        assert_eq!(queue.diagnostics().recovery_discard_count, 2);
    }

    #[test]
    fn worker_shutdown_drains_queued_commands_and_publishes_revisioned_snapshot() {
        let worker = RuntimeWorker::spawn(8);
        worker.send(RuntimeCommand::MarkReady).unwrap();
        worker.send(RuntimeCommand::Tick { at_ms: 12 }).unwrap();
        worker
            .send(RuntimeCommand::SubmitOperation { operation_id: 11 })
            .unwrap();
        worker.send(RuntimeCommand::BeginShutdown).unwrap();
        worker
            .send(RuntimeCommand::CompleteWork { count: 1 })
            .unwrap();

        let report = worker.shutdown();
        assert_eq!(
            report.exit,
            WorkerExit::Clean(ShutdownDisposition::Completed)
        );
        assert_eq!(report.snapshot.state, RuntimeState::Stopped);
        assert_eq!(report.snapshot.last_tick_ms, Some(12));
        assert_eq!(report.snapshot.pending_work, 0);
        assert_eq!(report.snapshot.accepted_operations, 1);
        assert!(report.snapshot.revision >= 7);
        assert_eq!(report.snapshot.worker_status, WorkerStatus::Stopped);
        assert!(report.snapshot.queue_closed);
    }

    #[test]
    fn worker_shutdown_reports_uncompleted_work_after_queue_drain() {
        let worker = RuntimeWorker::spawn(4);
        worker.send(RuntimeCommand::MarkReady).unwrap();
        worker
            .send(RuntimeCommand::SubmitOperation { operation_id: 3 })
            .unwrap();
        let report = worker.shutdown();
        assert_eq!(
            report.exit,
            WorkerExit::Clean(ShutdownDisposition::TimedOut { discarded_work: 1 })
        );
        assert_eq!(report.snapshot.state, RuntimeState::Stopped);
        assert_eq!(report.snapshot.pending_work, 0);
        assert_eq!(report.snapshot.accepted_operations, 1);
    }

    #[test]
    fn worker_panic_is_caught_and_reported_without_crossing_join_boundary() {
        let worker = RuntimeWorker::spawn(2);
        worker.send(RuntimeCommand::PanicForTest).unwrap();
        let report = worker.shutdown();
        assert_eq!(report.exit, WorkerExit::Panicked);
        assert_eq!(report.snapshot.worker_status, WorkerStatus::Panicked);
        assert_eq!(report.snapshot.worker_panics, 1);
        assert!(report.snapshot.queue_closed);
    }

    #[test]
    fn worker_keeps_running_after_rejected_command_and_reports_error() {
        let worker = RuntimeWorker::spawn(4);
        worker.send(RuntimeCommand::Tick { at_ms: 1 }).unwrap();
        worker.send(RuntimeCommand::MarkReady).unwrap();
        let report = worker.shutdown();
        assert_eq!(
            report.exit,
            WorkerExit::Clean(ShutdownDisposition::Completed)
        );
        assert_eq!(report.snapshot.command_errors, 1);
        assert_eq!(report.snapshot.state, RuntimeState::Stopped);
    }
}
