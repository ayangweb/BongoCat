#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
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
}
