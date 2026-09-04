#![forbid(unsafe_code)]

mod cursor;
mod gamepad;
mod input;
mod platform_input;
mod rendering;

use bongocat_audio::{
    MotionAudioClient, MotionAudioCommand, MotionAudioDiagnostics, MotionAudioStopReason,
    MotionAudioVolume,
};
use bongocat_model::{CommittedModel, ModelSnapshot};
use bongocat_render::{ModelCommitErrorCode, ModelCommitOutcome, ModelCommitToken, RenderConsumer};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub use cursor::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorSampleError,
    CursorSnapshot, CursorTransportDiagnostics, CursorViewport, NormalizedCursorPosition,
};
use cursor::{CursorSlot, CursorSmoother};
use gamepad::{DEFAULT_GAMEPAD_AXIS_CAPACITY, GamepadAxisSlot};
pub use gamepad::{
    GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample, GamepadAxisSettings,
    GamepadAxisTransportDiagnostics, GamepadConnectionError,
};
pub use input::{
    GamepadAxis, GamepadAxisKey, GamepadButton, GamepadButtonKey, GamepadConnection, HandSide,
    InputBindings, InputControl, InputDiagnostics, InputDisposition, InputEdge, InputEvent,
    InputResetReason, InputSnapshot, InputSource, InputTransportDiagnostics, ModelInputSnapshot,
    MonotonicMillis, MouseButton, PhysicalKey, ReconciliationPolicy, SequencedInputEvent,
};
use input::{InputState, InputTransportCounters};
pub use platform_input::{
    PlatformInputDiagnostics, PlatformInputDiagnosticsProducer,
    PlatformInputDiagnosticsPublishError, PlatformInputServiceStatus,
};
use rendering::{MotionStopStatus, RenderEvaluation, RuntimeRenderBootstrap, RuntimeRenderer};

const CURSOR_SAMPLE_INTERVAL: Duration = Duration::from_millis(16);

pub trait MonotonicClock: Send + Sync + 'static {
    fn now(&self) -> Duration;
}

struct SystemMonotonicClock {
    origin: Instant,
}

impl SystemMonotonicClock {
    fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

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
    MotionLoadFailed,
    ExpressionLoadFailed,
    GpuPreparationFailed,
    PlatformUnsupported,
    TransportClosed,
    OverlaySettingsInvalid,
}

impl RuntimeRenderErrorCode {
    pub const ALL: [Self; 8] = [
        Self::ModelLoadFailed,
        Self::ModelEvaluationFailed,
        Self::MotionLoadFailed,
        Self::ExpressionLoadFailed,
        Self::GpuPreparationFailed,
        Self::PlatformUnsupported,
        Self::TransportClosed,
        Self::OverlaySettingsInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLoadFailed => "model_load_failed",
            Self::ModelEvaluationFailed => "model_evaluation_failed",
            Self::MotionLoadFailed => "motion_load_failed",
            Self::ExpressionLoadFailed => "expression_load_failed",
            Self::GpuPreparationFailed => "gpu_preparation_failed",
            Self::PlatformUnsupported => "platform_unsupported",
            Self::TransportClosed => "transport_closed",
            Self::OverlaySettingsInvalid => "overlay_settings_invalid",
        }
    }
}

impl fmt::Display for RuntimeRenderErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySettings {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            click_through: false,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
        }
    }
}

impl OverlaySettings {
    pub const fn is_valid(self) -> bool {
        self.scale_percent >= 25
            && self.scale_percent <= 400
            && self.opacity_percent >= 1
            && self.opacity_percent <= 100
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModelSettings {
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    pub ignore_pointer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionId {
    group: String,
    index: usize,
}

impl MotionId {
    pub fn new(group: impl Into<String>, index: usize) -> Result<Self, MotionIdError> {
        let group = group.into();
        if group.trim().is_empty() {
            return Err(MotionIdError);
        }
        Ok(Self { group, index })
    }

    pub fn group(&self) -> &str {
        &self.group
    }

    pub const fn index(&self) -> usize {
        self.index
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionIdError;

impl fmt::Display for MotionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("motion group must not be blank")
    }
}

impl std::error::Error for MotionIdError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionId(String);

impl ExpressionId {
    pub fn new(name: impl Into<String>) -> Result<Self, ExpressionIdError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ExpressionIdError);
        }
        Ok(Self(name))
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpressionIdError;

impl fmt::Display for ExpressionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expression name must not be blank")
    }
}

impl std::error::Error for ExpressionIdError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MotionPriority {
    Idle,
    Normal,
    Force,
}

