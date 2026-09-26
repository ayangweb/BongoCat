//! What the sound is doing, and why it stopped.
//!
//! One voice at a time: a new motion replaces the one playing rather than
//! layering under it, because a layered mix of two model sounds is neither of
//! them. A stop reason is part of the state rather than a log line, because the
//! two reasons a sound stops on its own — the model asked, or the queue overflowed
//! — mean different things to the runtime.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioStopReason {
    MotionStopped,
    MotionReplaced,
    ModelSwitched,
    Disabled,
    Shutdown,
}
