//! The runtime: the single owner of pressed input state, the active model and the
//! published frame snapshot.
//!
//! Everything that crosses a thread boundary is a strongly typed message here —
//! `RuntimeCommand` in, `RuntimeSnapshot` out — so the renderer and the platform
//! never see each other's types. What lives in this root is the protocol itself
//! and the bounds that validate it; the machinery behind it is in [`pacing`],
//! [`transport`], [`client`], [`owner`] and [`worker`].

#![forbid(unsafe_code)]

mod client;
mod input_state;
mod owner;
mod pacing;
mod random_behavior;
mod rendering;
#[cfg(test)]
mod tests;
mod transport;
mod worker;

use bongocat_audio::{
    MotionAudioClient, MotionAudioCommand, MotionAudioDiagnostics, MotionAudioStopReason,
    MotionAudioVolume,
};
use bongocat_model::{CommittedModel, ModelOrigin, ModelSnapshot};
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

pub use bongocat_input::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorSampleError,
    CursorSnapshot, CursorTransportDiagnostics, CursorViewport, GamepadAxis, GamepadAxisKey,
    GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample, GamepadAxisSettings,
    GamepadAxisTransportDiagnostics, GamepadButton, GamepadButtonKey, GamepadConnection,
    GamepadConnectionError, HandSide, InputBindings, InputControl, InputDiagnostics, InputEdge,
    InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource, InputSubmitError,
    InputSubmitter, InputTransportDiagnostics, MonotonicMillis, MouseButton,
    NormalizedCursorPosition, PhysicalKey, PlatformInputDiagnostics,
    PlatformInputDiagnosticsProducer, PlatformInputDiagnosticsPublishError,
    PlatformInputServiceStatus, SequencedInputEvent, is_stable_platform_input_error_code,
};
use bongocat_input::{CursorSmoother, DEFAULT_GAMEPAD_AXIS_CAPACITY};
pub use client::RuntimeClient;
use input_state::{InputDisposition, InputState};
pub use input_state::{InputSnapshot, ModelInputSnapshot};
pub use owner::RuntimeOwner;
pub use pacing::{
    FramePacer, MonotonicClock, frame_interval_for_maximum_fps, frame_interval_for_runtime,
};
pub use random_behavior::{
    DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS, MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
    MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS, RandomBehaviorSettings,
};
use random_behavior::{RandomBehaviorScheduler, system_seed};
use rendering::{MotionStopStatus, RuntimeRenderBootstrap, RuntimeRenderer};
use transport::{SnapshotCell, publish};

pub const DEFAULT_MAXIMUM_FPS: u16 = 60;
pub const MINIMUM_FPS: u16 = 15;
pub const MAXIMUM_FPS: u16 = 240;
pub const DEFAULT_RELEASE_FALLBACK_TIMEOUT_MS: u32 = 500;
pub const MAX_RELEASE_FALLBACK_TIMEOUT_MS: u32 = 60_000;
pub const HIDDEN_OVERLAY_FRAME_INTERVAL: Duration = Duration::from_millis(100);
// Automatic playback uses the upper half of the runtime event sequence space.
// Audio commands use their own client-allocated sequence domain, so automatic
// playback can never collide with a product command or an audio waiter.
const AUTOMATIC_SEQUENCE_START: u64 = 1_u64 << 63;

pub const fn maximum_fps_is_valid(maximum_fps: u16) -> bool {
    maximum_fps >= MINIMUM_FPS && maximum_fps <= MAXIMUM_FPS
}

pub const fn release_fallback_timeout_is_valid(timeout_ms: u32) -> bool {
    timeout_ms <= MAX_RELEASE_FALLBACK_TIMEOUT_MS
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
    TransportClosed,
    OverlaySettingsInvalid,
    MaximumFpsInvalid,
    ReleaseFallbackTimeoutInvalid,
    RandomBehaviorSettingsInvalid,
}

