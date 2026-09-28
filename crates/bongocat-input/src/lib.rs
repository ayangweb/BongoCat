#![forbid(unsafe_code)]

//! Platform-neutral input contracts and transport state.
//!
//! This crate owns the typed input vocabulary used by platform adapters and the
//! single runtime owner. It deliberately has no operating-system, GPUI, Cubism,
//! configuration, or renderer dependency. The runtime still owns when these
//! values are applied; this crate only provides deterministic state machines and
//! bounded/latest-value transports.

mod cursor;
mod gamepad;
mod input;
mod platform_input;

pub use cursor::{
    CursorPosition, CursorProducer, CursorPublishError, CursorSample, CursorSampleError,
    CursorSmoother, CursorSnapshot, CursorTransportDiagnostics, CursorViewport,
    NormalizedCursorPosition,
};
pub use gamepad::{
    DEFAULT_GAMEPAD_AXIS_CAPACITY, GamepadAxisProducer, GamepadAxisPublishError, GamepadAxisSample,
    GamepadAxisSettings, GamepadAxisTransportDiagnostics, GamepadConnectionError,
};
pub use input::{
    GLOBE_KEY_USAGE, GamepadAxis, GamepadAxisKey, GamepadButton, GamepadButtonKey,
    GamepadConnection, HandSide, InputBindings, InputControl, InputDiagnostics, InputEdge,
    InputEvent, InputProducer, InputPublishError, InputResetReason, InputSource, InputSubmitError,
    InputSubmitter, InputTransportDiagnostics, MonotonicMillis, MouseButton, PhysicalKey,
    SequencedInputEvent,
};
pub use platform_input::{
    PlatformInputDiagnostics, PlatformInputDiagnosticsProducer,
    PlatformInputDiagnosticsPublishError, PlatformInputServiceStatus,
    is_stable_platform_input_error_code,
};
