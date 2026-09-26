//! What the sound has done, for a diagnostics report.
//!
//! A sound that fails silently is indistinguishable from one that was never
//! asked for, so every failure is counted and the last one is kept with its
//! message. The counts survive the worker thread, which is why they are behind the
//! same lock the commands are.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionAudioDiagnostics {
    pub state: MotionAudioState,
    pub enqueued_commands: u64,
    pub processed_commands: u64,
    pub discarded_commands: u64,
    pub prepare_requests: u64,
    pub prepared_resources: u64,
    pub play_requests: u64,
    pub playback_starts: u64,
    pub stop_requests: u64,
    pub voices_stopped: u64,
    pub queue_overflows: u64,
    pub rejected_after_shutdown: u64,
    pub resource_failures: u64,
    pub decode_failures: u64,
    pub output_failures: u64,
    pub current_voice_sequence: Option<u64>,
    pub last_processed_sequence: Option<u64>,
    pub last_error: Option<MotionAudioErrorCode>,
}

impl MotionAudioDiagnostics {
    pub(crate) fn starting() -> Self {
        Self {
            state: MotionAudioState::Starting,
            enqueued_commands: 0,
            processed_commands: 0,
            discarded_commands: 0,
            prepare_requests: 0,
            prepared_resources: 0,
            play_requests: 0,
            playback_starts: 0,
            stop_requests: 0,
            voices_stopped: 0,
            queue_overflows: 0,
            rejected_after_shutdown: 0,
            resource_failures: 0,
            decode_failures: 0,
            output_failures: 0,
            current_voice_sequence: None,
            last_processed_sequence: None,
            last_error: None,
        }
    }

    pub(crate) fn unavailable() -> Self {
        let mut diagnostics = Self::starting();
        diagnostics.state = MotionAudioState::Stopped;
        diagnostics.last_error = Some(MotionAudioErrorCode::WorkerUnavailable);
        diagnostics
    }
}
