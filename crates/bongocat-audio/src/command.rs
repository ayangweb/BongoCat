//! What may be asked of the sound, and what happens when the queue is full.
//!
//! Every command carries the audio sequence it belongs to, so a command that
//! arrives after the model has already moved on is dropped rather than played
//! against the wrong state. The sequence wraps at `u64::MAX`, and the wrap is
//! monotonic in the ordering it is compared with, so a long-running process does
//! not have to restart to keep ordering correctly.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum MotionAudioCommand {
    /// Prepares every distinct sound referenced by the model that is about to
    /// become active. This is the only path that may decode a FLAC file; the
    /// output device remains lazy and is opened by the first `Play`.
    Prepare { sequence: u64, paths: Vec<PathBuf> },
    /// Discards clips that do not belong to the committed active model.
    ActivatePrepared { sequence: u64, paths: Vec<PathBuf> },
    Play {
        sequence: u64,
        path: PathBuf,
        volume: MotionAudioVolume,
    },
    Stop {
        sequence: u64,
        reason: MotionAudioStopReason,
    },
}

impl MotionAudioCommand {
    pub const fn sequence(&self) -> u64 {
        match self {
            Self::Prepare { sequence, .. }
            | Self::ActivatePrepared { sequence, .. }
            | Self::Play { sequence, .. }
            | Self::Stop { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum MotionAudioPublishError {
    #[error("motion audio command queue is full")]
    QueueFull(MotionAudioCommand),
    #[error("motion audio command recovery is pending")]
    RecoveryPending(MotionAudioCommand),
    #[error("motion audio service is stopped")]
    ServiceStopped(MotionAudioCommand),
}