/// A shortcut action resolved by the configuration/platform boundary.
/// Runtime receives the already-typed model identity and never parses a
/// behavior string or platform key code on its real-time thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutAction {
    StartMotion {
        motion: MotionId,
        priority: MotionPriority,
    },
    StopMotion(MotionId),
    SetExpression(ExpressionId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMotionSnapshot {
    pub motion: MotionId,
    pub priority: MotionPriority,
    pub command_sequence: u64,
    pub stop_command_sequence: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveExpressionSnapshot {
    pub expression: ExpressionId,
    pub command_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionUserDataSnapshot {
    pub event_sequence: u64,
    pub motion: MotionId,
    pub cycle: u64,
    pub local_time: Duration,
    pub value: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MotionEventDiagnostics {
    pub emitted: u64,
    pub skipped: u64,
    pub last_event: Option<MotionUserDataSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeCommandFailure {
    pub sequence: u64,
    pub code: RuntimeRenderErrorCode,
}

/// Aggregate counters for the bounded command queue.
///
/// The counters intentionally contain no command payloads or platform data so
/// they can be safely exposed in runtime snapshots and diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeCommandTransportDiagnostics {
    pub enqueued: u64,
    pub queue_full: u64,
    pub runtime_stopped: u64,
    pub sequence_gap_count: u64,
    pub missing_sequence_count: u64,
    pub duplicate_sequence_count: u64,
    pub out_of_order_sequence_count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingModelSnapshot {
    pub token: ModelCommitToken,
    pub model: ModelSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeCommand {
    /// Drive one deterministic runtime evaluation using the injected clock.
    Tick,
    SetOverlayVisible(bool),
    SetOverlaySettings(OverlaySettings),
    SetModelSettings(ModelSettings),
    SetMotionAudioEnabled(bool),
    SetInputBindings(Arc<InputBindings>),
    SetGamepadAxisSettings(GamepadAxisSettings),
    ResetInput(InputResetReason),
    ApplyInput(Arc<SequencedInputEvent>),
    ActivateModel(Arc<CommittedModel>),
    ActivateModelWithBindings {
        model: Arc<CommittedModel>,
        input_bindings: Arc<InputBindings>,
    },
    StartMotion {
        motion: MotionId,
        priority: MotionPriority,
    },
    StopMotion(MotionId),
    SetExpression(ExpressionId),
}

impl ShortcutAction {
    fn into_runtime_command(self) -> RuntimeCommand {
        match self {
            Self::StartMotion { motion, priority } => {
                RuntimeCommand::StartMotion { motion, priority }
            }
            Self::StopMotion(motion) => RuntimeCommand::StopMotion(motion),
            Self::SetExpression(expression) => RuntimeCommand::SetExpression(expression),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub overlay_visible: bool,
    pub overlay_settings: OverlaySettings,
    pub model_settings: ModelSettings,
    pub gamepad_axis_settings: GamepadAxisSettings,
    pub motion_audio_enabled: bool,
    pub motion_audio: MotionAudioDiagnostics,
    pub active_model: Option<ModelSnapshot>,
    pub pending_model: Option<PendingModelSnapshot>,
    pub active_motion: Option<ActiveMotionSnapshot>,
    pub active_expression: Option<ActiveExpressionSnapshot>,
    pub motion_events: MotionEventDiagnostics,
    pub input: InputSnapshot,
    pub cursor: CursorSnapshot,
    pub gamepad_axis_transport: GamepadAxisTransportDiagnostics,
    pub platform_input: PlatformInputDiagnostics,
    pub command_transport: RuntimeCommandTransportDiagnostics,
    pub model_input: ModelInputSnapshot,
    pub render_error: Option<RuntimeRenderErrorCode>,
    pub last_command_failure: Option<RuntimeCommandFailure>,
    pub last_command_sequence: Option<u64>,
}

impl RuntimeSnapshot {
    fn starting(
        overlay_visible: bool,
        motion_audio_enabled: bool,
        motion_audio: MotionAudioDiagnostics,
    ) -> Self {
        Self {
            revision: 0,
            state: RuntimeState::Starting,
            overlay_visible,
            overlay_settings: OverlaySettings::default(),
            model_settings: ModelSettings::default(),
            gamepad_axis_settings: GamepadAxisSettings::default(),
            motion_audio_enabled,
            motion_audio,
            active_model: None,
            pending_model: None,
            active_motion: None,
            active_expression: None,
            motion_events: MotionEventDiagnostics::default(),
            input: InputSnapshot::default(),
            cursor: CursorSnapshot::default(),
            gamepad_axis_transport: GamepadAxisTransportDiagnostics::default(),
            platform_input: PlatformInputDiagnostics::default(),
            command_transport: RuntimeCommandTransportDiagnostics::default(),
            model_input: ModelInputSnapshot::default(),
            render_error: None,
            last_command_failure: None,
            last_command_sequence: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CommandEnvelope {
    sequence: u64,
    command: WorkerCommand,
}

#[derive(Clone, Debug, PartialEq)]
enum WorkerCommand {
    Product(RuntimeCommand),
    Shutdown,
}

#[derive(Debug, PartialEq)]
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
    command_transport: Arc<CommandTransportCounters>,
    accepting: Arc<AtomicBool>,
}

#[derive(Default)]
struct CommandTransportCounters {
    enqueued: AtomicU64,
    queue_full: AtomicU64,
    runtime_stopped: AtomicU64,
    sequence_gap_count: AtomicU64,
    missing_sequence_count: AtomicU64,
    duplicate_sequence_count: AtomicU64,
    out_of_order_sequence_count: AtomicU64,
}

impl CommandTransportCounters {
    fn enqueued(&self) {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
    }

    fn queue_full(&self) {
        self.queue_full.fetch_add(1, Ordering::Relaxed);
    }

    fn runtime_stopped(&self) {
        self.runtime_stopped.fetch_add(1, Ordering::Relaxed);
    }

    fn sequence_gap(&self, missing: u64) {
        self.sequence_gap_count.fetch_add(1, Ordering::Relaxed);
        self.missing_sequence_count
            .fetch_add(missing, Ordering::Relaxed);
    }

    fn duplicate_sequence(&self) {
        self.duplicate_sequence_count
            .fetch_add(1, Ordering::Relaxed);
    }

    fn out_of_order_sequence(&self) {
        self.out_of_order_sequence_count
            .fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> RuntimeCommandTransportDiagnostics {
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
enum CommandSequenceDisposition {
    First,
    InOrder,
    Deferred,
    Gap { missing: u64 },
    Duplicate,
    OutOfOrder,
}

#[derive(Default)]
struct CommandSequenceTracker {
    last: Option<u64>,
    deferred: BTreeSet<u64>,
}

/// Returns whether an observed sequence has reached a target in the forward
/// direction, including across the `u64::MAX -> 0` boundary.
///
/// Sequence producers are monotonic modulo `u64`; treating the forward half
/// of the number line as newer keeps waits correct after a wrap while still
/// rejecting stale observations from the backward half.
fn sequence_reached(observed: u64, target: u64) -> bool {
    observed.wrapping_sub(target) <= u64::MAX / 2
}

impl CommandSequenceTracker {
    fn defer(&mut self, sequence: u64) {
        self.deferred.insert(sequence);
    }

    fn observe(&mut self, sequence: u64) -> CommandSequenceDisposition {
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
    gamepad_axis_slot: Arc<GamepadAxisSlot>,
    platform_input_diagnostics: PlatformInputDiagnosticsProducer,
    motion_audio: MotionAudioClient,
}

impl RuntimeClient {
    pub fn send(&self, command: RuntimeCommand) -> Result<u64, SendError> {
        let mut next_sequence = self
            .producer
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.producer.accepting.load(Ordering::Acquire) {
            self.producer.command_transport.runtime_stopped();
            return Err(SendError::RuntimeStopped(command));
        }
        let envelope = CommandEnvelope {
            sequence: *next_sequence,
            command: WorkerCommand::Product(command),
        };
        match self.producer.sender.try_send(envelope) {
            Ok(()) => {
                let accepted = *next_sequence;
                *next_sequence = next_sequence.wrapping_add(1);
                self.producer.command_transport.enqueued();
                Ok(accepted)
            }
            Err(TrySendError::Full(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => {
                    self.producer.command_transport.queue_full();
                    Err(SendError::QueueFull(command))
                }
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
            Err(TrySendError::Disconnected(envelope)) => match envelope.command {
                WorkerCommand::Product(command) => {
                    self.producer.command_transport.runtime_stopped();
                    Err(SendError::RuntimeStopped(command))
                }
                WorkerCommand::Shutdown => unreachable!("clients cannot send shutdown"),
            },
        }
    }

    pub fn trigger_shortcut(&self, action: ShortcutAction) -> Result<u64, SendError> {
        self.send(action.into_runtime_command())
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

    pub fn gamepad_axis_producer(&self) -> GamepadAxisProducer {
        GamepadAxisProducer::new(Arc::clone(&self.gamepad_axis_slot))
    }

    pub fn platform_input_diagnostics_producer(&self) -> PlatformInputDiagnosticsProducer {
        self.platform_input_diagnostics.clone()
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
                .is_some_and(|sequence| sequence_reached(sequence, command_sequence))
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
                    .is_some_and(|sequence| sequence_reached(sequence, command_sequence))
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
                .is_some_and(|sequence| sequence_reached(sequence, command_sequence));
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
                    .is_some_and(|sequence| sequence_reached(sequence, command_sequence));
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
                .is_some_and(|sequence| sequence_reached(sequence, input_sequence))
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
                    .is_some_and(|sequence| sequence_reached(sequence, input_sequence))
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
        snapshot.command_transport = self.producer.command_transport.snapshot();
        snapshot.cursor.transport = self.cursor_slot.diagnostics();
        snapshot.gamepad_axis_transport = self.gamepad_axis_slot.diagnostics();
        snapshot.platform_input = self.platform_input_diagnostics.diagnostics();
        snapshot.motion_audio = self.motion_audio.diagnostics();
        snapshot
    }
}

pub struct RuntimeOwner {
    client: RuntimeClient,
    worker: Option<JoinHandle<()>>,
    shutdown: Arc<ShutdownSignal>,
}

#[derive(Default)]
struct ShutdownSignal {
    sequence: Mutex<Option<u64>>,
}

impl ShutdownSignal {
    fn request(&self, sequence: u64) {
        *self
            .sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sequence);
    }

    fn sequence(&self) -> Option<u64> {
        *self
            .sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RuntimeOwner {
    pub fn start(initial_overlay_visible: bool, command_capacity: usize) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            false,
            command_capacity,
            None,
            MotionAudioClient::unavailable(),
            Arc::new(SystemMonotonicClock::start()),
        )
    }

    pub fn start_with_audio(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
    ) -> Self {
        Self::start_internal(
            initial_overlay_visible,
            initial_motion_audio_enabled,
            command_capacity,
            None,
            motion_audio,
            Arc::new(SystemMonotonicClock::start()),
        )
    }

    pub fn start_with_rendering(
        initial_overlay_visible: bool,
        command_capacity: usize,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                false,
                command_capacity,
                Some(renderer),
                MotionAudioClient::unavailable(),
                Arc::new(SystemMonotonicClock::start()),
            ),
            consumer,
        )
    }

    pub fn start_with_rendering_and_audio(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                command_capacity,
                Some(renderer),
                motion_audio,
                Arc::new(SystemMonotonicClock::start()),
            ),
            consumer,
        )
    }

    pub fn start_with_rendering_and_clock(
        initial_overlay_visible: bool,
        command_capacity: usize,
        clock: Arc<dyn MonotonicClock>,
    ) -> (Self, RenderConsumer) {
        Self::start_with_rendering_audio_and_clock(
            initial_overlay_visible,
            false,
            command_capacity,
            MotionAudioClient::unavailable(),
            clock,
        )
    }

    pub fn start_with_rendering_audio_and_clock(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        motion_audio: MotionAudioClient,
        clock: Arc<dyn MonotonicClock>,
    ) -> (Self, RenderConsumer) {
        let (renderer, consumer) = RuntimeRenderer::channel();
        (
            Self::start_internal(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                command_capacity,
                Some(renderer),
                motion_audio,
                clock,
            ),
            consumer,
        )
    }

    fn start_internal(
        initial_overlay_visible: bool,
        initial_motion_audio_enabled: bool,
        command_capacity: usize,
        renderer: Option<RuntimeRenderBootstrap>,
        motion_audio: MotionAudioClient,
        clock: Arc<dyn MonotonicClock>,
    ) -> Self {
        assert!(
            command_capacity > 0,
            "runtime command capacity must be non-zero"
        );
        let (sender, receiver) = mpsc::sync_channel(command_capacity);
        let snapshot = Arc::new(SnapshotCell {
            value: Mutex::new(RuntimeSnapshot::starting(
                initial_overlay_visible,
                initial_motion_audio_enabled,
                motion_audio.diagnostics(),
            )),
            changed: Condvar::new(),
        });
        let input_transport = Arc::new(InputTransportCounters::default());
        let cursor_slot = Arc::new(CursorSlot::default());
        let gamepad_axis_slot = Arc::new(GamepadAxisSlot::with_capacity(
            DEFAULT_GAMEPAD_AXIS_CAPACITY,
        ));
        let platform_input_diagnostics = PlatformInputDiagnosticsProducer::default();
        let command_transport = Arc::new(CommandTransportCounters::default());
        let accepting = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(ShutdownSignal::default());
        let worker_shutdown = Arc::clone(&shutdown);
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
                command_transport: Arc::clone(&command_transport),
                accepting: Arc::clone(&accepting),
            }),
            snapshot: Arc::clone(&snapshot),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
            cursor_slot: Arc::clone(&cursor_slot),
            gamepad_axis_slot: Arc::clone(&gamepad_axis_slot),
            platform_input_diagnostics,
            motion_audio: motion_audio.clone(),
        };
        let worker = thread::Builder::new()
            .name("bongocat-runtime".into())
            .spawn(move || {
                run_worker(
                    receiver,
                    RuntimeWorkerBootstrap {
                        snapshot,
                        cursor_slot,
                        gamepad_axis_slot,
                        initial_overlay_visible,
                        initial_motion_audio_enabled,
                        renderer,
                        motion_audio,
                        clock,
                        command_transport,
                        shutdown: worker_shutdown,
                    },
                )
            })
            .expect("failed to start runtime thread");
        Self {
            client,
            worker: Some(worker),
            shutdown,
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

    pub fn gamepad_axis_producer(&self) -> GamepadAxisProducer {
        self.client.gamepad_axis_producer()
    }

    pub fn platform_input_diagnostics_producer(&self) -> PlatformInputDiagnosticsProducer {
        self.client.platform_input_diagnostics_producer()
    }

    pub fn shutdown(mut self, timeout: Duration) -> Result<RuntimeSnapshot, ShutdownError> {
        let current = self.client.snapshot();
        if current.state == RuntimeState::Stopped {
            self.join_worker()?;
            return Ok(current);
        }
        self.request_shutdown();
        let Some(stopped) = self.client.wait_for_state(RuntimeState::Stopped, timeout) else {
            // An explicit timeout is a bounded API contract. Dropping the join handle
            // lets the worker finish its already-admitted drain asynchronously instead
            // of making `Drop` block without a deadline after returning the error.
            self.worker.take();
            return Err(ShutdownError::TimedOut);
        };
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
        self.client.gamepad_axis_slot.stop();
        self.client.platform_input_diagnostics.stop();
        let mut next_sequence = self
            .client
            .producer
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.client.producer.accepting.swap(false, Ordering::AcqRel) {
            let sequence = *next_sequence;
            *next_sequence = next_sequence.wrapping_add(1);
            self.shutdown.request(sequence);
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

#[derive(Default)]
struct GamepadAxisValues {
    values: BTreeMap<GamepadAxisKey, GamepadAxisSample>,
}

impl GamepadAxisValues {
    fn consume(&mut self, slot: &GamepadAxisSlot) -> bool {
        let samples = slot.take();
        if samples.is_empty() {
            return false;
        }
        for sample in samples {
            self.values.retain(|key, _| {
                key.connection.device_id != sample.key.connection.device_id
                    || key.connection.generation == sample.key.connection.generation
            });
            self.values.insert(sample.key, sample);
        }
        true
    }

    fn activate_connection(
        &mut self,
        connection: GamepadConnection,
        connected_at: MonotonicMillis,
    ) {
        self.values
            .retain(|key, sample| key.connection != connection || sample.at >= connected_at);
    }

    fn project(&self, input_state: &InputState, settings: GamepadAxisSettings) -> [f32; 6] {
        let Some(connection) = self
            .values
            .keys()
            .filter(|key| input_state.is_gamepad_connected(key.connection))
            .map(|key| key.connection)
            .min()
        else {
            return [0.0; 6];
        };
        let mut values = [0.0; 6];
        for (key, value) in self.values.iter().filter(|(key, _)| {
            key.connection == connection && input_state.is_gamepad_connected(key.connection)
        }) {
            values[key.axis as usize] = settings.apply(key.axis, value.value);
        }
        values
    }

    fn clear(&mut self) {
        self.values.clear();
    }

    fn clear_connection(&mut self, connection: GamepadConnection) {
        self.values.retain(|key, _| key.connection != connection);
    }
}

struct RuntimeWorkerBootstrap {
    snapshot: Arc<SnapshotCell>,
    cursor_slot: Arc<CursorSlot>,
    gamepad_axis_slot: Arc<GamepadAxisSlot>,
    initial_overlay_visible: bool,
    initial_motion_audio_enabled: bool,
    renderer: Option<RuntimeRenderBootstrap>,
    motion_audio: MotionAudioClient,
    clock: Arc<dyn MonotonicClock>,
    command_transport: Arc<CommandTransportCounters>,
    shutdown: Arc<ShutdownSignal>,
}

fn run_worker(receiver: Receiver<CommandEnvelope>, bootstrap: RuntimeWorkerBootstrap) {
    let RuntimeWorkerBootstrap {
        snapshot,
        cursor_slot,
        gamepad_axis_slot,
        initial_overlay_visible,
        initial_motion_audio_enabled,
        renderer,
        motion_audio,
        clock,
        command_transport,
        shutdown,
    } = bootstrap;
    let mut renderer = renderer.map(RuntimeRenderer::start);
    let mut active_model = None;
    let mut active_motion = None;
    let mut active_expression = None;
    let mut input_state = InputState::default();
    let mut input_bindings = InputBindings::default();
    let mut cursor_smoother = CursorSmoother::default();
    let mut normalized_cursor = NormalizedCursorPosition::default();
    let mut gamepad_axis_values = GamepadAxisValues::default();
    let mut gamepad_axis_settings = GamepadAxisSettings::default();
    let mut overlay_visible = initial_overlay_visible;
    let mut motion_audio_enabled = initial_motion_audio_enabled;
    let mut pending_model = None;
    let mut deferred_commands = VecDeque::new();
    let mut next_motion_event_sequence = 0u64;
    let mut command_sequences = CommandSequenceTracker::default();
    publish(&snapshot, |current| current.state = RuntimeState::Ready);
    loop {
        process_model_commit_feedback(
            renderer.as_mut(),
            &mut pending_model,
            &input_state,
            &mut input_bindings,
            normalized_cursor,
            &mut active_model,
            &mut active_motion,
            &mut active_expression,
            &motion_audio,
            &mut next_motion_event_sequence,
            overlay_visible,
            &snapshot,
            clock.now(),
        );
        let received = if let Some(sequence) = shutdown.sequence() {
            if pending_model.is_some() {
                // A model commit may be waiting for an overlay acknowledgement. Do not
                // requeue deferred work forever during shutdown; the active model remains
                // valid and the shutdown command must be allowed to release its resources.
                deferred_commands.clear();
                Ok(CommandEnvelope {
                    sequence,
                    command: WorkerCommand::Shutdown,
                })
            } else if let Some(envelope) = deferred_commands.pop_front() {
                Ok(envelope)
            } else {
                match receiver.try_recv() {
                    Ok(envelope) => Ok(envelope),
                    Err(TryRecvError::Empty) => Ok(CommandEnvelope {
                        sequence,
                        command: WorkerCommand::Shutdown,
                    }),
                    Err(TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
                }
            }
        } else if pending_model.is_none() {
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
                    &mut cursor_smoother,
                    &mut normalized_cursor,
                    clock.now(),
                );
                consume_gamepad_axes(
                    &gamepad_axis_slot,
                    &snapshot,
                    &mut gamepad_axis_values,
                    &input_state,
                    &input_bindings,
                    normalized_cursor,
                    gamepad_axis_settings,
                );
                if pending_model.is_some()
                    && !matches!(envelope.command, WorkerCommand::Shutdown)
                    && !matches!(
                        envelope.command,
                        WorkerCommand::Product(RuntimeCommand::ApplyInput(_))
                    )
                {
                    let sequence = envelope.sequence;
                    deferred_commands.push_back(envelope);
                    command_sequences.defer(sequence);
                    continue;
                }
                match command_sequences.observe(envelope.sequence) {
                    CommandSequenceDisposition::Gap { missing } => {
                        command_transport.sequence_gap(missing);
                    }
                    CommandSequenceDisposition::Duplicate => {
                        command_transport.duplicate_sequence();
                        continue;
                    }
                    CommandSequenceDisposition::OutOfOrder => {
                        command_transport.out_of_order_sequence();
                        continue;
                    }
                    CommandSequenceDisposition::First
                    | CommandSequenceDisposition::InOrder
                    | CommandSequenceDisposition::Deferred => {}
                }
                let sequence = envelope.sequence;
                let mut evaluate_after_command = true;
                match envelope.command {
                    WorkerCommand::Product(RuntimeCommand::Tick) => {
                        publish(&snapshot, |current| {
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetOverlayVisible(visible)) => {
                        overlay_visible = visible;
                        publish(&snapshot, |current| {
                            current.overlay_visible = visible;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetOverlaySettings(settings)) => {
                        if !settings.is_valid() {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        } else {
                            publish(&snapshot, |current| {
                                current.overlay_settings = settings;
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::SetModelSettings(settings)) => {
                        if let Some(renderer) = &mut renderer {
                            renderer.set_model_settings(settings);
                        }
                        publish(&snapshot, |current| {
                            current.model_settings = settings;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetMotionAudioEnabled(enabled)) => {
                        motion_audio_enabled = enabled;
                        if !enabled {
                            stop_motion_audio(
                                &motion_audio,
                                sequence,
                                MotionAudioStopReason::Disabled,
                            );
                        }
                        publish(&snapshot, |current| {
                            current.motion_audio_enabled = enabled;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetInputBindings(bindings)) => {
                        input_bindings = Arc::unwrap_or_clone(bindings);
                        publish(&snapshot, |current| {
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetGamepadAxisSettings(settings)) => {
                        gamepad_axis_settings = settings;
                        publish(&snapshot, |current| {
                            current.gamepad_axis_settings = settings;
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::ResetInput(reason)) => {
                        input_state.force_reset(reason);
                        gamepad_axis_values.clear();
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        })
                    }
                    WorkerCommand::Product(RuntimeCommand::ApplyInput(envelope)) => {
                        let envelope = Arc::unwrap_or_clone(envelope);
                        let input_reset = matches!(envelope.event, InputEvent::Reset { .. });
                        let connected = match &envelope.event {
                            InputEvent::GamepadConnected { connection, at } => {
                                Some((*connection, *at))
                            }
                            _ => None,
                        };
                        let disconnected = match &envelope.event {
                            InputEvent::GamepadDisconnected { connection, .. } => Some(*connection),
                            _ => None,
                        };
                        let disposition = input_state.apply(envelope);
                        if input_reset {
                            gamepad_axis_values.clear();
                        } else if let Some(connection) = disconnected {
                            gamepad_axis_values.clear_connection(connection);
                        }
                        if let Some((connection, at)) = connected
                            && matches!(
                                disposition,
                                InputDisposition::Applied
                                    | InputDisposition::AppliedAfterSequenceGap { .. }
                            )
                        {
                            gamepad_axis_values.activate_connection(connection, at);
                        }
                        let activation_pending = pending_model.is_some();
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                            );
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
                            &mut active_motion,
                            &mut active_expression,
                            &mut pending_model,
                            &motion_audio,
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
                            &mut active_motion,
                            &mut active_expression,
                            &mut pending_model,
                            &motion_audio,
                            &snapshot,
                        );
                    }
                    WorkerCommand::Product(RuntimeCommand::StartMotion { motion, priority }) => {
                        let can_replace =
                            active_motion
                                .as_ref()
                                .is_none_or(|active: &ActiveMotionSnapshot| {
                                    priority >= active.priority
                                });
                        if !can_replace {
                            publish(&snapshot, |current| {
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        } else if let Some(renderer) = &mut renderer {
                            match renderer.start_motion(&motion, clock.now()) {
                                Ok(()) => {
                                    if motion_audio_enabled {
                                        if let Some(path) =
                                            motion_audio_path(active_model.as_deref(), &motion)
                                        {
                                            let _ = motion_audio.try_publish(
                                                MotionAudioCommand::Play {
                                                    sequence,
                                                    path,
                                                    volume: MotionAudioVolume::FULL,
                                                },
                                            );
                                        } else {
                                            stop_motion_audio(
                                                &motion_audio,
                                                sequence,
                                                MotionAudioStopReason::MotionReplaced,
                                            );
                                        }
                                    }
                                    let started = ActiveMotionSnapshot {
                                        motion,
                                        priority,
                                        command_sequence: sequence,
                                        stop_command_sequence: None,
                                    };
                                    active_motion = Some(started.clone());
                                    publish(&snapshot, |current| {
                                        current.active_motion = Some(started);
                                        current.last_command_failure = None;
                                        current.last_command_sequence = Some(sequence);
                                    });
                                }
                                Err(code) => publish(&snapshot, |current| {
                                    current.last_command_failure =
                                        Some(RuntimeCommandFailure { sequence, code });
                                    current.last_command_sequence = Some(sequence);
                                }),
                            }
                        } else {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::MotionLoadFailed,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::StopMotion(motion)) => {
                        let matching = active_motion
                            .as_ref()
                            .is_some_and(|active| active.motion == motion);
                        let already_stopping = active_motion
                            .as_ref()
                            .is_some_and(|active| active.stop_command_sequence.is_some());
                        if matching && !already_stopping {
                            stop_motion_audio(
                                &motion_audio,
                                sequence,
                                MotionAudioStopReason::MotionStopped,
                            );
                            let stop_status = renderer
                                .as_mut()
                                .map_or(MotionStopStatus::Finished, |renderer| {
                                    renderer.stop_motion(clock.now())
                                });
                            if stop_status == MotionStopStatus::Fading {
                                let stopping =
                                    active_motion.as_mut().expect("matching motion is active");
                                stopping.stop_command_sequence = Some(sequence);
                                let stopping = stopping.clone();
                                publish(&snapshot, |current| {
                                    current.active_motion = Some(stopping);
                                    current.last_command_failure = None;
                                    current.last_command_sequence = Some(sequence);
                                });
                            } else {
                                active_motion = None;
                                publish(&snapshot, |current| {
                                    current.active_motion = None;
                                    current.last_command_failure = None;
                                    current.last_command_sequence = Some(sequence);
                                });
                            }
                        } else {
                            publish(&snapshot, |current| {
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::SetExpression(expression)) => {
                        if let Some(renderer) = &mut renderer {
                            match renderer.set_expression(&expression, clock.now()) {
                                Ok(()) => {
                                    let active = ActiveExpressionSnapshot {
                                        expression,
                                        command_sequence: sequence,
                                    };
                                    active_expression = Some(active.clone());
                                    publish(&snapshot, |current| {
                                        current.active_expression = Some(active);
                                        current.last_command_failure = None;
                                        current.last_command_sequence = Some(sequence);
                                    });
                                }
                                Err(code) => publish(&snapshot, |current| {
                                    current.last_command_failure =
                                        Some(RuntimeCommandFailure { sequence, code });
                                    current.last_command_sequence = Some(sequence);
                                }),
                            }
                        } else {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::ExpressionLoadFailed,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Shutdown => {
                        stop_motion_audio(&motion_audio, sequence, MotionAudioStopReason::Shutdown);
                        publish(&snapshot, |current| {
                            current.state = RuntimeState::Stopping;
                            current.pending_model = None;
                            current.active_motion = None;
                            current.active_expression = None;
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
                        clock.now(),
                        &mut active_motion,
                        &mut next_motion_event_sequence,
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                consume_cursor(
                    &cursor_slot,
                    &snapshot,
                    &input_state,
                    &input_bindings,
                    &mut cursor_smoother,
                    &mut normalized_cursor,
                    clock.now(),
                );
                consume_gamepad_axes(
                    &gamepad_axis_slot,
                    &snapshot,
                    &mut gamepad_axis_values,
                    &input_state,
                    &input_bindings,
                    normalized_cursor,
                    gamepad_axis_settings,
                );
                process_model_commit_feedback(
                    renderer.as_mut(),
                    &mut pending_model,
                    &input_state,
                    &mut input_bindings,
                    normalized_cursor,
                    &mut active_model,
                    &mut active_motion,
                    &mut active_expression,
                    &motion_audio,
                    &mut next_motion_event_sequence,
                    overlay_visible,
                    &snapshot,
                    clock.now(),
                );
                if overlay_visible && pending_model.is_none() {
                    evaluate_renderer(
                        renderer.as_mut(),
                        input_state.model_snapshot(&input_bindings, normalized_cursor),
                        &snapshot,
                        clock.now(),
                        &mut active_motion,
                        &mut next_motion_event_sequence,
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
        &mut cursor_smoother,
        &mut normalized_cursor,
        clock.now(),
    );
    gamepad_axis_values.clear();
    gamepad_axis_slot.stop();
    if let Some(renderer) = &renderer {
        renderer.close();
    }
    stop_motion_audio(&motion_audio, u64::MAX, MotionAudioStopReason::Shutdown);
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
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    pending_model: &mut Option<PendingModelActivation>,
    motion_audio: &MotionAudioClient,
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
        *active_motion = None;
        *active_expression = None;
        stop_motion_audio(motion_audio, sequence, MotionAudioStopReason::ModelSwitched);
        publish(snapshot, |current| {
            current.state = RuntimeState::Ready;
            current.active_model = Some(model_snapshot);
            current.active_motion = None;
            current.active_expression = None;
            current.motion_events.last_event = None;
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
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    motion_audio: &MotionAudioClient,
    next_motion_event_sequence: &mut u64,
    overlay_visible: bool,
    snapshot: &SnapshotCell,
    now: Duration,
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
            *active_motion = None;
            *active_expression = None;
            stop_motion_audio(
                motion_audio,
                feedback.token.command_sequence,
                MotionAudioStopReason::ModelSwitched,
            );
            publish(snapshot, |current| {
                current.state = RuntimeState::Ready;
                current.active_model = Some(model_snapshot);
                current.pending_model = None;
                current.active_motion = None;
                current.active_expression = None;
                current.motion_events.last_event = None;
                current.model_input = model_input;
                current.render_error = None;
                current.last_command_failure = None;
                current.last_command_sequence = Some(feedback.token.command_sequence);
            });
            if overlay_visible {
                evaluate_renderer(
                    Some(renderer),
                    model_input,
                    snapshot,
                    now,
                    active_motion,
                    next_motion_event_sequence,
                );
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
                evaluate_renderer(
                    Some(renderer),
                    model_input,
                    snapshot,
                    now,
                    active_motion,
                    next_motion_event_sequence,
                );
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
    now: Duration,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    next_motion_event_sequence: &mut u64,
) {
    let Some(renderer) = renderer else {
        return;
    };
    let event_motion = active_motion.as_ref().map(|active| active.motion.clone());
    match renderer.evaluate(input, now) {
        Ok(RenderEvaluation {
            rendered: false, ..
        }) => {}
        Ok(evaluation) => {
            if !evaluation.motion_user_data.is_empty() || evaluation.skipped_motion_user_data > 0 {
                publish(snapshot, |current| {
                    current.motion_events.skipped = current
                        .motion_events
                        .skipped
                        .saturating_add(evaluation.skipped_motion_user_data);
                    if let Some(motion) = event_motion {
                        for occurrence in evaluation.motion_user_data {
                            let observed = MotionUserDataSnapshot {
                                event_sequence: *next_motion_event_sequence,
                                motion: motion.clone(),
                                cycle: occurrence.cycle,
                                local_time: occurrence.local_time,
                                value: occurrence.value,
                            };
                            *next_motion_event_sequence =
                                next_motion_event_sequence.wrapping_add(1);
                            current.motion_events.emitted =
                                current.motion_events.emitted.saturating_add(1);
                            current.motion_events.last_event = Some(observed);
                        }
                    } else {
                        current.motion_events.skipped = current
                            .motion_events
                            .skipped
                            .saturating_add(evaluation.motion_user_data.len() as u64);
                    }
                });
            }
            if evaluation.motion_finished {
                *active_motion = None;
                publish(snapshot, |current| current.active_motion = None);
            }
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

fn motion_audio_path(
    model: Option<&CommittedModel>,
    motion: &MotionId,
) -> Option<std::path::PathBuf> {
    let model = model?;
    let sound = model
        .index()
        .motion_groups
        .iter()
        .find(|group| group.name == motion.group())?
        .motions
        .get(motion.index())?
        .sound
        .as_deref()?;
    Some(model.root().join(sound))
}

fn stop_motion_audio(client: &MotionAudioClient, sequence: u64, reason: MotionAudioStopReason) {
    let _ = client.try_publish(MotionAudioCommand::Stop { sequence, reason });
}

fn consume_cursor(
    cursor_slot: &CursorSlot,
    snapshot: &SnapshotCell,
    input_state: &InputState,
    input_bindings: &InputBindings,
    smoother: &mut CursorSmoother,
    normalized_cursor: &mut NormalizedCursorPosition,
    now: Duration,
) {
    let sample = cursor_slot.take();
    if let Some(sample) = sample {
        smoother.set_target(sample, now);
    }
    let advanced = smoother.advance(now);
    if sample.is_none() && !advanced {
        return;
    }
    *normalized_cursor = smoother.normalized();
    publish(snapshot, |current| {
        if let Some(sample) = sample {
            current.cursor.sample = Some(sample);
        }
        current.model_input = input_state.model_snapshot(input_bindings, *normalized_cursor);
    });
}

fn consume_gamepad_axes(
    slot: &GamepadAxisSlot,
    snapshot: &SnapshotCell,
    values: &mut GamepadAxisValues,
    input_state: &InputState,
    input_bindings: &InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    settings: GamepadAxisSettings,
) {
    if !values.consume(slot) {
        return;
    }
    publish(snapshot, |current| {
        current.model_input = compose_model_input(
            input_state,
            input_bindings,
            normalized_cursor,
            values,
            settings,
        );
    });
}

fn compose_model_input(
    input_state: &InputState,
    input_bindings: &InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
) -> ModelInputSnapshot {
    let mut input = input_state.model_snapshot(input_bindings, normalized_cursor);
    let axes = gamepad_axis_values.project(input_state, gamepad_axis_settings);
    input.stick_left_x = axes[GamepadAxis::LeftStickX as usize];
    input.stick_left_y = axes[GamepadAxis::LeftStickY as usize];
    input.stick_right_x = axes[GamepadAxis::RightStickX as usize];
    input.stick_right_y = axes[GamepadAxis::RightStickY as usize];
    input.left_trigger = axes[GamepadAxis::LeftTrigger as usize];
    input.right_trigger = axes[GamepadAxis::RightTrigger as usize];
    input
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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use std::fs;
    use std::path::{Path, PathBuf};
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use tempfile::TempDir;
    use tempfile::tempdir;

    const TIMEOUT: Duration = Duration::from_secs(2);

    #[test]
    fn render_error_codes_are_stable_and_unique() {
        let mut codes = RuntimeRenderErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| !code.is_empty()));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), RuntimeRenderErrorCode::ALL.len());
        assert_eq!(
            RuntimeRenderErrorCode::GpuPreparationFailed.to_string(),
            "gpu_preparation_failed"
        );
    }

    #[test]
    fn sequence_wait_predicate_handles_wraparound() {
        assert!(sequence_reached(42, 42));
        assert!(sequence_reached(0, u64::MAX));
        assert!(sequence_reached(1, u64::MAX));
        assert!(!sequence_reached(u64::MAX, 0));
        assert!(!sequence_reached(10, 11));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[derive(Default)]
    struct ManualClock {
        now: Mutex<Duration>,
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    impl ManualClock {
        fn set(&self, now: Duration) {
            *self
                .now
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    impl MonotonicClock for ManualClock {
        fn now(&self) -> Duration {
            *self
                .now
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

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
    fn preset_model_with_motion_fade_out(fade_out_seconds: f64) -> (TempDir, CommittedModel) {
        let catalog = tempdir().expect("temporary preset catalog");
        let source = repository_root().join("native/resources/models/standard");
        let destination = catalog.path().join("fade-out-model");
        clone_model_tree(&source, &destination);

        let model3_path = destination.join("cat.model3.json");
        let mut model3: serde_json::Value =
            serde_json::from_slice(&fs::read(&model3_path).expect("read copied model3"))
                .expect("parse copied model3");
        let motions = model3
            .get_mut("FileReferences")
            .and_then(|references| references.get_mut("Motions"))
            .and_then(serde_json::Value::as_object_mut)
            .expect("model3 motion groups");
        for group in motions.values_mut() {
            for motion in group.as_array_mut().expect("motion array") {
                motion.as_object_mut().expect("motion object").insert(
                    "FadeOutTime".to_owned(),
                    serde_json::Value::from(fade_out_seconds),
                );
            }
        }
        fs::write(
            &model3_path,
            serde_json::to_vec_pretty(&model3).expect("serialize copied model3"),
        )
        .expect("write copied model3");

        let model = PresetModelCatalog::open(catalog.path(), ModelPackageLimits::default())
            .expect("temporary preset catalog")
            .load(&ModelId::parse("fade-out-model").expect("model id"))
            .expect("temporary preset model");
        (catalog, model)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn clone_model_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create copied model directory");
        for entry in fs::read_dir(source).expect("read source model directory") {
            let entry = entry.expect("source model entry");
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if entry.file_type().expect("source entry type").is_dir() {
                clone_model_tree(&source_path, &destination_path);
            } else if source_path.extension().and_then(|value| value.to_str()) == Some("json") {
                fs::copy(&source_path, &destination_path).expect("copy model JSON");
            } else if fs::hard_link(&source_path, &destination_path).is_err() {
                fs::copy(&source_path, &destination_path).expect("copy model resource");
            }
        }
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
        assert_eq!(changed.command_transport.enqueued, 1);
        assert_eq!(changed.command_transport.queue_full, 0);

        let settings = OverlaySettings {
            click_through: false,
            always_on_top: false,
            scale_percent: 125,
            opacity_percent: 80,
        };
        let sequence = client
            .send(RuntimeCommand::SetOverlaySettings(settings))
            .expect("overlay settings command accepted");
        let updated = client
            .wait_for_command(sequence, TIMEOUT)
            .expect("overlay settings snapshot");
        assert_eq!(updated.overlay_settings, settings);
        assert_eq!(updated.last_command_failure, None);

        let invalid = OverlaySettings {
            scale_percent: 0,
            ..settings
        };
        let sequence = client
            .send(RuntimeCommand::SetOverlaySettings(invalid))
            .expect("invalid settings command accepted for typed rejection");
        let rejected = client
            .wait_for_command(sequence, TIMEOUT)
            .expect("invalid settings rejection");
        assert_eq!(rejected.overlay_settings, settings);
        assert_eq!(
            rejected.last_command_failure,
            Some(RuntimeCommandFailure {
                sequence,
                code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
            })
        );

        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }

    #[test]
    fn shutdown_rejects_new_commands_and_drains_a_full_queue() {
        let owner = RuntimeOwner::start(true, 1);
        let client = owner.client();
        client
            .wait_for_state(RuntimeState::Ready, TIMEOUT)
            .expect("runtime ready");
        let mut queue_full = false;
        for _ in 0..100_000 {
            match client.send(RuntimeCommand::SetOverlayVisible(false)) {
                Ok(_) => {}
                Err(SendError::QueueFull(_)) => {
                    queue_full = true;
                    break;
                }
                Err(SendError::RuntimeStopped(_)) => panic!("runtime stopped unexpectedly"),
            }
        }
        assert!(queue_full, "test must observe a full command queue");

        owner.request_shutdown();
        assert!(matches!(
            client.send(RuntimeCommand::SetOverlayVisible(true)),
            Err(SendError::RuntimeStopped(_))
        ));
        let stopped = owner.shutdown(TIMEOUT).expect("shutdown drains queue");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }

    #[test]
    fn shutdown_timeout_returns_without_waiting_for_worker_join() {
        let owner = RuntimeOwner::start(true, 1);
        let client = owner.client();
        client
            .wait_for_state(RuntimeState::Ready, TIMEOUT)
            .expect("runtime ready");
        client
            .send(RuntimeCommand::SetOverlayVisible(false))
            .expect("queue accepts one command");

        let started = Instant::now();
        let result = owner.shutdown(Duration::ZERO);
        assert_eq!(result, Err(ShutdownError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "explicit shutdown timeout must bound the caller wait"
        );

        let stopped = client
            .wait_for_state(RuntimeState::Stopped, TIMEOUT)
            .expect("detached worker eventually drains and stops");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }

    #[test]
    fn model_settings_command_is_revisioned_and_published() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        let ready = client
            .wait_for_state(RuntimeState::Ready, TIMEOUT)
            .expect("ready snapshot");
        assert_eq!(ready.model_settings, ModelSettings::default());

        let settings = ModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_pointer: true,
        };
        let sequence = client
            .send(RuntimeCommand::SetModelSettings(settings))
            .expect("model settings command accepted");
        let updated = client
            .wait_for_command(sequence, TIMEOUT)
            .expect("model settings snapshot");
        assert_eq!(updated.model_settings, settings);
        assert_eq!(updated.last_command_failure, None);

        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
        assert_eq!(stopped.model_settings, settings);
    }

    #[test]
    fn platform_input_diagnostics_are_live_and_freeze_at_runtime_shutdown() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        let producer = owner.platform_input_diagnostics_producer();
        let live = PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::Running,
            service_start_attempts: 1,
            runtime_queue_overflows: 2,
            recovery_resets: 3,
            gamepad_connections: 4,
            gamepad_disconnections: 1,
            gamepad_axis_publish_rejections: 5,
            ..PlatformInputDiagnostics::default()
        };
        producer.publish(live).expect("live diagnostics accepted");
        assert_eq!(client.snapshot().platform_input, live);

        let final_diagnostics = PlatformInputDiagnostics {
            service_status: PlatformInputServiceStatus::Stopped,
            clean_shutdown: true,
            ..live
        };
        producer
            .publish(final_diagnostics)
            .expect("final diagnostics accepted");
        let stopped = owner.shutdown(TIMEOUT).expect("clean runtime stop");
        assert_eq!(stopped.platform_input, final_diagnostics);
        assert_eq!(
            producer.publish(PlatformInputDiagnostics::default()),
            Err(PlatformInputDiagnosticsPublishError::RuntimeStopped)
        );
        assert_eq!(client.snapshot().platform_input, final_diagnostics);
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
    fn runtime_coalesces_gamepad_axes_and_projects_dead_zone_without_blocking_edges() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        client
            .send(RuntimeCommand::SetGamepadAxisSettings(
                GamepadAxisSettings::new(0.2, 0.1).expect("settings"),
            ))
            .expect("axis settings accepted");
        let axis = owner.gamepad_axis_producer();
        let connection = axis.connect(0).expect("gamepad connection allocated");
        let input = owner.input_producer();
        input
            .publish(InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            })
            .expect("connection accepted");
        for index in 0..10_000 {
            axis.publish(GamepadAxisSample {
                key: GamepadAxisKey {
                    connection,
                    axis: GamepadAxis::LeftStickX,
                },
                value: index as f32 / 10_000.0,
                at: MonotonicMillis::new(index),
            })
            .expect("axis sample accepted");
        }
        let down = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey {
                    connection,
                    button: GamepadButton::South,
                }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(10_001),
            })
            .expect("button edge accepted");
        let snapshot = client
            .wait_for_input_sequence(down, TIMEOUT)
            .expect("button edge consumed");
        assert_eq!(snapshot.input.pressed_gamepad_button_count, 1);
        assert!((snapshot.model_input.stick_left_x - 0.999875).abs() < 0.0001);
        assert!(snapshot.gamepad_axis_transport.coalesced > 0);
        let reset_sequence = input
            .publish(InputEvent::Reset {
                reason: InputResetReason::DeviceRemoved,
                at: MonotonicMillis::new(10_002),
            })
            .expect("reset accepted");
        let reset = client
            .wait_for_input_sequence(reset_sequence, TIMEOUT)
            .expect("reset consumed");
        assert_eq!(reset.model_input.stick_left_x, 0.0);
        let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.model_input.stick_left_x, 0.0);
        assert_eq!(stopped.gamepad_axis_transport.pending, 0);
    }

    #[test]
    fn runtime_discards_axis_samples_until_the_connection_is_active() {
        let owner = RuntimeOwner::start(true, 8);
        let client = owner.client();
        client
            .wait_for_revision(1, TIMEOUT)
            .expect("ready snapshot");
        let axis = owner.gamepad_axis_producer();
        let connection = axis.connect(0).expect("gamepad connection allocated");
        let input = owner.input_producer();

        axis.publish(GamepadAxisSample {
            key: GamepadAxisKey {
                connection,
                axis: GamepadAxis::LeftStickX,
            },
            value: 0.9,
            at: MonotonicMillis::new(0),
        })
        .expect("axis sample accepted");
        let connected = input
            .publish(InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(1),
            })
            .expect("connection accepted");
        let before_axis = client
            .wait_for_input_sequence(connected, TIMEOUT)
            .expect("connection consumed");
        assert_eq!(before_axis.model_input.stick_left_x, 0.0);

        axis.publish(GamepadAxisSample {
            key: GamepadAxisKey {
                connection,
                axis: GamepadAxis::LeftStickX,
            },
            value: 0.9,
            at: MonotonicMillis::new(2),
        })
        .expect("active axis sample accepted");
        let edge = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey {
                    connection,
                    button: GamepadButton::South,
                }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(2),
            })
            .expect("button edge accepted");
        let active = client
            .wait_for_input_sequence(edge, TIMEOUT)
            .expect("active edge consumed");
        assert!(active.model_input.stick_left_x > 0.8);
        owner.shutdown(TIMEOUT).expect("clean shutdown");
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn runtime_smooths_cursor_with_the_injected_monotonic_clock() {
        let clock = Arc::new(ManualClock::default());
        let (owner, _render_consumer) =
            RuntimeOwner::start_with_rendering_and_clock(true, 4, clock.clone());
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let cursor = owner.cursor_producer();

        cursor
            .publish(cursor_sample(50.0, 50.0, 0))
            .expect("initial cursor accepted");
        client
            .wait_for_cursor_samples(1, TIMEOUT)
            .expect("initial cursor consumed");
        cursor
            .publish(cursor_sample(0.0, 50.0, 1))
            .expect("target cursor accepted");
        let targeted = client
            .wait_for_cursor_samples(2, TIMEOUT)
            .expect("target cursor consumed");
        assert_eq!(targeted.model_input.pointer_x, 0.0);

        let frame = Duration::from_secs_f64(1.0 / 60.0);
        clock.set(frame);
        let first_tick = client
            .send(RuntimeCommand::Tick)
            .expect("first tick accepted");
        let first = client
            .wait_for_command(first_tick, TIMEOUT)
            .expect("first tick applied");
        assert!((first.model_input.pointer_x - 0.25).abs() < 1e-6);

        clock.set(frame * 2);
        let second_tick = client
            .send(RuntimeCommand::Tick)
            .expect("second tick accepted");
        let second = client
            .wait_for_command(second_tick, TIMEOUT)
            .expect("second tick applied");
        assert!((second.model_input.pointer_x - 0.4375).abs() < 1e-6);

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
    fn motion_commands_use_priority_identity_and_injectable_time() {
        let clock = Arc::new(ManualClock::default());
        let (owner, consumer) = RuntimeOwner::start_with_rendering_audio_and_clock(
            true,
            true,
            8,
            MotionAudioClient::unavailable(),
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
                "standard",
            ))))
            .expect("activation command");
        let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
        let activated = report_model_prepared(&client, &consumer, &candidate);
        let audio_rejections_before_motion = activated.motion_audio.rejected_after_shutdown;
        let baseline = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == candidate.model_generation
                && frame.frame_number > candidate.frame_number
        });

        let first = MotionId::new("CAT_motion", 0).expect("motion id");
        let first_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: first.clone(),
                priority: MotionPriority::Normal,
            })
            .expect("start motion");
        let started = client
            .wait_for_command(first_sequence, TIMEOUT)
            .expect("motion started");
        assert_eq!(
            started.active_motion,
            Some(ActiveMotionSnapshot {
                motion: first.clone(),
                priority: MotionPriority::Normal,
                command_sequence: first_sequence,
                stop_command_sequence: None,
            })
        );
        assert_eq!(
            started.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 1
        );

        clock.set(Duration::from_millis(500));
        let tick_sequence = client
            .send(RuntimeCommand::Tick)
            .expect("deterministic tick");
        client
            .wait_for_command(tick_sequence, TIMEOUT)
            .expect("tick completed");
        let animated = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > baseline.transport_sequence
                && frame.snapshot != baseline.snapshot
        });
        assert_ne!(animated.snapshot, baseline.snapshot);

        let second = MotionId::new("CAT_motion", 1).expect("motion id");
        let ignored_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: second.clone(),
                priority: MotionPriority::Idle,
            })
            .expect("lower priority request");
        let ignored = client
            .wait_for_command(ignored_sequence, TIMEOUT)
            .expect("lower priority result");
        assert_eq!(ignored.active_motion, started.active_motion);
        assert_eq!(
            ignored.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 1
        );

        let force_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: second.clone(),
                priority: MotionPriority::Force,
            })
            .expect("force motion");
        let forced = client
            .wait_for_command(force_sequence, TIMEOUT)
            .expect("force motion result");
        assert_eq!(
            forced.active_motion,
            Some(ActiveMotionSnapshot {
                motion: second.clone(),
                priority: MotionPriority::Force,
                command_sequence: force_sequence,
                stop_command_sequence: None,
            })
        );
        assert_eq!(
            forced.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 2
        );

        let invalid_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: MotionId::new("missing", 0).expect("syntactically valid motion id"),
                priority: MotionPriority::Force,
            })
            .expect("invalid resource request");
        let invalid = client
            .wait_for_command(invalid_sequence, TIMEOUT)
            .expect("invalid resource result");
        assert_eq!(
            invalid.last_command_failure,
            Some(RuntimeCommandFailure {
                sequence: invalid_sequence,
                code: RuntimeRenderErrorCode::MotionLoadFailed,
            })
        );
        assert_eq!(invalid.active_motion, forced.active_motion);
        assert_eq!(
            invalid.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 2
        );

        let stale_stop_sequence = client
            .send(RuntimeCommand::StopMotion(first))
            .expect("stale stop");
        let stale_stop = client
            .wait_for_command(stale_stop_sequence, TIMEOUT)
            .expect("stale stop result");
        assert_eq!(stale_stop.active_motion, forced.active_motion);
        assert_eq!(
            stale_stop.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 2
        );

        let stop_sequence = client
            .send(RuntimeCommand::StopMotion(second))
            .expect("current stop");
        let stopped = client
            .wait_for_command(stop_sequence, TIMEOUT)
            .expect("current stop result");
        assert!(stopped.active_motion.is_none());
        assert_eq!(
            stopped.motion_audio.rejected_after_shutdown,
            audio_rejections_before_motion + 3
        );
        let duplicate_stop_sequence = client
            .send(RuntimeCommand::StopMotion(
                MotionId::new("CAT_motion", 1).expect("motion id"),
            ))
            .expect("duplicate stop");
        let duplicate_stop = client
            .wait_for_command(duplicate_stop_sequence, TIMEOUT)
            .expect("duplicate stop result");
        assert!(duplicate_stop.active_motion.is_none());
        owner.shutdown(TIMEOUT).expect("runtime shutdown");
        assert!(matches!(
            client.send(RuntimeCommand::SetOverlayVisible(true)),
            Err(SendError::RuntimeStopped(_))
        ));
        assert_eq!(client.snapshot().command_transport.runtime_stopped, 1);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn shortcut_action_dispatch_reuses_typed_motion_and_expression_commands() {
        let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
                "standard",
            ))))
            .expect("activation command");
        let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
        report_model_prepared(&client, &consumer, &candidate);

        let motion = MotionId::new("CAT_motion", 0).expect("motion id");
        let start_sequence = client
            .trigger_shortcut(ShortcutAction::StartMotion {
                motion: motion.clone(),
                priority: MotionPriority::Normal,
            })
            .expect("shortcut start");
        let started = client
            .wait_for_command(start_sequence, TIMEOUT)
            .expect("shortcut motion result");
        assert_eq!(
            started.active_motion.as_ref().map(|active| &active.motion),
            Some(&motion)
        );

        let expression = ExpressionId::new("live2d_expression0.exp3.json").expect("expression id");
        let expression_sequence = client
            .trigger_shortcut(ShortcutAction::SetExpression(expression.clone()))
            .expect("shortcut expression");
        let expressed = client
            .wait_for_command(expression_sequence, TIMEOUT)
            .expect("shortcut expression result");
        assert_eq!(
            expressed
                .active_expression
                .as_ref()
                .map(|active| &active.expression),
            Some(&expression)
        );

        let stop_sequence = client
            .trigger_shortcut(ShortcutAction::StopMotion(motion))
            .expect("shortcut stop");
        let stopped = client
            .wait_for_command(stop_sequence, TIMEOUT)
            .expect("shortcut stop result");
        assert!(stopped.active_motion.is_none());
        owner.shutdown(TIMEOUT).expect("runtime shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn stopped_motion_fades_without_a_jump_and_duplicate_stop_does_not_restart_it() {
        let (_catalog, model) = preset_model_with_motion_fade_out(1.0);
        let clock = Arc::new(ManualClock::default());
        let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
            true,
            8,
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(model)))
            .expect("activation command");
        let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
        report_model_prepared(&client, &consumer, &candidate);
        let baseline = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == candidate.model_generation
                && frame.frame_number > candidate.frame_number
        });

        let motion = MotionId::new("CAT_motion", 0).expect("motion id");
        let start_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: motion.clone(),
                priority: MotionPriority::Normal,
            })
            .expect("start motion");
        client
            .wait_for_command(start_sequence, TIMEOUT)
            .expect("motion started");
        clock.set(Duration::from_millis(500));
        let before_stop = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > baseline.transport_sequence
                && frame.snapshot != baseline.snapshot
        });

        let stop_sequence = client
            .send(RuntimeCommand::StopMotion(motion.clone()))
            .expect("stop motion");
        let stopping = client
            .wait_for_command(stop_sequence, TIMEOUT)
            .expect("motion stopping");
        assert_eq!(
            stopping.active_motion,
            Some(ActiveMotionSnapshot {
                motion: motion.clone(),
                priority: MotionPriority::Normal,
                command_sequence: start_sequence,
                stop_command_sequence: Some(stop_sequence),
            })
        );
        let first_fade_frame = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > before_stop.transport_sequence
        });
        assert_eq!(first_fade_frame.snapshot, before_stop.snapshot);

        clock.set(Duration::from_millis(700));
        let duplicate_sequence = client
            .send(RuntimeCommand::StopMotion(motion))
            .expect("duplicate stop");
        let duplicate = client
            .wait_for_command(duplicate_sequence, TIMEOUT)
            .expect("duplicate stop completed");
        assert_eq!(
            duplicate
                .active_motion
                .as_ref()
                .and_then(|active| active.stop_command_sequence),
            Some(stop_sequence)
        );

        clock.set(Duration::from_secs(1));
        let half_faded = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > first_fade_frame.transport_sequence
                && frame.snapshot != first_fade_frame.snapshot
                && frame.snapshot != baseline.snapshot
        });
        assert_ne!(half_faded.snapshot, before_stop.snapshot);

        clock.set(Duration::from_millis(1500));
        let completed = client
            .wait_for_revision(duplicate.revision.saturating_add(1), TIMEOUT)
            .expect("fade completion");
        assert!(completed.active_motion.is_none());
        let final_frame = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > half_faded.transport_sequence
        });
        assert_eq!(
            final_frame.snapshot.model_opacity,
            baseline.snapshot.model_opacity
        );
        assert_eq!(
            final_frame.snapshot.drawables.len(),
            baseline.snapshot.drawables.len()
        );

        owner.shutdown(TIMEOUT).expect("runtime shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn expression_commands_crossfade_and_preserve_the_active_expression_on_error() {
        let clock = Arc::new(ManualClock::default());
        let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
            true,
            8,
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let client = owner.client();
        client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
        let activation_sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
                "standard",
            ))))
            .expect("activation command");
        let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
        report_model_prepared(&client, &consumer, &candidate);
        let baseline = wait_for_render_frame(&consumer, |frame| {
            frame.model_generation == candidate.model_generation
                && frame.frame_number > candidate.frame_number
        });

        let first = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
        let first_sequence = client
            .send(RuntimeCommand::SetExpression(first.clone()))
            .expect("set first expression");
        let first_active = client
            .wait_for_command(first_sequence, TIMEOUT)
            .expect("first expression active");
        assert_eq!(
            first_active.active_expression,
            Some(ActiveExpressionSnapshot {
                expression: first.clone(),
                command_sequence: first_sequence,
            })
        );

        clock.set(Duration::from_millis(400));
        let first_frame = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > baseline.transport_sequence
                && frame.snapshot != baseline.snapshot
        });

        let second = ExpressionId::new("live2d_expression2.exp3.json").expect("expression id");
        let second_sequence = client
            .send(RuntimeCommand::SetExpression(second.clone()))
            .expect("replace expression");
        let second_active = client
            .wait_for_command(second_sequence, TIMEOUT)
            .expect("replacement expression active");
        assert_eq!(
            second_active.active_expression,
            Some(ActiveExpressionSnapshot {
                expression: second.clone(),
                command_sequence: second_sequence,
            })
        );
        clock.set(Duration::from_millis(650));
        let crossfaded = wait_for_render_frame(&consumer, |frame| {
            frame.transport_sequence > first_frame.transport_sequence
                && frame.snapshot != first_frame.snapshot
        });
        assert_ne!(crossfaded.snapshot, first_frame.snapshot);

        let invalid_sequence = client
            .send(RuntimeCommand::SetExpression(
                ExpressionId::new("missing").expect("syntactically valid expression id"),
            ))
            .expect("invalid expression request");
        let invalid = client
            .wait_for_command(invalid_sequence, TIMEOUT)
            .expect("invalid expression result");
        assert_eq!(
            invalid.last_command_failure,
            Some(RuntimeCommandFailure {
                sequence: invalid_sequence,
                code: RuntimeRenderErrorCode::ExpressionLoadFailed,
            })
        );
        assert_eq!(invalid.active_expression, second_active.active_expression);

        let stopped = owner.shutdown(TIMEOUT).expect("runtime shutdown");
        assert!(stopped.active_expression.is_none());
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
        let active_motion_id = MotionId::new("CAT_motion", 0).expect("motion id");
        let motion_sequence = client
            .send(RuntimeCommand::StartMotion {
                motion: active_motion_id.clone(),
                priority: MotionPriority::Normal,
            })
            .expect("motion command");
        let motion_active = client
            .wait_for_command(motion_sequence, TIMEOUT)
            .expect("motion active");
        let expected_motion = motion_active.active_motion.clone();
        assert!(expected_motion.is_some());
        let expression_sequence = client
            .send(RuntimeCommand::SetExpression(
                ExpressionId::new("live2d_expression1.exp3.json").expect("expression id"),
            ))
            .expect("expression command");
        let expression_active = client
            .wait_for_command(expression_sequence, TIMEOUT)
            .expect("expression active");
        let expected_expression = expression_active.active_expression.clone();
        assert!(expected_expression.is_some());

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
        assert_eq!(rejected.active_motion, expected_motion);
        assert_eq!(rejected.active_expression, expected_expression);
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
        assert_eq!(gpu_rejected.active_motion, expected_motion);
        assert_eq!(gpu_rejected.active_expression, expected_expression);
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
        assert!(replaced.active_motion.is_none());
        assert!(replaced.active_expression.is_none());
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
        let motion_audio = MotionAudioClient::unavailable();
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
                command_transport: Arc::new(CommandTransportCounters::default()),
                accepting: Arc::new(AtomicBool::new(true)),
            }),
            snapshot: Arc::new(SnapshotCell {
                value: Mutex::new(RuntimeSnapshot::starting(
                    true,
                    false,
                    motion_audio.diagnostics(),
                )),
                changed: Condvar::new(),
            }),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
            cursor_slot: Arc::new(CursorSlot::default()),
            gamepad_axis_slot: Arc::new(GamepadAxisSlot::with_capacity(
                DEFAULT_GAMEPAD_AXIS_CAPACITY,
            )),
            platform_input_diagnostics: PlatformInputDiagnosticsProducer::default(),
            motion_audio,
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
        assert_eq!(
            client.snapshot().command_transport,
            RuntimeCommandTransportDiagnostics {
                enqueued: 1,
                queue_full: 1,
                runtime_stopped: 0,
                sequence_gap_count: 0,
                missing_sequence_count: 0,
                duplicate_sequence_count: 0,
                out_of_order_sequence_count: 0,
            }
        );
    }

    #[test]
    fn command_sequence_tracker_classifies_gaps_duplicates_and_wraparound() {
        let mut tracker = CommandSequenceTracker::default();
        assert_eq!(tracker.observe(7), CommandSequenceDisposition::First);
        assert_eq!(
            tracker.observe(9),
            CommandSequenceDisposition::Gap { missing: 1 }
        );
        assert_eq!(tracker.observe(9), CommandSequenceDisposition::Duplicate);
        assert_eq!(tracker.observe(8), CommandSequenceDisposition::OutOfOrder);

        let mut wrapping = CommandSequenceTracker {
            last: Some(u64::MAX),
            ..Default::default()
        };
        assert_eq!(wrapping.observe(0), CommandSequenceDisposition::InOrder);
        assert_eq!(
            wrapping.observe(2),
            CommandSequenceDisposition::Gap { missing: 1 }
        );
        assert_eq!(wrapping.observe(2), CommandSequenceDisposition::Duplicate);
        assert_eq!(
            wrapping.observe(u64::MAX),
            CommandSequenceDisposition::OutOfOrder
        );
    }

    #[test]
    fn input_producer_overflow_is_observable_and_recovery_resets_state() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let input_transport = Arc::new(InputTransportCounters::default());
        let motion_audio = MotionAudioClient::unavailable();
        let client = RuntimeClient {
            producer: Arc::new(Producer {
                sender,
                next_sequence: Mutex::new(0),
                command_transport: Arc::new(CommandTransportCounters::default()),
                accepting: Arc::new(AtomicBool::new(true)),
            }),
            snapshot: Arc::new(SnapshotCell {
                value: Mutex::new(RuntimeSnapshot::starting(
                    true,
                    false,
                    motion_audio.diagnostics(),
                )),
                changed: Condvar::new(),
            }),
            input_transport,
            input_producer_state: Arc::new(Mutex::new(InputProducerState::default())),
            cursor_slot: Arc::new(CursorSlot::default()),
            gamepad_axis_slot: Arc::new(GamepadAxisSlot::with_capacity(
                DEFAULT_GAMEPAD_AXIS_CAPACITY,
            )),
            platform_input_diagnostics: PlatformInputDiagnosticsProducer::default(),
            motion_audio,
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
