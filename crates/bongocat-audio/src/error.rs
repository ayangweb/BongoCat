//! Why a sound could not be played.
//!
//! A missing file, an unreadable file and a device that will not open are three
//! different problems with three different fixes, so they are three codes rather
//! than one. A start failure wraps the platform's own error because the platform's
//! message is the only thing that says which file or which device.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioErrorCode {
    ResourceIo,
    DecodeFailed,
    OutputUnavailable,
    WorkerUnavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot start motion audio worker: {0}")]
pub struct MotionAudioStartError(pub(crate) io::Error);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MotionAudioShutdownError {
    #[error("motion audio shutdown timed out")]
    TimedOut,
    #[error("motion audio worker panicked")]
    WorkerPanicked,
}