impl RuntimeRenderErrorCode {
    pub const ALL: [Self; 10] = [
        Self::ModelLoadFailed,
        Self::ModelEvaluationFailed,
        Self::MotionLoadFailed,
        Self::ExpressionLoadFailed,
        Self::GpuPreparationFailed,
        Self::TransportClosed,
        Self::OverlaySettingsInvalid,
        Self::MaximumFpsInvalid,
        Self::ReleaseFallbackTimeoutInvalid,
        Self::RandomBehaviorSettingsInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLoadFailed => "model_load_failed",
            Self::ModelEvaluationFailed => "model_evaluation_failed",
            Self::MotionLoadFailed => "motion_load_failed",
            Self::ExpressionLoadFailed => "expression_load_failed",
            Self::GpuPreparationFailed => "gpu_preparation_failed",
            Self::TransportClosed => "transport_closed",
            Self::OverlaySettingsInvalid => "overlay_settings_invalid",
            Self::MaximumFpsInvalid => "maximum_fps_invalid",
            Self::ReleaseFallbackTimeoutInvalid => "release_fallback_timeout_invalid",
            Self::RandomBehaviorSettingsInvalid => "random_behavior_settings_invalid",
        }
    }
}

impl fmt::Display for RuntimeRenderErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Upper bound of the overlay hover hide delay, in whole seconds.
///
/// The legacy input accepted whole seconds with no upper bound; the first
/// version caps them at one minute. Seconds are also the unit the configuration
/// and the settings page use, so nothing converts between the stored value and
/// the visible one. See
/// `bongocat_config::OverlayConfig::hide_on_pointer_hover_delay_seconds`.
pub const MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS: u32 = 60;

/// The same ceiling in the milliseconds the overlay frame loop counts in.
///
/// The hover state machine compares against `MonotonicMillis`, so the overlay
/// options stay millisecond-valued; this constant keeps the platform option
/// validation on the shared bound instead of a second literal.
pub const MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS: u32 =
    MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS * 1_000;

/// The hover hide delay in the milliseconds the overlay frame loop counts in.
///
/// The current v1 configuration and the settings page store whole seconds; the
/// overlay compares the delay against `MonotonicMillis`. This is the single
/// conversion between the stored unit and the frame clock, so callers do not
/// repeat the factor. The multiply saturates rather than wraps, which keeps an
/// out-of-range value large enough for the option validation to reject it.
pub const fn hover_hide_delay_ms(seconds: u32) -> u32 {
    seconds.saturating_mul(1_000)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySettings {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    /// Corner radius of the overlay window box as a percentage of its width and
    /// height, mirroring the legacy `border-radius: N%` window setting. `0` keeps
    /// square corners and `50` clips the window content to the full inscribed
    /// ellipse, which is also the legacy ceiling: the legacy implementation
    /// scaled every larger radius back down to that same ellipse.
    pub corner_radius_percent: u8,
    /// Hide the overlay content while the pointer rests on the overlay window,
    /// mirroring the legacy `window.hideOnHover` switch. The overlay keeps its
    /// window and keeps presenting frames; it drops its rendered alpha to zero
    /// and passes pointer events through until the pointer leaves again.
    pub hide_on_pointer_hover: bool,
    /// How long the pointer must stay inside the overlay window before the
    /// hover hide starts, in whole seconds. `0` hides immediately.
    pub hide_on_pointer_hover_delay_seconds: u32,
    /// Keep the overlay window fully on a display. The region is the union of
    /// the connected displays rather than one display's work area, so the
    /// window may cover a taskbar, Dock or menu bar; a window dragged off the
    /// desktop is moved back after a short settle delay instead of snapping
    /// during the drag.
    pub keep_inside_screen: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            click_through: false,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            corner_radius_percent: 0,
            hide_on_pointer_hover: false,
            hide_on_pointer_hover_delay_seconds: 0,
            keep_inside_screen: true,
        }
    }
}

impl OverlaySettings {
    pub const fn is_valid(self) -> bool {
        self.scale_percent >= 25
            && self.scale_percent <= 400
            && self.opacity_percent >= 1
            && self.opacity_percent <= 100
            && self.corner_radius_percent <= 50
            && self.hide_on_pointer_hover_delay_seconds
                <= MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModelSettings {
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    /// Whether keyboard input is excluded from the model's input projection.
    pub ignore_keyboard: bool,
    /// Whether gamepad input is excluded from the model's input projection.
    pub ignore_gamepad: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("motion group must not be blank")]
pub struct MotionIdError;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("expression name must not be blank")]
pub struct ExpressionIdError;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MotionPriority {
    Idle,
    Normal,
    Force,
}

/// A shortcut action resolved by the configuration/platform boundary.
/// Runtime receives the already-typed model identity and never parses a
/// behavior string or platform key code on its real-time thread.
///
/// [`ShortcutAction::StartMotion`] plays one cycle and is idempotent while that
/// cycle is in flight; see [`RuntimeCommand::StartMotion`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutAction {
    StartMotion {
        motion: MotionId,
        priority: MotionPriority,
    },
    StopMotion(MotionId),
    SetExpression(ExpressionId),
}

/// The runtime's current motion layer. A one-shot motion remains present after
/// completion so its terminal pose can still be stopped or replaced.
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeWorkDiagnostics {
    pub budget_exceeded: u64,
    pub last_over_budget_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeShutdownDiagnostics {
    pub timed_out: u64,
    pub worker_panicked: u64,
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
    SetMaximumFps(u16),
    SetReleaseFallbackTimeout(u32),
    SetRandomBehaviorSettings(RandomBehaviorSettings),
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
    /// Starts a product motion. The runtime plays the clip exactly once, then
    /// keeps its final evaluated pose as the current motion layer. The clip's
    /// declared `Meta.Loop` never keeps it advancing. While that run is still
    /// in flight, a repeat request for the same motion at the same priority is
    /// a no-op; after completion, the next request may replace or restart it.
    StartMotion {
        motion: MotionId,
        priority: MotionPriority,
    },
    /// Plays one clip once for a UI preview at [`MotionPriority::Force`].
    /// Unlike [`RuntimeCommand::StartMotion`], every request restarts the
    /// preview.
    PreviewMotion(MotionId),
    /// Stops the current motion with this identity. A replayed run is still the
    /// current run, so a later stop for the same ID intentionally targets it.
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

/// What a frame source needs to choose its frame interval.
///
/// Read through [`RuntimeClient::frame_scheduling`] instead of cloning a whole
/// [`RuntimeSnapshot`] per frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameScheduling {
    pub maximum_fps: u16,
    pub overlay_visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: RuntimeState,
    pub overlay_visible: bool,
    pub overlay_settings: OverlaySettings,
    pub maximum_fps: u16,
    pub release_fallback_timeout_ms: u32,
    pub random_behavior_settings: RandomBehaviorSettings,
    pub model_settings: ModelSettings,
    pub gamepad_axis_settings: GamepadAxisSettings,
    pub motion_audio_enabled: bool,
    pub motion_audio: MotionAudioDiagnostics,
    pub active_model: Option<ModelSnapshot>,
    /// Storage origin of the model currently owned by the runtime. The model
    /// id alone is not a complete identity because an imported package may use
    /// the same id as a build-shipped model.
    pub active_model_origin: Option<ModelOrigin>,
    pub pending_model: Option<PendingModelSnapshot>,
    pub active_motion: Option<ActiveMotionSnapshot>,
    pub active_expression: Option<ActiveExpressionSnapshot>,
    pub motion_events: MotionEventDiagnostics,
    pub input: InputSnapshot,
    pub cursor: CursorSnapshot,
    pub gamepad_axis_transport: GamepadAxisTransportDiagnostics,
    pub platform_input: PlatformInputDiagnostics,
    pub command_transport: RuntimeCommandTransportDiagnostics,
    pub work: RuntimeWorkDiagnostics,
    pub shutdown: RuntimeShutdownDiagnostics,
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
            maximum_fps: DEFAULT_MAXIMUM_FPS,
            release_fallback_timeout_ms: DEFAULT_RELEASE_FALLBACK_TIMEOUT_MS,
            random_behavior_settings: RandomBehaviorSettings::default(),
            model_settings: ModelSettings::default(),
            gamepad_axis_settings: GamepadAxisSettings::default(),
            motion_audio_enabled,
            motion_audio,
            active_model: None,
            active_model_origin: None,
            pending_model: None,
            active_motion: None,
            active_expression: None,
            motion_events: MotionEventDiagnostics::default(),
            input: InputSnapshot::default(),
            cursor: CursorSnapshot::default(),
            gamepad_axis_transport: GamepadAxisTransportDiagnostics::default(),
            platform_input: PlatformInputDiagnostics::default(),
            command_transport: RuntimeCommandTransportDiagnostics::default(),
            work: RuntimeWorkDiagnostics::default(),
            shutdown: RuntimeShutdownDiagnostics::default(),
            model_input: ModelInputSnapshot::default(),
            render_error: None,
            last_command_failure: None,
            last_command_sequence: None,
        }
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum SendError {
    #[error("runtime command queue is full")]
    QueueFull(RuntimeCommand),
    #[error("runtime is stopped")]
    RuntimeStopped(RuntimeCommand),
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShutdownError {
    #[error("runtime shutdown timed out")]
    TimedOut,
    #[error("runtime worker panicked")]
    WorkerPanicked,
}
